//! exec 系统调用实现

use crate::arch::riscv::*;
use crate::kernel::trap::TrapFrame;
use crate::kernel::pgtable;
use crate::kernel::vm;
use crate::kernel::page;
use crate::println;

/// ELF 头部
#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Ehdr {
    e_ident: [u8; 16],     // Magic number and other info
    e_type: u16,           // Object file type
    e_machine: u16,        // Architecture
    e_version: u32,        // Object file version
    e_entry: u64,          // Entry point virtual address
    e_phoff: u64,          // Program header table file offset
    e_shoff: u64,          // Section header table file offset
    e_flags: u32,          // Processor-specific flags
    e_ehsize: u16,         // ELF header size in bytes
    e_phentsize: u16,      // Program header table entry size
    e_phnum: u16,          // Program header table entry count
    e_shentsize: u16,      // Section header table entry size
    e_shnum: u16,          // Section header table entry count
    e_shstrndx: u16,       // Section header string table index
}

/// ELF 程序头
#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,           // Segment type
    p_flags: u32,          // Segment flags
    p_offset: u64,         // Segment file offset
    p_vaddr: u64,          // Segment virtual address
    p_paddr: u64,          // Segment physical address
    p_filesz: u64,         // Segment size in file
    p_memsz: u64,          // Segment size in memory
    p_align: u64,          // Segment alignment
}

/// ELF 魔数
const ELFMAG0: u8 = 0x7f;
const ELFMAG1: u8 = b'E';
const ELFMAG2: u8 = b'L';
const ELFMAG3: u8 = b'F';

/// ELF 类型
const ET_EXEC: u16 = 2;  // Executable file

/// ELF 机器类型
const EM_RISCV: u16 = 243;

/// 程序头类型
const PT_LOAD: u32 = 1;

/// 程序头标志
const PF_X: u32 = 0x1;  // Execute
const PF_W: u32 = 0x2;  // Write
const PF_R: u32 = 0x4;  // Read

pub struct LoadedImage {
    pagetable: &'static mut pgtable::PageTable,
    user_stack: usize,
    entry: usize,
    image_base: usize,
    image_size: usize,
}

/// 验证ELF头
fn elf_verify(ehdr: &Elf64Ehdr, size: usize) -> bool {
    if size < core::mem::size_of::<Elf64Ehdr>() { return false; }
    if ehdr.e_ident[0] != ELFMAG0 ||
       ehdr.e_ident[1] != ELFMAG1 ||
       ehdr.e_ident[2] != ELFMAG2 ||
       ehdr.e_ident[3] != ELFMAG3 {
        return false;
    }
    
    if ehdr.e_type != ET_EXEC {
        return false;
    }
    
    if ehdr.e_machine != EM_RISCV || ehdr.e_phentsize as usize != core::mem::size_of::<Elf64Phdr>() {
        return false;
    }
    (ehdr.e_phoff as usize).checked_add(ehdr.e_phnum as usize * ehdr.e_phentsize as usize)
        .is_some_and(|end| end <= size)
}

/// 加载ELF段
fn elf_load(
    pagetable: &mut pgtable::PageTable,
    elf_data: &[u8],
    ehdr: &Elf64Ehdr,
) -> Result<(usize, usize, usize), ()> {
    let entry = ehdr.e_entry as usize;
    let mut image_start = usize::MAX;
    let mut image_end = 0usize;
    
    for i in 0..ehdr.e_phnum {
        let phdr_offset = ehdr.e_phoff as usize + i as usize * ehdr.e_phentsize as usize;
        let phdr = unsafe {
            (elf_data.as_ptr().add(phdr_offset) as *const Elf64Phdr).read_unaligned()
        };
        
        if phdr.p_type != PT_LOAD || phdr.p_memsz == 0 {
            continue;
        }
        let segment_file_end = (phdr.p_offset as usize).checked_add(phdr.p_filesz as usize).ok_or(())?;
        if phdr.p_filesz > phdr.p_memsz || segment_file_end > elf_data.len() { return Err(()); }
        
        let va_start = phdr.p_vaddr as usize & !(PAGE_SIZE - 1);
        let va_end = ((phdr.p_vaddr + phdr.p_memsz) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let num_pages = (va_end - va_start) / PAGE_SIZE;
        image_start = image_start.min(va_start);
        image_end = image_end.max(va_end);
        
        for j in 0..num_pages {
            // 分配页面
            let page = page::alloc().ok_or(())?;
            
            // 清零页面
            unsafe {
                core::ptr::write_bytes(page as *mut u8, 0, PAGE_SIZE);
            }
            
            // 复制文件数据
            let page_va = va_start + j * PAGE_SIZE;
            let page_end = page_va + PAGE_SIZE;
            let file_va_start = phdr.p_vaddr as usize;
            let file_va_end = file_va_start + phdr.p_filesz as usize;
            let copy_va_start = page_va.max(file_va_start);
            let copy_va_end = page_end.min(file_va_end);
            if copy_va_start < copy_va_end {
                let src_offset = phdr.p_offset as usize + copy_va_start - file_va_start;
                let dst_offset = copy_va_start - page_va;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        elf_data.as_ptr().add(src_offset),
                        (page as *mut u8).add(dst_offset),
                        copy_va_end - copy_va_start,
                    );
                }
            }
            
            // 设置页面标志
            let mut flags = PTE_U;
            if phdr.p_flags & PF_R != 0 { flags |= PTE_R; }
            if phdr.p_flags & PF_W != 0 { flags |= PTE_W; }
            if phdr.p_flags & PF_X != 0 { flags |= PTE_X; }
            
            // 映射页面
            pgtable::map(pagetable, page_va, page, PAGE_SIZE, flags)?;
        }
    }
    
    if image_start == usize::MAX { return Err(()); }
    Ok((entry, image_start, image_end - image_start))
}

/// 从 initrd 创建一个完整、但尚未注册到调度器的用户地址空间。
fn load(filename: &str) -> Result<LoadedImage, ()> {
    let elf_data = crate::kernel::initrd::find(filename).ok_or_else(|| {
        println!("exec: file '{}' not found", filename);
    })?;
    if elf_data.len() < core::mem::size_of::<Elf64Ehdr>() {
        println!("exec: invalid ELF file '{}'", filename);
        return Err(());
    }
    let ehdr = unsafe { (elf_data.as_ptr() as *const Elf64Ehdr).read_unaligned() };
    if !elf_verify(&ehdr, elf_data.len()) {
        println!("exec: invalid ELF file '{}'", filename);
        return Err(());
    }

    let pagetable = pgtable::create().ok_or(())?;
    vm::map_kernel(pagetable)?;
    let (entry, image_base, image_size) = elf_load(pagetable, elf_data, &ehdr)?;
    let user_stack = page::alloc().ok_or(())?;
    unsafe { core::ptr::write_bytes(user_stack as *mut u8, 0, PAGE_SIZE); }
    pgtable::map(
        pagetable,
        USER_STACK_TOP - PAGE_SIZE,
        user_stack,
        PAGE_SIZE,
        PTE_R | PTE_W | PTE_U,
    )?;
    Ok(LoadedImage {
        pagetable,
        user_stack,
        entry,
        image_base,
        image_size,
    })
}

/// 从 initrd 加载 ELF 并创建一个新的用户进程。
pub fn spawn(filename: &str) -> Result<u32, ()> {
    let image = load(filename)?;
    let entry = image.entry;
    let pid = crate::kernel::task::create_user_image(
        image.pagetable,
        image.user_stack,
        image.entry,
        image.image_base,
        image.image_size,
    )?;
    println!("spawn: loaded '{}' pid={} entry=0x{:x}", filename, pid, entry);
    Ok(pid)
}

/// exec 系统调用实现
pub fn exec(filename: &str, tf: &mut TrapFrame) -> Result<(), ()> {
    let image = load(filename)?;
    crate::kernel::task::replace_current(
        image.pagetable,
        image.user_stack,
        image.entry,
        image.image_base,
        image.image_size,
        tf,
    )?;
    println!("exec: loaded '{}' entry=0x{:x}", filename, image.entry);
    Ok(())
}
