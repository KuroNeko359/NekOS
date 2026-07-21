//! 虚拟内存管理

use crate::arch::riscv::*;
use crate::kernel::pgtable;
use crate::arch::riscv::csr;
use crate::println;

/// 内核页表
static mut KERNEL_PAGETABLE: Option<&'static mut pgtable::PageTable> = None;

/// 初始化内核虚拟内存
pub fn init() {
    unsafe {
        // 创建内核页表
        let pt = pgtable::create().expect("Failed to create kernel page table");
        
        // 映射内核代码和数据
        // 内核在0x80200000开始，我们映射整个内核空间
        let kernel_start = 0x80200000usize;
        let kernel_size = 0x01000000usize;  // 16MB
        
        // 直接映射内核空间 (PA = VA)
        pgtable::map(
            pt,
            kernel_start,
            kernel_start,
            kernel_size,
            PTE_R | PTE_W | PTE_X,
        ).expect("Failed to map kernel space");
        
        // 映射UART
        pgtable::map(
            pt,
            0x10000000,
            0x10000000,
            PAGE_SIZE,
            PTE_R | PTE_W,
        ).expect("Failed to map UART");
        
        // 设置SATP寄存器
        let satp = (8usize << 60) | ((pt as *mut pgtable::PageTable as usize) >> PAGE_SHIFT);
        csr::write_satp(satp);
        csr::sfence_vma();
        
        println!("vm: Sv39 enabled, satp=0x{:x}", satp);
        
        KERNEL_PAGETABLE = Some(pt);
    }
}

/// 映射内核页面到新的页表
pub fn map_kernel(pt: &mut pgtable::PageTable) -> Result<(), ()> {
    // 映射内核空间
    let kernel_start = 0x80200000usize;
    let kernel_size = 0x01000000usize;
    
    pgtable::map(
        pt,
        kernel_start,
        kernel_start,
        kernel_size,
        PTE_R | PTE_W | PTE_X,
    )?;
    
    // 映射UART
    pgtable::map(
        pt,
        0x10000000,
        0x10000000,
        PAGE_SIZE,
        PTE_R | PTE_W,
    )?;
    
    Ok(())
}

/// 切换页表
pub fn switch(pt: &pgtable::PageTable) {
    let satp = (8usize << 60) | ((pt as *const pgtable::PageTable as usize) >> PAGE_SHIFT);
    unsafe {
        csr::write_satp(satp);
        csr::sfence_vma();
    }
}

/// 当前内核页表的 SATP 值。
pub fn kernel_satp() -> usize {
    unsafe {
        let pt = KERNEL_PAGETABLE.as_ref().expect("kernel page table not initialized");
        (8usize << 60) | ((*pt as *const pgtable::PageTable as usize) >> PAGE_SHIFT)
    }
}

/// 把用户缓冲页映射到内核页表，供系统调用复制数据。
/// 当前内核是单进程原型；多进程版本应改为 copy_from/to_user。
pub fn map_user_buffer(va: usize, pa: usize) -> Result<(), ()> {
    unsafe {
        let pt = KERNEL_PAGETABLE.as_mut().ok_or(())?;
        pgtable::map(pt, va, pa, PAGE_SIZE, PTE_R | PTE_W)?;
        csr::sfence_vma();
    }
    Ok(())
}
