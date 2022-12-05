//! Implementation of [`FrameAllocator`] which 
//! controls all the frames in the operating system.

use super::{PhysAddr, PhysPageNum};
use crate::config::MEMORY_END;
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};
use lazy_static::*;

/// manage a frame which has the same lifecycle as the tracker
pub struct FrameTracker {
    pub ppn: PhysPageNum,
}

/**
 * 将 PhysPageNum 在封装一层
 * 猜测为什么封装一层，应该是为了方便将物理页号看做一个对象
 * 释放出去使用，新建一个对象时新建一个物理页号，回收这个对象时
 * 物理页号就可以跟着回收，而不是每次需要显示的回收，整个物理页号的
 * 生命周期页不需要太过关注
 */
impl FrameTracker {
    pub fn new(ppn: PhysPageNum) -> Self {
        // page cleaning
        // 从 FRAME_ALLOCATOR 中分配一个物理页帧
        // 将分配来的物理页帧的物理页号作为参数传给 
        // FrameTracker 的 new 方法来创建一个 FrameTracker 实例
        let bytes_array = ppn.get_bytes_array();
        // 由于这个物理页帧之前可能被分配过并用做其他用途，
        // 我们在这里直接将这个物理页帧上的所有字节清零
        for i in bytes_array {
            *i = 0;
        }
        Self { ppn }
    }
}

impl Debug for FrameTracker {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("FrameTracker:PPN={:#x}", self.ppn.0))
    }
}

/**
 * 当 FrameTracker 实例被回收的时候，它的 Drop 方法会被编译器调用
 */
impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn);
    }
}

/// BASE_ADDRESS 和 MEMORY_END 传给 FrameAllocator ，
/// 用于物理页帧管理器用于初始化
trait FrameAllocator {
    fn new() -> Self;
    fn alloc(&mut self) -> Option<PhysPageNum>;
    fn dealloc(&mut self, ppn: PhysPageNum);
}

/// an implementation for frame allocator
/**
 * 实现一种最简单的栈式物理页帧管理器
 * 而向量 recycled 以后入先出的方式保存了被回收的物理页号
 * current: 可分配的物理地址的起始位置
 * end: 可分配物理地址的最终位置，current-end 表示这里的地址从来没有使用过
 * recycled: 已经分配过回收的内存地址，可重复使用的地址
 */
pub struct StackFrameAllocator {
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}

impl StackFrameAllocator {
    /**
     * 初始化，修改 current 和 end 为真实可用的物理空间
     */
    pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
        self.current = l.0;
        self.end = r.0;
    }
}
impl FrameAllocator for StackFrameAllocator {
    fn new() -> Self {
        Self {
            current: 0,
            end: 0,
            recycled: Vec::new(),
        }
    }

    /**
     * 物理页帧的分配
     * 分配一个，向前新增一个所以永远不会重复
     */
    fn alloc(&mut self) -> Option<PhysPageNum> {
        // 如果从回收的物理内存中可以获取到可再利用的地址
        // 就返回可以回收的地址空间
        if let Some(ppn) = self.recycled.pop() {
            Some(ppn.into())
        } else if self.current == self.end {
            // 否则，判断是否可用物理内存耗尽
            None
        } else {
            // 没有则加1
            self.current += 1;
            // 分配物理地址
            Some((self.current - 1).into())
        }
    }
    /**
     * 物理页帧的回收
     */
    fn dealloc(&mut self, ppn: PhysPageNum) {
        let ppn = ppn.0;
        // validity check
        // 回收条件
        // 1. 该页面之前一定被分配出去过，因此它的物理页号一定 < current  ；
        // 2. 该页面没有正处在回收状态，即它的物理页号不能在栈 recycled 中找到。
        // any 是为了在回收过的地址中找到一个与其相同的值，如果找到了就表示出现了内核错误
        if ppn >= self.current || self.recycled.iter().any(|v| *v == ppn) {
            panic!("Frame ppn={:#x} has not been allocated!", ppn);
        }
        // recycle
        // 回收地址空间
        self.recycled.push(ppn);
    }
}

type FrameAllocatorImpl = StackFrameAllocator;

lazy_static! {
    /// frame allocator instance through lazy_static!
    pub static ref FRAME_ALLOCATOR: UPSafeCell<FrameAllocatorImpl> =
        unsafe { UPSafeCell::new(FrameAllocatorImpl::new()) };
}

/// initiate the frame allocator using `ekernel` and `MEMORY_END`
pub fn init_frame_allocator() {
    extern "C" {
        fn ekernel();
    }
    FRAME_ALLOCATOR.exclusive_access().init(
        PhysAddr::from(ekernel as usize).ceil(),
        PhysAddr::from(MEMORY_END).floor(),
    );
}

/// allocate a frame
/**
 * frame_alloc 的返回值并不是 FrameAllocator 要求的物理页号 PhysPageNum
 * 而是进一步封装为 FrameTracker
 */
pub fn frame_alloc() -> Option<FrameTracker> {
    FRAME_ALLOCATOR
        .exclusive_access()
        .alloc()
        .map(FrameTracker::new)
}

/// deallocate a frame
fn frame_dealloc(ppn: PhysPageNum) {
    FRAME_ALLOCATOR.exclusive_access().dealloc(ppn);
}

#[allow(unused)]
/// a simple test for frame allocator
pub fn frame_allocator_test() {
    let mut v: Vec<FrameTracker> = Vec::new();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        info!("{:?}", frame);
        v.push(frame);
    }
    v.clear();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        info!("{:?}", frame);
        v.push(frame);
    }
    drop(v);
    info!("frame_allocator_test passed!");
}
