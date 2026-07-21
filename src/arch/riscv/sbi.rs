//! SBI (Supervisor Binary Interface) 接口

/// SBI 调用结果
#[derive(Debug, Clone, Copy)]
pub struct SbiResult {
    pub error: isize,
    pub value: isize,
}

/// SBI 调用
fn sbi_call(eid: usize, fid: usize, arg0: usize, arg1: usize, arg2: usize) -> SbiResult {
    let error: isize;
    let value: isize;
    
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") arg0 => error,
            inlateout("a1") arg1 => value,
            in("a2") arg2,
            in("a6") fid,
            in("a7") eid,
            options(nomem, nostack),
        );
    }
    
    SbiResult { error, value }
}

/// 打印字符到控制台
pub fn console_putchar(c: u8) {
    sbi_call(1, 0, c as usize, 0, 0);
}

/// 从控制台读取字符
pub fn console_getchar() -> Option<u8> {
    let result = sbi_call(2, 0, 0, 0, 0);
    if result.error >= 0 {
        Some(result.error as u8)
    } else {
        None
    }
}

/// 关机
pub fn shutdown() -> ! {
    sbi_call(8, 0, 0, 0, 0);
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// 重启
pub fn reboot() -> ! {
    sbi_call(8, 1, 0, 0, 0);
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// 设置定时器
pub fn set_timer(stime_value: u64) {
    sbi_call(0, 0, stime_value as usize, 0, 0);
}
