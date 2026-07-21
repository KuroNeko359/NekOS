# KuroOS Rust - RISC-V 微内核操作系统

用 Rust 编写的 RISC-V 64 位微内核操作系统。

## 特性

- **Sv39 虚拟内存**: 支持 39 位虚拟地址空间
- **进程管理**: 支持用户态进程创建和调度
- **系统调用**: write, read, exit, yield, getpid, ps
- **UART 驱动**: NS16550A 串口驱动
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
riscv-os-rust/
├── Cargo.toml          # Cargo 配置
├── Makefile            # 构建脚本
├── linker.ld           # 链接脚本
├── build.rs            # 构建脚本
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
│   │   ├── exec.rs     # exec 实现
│   │   └── timer.rs    # 定时器
│   ├── drivers/
│   │   ├── mod.rs      # 驱动模块
│   │   └── uart.rs     # UART 驱动
│   └── user/
│       ├── mod.rs      # 用户模块
│       └── shell.rs    # 用户 Shell
└── README.md
```

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

## 许可证

MIT License
