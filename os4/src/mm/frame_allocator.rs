
/// 描述物理帧管理器需要提供哪些功能
trait FrameAllocator{
    fn new()-> Self;
    fn alloc(&mut self) -> Option<PhysPageNum>;
    fn dealloc(&mut self, ppn: PhysPageNum);
}

/**
 * 实现一种最简单的栈式物理页帧管理策略 StackFrameAllocator
 * 物理页号区间  此前均 从未 被分配出去过，
 * 而向量 recycled 以后入先出的方式保存了被回收的物理页号
 */
pub struct StackFrameAllocator{
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}

