//! 用户空间代码

pub mod shell;
pub mod console;
pub mod ipc;
pub mod io;

/// 用户程序入口点
extern "C" {
    pub fn user_main();
}
