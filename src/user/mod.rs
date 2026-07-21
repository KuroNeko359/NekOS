//! 用户空间代码

pub mod shell;

/// 用户程序入口点
extern "C" {
    pub fn user_main();
}
