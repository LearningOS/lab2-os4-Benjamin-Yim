
use bitflags::*;

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