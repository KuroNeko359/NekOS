    .section .text.entry
    .globl _start
    .align 2

_start:
    # 设置栈指针 (临时栈)
    la sp, boot_stack_top
    
    # 清零BSS段
    la t0, __bss_start
    la t1, __bss_end
bss_loop:
    bgeu t0, t1, bss_done
    sd zero, 0(t0)
    addi t0, t0, 8
    j bss_loop
bss_done:
    
    # 跳转到Rust内核入口
    call kernel_main
    
    # 如果kernel_main返回，死循环
hang:
    wfi
    j hang

    .section .bss.stack, "aw", @nobits
    .align 12
boot_stack:
    .skip 65536
boot_stack_top:
