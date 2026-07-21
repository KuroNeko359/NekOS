#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod arch;
#[macro_use]
mod drivers;
mod kernel;
mod user;

unsafe extern "C" {
    unsafe static __kernel_end: u8;
}

/// 内核入口点
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // 初始化UART
    drivers::uart::init();
    
    println!("Hello, RISC-V OS!");
    println!("KuroOS Rust - Microkernel Operating System");
    println!("kernel_main = 0x{:x}", kernel_main as usize);

    kernel::page::init();
    
    kernel::vm::init();

    // 初始化内嵌用户程序归档
    kernel::initrd::init();
    
    kernel::task::init();
    
    kernel::trap::init();
    
    kernel::timer::init();
    
    let image_start: usize = arch::riscv::KERNEL_ENTRY;
    let image_end = unsafe { &__kernel_end as *const u8 as usize };
    let user_entry = user::shell::user_main as usize;
    let pid = kernel::task::create_user(
        image_start,
        image_end,
        user_entry,
    ).expect("Failed to create user process");
    
    println!("entering user mode: pid={}", pid);
    
    // 进入用户模式
    unsafe {
        let kernel_satp = kernel::vm::kernel_satp();
        kernel::task::set_current(pid);
        let user_satp = kernel::task::task_satp(pid).expect("missing user page table");
        let user_sp = arch::riscv::USER_STACK_TOP;
        let trap_stack = kernel::task::kernel_stack_top(pid).expect("missing kernel stack") - 16;
        
        kernel::task::enter_user(user_sp, user_entry, trap_stack, kernel_satp, user_satp);
    }
    
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// panic处理函数
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
