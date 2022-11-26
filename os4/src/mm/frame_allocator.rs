
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
 * current: 可分配的物理地址的起始位置
 * end: 可分配物理地址的最终位置
 * recycled: 已经分配过回收的内存地址，可重复使用的地址
 */
pub struct StackFrameAllocator{
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}

impl FrameAllocator for StackFrameAllocator{
    fn new() -> Self{
        Self{
            current: 0,
            end: 0,
            recycled: Vec::new(),
        }
    }
}

impl FrameAllocator{
    /**
     * 初始化地址空间
     */
    pub fn init(&mut self, l: PhysPageNum, r:PhysPageNum){
        self.current = l.0;
        self.end = l.0;
    }
}

/**
 * 物理页帧的分配与回收
 */
impl FrameAllocator for StackFrameAllocator{
    fn alloc(&mut self) -> Option<PhysPageNum>{
        // 如果从回收的物理内存中可以获取到可再利用的地址
        // 就返回可以回收的地址空间
        if let Some(ppn) = self.recycled.pop() {
            Some(pph.into())
        } else{
            // 否则，判断是否可用物理内存耗尽
            if self.current == self.end {
                None
            } else{
                // 没有则加1
                self.current += 1;
                // 分配物理地址
                Some((self.current - 1).into())
            }
        }
    }

    fn dealloc(&mut self, ppn: PhysPageNum){
        let ppn = ppn.0;
        // 回收条件
        // 1. 该页面之前一定被分配出去过，因此它的物理页号一定  ；
        // 2. 该页面没有正处在回收状态，即它的物理页号不能在栈 recycled 中找到。
        if ppn>= self.current ||self.recycled
        .iter()
        .find(|&v| {v == ppn})
        .is_some() {
            panic!("Frame ppn = {:#x} has not been allocted!", ppn);
        }
        // 回收地址空间
        self.recycled.push(ppn);
    }
}