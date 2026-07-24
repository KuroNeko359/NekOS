//! nekos 用户态同步 IPC 接口。

const SYS_IPC_CALL: usize = 10;
const SYS_IPC_RECV: usize = 11;
const SYS_IPC_REPLY: usize = 12;
const SYS_IPC_CALL_BUF: usize = 15;
const SYS_IPC_RECV_BUF: usize = 16;
const SYS_IPC_REPLY_BUF: usize = 17;

pub const ERROR: usize = usize::MAX;

pub fn call(endpoint: usize, words: [usize; 4]) -> Result<[usize; 4], ()> {
    let mut a0 = endpoint;
    let mut a1 = words[0];
    let mut a2 = words[1];
    let mut a3 = words[2];
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0,
            inlateout("a1") a1,
            inlateout("a2") a2,
            inlateout("a3") a3,
            in("a4") words[3],
            in("a7") SYS_IPC_CALL,
        );
    }
    if a0 == ERROR { Err(()) } else { Ok([a0, a1, a2, a3]) }
}

pub fn recv(endpoint: usize) -> Result<(u32, [usize; 4]), ()> {
    let mut a0 = endpoint;
    let a1: usize;
    let a2: usize;
    let a3: usize;
    let a4: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0,
            lateout("a1") a1,
            lateout("a2") a2,
            lateout("a3") a3,
            lateout("a4") a4,
            in("a7") SYS_IPC_RECV,
        );
    }
    if a0 == ERROR {
        Err(())
    } else {
        Ok((a0 as u32, [a1, a2, a3, a4]))
    }
}

pub fn reply(client: u32, words: [usize; 4]) -> Result<(), ()> {
    let mut result = client as usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") result,
            in("a1") words[0],
            in("a2") words[1],
            in("a3") words[2],
            in("a4") words[3],
            in("a7") SYS_IPC_REPLY,
        );
    }
    if result == 0 { Ok(()) } else { Err(()) }
}

/// 带缓冲区的 ipc_recv：接收消息，如果有附带数据则复制到 buf。
/// 返回 (client, words, out_len)。
pub fn recv_buf(endpoint: usize, buf: &mut [u8]) -> Result<(u32, [usize; 4], usize), ()> {
    let mut a0 = endpoint;
    let mut a1: usize = buf.as_mut_ptr() as usize;
    let mut a2: usize = buf.len();
    let a3: usize;
    let a4: usize;
    let a5: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0,
            inlateout("a1") a1,
            inlateout("a2") a2,
            lateout("a3") a3,
            lateout("a4") a4,
            lateout("a5") a5,
            in("a7") SYS_IPC_RECV_BUF,
        );
    }
    if a0 == ERROR {
        Err(())
    } else {
        Ok((a0 as u32, [a1, a2, a3, a4], a5))
    }
}

/// 带缓冲区的 ipc_call：发送 words 和 buf。
pub fn call_buf(endpoint: usize, words: [usize; 4], buf: &[u8]) -> Result<[usize; 4], ()> {
    let mut a0 = endpoint;
    let mut a1 = words[0];
    let mut a2 = words[1];
    let mut a3 = words[2];
    let mut a4 = words[3];
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0,
            inlateout("a1") a1,
            inlateout("a2") a2,
            inlateout("a3") a3,
            inlateout("a4") a4,
            in("a5") buf.as_ptr() as usize,
            in("a6") buf.len(),
            in("a7") SYS_IPC_CALL_BUF,
        );
    }
    if a0 == ERROR { Err(()) } else { Ok([a0, a1, a2, a3]) }
}

/// 带缓冲区的 ipc_reply：回复 words + 将 buf[0..buf_len] 复制到 caller 的用户空间。
pub fn reply_buf(client: u32, words: [usize; 4], buf: &[u8]) -> Result<(), ()> {
    let mut result = client as usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") result,
            in("a1") words[0],
            in("a2") words[1],
            in("a3") words[2],
            in("a4") words[3],
            in("a5") buf.as_ptr() as usize,
            in("a6") buf.len(),
            in("a7") SYS_IPC_REPLY_BUF,
        );
    }
    if result == 0 { Ok(()) } else { Err(()) }
}
