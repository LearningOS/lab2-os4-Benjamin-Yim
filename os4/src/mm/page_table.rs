
use alloc::vec::Vec;
use bitflags::*;
use crate::mm::address::*;

use super::frame_allocator::FrameTracker;
use super::frame_allocator::frame_alloc;
///
/// 页表项
/// 

bitflags!{
    pub struct PTEFlags: u8{
        const V = 1 << 0; // 仅当 V(Valid) 位为 1 时，页表项才是合法的；
        const R = 1 << 1; // 虚拟页面是否允许读
        const W = 1 << 2; // 虚拟页面是否允许写
        const X = 1 << 3; // 虚拟页面是否允许执行
        const U = 1 << 4; // 控制索引到这个页表项的对应虚拟页面是否在 CPU 处于 U 特权级的情况下是否被允许访问；
        const G = 1 << 5; // 忽略
        const A = 1 << 6; // 记录自从页表项上的这一位被清零之后，页表项的对应虚拟页面是否被访问过；
        const D = 1 << 7; // 则记录自从页表项上的这一位被清零之后，页表项的对应虚拟页表是否被修改过
    }
}

// 实现 Copy/Clone Trait，来让这个类型以值语义赋值/传参的时候 
// 不会发生所有权转移，而是拷贝一份新的副本。
#[derive(Copy,Clone)]
#[repr(C)]
pub struct PageTableEntry{
    pub bits: usize  // 页表项字节位
}


impl PageTableEntry{
    /**
     * 可以从一个物理页号 PhysPageNum 和一个页表项标志位 PTEFlags 生成一个页表项 
     * PageTableEntry 实例；
     */
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self{
        PageTableEntry{
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }

    /**
     * 生成全 0 的页表项
     */
    pub fn empty() -> Self{
        PageTableEntry {
            bits: 0
        }
    }
    /**
     * 取出物理页号
     */
    pub fn ppn(&self) -> PhysPageNum{
        (self.bits >> 10 & ((1usize << 44) -1 )).into()
    }
    /**
     * 取出页表项
     */
    pub fn flags(&self) -> PTEFlags{
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }

    pub fn is_valid(&self) -> bool{
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
}


/**
 * 多级页表是以节点为单位进行管理的。
 * 每个节点恰好存储在一个物理页帧中，它的位置可以用一个物理页号来表示。
 * 每个应用的地址空间都对应一个不同的多级页表，这也就意味这不同页表的起始地址
 * 是不一样的。PageTable 要保存它根节点的物理页号。
 * 因此 PageTable 要保存它根节点的物理页号
 * root_ppn 作为页表唯一的区分标志
 * frames 以 FrameTracker 的形式保存了页表所有的节点
 */
pub struct PageTable{
    root_ppn: PhysPageNum,
    // 当 PageTable 生命周期结束后，向量 frames 里面的
    // 那些 FrameTracker 也会被回收，也就意味着存放多级
    // 页表节点的那些物理页帧 被回收了。
    frames: Vec<FrameTracker>,
}

impl PageTable {
    // 当我们通过 new 方法新建一个 PageTable 的时候，它只需有一个根节点
    // 为此我们需要分配一个物理页帧 FrameTracker 并挂在向量 frames 下，
    // 然后更新根节点的物理页号 root_ppn 。
    pub fn new() -> Self{
        let frame = frame_alloc().unwrap();
        PageTable { 
            root_ppn: frame.ppn, 
            frames: vec![frame] 
        }
    }
    // 多级页表并不是被创建出来之后就不再变化的，
    // 为了 MMU 能够通过地址转换正确找到应用地址空间中的数据实际被内核放在内存中 位置，
    // 操作系统需要动态维护一个虚拟页号到页表项的映射，支持插入/删除键值对

    // map 方法来在多级页表中插入一个键值对，注意这里我们将物理页号 ppn 和页表项
    // 标志位 flags 作为 不同的参数传入而不是整合为一个页表项
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags){
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V)
    }
    // 我们通过 unmap 方法来删除一个键值对，在调用时仅需给出作为索引的虚拟页号即可。
    pub fn unmap(&mut self, vpn: VirtPageNum){
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::empty();
    }
    /**
     * 多级页表找到一个虚拟页号对应的页表项的可变引用方便后续的读写。
     * 如果在 遍历的过程中发现有节点尚未创建则会新建一个节点
     */
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry>{
        // 获取三级页表项
        let idx =  vpn.indexex();
        // 变量 ppn 表示当前节点的物理页号，最开始指向多级页表的根节点
        let mut ppn = self.root_ppn;
        let mut result:Option<&mut PageTableEntry> = None;
        for i in (0..3){
            // get_pte_array 将取出当前节点的页表项数组，并根据当前级页索引找到对应的页表项。
            let pte = &mut ppn.get_pte_array()[idx[i]];
            // 如果当前节点是一个叶节点，那么直接返回这个页表项 的可变引用；
            if i == 2 {
                result = Some(pte);
                break;
            }
            // 如果不是有效的
            if !pte.is_valid() {
                // 走不下去的话就新建一个节点，更新作为下级节点指针的页表项，
                let frame = frame_alloc().unwrap();
                // 注意在更新页表项的时候，不仅要更新物理页号，还要将标志位 V 置 1， 
                // 不然硬件在查多级页表的时候，会认为这个页表项不合法，
                // 从而触发 Page Fault 而不能向下走。
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                // 并将新分配的物理页帧移动到 向量 frames 中方便后续的自动回收。
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }

    // 为了方便后面的实现，我们还需要 PageTable 提供一种不经过 MMU 而是手动查页表的方法：

    // from_token 可以临时创建一个专用来手动查页表的 PageTable
    // 它仅有一个从传入的 satp token 中得到的多级页表根节点的物理页号，
    // 它的 frames 字段为空，也即不实际控制任何资源；
    pub fn from_token(satp: usize) -> Self{
        Self { root_ppn: PhysPageNum::from(satp & ((1usize << 44)-1)), frames: Vec::new() }
    }

    // find_pte 和之前的 find_pte_create 不同之处在于它不会试图分配物理页帧。
    // 一旦在多级页表上遍历 遇到空指针它就会直接返回 None 
    // 表示无法正确找到传入的虚拟页号对应的页表项
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry>{
        let idxs = vpn.indexex();
        let mut ppn = self.root_ppn;
        let mut result: Option<&PageTableEntry> = None;
        for i in (0..3){
            let pte = &ppn.get_pte_array()[idxs[i]];
            if i == 2{
                result = Some(pte);
                break;
            }
            if !pte.is_valid(){
                return None;
            }
            ppn = pte.ppn();
        }
    }

    // translate 调用 find_pte 来实现，如果能够找到页表项，
    // 那么它会将页表项拷贝一份并返回，否则就 返回一个 None 。
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry>{
        self.find_pte(vpn)
            .map(|pte| {pte.clone()})
    }

}
