//! nekos 用户态同步 IPC 接口。

const SYS_IPC_CALL: usize = 10;
const SYS_IPC_RECV: usize = 11;
const SYS_IPC_REPLY: usize = 12;

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
