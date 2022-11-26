use crate::config::PAGE_SIZE;
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


impl PhysAddr{
    pub fn page_offset(&self) -> usize {self.0 & (PAGE_SIZE -1)}
    /**
     * 向下取整
     */
    pub fn floor(&self) -> PhysPageNum {PhysPageNum(self.0 / PAGE_SIZE)}
    /**
     * 向上取整
     */
    pub fn ceil(&self) -> PhysPageNum { PhysPageNum(self.0 + PAGE_SIZE -1) / PAGE_SIZE}
}

impl From<PhysAddr> for PhysPageNum{
    fn from(v: PhysAddr) -> Self{
        assert_eq!(v.page_offset(), 0);
        v.floor();
    }
}

impl From<PhysPageNum> for PhysPageNum{
    fn from(v: PhysPageNum) -> Self{
        Self(v.0 << PAGE_SIZE_BITS)
    }
}

