    .section .trampoline, "ax"
    .globl enter_user
    .align 2

# enter_user(user_stack_top, user_entry, trap_stack, kernel_satp, user_satp)
enter_user:
    # a0 = user stack top
    # a1 = user entry point
    # a2 = trap stack（顶部预留16字节页表元数据）
    # a3 = kernel satp
    # a4 = user satp

    sd a3, 0(a2)
    sd a4, 8(a2)

    # trampoline 在两套页表中都保持 supervisor-only，可安全完成切换。
    csrw satp, a4
    sfence.vma
    
    # 保存kernel stack top到sscratch
    csrw sscratch, a2
    
    # 设置用户栈指针
    mv sp, a0
    
    # 设置用户程序入口点
    csrw sepc, a1
    
    # 清除SPP位，使sret返回到U-mode
    csrr t0, sstatus
    li t1, 1
    slli t1, t1, 8
    not t1, t1
    and t0, t0, t1
    
    # 设置SPIE位，使中断在返回用户态后启用
    li t1, 1
    slli t1, t1, 5
    or t0, t0, t1
    
    csrw sstatus, t0
    
    # 返回用户态
    sret
