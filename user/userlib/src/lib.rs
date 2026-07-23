#![no_std]

pub mod io;
pub mod ipc;
pub mod syscall;

mod print;

pub use print::_print;

/// 为用户程序生成 `_start` 和 panic handler。
///
/// 用户程序只需要定义普通的 `fn main()`，然后调用 `entry!(main)`。
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        core::arch::global_asm!(
            r#"
            .section .text.start
            .globl _start
        _start:
            .option push
            .option norelax
            la gp, __global_pointer$
            .option pop
            call __user_main
        1:
            j 1b
            "#
        );

        #[allow(unreachable_code)]
        #[no_mangle]
        extern "C" fn __user_main() -> ! {
            $main();
            $crate::syscall::exit(0)
        }

        #[panic_handler]
        fn __user_panic(_info: &core::panic::PanicInfo) -> ! {
            $crate::syscall::exit(1)
        }
    };
}
