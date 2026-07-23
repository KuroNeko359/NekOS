#![no_std]
#![no_main]

use userlib::{entry, io, ipc, syscall};

const UART_BASE: usize = 0x1000_0000;
const RBR: usize = 0;
const THR: usize = 0;
const LSR: usize = 5;

unsafe fn read_reg(offset: usize) -> u8 {
    core::ptr::read_volatile((UART_BASE + offset) as *const u8)
}

unsafe fn write_reg(offset: usize, value: u8) {
    core::ptr::write_volatile((UART_BASE + offset) as *mut u8, value);
}

fn putc(byte: u8) {
    unsafe {
        while read_reg(LSR) & 0x20 == 0 {}
        write_reg(THR, byte);
    }
}

fn getc() -> u8 {
    loop {
        unsafe {
            if read_reg(LSR) & 0x01 != 0 {
                return read_reg(RBR);
            }
        }
        syscall::irq_wait(syscall::UART0_IRQ).expect("console irq_wait failed");
    }
}

fn main() -> ! {
    loop {
        let (client, words) =
            ipc::recv(io::CONSOLE_ENDPOINT).expect("console ipc_recv failed");
        match words[0] {
            io::CONSOLE_WRITE => {
                let byte = words[1] as u8;
                if byte == b'\n' {
                    putc(b'\r');
                }
                putc(byte);
                let _ = ipc::reply(client, [1, 0, 0, 0]);
            }
            io::CONSOLE_READ => {
                let byte = match getc() {
                    b'\r' => b'\n',
                    byte => byte,
                };
                let _ = ipc::reply(client, [byte as usize, 0, 0, 0]);
            }
            _ => {
                let _ = ipc::reply(client, [usize::MAX, 0, 0, 0]);
            }
        }
    }
}

entry!(main);
