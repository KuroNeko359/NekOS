# nekos - RISC-V 微内核操作系统

用 Rust 编写的 RISC-V 64 位微内核操作系统。

## 特性

- **Sv39 虚拟内存**: 支持 39 位虚拟地址空间
- **进程管理**: 支持用户态进程创建和调度
- **同步 IPC**: endpoint、call、recv、reply
- **独立用户态服务**: Console Server 与 Shell 作为 ELF 从 initrd 加载，Shell 通过 IPC 访问终端
- **中断驱动输入**: Console Server 通过 irq_wait 阻塞，UART RX 中断到达后由内核唤醒
- **进程生命周期**: fork、exec、waitpid、Zombie 回收
- **陷阱处理**: 支持中断和异常处理
- **定时器**: 基于 SBI 的定时器中断

## 构建要求

- Rust 工具链 (nightly)
- RISC-V 目标: `rustup target add riscv64gc-unknown-none-elf`
- Rust 源码: `rustup component add rust-src`
- QEMU: `brew install qemu`
- RISC-V GCC: `brew install riscv64-elf-gcc`

## 构建

```bash
# 安装依赖
make install-target

# 构建内核
make build

# 运行内核
make run

# 调试内核
make debug
```

## 项目结构

```
nekos/
├── Cargo.toml          # Cargo 配置
├── Makefile            # 构建脚本
├── linker.ld           # 链接脚本
├── build.rs            # 构建脚本
├── programs/
│   └── user.ld         # 用户 ELF 链接脚本
├── user/
│   ├── Cargo.toml      # 用户程序 Cargo workspace
│   ├── userlib/        # 用户态入口、系统调用、IPC 和打印库
│   ├── include/nekos.h  # C 用户程序 API
│   ├── libc/           # C 启动入口与最小运行库
│   └── programs/
│       ├── console/    # Console Server
│       ├── shell/      # Shell
│       ├── hello/      # Rust hello
│       ├── hello-c/    # C hello
│       └── test/       # 最小测试程序
├── src/
│   ├── main.rs         # 内核入口点
│   ├── arch/
│   │   └── riscv/
│   │       ├── mod.rs  # 架构模块
│   │       ├── csr.rs  # CSR 操作
│   │       ├── sbi.rs  # SBI 接口
│   │       ├── start.S # 启动代码
│   │       ├── trap.S  # 陷阱处理
│   │       └── user.S  # 用户模式入口
│   ├── kernel/
│   │   ├── mod.rs      # 内核模块
│   │   ├── page.rs     # 页面分配器
│   │   ├── pgtable.rs  # 页表管理
│   │   ├── vm.rs       # 虚拟内存
│   │   ├── trap.rs     # 陷阱处理
│   │   ├── task.rs     # 进程管理
│   │   ├── syscall.rs  # 系统调用
│   │   ├── ipc.rs      # 同步 IPC 与 endpoint ABI
│   │   ├── idle.rs     # PID 0 内核 idle 任务
│   │   ├── exec.rs     # exec 实现
│   │   └── timer.rs    # 定时器
│   ├── drivers/
│   │   ├── mod.rs      # 驱动模块
│   │   ├── plic.rs     # PLIC 外部中断控制器
│   │   └── uart.rs     # UART 驱动
└── README.md
```

## 编写用户程序

用户程序通过 `userlib` 使用 nekos。最小程序如下：

```rust
#![no_std]
#![no_main]

use userlib::{entry, println};

fn main() {
    println!("hello from Rust");
}

entry!(main);
```

在 `user/programs/` 下新增一个包含 `Cargo.toml` 和 `src/main.rs` 的程序目录后，
`make build` 会自动发现、编译并将它打包进 initrd。用户程序也可以单独检查：

```bash
cargo build --manifest-path user/Cargo.toml --release
```

### 编写 C 用户程序

在 `user/programs/程序名/src/main.c` 中编写程序：

```c
#include <nekos.h>

int main(void) {
    static const char message[] = "Hello from C!\n";
    nekos_write(1, message, sizeof(message) - 1);
    return 0;
}
```

执行 `make build` 时，构建脚本会自动识别 `src/main.c`，使用
`riscv64-elf-gcc`、`libnekos` 和 `programs/user.ld` 生成 RISC-V ELF，
然后将其加入 initrd。程序从 `main` 返回后，C 启动代码会自动调用
`nekos_exit`。

C API 在 `user/include/nekos.h` 中，目前包括：

- `nekos_exit`、`nekos_yield`、`nekos_getpid`
- `nekos_fork`、`nekos_exec`、`nekos_waitpid`、`nekos_ps`
- `nekos_ipc_call`、`nekos_ipc_recv`、`nekos_ipc_reply`
- `nekos_read`、`nekos_write`、`nekos_irq_wait`

同一程序目录如果同时存在 `src/main.rs` 和 `src/main.c`，构建脚本优先编译
Rust 文件。

## 系统调用

| 编号 | 名称 | 描述 |
|------|------|------|
| 1 | write | 写入文件描述符 |
| 2 | exit | 退出进程 |
| 3 | read | 读取文件描述符 |
| 4 | yield | 让出 CPU |
| 5 | getpid | 获取进程 ID |
| 6 | fork | 创建子进程 |
| 7 | ps | 列出进程 |
| 8 | exec | 执行程序 |
| 9 | waitpid | 等待并回收子进程 |
| 10 | ipc_call | 发送请求并等待回复 |
| 11 | ipc_recv | 服务端接收请求 |
| 12 | ipc_reply | 服务端回复客户端 |
| 13 | irq_wait | 设备服务等待已授权的硬件中断 |

## 微内核边界

内核负责地址空间、ELF 加载、任务调度、陷阱、定时器和 IPC endpoint。启动时内核从 initrd
分别加载 `console` 和 `shell` ELF；它们不再链接进内核，也不能访问内核代码页。Console Server
运行在 U-mode，是唯一获得 UART MMIO 用户映射的任务；Shell 不直接访问 UART，而是通过
endpoint 1 同步调用服务。
Console 等待输入时调用 `irq_wait` 进入 Sleeping，UART RX 中断将其唤醒；没有普通任务可运行时
PID 0 执行 `wfi`。
`read/write` 系统调用目前仅作为 initrd 旧程序的兼容接口保留，后续服务全部迁移后可删除。

## 许可证

MIT License
