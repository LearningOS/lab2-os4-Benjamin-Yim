//! Implementation of physical and virtual address and page number.

use super::PageTableEntry;
use crate::config::{PAGE_SIZE, PAGE_SIZE_BITS};
use core::fmt::{self, Debug, Formatter};

/**
 * 物理地址
 */
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);

/**
 * 虚拟地址
 */
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);

/**
 * 物理页号
 */
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);

/**
 * 虚拟页号
 */
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);

/// Debugging

impl Debug for VirtAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl Debug for VirtPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#x}", self.0))
    }
}
impl Debug for PhysAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl Debug for PhysPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#x}", self.0))
    }
}

/// T: {PhysAddr, VirtAddr, PhysPageNum, VirtPageNum}
/// T -> usize: T.0
/// usize -> T: usize.into()

impl From<usize> for PhysAddr {
    fn from(v: usize) -> Self {
        Self(v)
    }
}
impl From<usize> for PhysPageNum {
    fn from(v: usize) -> Self {
        Self(v)
    }
}
impl From<usize> for VirtAddr {
    fn from(v: usize) -> Self {
        Self(v)
    }
}
impl From<usize> for VirtPageNum {
    fn from(v: usize) -> Self {
        Self(v)
    }
}
impl From<PhysAddr> for usize {
    fn from(v: PhysAddr) -> Self {
        v.0
    }
}
impl From<PhysPageNum> for usize {
    fn from(v: PhysPageNum) -> Self {
        v.0
    }
}
impl From<VirtAddr> for usize {
    fn from(v: VirtAddr) -> Self {
        v.0
    }
}
impl From<VirtPageNum> for usize {
    fn from(v: VirtPageNum) -> Self {
        v.0
    }
}

impl VirtAddr {
    pub fn floor(&self) -> VirtPageNum {
        VirtPageNum(self.0 / PAGE_SIZE)
    }
    pub fn ceil(&self) -> VirtPageNum {
        VirtPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE)
    }
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
    pub fn aligned(&self) -> bool {
        self.page_offset() == 0
    }
}
impl From<VirtAddr> for VirtPageNum {
    fn from(v: VirtAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}
impl From<VirtPageNum> for VirtAddr {
    fn from(v: VirtPageNum) -> Self {
        Self(v.0 << PAGE_SIZE_BITS)
    }
}
impl PhysAddr {
    /**
     * 向下取整
     */
    pub fn floor(&self) -> PhysPageNum {
        PhysPageNum(self.0 / PAGE_SIZE)
    }
    /**
     * 向上取整
     */    
    pub fn ceil(&self) -> PhysPageNum {
        PhysPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE)
    }
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
    pub fn aligned(&self) -> bool {
        self.page_offset() == 0
    }
}
impl From<PhysAddr> for PhysPageNum {
    fn from(v: PhysAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}
impl From<PhysPageNum> for PhysAddr {
    fn from(v: PhysPageNum) -> Self {
        Self(v.0 << PAGE_SIZE_BITS)
    }
}

// 建立和拆除虚实地址映射关系
impl VirtPageNum {
    /**
     * indexes 可以取出虚拟页号的三级页索引
     * ，并按照从高到低的顺序返回。注意它里面包裹的 usize 可能有 27 位，
     * 也有可能有 64-12=52 位，但这里我们是用来在多级页表上进行遍历，
     * 因此只取出低 27 位。
     */
    pub fn indexes(&self) -> [usize; 3] {
        let mut vpn = self.0;
        let mut idx = [0usize; 3];
        for i in (0..3).rev() {
            idx[i] = vpn & 511;
            vpn >>= 9;
        }
        idx
    }
}

// 每一个对应于某一特定物理页帧的物理页号 ppn ，均存在一个虚拟页号 vpn 能够映射到它
// 要能够较为简单的针对一个 ppn 找到某一个能映射到它的 vpn
// 这里我们采用一种最 简单的 恒等映射 (Identical Mapping) ，
// 也就是说对于物理内存上的每个物理页帧，我们都在多级页表中用一个与其 
// 物理页号相等的虚拟页号映射到它。当我们想针对物理页号构造一个能映射到它的虚拟页号的时候，
// 也只需使用一个和该物理页号 相等的虚拟页号即可。
// 参考[from_raw_parts_mut]https://rust.longyb.com/ch19-01-unsafe-rust.html
// 构造可变引用来直接访问一个物理页号 PhysPageNum 对应的物理页帧，
// 不同的引用类型对应于物理页帧上的一种不同的 内存布局
impl PhysPageNum {
    // 返回值类型上附加了 静态生命周期泛型 'static ，这是为了绕过 Rust 编译器的借用检查，
    // 实质上可以将返回的类型也看成一个裸指针，因为 它也只是标识数据存放的位置以及类型。
    // 但与裸指针不同的是，无需通过 unsafe 的解引用访问它指向的数据，而是可以像一个 
    // 正常的可变引用一样直接访问。

    //  返回的是一个页表项定长数组的可变引用，可以用来修改多级页表中的一个节点
    pub fn get_pte_array(&self) -> &'static mut [PageTableEntry] {
        // 先把物理页号转为物理地址 PhysAddr ，然后再转成 usize 形式的物理地址
        let pa: PhysAddr = (*self).into();
        unsafe { 
            // 我们直接将它 转为裸指针用来访问物理地址指向的物理内存
            // from_raw_parts_mut 函数通过指针和长度来创建一个新的切片，
            // 简单来说，该切片的初始地址是 data 指针 ，长度为 len
            core::slice::from_raw_parts_mut(pa.0 as *mut PageTableEntry, 512) 
        }
    }
    // 返回的是一个字节数组的可变引用，可以以字节为粒度
    // 对物理页帧上的数据进行访问，4K 大小每页
    pub fn get_bytes_array(&self) -> &'static mut [u8] {
        // 先把物理页号转为物理地址 PhysAddr ，然后再转成 usize 形式的物理地址
        let pa: PhysAddr = (*self).into();
        unsafe { 
            // 我们直接将它 转为裸指针用来访问物理地址指向的物理内存
            // from_raw_parts_mut 函数通过指针和长度来创建一个新的切片，
            // 简单来说，该切片的初始地址是 data 指针 ，长度为 len
            core::slice::from_raw_parts_mut(pa.0 as *mut u8, 4096) 
        }
    }
    // 可以获取一个恰好放在一个物理页帧开头的类型为 T 的数据的可变引用
    pub fn get_mut<T>(&self) -> &'static mut T {
        // 先把物理页号转为物理地址 PhysAddr ，然后再转成 usize 形式的物理地址
        let pa: PhysAddr = (*self).into();
        unsafe { 
            // 我们直接将它 转为裸指针用来访问物理地址指向的物理内存
            (pa.0 as *mut T).as_mut().unwrap() 
        }
    }
}

pub trait StepByOne {
    fn step(&mut self);
}
impl StepByOne for VirtPageNum {
    fn step(&mut self) {
        self.0 += 1;
    }
}

#[derive(Copy, Clone)]
/// a simple range structure for type T
pub struct SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    l: T,
    r: T,
}
impl<T> SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    pub fn new(start: T, end: T) -> Self {
        assert!(start <= end, "start {:?} > end {:?}!", start, end);
        Self { l: start, r: end }
    }
    pub fn get_start(&self) -> T {
        self.l
    }
    pub fn get_end(&self) -> T {
        self.r
    }
}
impl<T> IntoIterator for SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    type Item = T;
    type IntoIter = SimpleRangeIterator<T>;
    fn into_iter(self) -> Self::IntoIter {
        SimpleRangeIterator::new(self.l, self.r)
    }
}
/// iterator for the simple range structure
pub struct SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    current: T,
    end: T,
}
impl<T> SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    pub fn new(l: T, r: T) -> Self {
        Self { current: l, end: r }
    }
}
impl<T> Iterator for SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.end {
            None
        } else {
            let t = self.current;
            self.current.step();
            Some(t)
        }
    }
}

/// a simple range structure for virtual page number
pub type VPNRange = SimpleRange<VirtPageNum>;
