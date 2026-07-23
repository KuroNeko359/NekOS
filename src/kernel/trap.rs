//! 陷阱处理

use crate::arch::riscv::csr;
use crate::println;

/// 陷阱帧结构 (必须与汇编代码中的保存顺序一致)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TrapFrame {
    pub ra: usize,       // x1
    pub sp: usize,       // x2
    pub gp: usize,       // x3
    pub tp: usize,       // x4
    pub t0: usize,       // x5
    pub t1: usize,       // x6
    pub t2: usize,       // x7
    pub s0: usize,       // x8
    pub s1: usize,       // x9
    pub a0: usize,       // x10
    pub a1: usize,       // x11
    pub a2: usize,       // x12
    pub a3: usize,       // x13
    pub a4: usize,       // x14
    pub a5: usize,       // x15
    pub a6: usize,       // x16
    pub a7: usize,       // x17
    pub s2: usize,       // x18
    pub s3: usize,       // x19
    pub s4: usize,       // x20
    pub s5: usize,       // x21
    pub s6: usize,       // x22
    pub s7: usize,       // x23
    pub s8: usize,       // x24
    pub s9: usize,       // x25
    pub s10: usize,      // x26
    pub s11: usize,      // x27
    pub t3: usize,       // x28
    pub t4: usize,       // x29
    pub t5: usize,       // x30
    pub t6: usize,       // x31
    pub sstatus: usize,  // sstatus
    pub sepc: usize,     // sepc
}

impl TrapFrame {
    /// 创建新的空陷阱帧
    pub fn new() -> Self {
        Self {
            ra: 0, sp: 0, gp: 0, tp: 0,
            t0: 0, t1: 0, t2: 0, s0: 0,
            s1: 0, a0: 0, a1: 0, a2: 0,
            a3: 0, a4: 0, a5: 0, a6: 0,
            a7: 0, s2: 0, s3: 0, s4: 0,
            s5: 0, s6: 0, s7: 0, s8: 0,
            s9: 0, s10: 0, s11: 0, t3: 0,
            t4: 0, t5: 0, t6: 0,
            sstatus: 0, sepc: 0,
        }
    }
}

/// 陷阱原因
#[derive(Debug, Clone, Copy)]
pub enum TrapCause {
    /// 用户态系统调用
    UserEcall,
    /// 定时器中断
    Timer,
    /// 外部中断
    External,
    /// 指令页错误
    InstructionPageFault,
    /// 加载页错误
    LoadPageFault,
    /// 存储页错误
    StorePageFault,
    /// 未知原因
    Unknown(usize),
}

/// 解析陷阱原因
pub fn parse_cause(scause: usize) -> TrapCause {
    let is_interrupt = (scause >> 63) != 0;
    let code = scause & 0x7FFF_FFFF;
    
    if is_interrupt {
        match code {
            5 => TrapCause::Timer,
            9 => TrapCause::External,
            _ => TrapCause::Unknown(scause),
        }
    } else {
        match code {
            8 => TrapCause::UserEcall,
            12 => TrapCause::InstructionPageFault,
            13 => TrapCause::LoadPageFault,
            15 => TrapCause::StorePageFault,
            _ => TrapCause::Unknown(scause),
        }
    }
}

/// 初始化陷阱处理
pub fn init() {
    unsafe {
        // 设置陷阱入口点
        let trap_entry_addr = trap_entry as usize;
        csr::write_stvec(trap_entry_addr);
        
        // 启用中断
        crate::set_csr!(csr::SSTATUS, csr::SSTATUS_SIE);
        
        // 启用定时器和外部中断
        crate::write_csr!(csr::SIE, (1 << 5) | (1 << 9));
        
        println!("trap: enabled, entry=0x{:x}", trap_entry_addr);
    }
}

/// 陷阱处理函数 (从汇编调用)
#[no_mangle]
pub extern "C" fn trap_handler(tf: &mut TrapFrame) -> *mut TrapFrame {
    let scause = csr::read_scause();
    let stval = csr::read_stval();
    
    let cause = parse_cause(scause);
    
    match cause {
        TrapCause::UserEcall => {
            return crate::kernel::syscall::handle(tf);
        }
        TrapCause::Timer => {
            crate::kernel::timer::handle();
            return crate::kernel::task::schedule(tf);
        }
        TrapCause::External => {
            let irq = crate::drivers::plic::claim();
            if irq == crate::drivers::plic::UART0_IRQ {
                // UART RX 是电平中断。先屏蔽，待 Console 再次 irq_wait 时重开，
                // 避免它读取 RBR 前不断重新触发。
                crate::drivers::plic::disable(irq);
                crate::kernel::task::wake_uart();
            } else if irq != 0 {
                println!("unhandled external irq {}", irq);
            }
            if irq != 0 {
                crate::drivers::plic::complete(irq);
            }
            return crate::kernel::task::schedule(tf);
        }
        TrapCause::InstructionPageFault => {
            println!("instruction page fault at 0x{:x}, sepc=0x{:x}", stval, tf.sepc);
            panic!("instruction page fault");
        }
        TrapCause::LoadPageFault => {
            println!("load page fault at 0x{:x}, sepc=0x{:x}", stval, tf.sepc);
            panic!("load page fault");
        }
        TrapCause::StorePageFault => {
            println!("store page fault at 0x{:x}, sepc=0x{:x}", stval, tf.sepc);
            panic!("store page fault");
        }
        TrapCause::Unknown(cause) => {
            println!("unknown trap: cause=0x{:x} stval=0x{:x}", cause, stval);
            panic!("unknown trap");
        }
    }
}

/// 外部汇编函数
extern "C" {
    pub fn trap_entry();
    pub fn trap_return();
}
