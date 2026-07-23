#![no_std]
#![no_main]

use userlib::{entry, io, print, println, syscall};

fn main() -> ! {
    println!("=== nekos Shell ===");
    println!("type 'help' for commands");
    println!();

    let mut line = [0u8; 80];
    loop {
        print!("> ");
        let len = read_line(&mut line);
        if len == 0 {
            continue;
        }

        let cmd = core::str::from_utf8(&line[..len]).unwrap_or("").trim();
        match cmd {
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
            "hello" => println!("Hello from nekos Shell!"),
            "about" => {
                println!("nekos - Minimal RISC-V Operating System");
                println!("Written in Rust");
                println!("Features:");
                println!("  - Sv39 virtual memory");
                println!("  - Preemptive scheduling");
                println!("  - User-mode processes");
                println!("  - User-mode Console Server over IPC");
            }
            "pid" => println!("current pid: {}", syscall::getpid()),
            "ps" => {
                println!("process list:");
                syscall::ps();
            }
            "yield" => {
                println!("yielding CPU...");
                syscall::yield_now();
                println!("resumed after yield");
            }
            "fork" => {
                let pid = syscall::fork();
                if pid == 0 {
                    println!("[child] pid={} hello!", syscall::getpid());
                    syscall::exit(0);
                } else if pid == syscall::ERROR {
                    println!("fork failed");
                } else {
                    println!("[parent] forked child pid={}", pid);
                }
            }
            "exectest" => {
                let pid = syscall::fork();
                if pid == 0 {
                    if syscall::exec("hello") == syscall::ERROR {
                        println!("[child] exec failed");
                        syscall::exit(1);
                    }
                } else if pid == syscall::ERROR {
                    println!("fork failed");
                } else {
                    println!("[parent] waiting for child pid={}", pid);
                    let code = syscall::waitpid(pid as u32);
                    println!("[parent] child exited with code={}", code);
                }
            }
            "exit" => {
                println!("goodbye!");
                syscall::exit(0);
            }
            command if command.starts_with("exec ") => {
                let name = command[5..].trim();
                if syscall::exec(name) == syscall::ERROR {
                    println!("exec: failed to run '{}'", name);
                }
            }
            command if command.starts_with("wait ") => {
                match command[5..].trim().parse::<u32>() {
                    Ok(pid) => {
                        let code = syscall::waitpid(pid);
                        if code < 0 {
                            println!("wait: pid {} is not a child", pid);
                        } else {
                            println!("wait: child {} exited with code={}", pid, code);
                        }
                    }
                    Err(_) => println!("usage: wait PID"),
                }
            }
            _ => println!("unknown command: {}", cmd),
        }
    }
}

fn read_line(buf: &mut [u8]) -> usize {
    let mut len = 0usize;
    while len + 1 < buf.len() {
        let mut input = [0u8; 1];
        if io::read(0, &mut input) != 1 {
            return len;
        }
        let byte = input[0];
        if byte == 0x7f || byte == 8 {
            if len > 0 {
                len -= 1;
                print!("\x08 \x08");
            }
            continue;
        }
        if io::write(1, &[byte]) != 1 {
            return len;
        }
        if byte == b'\n' {
            break;
        }
        buf[len] = byte;
        len += 1;
    }
    len
}

entry!(main);
