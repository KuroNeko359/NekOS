#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[path = "../src/user/ipc.rs"]
pub mod ipc_impl;
#[path = "../src/user/io.rs"]
pub mod io_impl;

pub mod user {
    pub use crate::io_impl as io;
    pub use crate::ipc_impl as ipc;
}

#[path = "../src/user/shell.rs"]
mod shell_impl;

core::arch::global_asm!(
    r#"
    .section .text.start
    .globl _start
_start:
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    call user_main
    li a0, 0
    li a7, 2
    ecall
1:
    j 1b
"#
);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") 1usize,
            in("a7") 2usize,
        );
    }
    loop {
        core::hint::spin_loop();
    }
}
