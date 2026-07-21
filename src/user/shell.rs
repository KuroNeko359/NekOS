//! 用户态 Shell

use core::fmt;

macro_rules! print {
    ($($arg:tt)*) => {
        print_fmt(format_args!($($arg)*));
    };
}

macro_rules! println {
    () => {
        print!("\n");
    };
    ($($arg:tt)*) => {
        print!($($arg)*);
        print!("\n");
    };
}

struct UserWriter;

impl fmt::Write for UserWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        syscall_write(1, s.as_ptr(), s.len());
        Ok(())
    }
}

fn print_fmt(args: fmt::Arguments) {
    use fmt::Write;

    UserWriter.write_fmt(args).unwrap();
}

/// 用户主函数
#[no_mangle]
pub extern "C" fn user_main() {
    println!("=== KuroOS Rust Shell ===");
    println!("type 'help' for commands");
    println!();
    
    let mut line = [0u8; 80];
    
    loop {
        print!("> ");
        
        // 读取一行
        let len = read_line(&mut line);
        
        if len == 0 {
            continue;
        }
        
        let cmd = core::str::from_utf8(&line[..len]).unwrap_or("");
        
        match cmd.trim() {
            "help" => {
                println!("available commands:");
                println!("  help      - show this help");
                println!("  hello     - print hello message");
                println!("  about     - about this OS");
                println!("  pid       - show current process ID");
                println!("  ps        - list all processes");
                println!("  yield     - yield CPU to other tasks");
                println!("  fork      - fork a child process");
                println!("  exec NAME - execute program from initrd");
                println!("  exectest  - fork then exec hello in child");
                println!("  wait PID  - wait for and reap a child");
                println!("  exit      - exit the shell");
            }
            "hello" => {
                println!("Hello from KuroOS Rust Shell!");
            }
            "about" => {
                println!("KuroOS - Minimal RISC-V Operating System");
                println!("Written in Rust");
                println!("Features:");
                println!("  - Sv39 virtual memory");
                println!("  - Preemptive scheduling (timer interrupt)");
                println!("  - User mode (U-mode) processes");
                println!("  - Microkernel architecture (planned)");
            }
            "pid" => {
                let pid = syscall_getpid();
                println!("current pid: {}", pid);
            }
            "ps" => {
                println!("process list:");
                syscall_ps();
            }
            "yield" => {
                println!("yielding CPU...");
                syscall_yield();
                println!("resumed after yield");
            }
            "fork" => {
                let pid = syscall_fork();
                if pid == 0 {
                    println!("[child] pid={} hello!", syscall_getpid());
                    syscall_exit(0);
                } else if pid == usize::MAX {
                    println!("fork failed");
                } else {
                    println!("[parent] forked child pid={}", pid);
                }
            }
            "exectest" => {
                let pid = syscall_fork();
                if pid == 0 {
                    let name = b"hello";
                    if syscall_exec(name.as_ptr(), name.len()) == usize::MAX {
                        println!("[child] exec failed");
                        syscall_exit(1);
                    }
                } else if pid == usize::MAX {
                    println!("fork failed");
                } else {
                    println!("[parent] waiting for child pid={}", pid);
                    let code = syscall_waitpid(pid);
                    println!("[parent] child exited with code={}", code);
                }
            }
            "exit" => {
                println!("goodbye!");
                syscall_exit(0);
            }
            command if command.starts_with("exec ") => {
                let name = command[5..].trim();
                if syscall_exec(name.as_ptr(), name.len()) == usize::MAX {
                    println!("exec: failed to run '{}'", name);
                }
            }
            command if command.starts_with("wait ") => {
                match command[5..].trim().parse::<usize>() {
                    Ok(pid) => {
                        let code = syscall_waitpid(pid);
                        if code < 0 {
                            println!("wait: pid {} is not a child", pid);
                        } else {
                            println!("wait: child {} exited with code={}", pid, code);
                        }
                    }
                    Err(_) => {
                        println!("usage: wait PID");
                    }
                }
            }
            _ => {
                println!("unknown command: {}", cmd.trim());
            }
        }
    }
}

/// 读取一行输入
fn read_line(buf: &mut [u8]) -> usize {
    syscall_read(0, buf.as_mut_ptr(), buf.len())
}

/// 系统调用：写入
fn syscall_write(fd: usize, buf: *const u8, len: usize) -> usize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") fd => result,
            in("a1") buf as usize,
            in("a2") len,
            in("a7") 1usize,
        );
    }
    result
}

/// 系统调用：读取
fn syscall_read(fd: usize, buf: *mut u8, len: usize) -> usize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") fd => result,
            in("a1") buf as usize,
            in("a2") len,
            in("a7") 3usize,
        );
    }
    result
}

/// 系统调用：获取PID
fn syscall_getpid() -> u32 {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") 0usize => result,
            in("a7") 5usize,  // SYS_GETPID
        );
    }
    result as u32
}

/// 系统调用：列出进程
fn syscall_ps() {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") 0usize,
            in("a7") 7usize,  // SYS_PS
        );
    }
}

/// 系统调用：让出CPU
fn syscall_yield() {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") 4usize,  // SYS_YIELD
        );
    }
}

fn syscall_fork() -> usize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") 0usize => result,
            in("a7") 6usize,
        );
    }
    result
}

fn syscall_exec(filename: *const u8, len: usize) -> usize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") filename as usize => result,
            in("a1") len,
            in("a7") 8usize,
        );
    }
    result
}

/// 系统调用：退出
fn syscall_exit(code: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") code,
            in("a7") 2usize,  // SYS_EXIT
        );
    }
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

//
fn syscall_waitpid(pid:usize) -> isize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") pid => result,
            in("a7") 9usize,
        );
    }
    result as isize
}
