//! 定时器处理

use crate::arch::riscv::sbi;
use crate::println;

/// 定时器间隔 (约100ms)
const TIMER_INTERVAL: u64 = 1000000;

/// 下一次定时器中断时间
static mut NEXT_TIMER: u64 = 0;

/// 初始化定时器
pub fn init() {
    unsafe {
        // 读取当前时间
        let time: u64;
        core::arch::asm!("csrr {}, time", out(reg) time);
        
        // 设置第一次定时器中断
        NEXT_TIMER = time + TIMER_INTERVAL;
        sbi::set_timer(NEXT_TIMER);
        
        println!("timer: enabled, interval={}", TIMER_INTERVAL);
    }
}

/// 处理定时器中断
pub fn handle() {
    unsafe {
        // 设置下一次定时器中断
        NEXT_TIMER += TIMER_INTERVAL;
        sbi::set_timer(NEXT_TIMER);
    }
    
    // TODO: 实现进程调度
}
