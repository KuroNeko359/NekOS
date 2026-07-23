    .section .text.start
    .globl _start
    .type _start, @function
_start:
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop

    call main
    tail nekos_exit

    .size _start, . - _start
