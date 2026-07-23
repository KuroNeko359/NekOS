//! 用户态 Console Server。只有该任务拥有 UART MMIO 权限。

const UART_BASE: usize = 0x1000_0000;
const RBR: usize = 0;
const THR: usize = 0;
const LSR: usize = 5;

const ENDPOINT: usize = 1;
const WRITE: usize = 1;
const READ: usize = 2;

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
        irq_wait();
    }
}

fn irq_wait() {
    let mut result = 10usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") result,
            in("a7") 13usize,
        );
    }
    if result == usize::MAX {
        panic!("console irq_wait failed");
    }
}

fn recv() -> (u32, [usize; 4]) {
    crate::user::ipc::recv(ENDPOINT).expect("console ipc_recv failed")
}

fn reply(client: u32, words: [usize; 4]) {
    let _ = crate::user::ipc::reply(client, words);
}

#[no_mangle]
pub extern "C" fn console_server_main() -> ! {
    loop {
        let (client, words) = recv();
        match words[0] {
            WRITE => {
                let byte = words[1] as u8;
                if byte == b'\n' { putc(b'\r'); }
                putc(byte);
                reply(client, [1, 0, 0, 0]);
            }
            READ => {
                let byte = match getc() {
                    b'\r' => b'\n',
                    byte => byte,
                };
                reply(client, [byte as usize, 0, 0, 0]);
            }
            _ => reply(client, [usize::MAX, 0, 0, 0]),
        }
    }
}
