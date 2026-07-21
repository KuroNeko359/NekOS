//! UART 驱动 (NS16550A)

use core::fmt;
use spin::Mutex;

/// UART 基地址 (QEMU virt 机器)
const UART_BASE: usize = 0x1000_0000;

/// UART 寄存器偏移
const RBR: usize = 0;  // 接收缓冲寄存器
const THR: usize = 0;  // 发送保持寄存器
const IER: usize = 1;  // 中断使能寄存器
const FCR: usize = 2;  // FIFO控制寄存器
const LCR: usize = 3;  // 线路控制寄存器
const MCR: usize = 4;  // 调制解调器控制寄存器
const LSR: usize = 5;  // 线路状态寄存器

/// UART 驱动
pub struct Uart {
    base: usize,
}

impl Uart {
    /// 创建新的UART实例
    pub const fn new(base: usize) -> Self {
        Self { base }
    }
    
    /// 读取寄存器
    unsafe fn read_reg(&self, offset: usize) -> u8 {
        core::ptr::read_volatile((self.base + offset) as *const u8)
    }
    
    /// 写入寄存器
    unsafe fn write_reg(&self, offset: usize, val: u8) {
        core::ptr::write_volatile((self.base + offset) as *mut u8, val);
    }
    
    /// 初始化UART
    pub fn init(&self) {
        unsafe {
            // 禁用中断
            self.write_reg(IER, 0x00);
            
            // 启用FIFO
            self.write_reg(FCR, 0x01);
            
            // 设置波特率 (9600)
            self.write_reg(LCR, 0x80);  // 设置DLAB位
            self.write_reg(0, 0x0C);    // 低字节
            self.write_reg(1, 0x00);    // 高字节
            
            // 8位数据，无校验，1位停止
            self.write_reg(LCR, 0x03);
            
            // 启用RTS/DSR
            self.write_reg(MCR, 0x03);
            
            // 启用接收中断
            self.write_reg(IER, 0x01);
        }
    }
    
    /// 发送字符
    pub fn putc(&self, c: u8) {
        unsafe {
            // 等待发送缓冲区为空
            while (self.read_reg(LSR) & 0x20) == 0 {}
            
            // 发送字符
            self.write_reg(THR, c);
        }
    }
    
    /// 接收字符
    pub fn getc(&self) -> Option<u8> {
        unsafe {
            // 检查是否有数据
            if (self.read_reg(LSR) & 0x01) != 0 {
                Some(self.read_reg(RBR))
            } else {
                None
            }
        }
    }
    
    /// 检查是否有数据
    pub fn has_data(&self) -> bool {
        unsafe {
            (self.read_reg(LSR) & 0x01) != 0
        }
    }
}

/// 全局UART实例
static UART: Mutex<Uart> = Mutex::new(Uart::new(UART_BASE));

/// 初始化UART
pub fn init() {
    UART.lock().init();
}

/// 发送字符
pub fn putc(c: u8) {
    UART.lock().putc(c);
}

/// 接收字符
pub fn getc() -> Option<u8> {
    UART.lock().getc()
}

/// 检查是否有数据
pub fn has_data() -> bool {
    UART.lock().has_data()
}

/// 发送字符串
pub fn puts(s: &str) {
    for c in s.bytes() {
        if c == b'\n' {
            putc(b'\r');
        }
        putc(c);
    }
}

/// 格式化输出
pub fn print_fmt(args: fmt::Arguments) {
    use fmt::Write;
    
    struct UartWriter;
    
    impl Write for UartWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            puts(s);
            Ok(())
        }
    }
    
    UartWriter.write_fmt(args).unwrap();
}

/// 内核打印宏
#[macro_export]
macro_rules! printk {
    ($($arg:tt)*) => {
        $crate::drivers::uart::print_fmt(format_args!($($arg)*));
    };
}

/// 内核打印宏（不带换行）
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::printk!($($arg)*);
    };
}

/// 内核打印宏（带换行）
#[macro_export]
macro_rules! println {
    () => {
        $crate::printk!("\n");
    };
    ($($arg:tt)*) => {
        $crate::printk!($($arg)*);
        $crate::printk!("\n");
    };
}

