//! QEMU virt 平台的 RISC-V PLIC 驱动。

use core::ptr::{read_volatile, write_volatile};

const PLIC_BASE: usize = 0x0c00_0000;
const PRIORITY_BASE: usize = PLIC_BASE;
const SENABLE_BASE: usize = PLIC_BASE + 0x2080;
const SCONTEXT_BASE: usize = PLIC_BASE + 0x20_1000;

pub const UART0_IRQ: u32 = 10;

#[inline]
unsafe fn read32(address: usize) -> u32 {
    read_volatile(address as *const u32)
}

#[inline]
unsafe fn write32(address: usize, value: u32) {
    write_volatile(address as *mut u32, value);
}

/// 初始化 hart 0 的 S-mode PLIC context。设备 IRQ 默认保持屏蔽。
pub fn init() {
    unsafe {
        write32(PRIORITY_BASE + UART0_IRQ as usize * 4, 1);
        write32(SENABLE_BASE, 0);
        write32(SCONTEXT_BASE, 0);
    }
}

pub fn enable(irq: u32) {
    let register = SENABLE_BASE + (irq as usize / 32) * 4;
    let bit = 1u32 << (irq % 32);
    unsafe {
        write32(register, read32(register) | bit);
    }
}

pub fn disable(irq: u32) {
    let register = SENABLE_BASE + (irq as usize / 32) * 4;
    let bit = 1u32 << (irq % 32);
    unsafe {
        write32(register, read32(register) & !bit);
    }
}

pub fn claim() -> u32 {
    unsafe { read32(SCONTEXT_BASE + 4) }
}

pub fn complete(irq: u32) {
    unsafe {
        write32(SCONTEXT_BASE + 4, irq);
    }
}
