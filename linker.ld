/* 链接脚本 - RISC-V 64位 */
OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0x80200000, LENGTH = 128M
}

SECTIONS
{
    . = 0x80200000;
    
    .text : {
        __text_start = .;
        *(.text.entry)
        *(.text .text.*)
        __text_end = .;
    } > RAM

    /* 必须独占页面：用户页表中该页保持为 supervisor-only。 */
    .trampoline ALIGN(4K) : {
        __trampoline_start = .;
        *(.trampoline)
        . = ALIGN(4K);
        __trampoline_end = .;
    } > RAM
    
    .rodata : {
        __rodata_start = .;
        *(.rodata .rodata.*)
        __rodata_end = .;
    } > RAM
    
    .data : {
        __data_start = .;
        *(.data .data.*)
        __data_end = .;
    } > RAM
    
    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM
    
    . = ALIGN(4K);
    __kernel_end = .;
    
    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
        *(.note.*)
    }
}
