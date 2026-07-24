//! nekos 用户态系统调用封装。

const SYS_EXIT: usize = 2;
const SYS_YIELD: usize = 4;
const SYS_GETPID: usize = 5;
const SYS_FORK: usize = 6;
const SYS_PS: usize = 7;
const SYS_EXEC: usize = 8;
const SYS_WAITPID: usize = 9;
const SYS_IRQ_WAIT: usize = 13;
const SYS_SBRK: usize = 14;
const SYS_IPC_CALL_BUF: usize = 15;
const SYS_IPC_RECV_BUF: usize = 16;

pub const ERROR: isize = -1;
pub const UART0_IRQ: usize = 10;

pub fn exit(code: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") code as usize,
            in("a7") SYS_EXIT,
        );
    }
    loop {
        core::hint::spin_loop();
    }
}

pub fn yield_now() {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") SYS_YIELD,
        );
    }
}

pub fn getpid() -> u32 {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") 0usize => result,
            in("a7") SYS_GETPID,
        );
    }
    result as u32
}

pub fn fork() -> isize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") 0usize => result,
            in("a7") SYS_FORK,
        );
    }
    result as isize
}

pub fn ps() {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") 0usize,
            in("a7") SYS_PS,
        );
    }
}

pub fn exec(filename: &str) -> isize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") filename.as_ptr() as usize => result,
            in("a1") filename.len(),
            in("a7") SYS_EXEC,
        );
    }
    result as isize
}

pub fn waitpid(pid: u32) -> isize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") pid as usize => result,
            in("a7") SYS_WAITPID,
        );
    }
    result as isize
}

pub fn irq_wait(irq: usize) -> Result<(), ()> {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") irq => result,
            in("a7") SYS_IRQ_WAIT,
        );
    }
    if result == usize::MAX { Err(()) } else { Ok(()) }
}

pub fn sbrk(increment: isize) -> Result<usize, ()> {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") increment as usize => result,
            in("a7") SYS_SBRK,
        );
    }
    if result == usize::MAX { Err(()) } else { Ok(result) }
}

/// 缓冲区版 ipc_call：发送 words 和 buf[0..buf_len]。
pub fn ipc_call_buf(
    endpoint: usize,
    words: [usize; 4],
    buf: *const u8,
    buf_len: usize,
) -> isize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") endpoint => result,
            in("a1") words[0],
            in("a2") words[1],
            in("a3") words[2],
            in("a4") words[3],
            in("a5") buf as usize,
            in("a6") buf_len,
            in("a7") SYS_IPC_CALL_BUF,
        );
    }
    result as isize
}

/// 缓冲区版 ipc_recv：接收消息，如果有附带数据则复制到 buf[0..capacity]。
/// 返回值：成功返回 0，错误返回 -1。接收到的缓冲区长度写在 a5 寄存器中。
pub fn ipc_recv_buf(
    endpoint: usize,
    buf: *mut u8,
    capacity: usize,
) -> (isize, usize) {
    let result: usize;
    let out_len: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") endpoint => result,
            in("a1") buf as usize,
            in("a2") capacity,
            lateout("a5") out_len,
            in("a7") SYS_IPC_RECV_BUF,
        );
    }
    (result as isize, out_len)
}
