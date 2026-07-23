use core::arch::asm;

#[no_mangle]
pub extern "C" fn idle_main() -> ! {
    loop {
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }
}

/// 注册固定 PID 0 的 S-mode idle 任务。
pub fn create_idle() -> Result<u32, ()> {
    crate::kernel::task::create_idle(idle_main as usize)
}
