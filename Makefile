# Makefile for RISC-V OS in Rust

TARGET = riscv64gc-unknown-none-elf
MODE = release
CARGO = cargo
CARGO_FLAGS = --target $(TARGET) --$(MODE) -Z build-std=core,alloc
QEMU = qemu-system-riscv64
QEMU_FLAGS = -machine virt -nographic -kernel
KERNEL = target/$(TARGET)/$(MODE)/riscv-os-rust

.PHONY: all build run debug clean

all: build

build:
	RUSTC_BOOTSTRAP=1 $(CARGO) build $(CARGO_FLAGS)

run: build
	$(QEMU) $(QEMU_FLAGS) $(KERNEL)

debug: build
	$(QEMU) $(QEMU_FLAGS) $(KERNEL) -S -s

clean:
	$(CARGO) clean
