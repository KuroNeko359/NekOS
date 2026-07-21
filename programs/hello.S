    .section .text.start
    .globl _start
_start:
    li a0, 1
    la a1, message
    li a2, 26
    li a7, 1
    ecall
    li a0, 0
    li a7, 2
    ecall
1:  wfi
    j 1b

    .section .rodata
message:
    .ascii "Hello from hello program!\n"
