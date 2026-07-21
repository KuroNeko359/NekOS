//! RISC-V CSR (控制状态寄存器) 操作

/// 读取CSR寄存器
#[macro_export]
macro_rules! read_csr {
    ($csr:expr) => {
        {
            let val: usize;
            unsafe {
                core::arch::asm!(
                    "csrr {rd}, {csr}",
                    rd = out(reg) val,
                    csr = const $csr,
                    options(nomem, nostack),
                );
            }
            val
        }
    };
}

/// 写入CSR寄存器
#[macro_export]
macro_rules! write_csr {
    ($csr:expr, $val:expr) => {
        unsafe {
            core::arch::asm!(
                "csrw {csr}, {rs}",
                csr = const $csr,
                rs = in(reg) $val,
                options(nomem, nostack),
            );
        }
    };
}

/// 设置CSR位
#[macro_export]
macro_rules! set_csr {
    ($csr:expr, $val:expr) => {
        unsafe {
            core::arch::asm!(
                "csrs {csr}, {rs}",
                csr = const $csr,
                rs = in(reg) $val,
                options(nomem, nostack),
            );
        }
    };
}

/// 清除CSR位
#[macro_export]
macro_rules! clear_csr {
    ($csr:expr, $val:expr) => {
        unsafe {
            core::arch::asm!(
                "csrc {csr}, {rs}",
                csr = const $csr,
                rs = in(reg) $val,
                options(nomem, nostack),
            );
        }
    };
}

/// 常用CSR寄存器地址
pub const SSTATUS: usize = 0x100;
pub const SIE: usize = 0x104;
pub const STVEC: usize = 0x105;
pub const SSCRATCH: usize = 0x140;
pub const SEPC: usize = 0x141;
pub const SCAUSE: usize = 0x142;
pub const STVAL: usize = 0x143;
pub const SIP: usize = 0x144;
pub const SATP: usize = 0x180;

/// sstatus 位定义
pub const SSTATUS_SIE: usize = 1 << 1;
pub const SSTATUS_SPIE: usize = 1 << 5;
pub const SSTATUS_SPP: usize = 1 << 8;
pub const SSTATUS_SUM: usize = 1 << 18;

/// 读取sstatus
pub fn read_sstatus() -> usize {
    read_csr!(SSTATUS)
}

/// 写入sstatus
pub fn write_sstatus(val: usize) {
    write_csr!(SSTATUS, val);
}

/// 读取stvec
pub fn read_stvec() -> usize {
    read_csr!(STVEC)
}

/// 写入stvec
pub fn write_stvec(val: usize) {
    write_csr!(STVEC, val);
}

/// 读取sscratch
pub fn read_sscratch() -> usize {
    read_csr!(SSCRATCH)
}

/// 写入sscratch
pub fn write_sscratch(val: usize) {
    write_csr!(SSCRATCH, val);
}

/// 读取sepc
pub fn read_sepc() -> usize {
    read_csr!(SEPC)
}

/// 写入sepc
pub fn write_sepc(val: usize) {
    write_csr!(SEPC, val);
}

/// 读取scause
pub fn read_scause() -> usize {
    read_csr!(SCAUSE)
}

/// 读取stval
pub fn read_stval() -> usize {
    read_csr!(STVAL)
}

/// 读取satp
pub fn read_satp() -> usize {
    read_csr!(SATP)
}

/// 写入satp
pub fn write_satp(val: usize) {
    write_csr!(SATP, val);
}

/// 刷新TLB
pub fn sfence_vma() {
    unsafe {
        core::arch::asm!("sfence.vma", options(nomem, nostack));
    }
}

/// 启用中断
pub fn enable_interrupts() {
    set_csr!(SSTATUS, SSTATUS_SIE);
}

/// 禁用中断
pub fn disable_interrupts() {
    clear_csr!(SSTATUS, SSTATUS_SIE);
}

/// 检查中断是否启用
pub fn interrupts_enabled() -> bool {
    (read_sstatus() & SSTATUS_SIE) != 0
}
