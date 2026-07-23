    .section .text.start
    .globl _start
_start:
    la s0, message
    la s1, message_end

1:
    beq s0, s1, 2f

    li a0, 1
    li a1, 1
    lbu a2, 0(s0)
    li a3, 0
    li a4, 0
    li a7, 10
    ecall

    addi s0, s0, 1
    j 1b

2:
    li a0, 0
    li a7, 2
    ecall

3:
    j 3b

    .section .rodata
message:
    .ascii "Hello from hello program!\n"
message_end:
