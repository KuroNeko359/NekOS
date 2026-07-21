//! RISC-V 架构特定代码

pub mod csr;
pub mod sbi;

/// RISC-V 特权级
pub const PRV_U: usize = 0;
pub const PRV_S: usize = 1;
pub const PRV_M: usize = 3;

/// 页面大小
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;

/// Sv39 虚拟地址位数
pub const VA_BITS: usize = 39;
pub const PA_BITS: usize = 56;

/// 页表项标志位
pub const PTE_V: usize = 1 << 0;  // Valid
pub const PTE_R: usize = 1 << 1;  // Read
pub const PTE_W: usize = 1 << 2;  // Write
pub const PTE_X: usize = 1 << 3;  // Execute
pub const PTE_U: usize = 1 << 4;  // User
pub const PTE_G: usize = 1 << 5;  // Global
pub const PTE_A: usize = 1 << 6;  // Accessed
pub const PTE_D: usize = 1 << 7;  // Dirty

/// Sv39 页表级别数
pub const PT_LEVELS: usize = 3;

/// VPN 位数 (每个级别9位)
pub const VPN_BITS: usize = 9;

/// PPN 位数
pub const PPN_BITS: usize = 9;

/// 用户空间布局
pub const USER_TEXT_BASE: usize = 0x0001_0000;
pub const USER_STACK_TOP: usize = 0x4000_0000;
pub const USER_STACK_SIZE: usize = PAGE_SIZE;

/// 内核空间布局
pub const KERNEL_BASE: usize = 0x8000_0000;
pub const KERNEL_ENTRY: usize = 0x8020_0000;
