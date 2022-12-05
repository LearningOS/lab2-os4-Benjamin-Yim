//! Implementation of [`PageTableEntry`] and [`PageTable`].

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        const V = 1 << 0; // 仅当 V(Valid) 位为 1 时，页表项才是合法的；
        const R = 1 << 1; // 可读
        const W = 1 << 2; // 可写
        const X = 1 << 3; // 可执行
        const U = 1 << 4; // 控制索引到这个页表项的对应虚拟页面是否在 CPU 处于 U 特权级的情况下是否被允许访问
        const G = 1 << 5; // 忽略
        const A = 1 << 6; // 记录自从页表项上的这一位被清零之后，页表项的对应虚拟页面是否被访问过
        const D = 1 << 7; // 则记录自从页表项上的这一位被清零之后，页表项的对应虚拟页表是否被修改过
    }
}

// 让编译器自动为 PageTableEntry 实现 Copy/Clone Trait
#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
pub struct PageTableEntry {
    pub bits: usize,
}

impl PageTableEntry {
    // 从一个物理页号 PhysPageNum 和一个页表项标志位 PTEFlags 生成一个页表项实例
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /**
     * 取出物理页号
     */
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /**
     * 取出页表项
     */
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /**
     * 判断 V 位是否为 1
     */
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /**
     * 是否可读
     */
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /**
     * 是否可写
     */
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /**
     * 是否可执行
     */
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// page table structure
/// 多级页表是以节点为单位进行管理的。
/// 每个节点恰好存储在一个物理页帧中，它的位置可以用一个物理页号来表示。
/// 每个应用一个页表 root 用来区分
/// 每个应用所应用到的所有物理页帧都存放到 frame 中
pub struct PageTable {
    // 保存它根节点的物理页号 root_ppn 作为页表唯一的区分标志。
    root_ppn: PhysPageNum,
    // frames 以 FrameTracker 的形式保存了页表所有的节点（包括根节点）所在的物理页帧。
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    pub fn new() -> Self {
        // 分配一个物理页号
        let frame = frame_alloc().unwrap();
        PageTable {
            // 将物理页号挂到根节点
            root_ppn: frame.ppn,
            // 并将自己至于也表所有节点列表里
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    /// 临时创建一个专用来手动查页表的 PageTable
    /// 仅有一个从传入的 satp token 中得到的多级页表根节点的物理页号，
    /// frames 字段为空，也即不实际控制任何资源；
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /**
     * 根据虚拟地址查找或者创建一个新的页表项
     */
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        // 取出虚拟页表三级页索引
        let mut idxs = vpn.indexes();
        // 取出根节点的物理页号
        let mut ppn = self.root_ppn;
        // 物理位置
        // root[idxs[0]] 
        //   -- (*root[idxs[0]])[idxs[1]]
        //      -- (*(root[idxs[0]])[idxs[1]])[idxs[2]]
        // 获取结果
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter_mut().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                // 三级索引查找结束
                result = Some(pte);
                break;
            }
            // 如果当前页表不可用，说明未创建过
            if !pte.is_valid() {
                // 分配一个新的物理页号
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                // 将使用的物理页号保存关联
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }

    /// 在多级页表上遍历 遇到空指针它就会直接返回 None 
    /// 表示无法正确找到传入的虚拟页号对应的页表项；
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    /**
     * 每个页表被创建出来之后，为了方便 MMU 通过地址转换正确定
     * 找到应用地址空间中的数据实际被内存存放在内存中的位置，需要操作系统动态维护一个
     * 虚拟页号到页表项的映射
     * 
     * 通过 map 方法来在多级页表中插入一个键值对，注意这里我们将物理页号 ppn 和
     * 页表项标志位 flags 作为 不同的参数传入而不是整合为一个页表项
     */
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }

    /**
     * 通过 unmap 方法来删除一个键值对，在调用时仅需给出作为索引的虚拟页号即可。
     */
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }

    // 如果能够找到页表项，那么它会将页表项拷贝一份并返回，否则就 返回一个 None 。
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).copied()
    }
    /**
     * 按照 satp CSR 格式要求 构造一个无符号 64 位无符号整数
     */
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

/// translate a pointer to a mutable u8 Vec through page table
/// token 是某个应用地址空间的 token
/// ptr 和 len 则分别表示该地址空间中的一段缓冲区的起始地址 和长度
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}
