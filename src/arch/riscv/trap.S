    .section .trampoline, "ax"
    .globl trap_entry
    .globl trap_return
    .align 2

# 陷阱帧大小 (31个通用寄存器 + sstatus + sepc)
#define TRAP_FRAME_SIZE 264

# 保存所有通用寄存器到陷阱帧
.macro SAVE_ALL
    # 保存sp到sscratch，然后从sscratch恢复sp
    csrrw sp, sscratch, sp
    
    # 如果sp为0，说明是从内核态陷入
    bnez sp, 1f
    csrr sp, sscratch
1:
    # 为陷阱帧分配空间
    addi sp, sp, -TRAP_FRAME_SIZE
    
    # 保存寄存器
    sd x1,  0*8(sp)   # ra
    sd x3,  2*8(sp)   # gp
    sd x4,  3*8(sp)   # tp
    sd x5,  4*8(sp)   # t0
    sd x6,  5*8(sp)   # t1
    sd x7,  6*8(sp)   # t2
    sd x8,  7*8(sp)   # s0
    sd x9,  8*8(sp)   # s1
    sd x10, 9*8(sp)   # a0
    sd x11, 10*8(sp)  # a1
    sd x12, 11*8(sp)  # a2
    sd x13, 12*8(sp)  # a3
    sd x14, 13*8(sp)  # a4
    sd x15, 14*8(sp)  # a5
    sd x16, 15*8(sp)  # a6
    sd x17, 16*8(sp)  # a7
    sd x18, 17*8(sp)  # s2
    sd x19, 18*8(sp)  # s3
    sd x20, 19*8(sp)  # s4
    sd x21, 20*8(sp)  # s5
    sd x22, 21*8(sp)  # s6
    sd x23, 22*8(sp)  # s7
    sd x24, 23*8(sp)  # s8
    sd x25, 24*8(sp)  # s9
    sd x26, 25*8(sp)  # s10
    sd x27, 26*8(sp)  # s11
    sd x28, 27*8(sp)  # t3
    sd x29, 28*8(sp)  # t4
    sd x30, 29*8(sp)  # t5
    sd x31, 30*8(sp)  # t6
    
    # 保存CSR
    csrr t0, sstatus
    sd t0, 31*8(sp)
    csrr t0, sepc
    sd t0, 32*8(sp)

    # 元数据位于陷阱帧上方：kernel satp、user satp。
    # 先保存完用户寄存器，再切回内核页表。
    ld t0, (TRAP_FRAME_SIZE + 0)(sp)
    csrw satp, t0
    sfence.vma
    
    # 保存原始sp (从sscratch读取)
    csrr t0, sscratch
    sd t0, 1*8(sp)
    
    # 更新sscratch为当前sp
    csrw sscratch, sp
.endm

# 恢复所有通用寄存器从陷阱帧
.macro RESTORE_ALL
    # 返回前恢复用户页表，并让下一次陷阱仍从内核栈元数据处开始。
    addi t0, sp, TRAP_FRAME_SIZE
    csrw sscratch, t0
    ld t0, (TRAP_FRAME_SIZE + 8)(sp)
    csrw satp, t0
    sfence.vma

    # 恢复CSR
    ld t0, 31*8(sp)
    csrw sstatus, t0
    ld t0, 32*8(sp)
    csrw sepc, t0
    
    # 恢复寄存器
    ld x1,  0*8(sp)   # ra
    ld x3,  2*8(sp)   # gp
    ld x4,  3*8(sp)   # tp
    ld x5,  4*8(sp)   # t0
    ld x6,  5*8(sp)   # t1
    ld x7,  6*8(sp)   # t2
    ld x8,  7*8(sp)   # s0
    ld x9,  8*8(sp)   # s1
    ld x10, 9*8(sp)   # a0
    ld x11, 10*8(sp)  # a1
    ld x12, 11*8(sp)  # a2
    ld x13, 12*8(sp)  # a3
    ld x14, 13*8(sp)  # a4
    ld x15, 14*8(sp)  # a5
    ld x16, 15*8(sp)  # a6
    ld x17, 16*8(sp)  # a7
    ld x18, 17*8(sp)  # s2
    ld x19, 18*8(sp)  # s3
    ld x20, 19*8(sp)  # s4
    ld x21, 20*8(sp)  # s5
    ld x22, 21*8(sp)  # s6
    ld x23, 22*8(sp)  # s7
    ld x24, 23*8(sp)  # s8
    ld x25, 24*8(sp)  # s9
    ld x26, 25*8(sp)  # s10
    ld x27, 26*8(sp)  # s11
    ld x28, 27*8(sp)  # t3
    ld x29, 28*8(sp)  # t4
    ld x30, 29*8(sp)  # t5
    ld x31, 30*8(sp)  # t6
    
    # 恢复sp
    ld x2,  1*8(sp)
.endm

trap_entry:
    SAVE_ALL
    
    # 设置参数并调用Rust陷阱处理函数
    mv a0, sp
    call trap_handler
    mv sp, a0
    
trap_return:
    RESTORE_ALL
    sret
