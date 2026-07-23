//! 系统调用处理

use crate::kernel::trap::TrapFrame;
use crate::drivers::uart;
use crate::println;

/// 系统调用号
pub const SYS_WRITE: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_READ: usize = 3;
pub const SYS_YIELD: usize = 4;
pub const SYS_GETPID: usize = 5;
pub const SYS_FORK: usize = 6;
pub const SYS_PS: usize = 7;
pub const SYS_EXEC: usize = 8;
pub const SYS_WAITPID: usize = 9;
pub const SYS_IPC_CALL: usize = 10;
pub const SYS_IPC_RECV: usize = 11;
pub const SYS_IPC_REPLY: usize = 12;

/// 系统调用处理
pub fn handle(tf: &mut TrapFrame) -> *mut TrapFrame {
    let syscall_num = tf.a7;
    if syscall_num == SYS_READ && !uart::has_data() {
        return crate::kernel::task::schedule(tf);
    }
    tf.sepc += 4;
    match syscall_num {
        SYS_WRITE => {
            tf.a0 = sys_write(tf.a0, tf.a1, tf.a2);
        }
        SYS_READ => {
            tf.a0 = sys_read(tf.a0, tf.a1, tf.a2);
        }
        SYS_EXIT => {
            return sys_exit(tf.a0 as i32);
        }
        SYS_YIELD => {
            tf.a0 = 0;
            return crate::kernel::task::schedule(tf);
        }
        SYS_GETPID => {
            tf.a0 = sys_getpid() as usize;
        }
        SYS_FORK => {
            let result = crate::kernel::task::fork(tf)
                .map(|pid| pid as usize)
                .unwrap_or(!0usize);
            tf.a0 = result;
        }
        SYS_PS => {
            sys_ps();
            tf.a0 = 0;
        }
        SYS_EXEC => {
            tf.a0 = sys_exec(tf.a0, tf.a1, tf);
        }
        SYS_WAITPID => {
            let pid = tf.a0 as u32;
            match crate::kernel::task::waitpid(pid, tf) {
                crate::kernel::task::WaitResult::Reaped(exit_code) => {
                    tf.a0 = exit_code as usize;
                }
                crate::kernel::task::WaitResult::Blocked(next) => return next,
                crate::kernel::task::WaitResult::Error => tf.a0 = usize::MAX,
            }
        }
        SYS_IPC_CALL => {
            let endpoint = tf.a0;
            let words = [tf.a1, tf.a2, tf.a3, tf.a4];
            match crate::kernel::ipc::call(endpoint, words, tf) {
                crate::kernel::ipc::IpcResult::Continue => {}
                crate::kernel::ipc::IpcResult::Blocked(next) => return next,
                crate::kernel::ipc::IpcResult::Error => tf.a0 = usize::MAX,
            }
        }
        SYS_IPC_RECV => {
            match crate::kernel::ipc::recv(tf.a0, tf) {
                crate::kernel::ipc::IpcResult::Continue => {}
                crate::kernel::ipc::IpcResult::Blocked(next) => return next,
                crate::kernel::ipc::IpcResult::Error => tf.a0 = usize::MAX,
            }
        }
        SYS_IPC_REPLY => {
            let result = crate::kernel::ipc::reply(
                tf.a0 as u32,
                [tf.a1, tf.a2, tf.a3, tf.a4],
            );
            tf.a0 = if result.is_ok() { 0 } else { usize::MAX };
        }
        _ => {
            println!("unknown syscall: {}", syscall_num);
            panic!("unknown syscall");
        }
    }
    tf as *mut TrapFrame
}

/// 写入系统调用
fn sys_write(fd: usize, buf: usize, len: usize) -> usize {
    if fd != 1 && fd != 2 {
        return !0usize;  // -1
    }
    
    for i in 0..len {
        let Some(pa) = crate::kernel::task::translate_user(buf + i) else {
            return !0usize;
        };
        let c = unsafe { core::ptr::read(pa as *const u8) };
        if c == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(c);
    }
    
    len
}

/// 读取系统调用
fn sys_read(fd: usize, buf: usize, len: usize) -> usize {
    if fd != 0 {
        return !0usize;  // -1
    }
    
    if len == 0 {
        return 0;
    }
    
    let mut i = 0;
    
    while i + 1 < len {
        let c = match uart::getc() {
            Some(c) => c,
            None => {
                // 没有数据，让出CPU
                // TODO: 实现正确的阻塞
                continue;
            }
        };
        
        // 处理特殊字符
        let c = if c == b'\r' { b'\n' } else { c };
        
        if c == 0x7f || c == 8 {
            // 退格
            if i > 0 {
                i -= 1;
                uart::puts("\x08 \x08");
            }
            continue;
        }
        
        let Some(pa) = crate::kernel::task::translate_user(buf + i) else { return !0usize; };
        unsafe { core::ptr::write(pa as *mut u8, c); }
        
        // 回显
        if c == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(c);
        
        i += 1;
        
        if c == b'\n' {
            break;
        }
    }
    
    // 添加null终止符
    let Some(pa) = crate::kernel::task::translate_user(buf + i) else { return !0usize; };
    unsafe { core::ptr::write(pa as *mut u8, 0); }
    
    i
}

/// 退出系统调用
fn sys_exit(code: i32) -> *mut TrapFrame {
    println!("user exited with code {}", code);
    crate::kernel::task::exit_current(code)
}

fn sys_exec(filename_va: usize, requested_len: usize, tf: &mut TrapFrame) -> usize {
    let mut bytes = [0u8; 64];
    let mut len = 0usize;
    while len < requested_len.min(bytes.len() - 1) {
        let Some(pa) = crate::kernel::task::translate_user(filename_va + len) else {
            return !0usize;
        };
        let byte = unsafe { core::ptr::read(pa as *const u8) };
        bytes[len] = byte;
        len += 1;
    }
    let Ok(name) = core::str::from_utf8(&bytes[..len]) else { return !0usize; };
    crate::kernel::exec::exec(name, tf).map(|_| 0).unwrap_or(!0usize)
}

/// 让出CPU系统调用
/// 获取PID系统调用
fn sys_getpid() -> u32 {
    crate::kernel::task::current_pid()
}

/// 列出进程系统调用
fn sys_ps() {
    crate::kernel::task::list_all();
}
