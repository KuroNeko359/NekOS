#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod arch;
#[macro_use]
mod drivers;
mod kernel;

/// 内核入口点
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // 初始化UART
    drivers::uart::init();
    
    println!("Hello, nekos!");
    println!("nekos - Microkernel Operating System");
    println!("kernel_main = 0x{:x}", kernel_main as usize);

    kernel::page::init();
    
    kernel::vm::init();

    drivers::plic::init();

    // 初始化内嵌用户程序归档
    kernel::initrd::init();
    
    kernel::task::init();
    
    kernel::trap::init();
    
    kernel::timer::init();
    
    // PID 0 是 S-mode idle，只在没有普通 Ready 任务时运行。
    kernel::idle::create_idle().expect("Failed to create idle task");

    let console_pid = kernel::exec::spawn("console")
        .expect("Failed to load console server");
    kernel::task::grant_uart(console_pid).expect("Failed to grant UART to console server");
    kernel::ipc::register(kernel::ipc::CONSOLE_ENDPOINT, console_pid)
        .expect("Failed to register console endpoint");

    let fs_pid = kernel::exec::spawn("fs-server")
        .expect("Failed to load fs-server");
    kernel::ipc::register(kernel::ipc::FS_ENDPOINT, fs_pid)
        .expect("Failed to register fs-server endpoint");
    println!("fs-server pid = {}", fs_pid);
    
    let shell_pid = kernel::exec::spawn("shell")
        .expect("Failed to load shell");
    
    println!("microkernel: console_pid={} fs_pid={} shell_pid={}", console_pid, fs_pid, shell_pid);

    // 进入第一个用户任务；之后的切换都由调度器完成。
    unsafe {
        kernel::task::enter_task(shell_pid);
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
