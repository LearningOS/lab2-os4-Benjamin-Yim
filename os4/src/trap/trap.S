.altmacro
.macro SAVE_GP n
    sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
    .section .text.trampoline
    .globl __alltraps
    .globl __restore
    .align 2
__alltraps:
    # 当应用 Trap 进入内核的时候，硬件会设置一些 CSR 并在 S 特权级下跳转到 __alltraps 保存 Trap 上下文
    # 此时 sp 寄存器仍指向用户栈，但 sscratch 则被设置为指向应用地址空间中存放 Trap 上下文的位置，实际在次高页面。
    #  csrrw 交换 sp 和 sscratch
    csrrw sp, sscratch, sp
    # now sp->*TrapContext in user space, sscratch->user stack
    # 开始保存 Trap 上下文到用户空间
    # save other general purpose registers
    # 保存其他通用目标寄存器
    sd x1, 1*8(sp)
    # skip sp(x2), we will save it later
    sd x3, 3*8(sp)
    # skip tp(x4), application does not use it
    # save x5~x31
    .set n, 5
    .rept 27
        SAVE_GP %n
        .set n, n+1
    .endr
    # we can use t0/t1/t2 freely, because they have been saved in TrapContext
    csrr t0, sstatus
    csrr t1, sepc
    sd t0, 32*8(sp)
    sd t1, 33*8(sp)
    # read user stack from sscratch and save it in TrapContext
    csrr t2, sscratch
    # 保存结束
    sd t2, 2*8(sp)
    # load kernel_satp into t0
    # 获取内核 token 到 t0 寄存器
    ld t0, 34*8(sp)
    # load trap_handler into t1
    # trap 入口位置保存到 t1
    ld t1, 36*8(sp)
    # move to kernel_sp
    # 修改sp 为应用内核的栈顶地址
    ld sp, 35*8(sp)
    # switch to kernel space
    # 切换到内核空间
    # 将 satp 修改为内核地址空间的 token 
    csrw satp, t0
    # 清空 TLB
    sfence.vma
    # jump to trap_handler
    jr t1

__restore:
    # a0: *TrapContext in user space(Constant); a1: user space token
    # switch to user space
    # 切换回应用地址空间
    csrw satp, a1
    sfence.vma
    # 将传入的 Trap 上下文位置保存在 sscratch 寄存器中
    csrw sscratch, a0
    # 将 sp 修改为 Trap 上下文的位置，后面基于它恢复各通用寄存器和 CSR
    mv sp, a0
    # now sp points to TrapContext in user space, start restoring based on it
    # restore sstatus/sepc
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    # restore general purpose registers except x0/sp/tp
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # back to user stack
    ld sp, 2*8(sp)
    # 返回用户态
    sret
