//! 用户态同步 IPC 系统调用封装。

pub const IPC_ERROR: usize = usize::MAX;

/// 向端点发送四个机器字，并阻塞到服务端回复。
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
            in("a7") 10usize,
        );
    }
    if a0 == IPC_ERROR { Err(()) } else { Ok([a0, a1, a2, a3]) }
}

/// 接收发往端点的请求；没有请求时阻塞。
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
            in("a7") 11usize,
        );
    }
    if a0 == IPC_ERROR {
        Err(())
    } else {
        Ok((a0 as u32, [a1, a2, a3, a4]))
    }
}

/// 回复一个因 `call` 阻塞的客户端。
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
            in("a7") 12usize,
        );
    }
    if result == 0 { Ok(()) } else { Err(()) }
}
