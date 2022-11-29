//! Types related to task management
use super::TaskContext;
use crate::config::{kernel_stack_position, TRAP_CONTEXT};
use crate::mm::{MapPermission, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::trap::{trap_handler, TrapContext};

/// task control block structure
pub struct TaskControlBlock {
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
    // 应用的地址空间
    pub memory_set: MemorySet,
    // 位于应用地址空间次高页的 Trap 上下文被实际存放在物理页帧的物理页号
    pub trap_cx_ppn: PhysPageNum,
    // 统计了应用数据的大小.应用地址空间中从 0x00 开始到用户栈结束一共包含多少字节
    pub base_size: usize,
}

impl TaskControlBlock {
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        // 我们解析传入的 ELF 格式数据构造应用的地址空间 memory_set 并获得其他信息
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        // 从地址空间 memory_set 中查多级页表找到应用地址空间中的 Trap 上下文实际被放在哪个物理页帧；
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;
        // map a kernel-stack in kernel space
        // 根据传入的应用 ID app_id 调用在 config 子模块中定义的 kernel_stack_position 
        // 找到 应用的内核栈预计放在内核地址空间 KERNEL_SPACE 中的哪个位置
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        // 通过 insert_framed_area 实际将这个逻辑段 加入到内核地址空间中
        KERNEL_SPACE.lock().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        // 在应用的内核栈顶压入一个跳转到 trap_return 而不是 __restore 的任务上下文， 
        // 这主要是为了能够支持对该应用的启动并顺利切换到用户地址空间执行。
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
        };
        // prepare TrapContext in user space
        // 由于它是在应用地址空间而不是在内核地址空间中，我们只能手动查页表找到 Trap 
        // 上下文实际被放在的物理页帧，再获得在用户空间的 Trap 上下文的可变引用用于初始化
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.lock().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
pub enum TaskStatus {
    UnInit,
    Ready,
    Running,
    Exited,
}
