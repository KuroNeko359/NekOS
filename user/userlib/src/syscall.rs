//! nekos 用户态系统调用封装。

const SYS_EXIT: usize = 2;
const SYS_YIELD: usize = 4;
const SYS_GETPID: usize = 5;
const SYS_FORK: usize = 6;
const SYS_PS: usize = 7;
const SYS_EXEC: usize = 8;
const SYS_WAITPID: usize = 9;
const SYS_IRQ_WAIT: usize = 13;

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
