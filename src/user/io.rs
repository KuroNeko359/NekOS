//! 用户态标准输入输出接口，底层通过 Console Server IPC 实现。

use crate::user::ipc;

const CONSOLE_ENDPOINT: usize = 1;
const CONSOLE_WRITE: usize = 1;
const CONSOLE_READ: usize = 2;

/// 向标准输出（fd 1）或标准错误（fd 2）写入字节。
pub fn write(fd: usize, buf: &[u8]) -> isize {
    if fd != 1 && fd != 2 {
        return -1;
    }

    for &byte in buf {
        if ipc::call(CONSOLE_ENDPOINT, [CONSOLE_WRITE, byte as usize, 0, 0]).is_err() {
            return -1;
        }
    }

    buf.len() as isize
}
/// 从标准输入（fd 0）读取字节，遇到换行或填满缓冲区时返回。
pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    if fd != 0 {
        return -1;
    }

    for (index, slot) in buf.iter_mut().enumerate() {
        let reply = match ipc::call(
            CONSOLE_ENDPOINT,
            [CONSOLE_READ, 0, 0, 0],
        ) {
            Ok(reply) => reply,
            Err(()) => return -1,
        };

        let byte = reply[0] as u8;
        *slot = byte;

        if byte == b'\n' {
            return (index + 1) as isize;
        }
    }

    buf.len() as isize
}
