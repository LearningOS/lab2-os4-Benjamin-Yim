use alloc::{collections::BTreeMap, vec::Vec};
use riscv::addr::page;

use super::{
    address::{VirtAddr, VirtPageNum},
    page_table::PageTable,
};

bitflags! {
    pub struct MapPermission: u8{
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
    }
}

/**
 * MapType 描述该逻辑段内的所有虚拟页面映射到物理页帧的同一种方式
 * Identical 表示之前也有提到的恒等映射，用于在启用多级页表之后仍能够
 * 访问一个特定的物理地址指向的物理内存；
 * Framed 则表示对于每个虚拟页面都需要映射到一个新分配的物理页帧。
 *
 *  MapType::Framed 方式映射到物理内存的时候， data_frames 是
 * 一个保存了该逻辑段内的每个虚拟页面 和它被映射到的物理页帧 FrameTracker
 * 的一个键值对容器 BTreeMap 中，这些物理页帧被用来存放实际内存数据而不是
 * 作为多级页表中的中间节点
 *
 * MapPermission 表示控制该逻辑段的访问方式，它是页表项标志位
 * PTEFlags 的一个子集，仅保留 U/R/W/X 四个标志位
 */
pub enum MapType {
    Identical,
    Framed,
}
/**
 *  逻辑段 MapArea 为单位描述一段连续地址的虚拟内存。所谓逻辑段，
 *  就是指地址区间中的一段实际可用（即 MMU 通过查多级页表 可以正确完成地址转换）
 *  的地址连续的虚拟地址区间，该区间内包含的所有虚拟页面都以一种相同的方式映射到物理页帧，
 *  具有可读/可写/可执行等属性。
 *  VPNRange 描述一段虚拟页号的连续区间，表示该逻辑段在地址区间中的位置和长度
 */
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

/**
 * 地址空间：一系列有关联的逻辑段
 * 地址空间是一系列有关联的逻辑段，这种关联一般是指这些逻辑段属于一个运行的程序
 * 用来表明正在运行的应用所在执行环境中的可访问内存空间，在这个内存空间中，
 * 包含了一系列的不一定连续的逻辑段。
 * 多级页表 page_table 和一个逻辑段 MapArea 的向量 areas
 * PageTable 下 挂着所有多级页表的节点所在的物理页帧
 * MapArea 下则挂着对应逻辑段中的数据所在的物理页帧
 * 这两部分 合在一起构成了一个地址空间所需的所有物理页帧
 * */
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}

impl MapArea {
    /**
     * 新建一个逻辑段结构体，注意传入的起始/终止虚拟地址会分别被下取整/上
     * 取整为虚拟页号并传入 迭代器 vpn_range 中
     */
    pub fn new(start_va: VirtAddr, end_va: VirtAddr, map_type: MapType, map_perm: MapPermission) {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_va, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
    /**
     * 可以将当前逻辑段到物理内存的映射从传入的该逻辑段所属的地址空间的 多级页表中加入
     */
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            // 每个虚拟页面为单位依次在多级页表中进行 键值对的插入
            self.map_one(page_table, vpn);
        }
    }

    /**
     * 可以将当前逻辑段到物理内存的映射从传入的该逻辑段所属的地址空间的 多级页表中删除
     */
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            // 每个虚拟页面为单位依次在多级页表中进行 键值对的删除
            self.unmap_one(page_table, vpn);
        }
    }

    /**
     * copy_data 方法将切片 data 中的数据拷贝到当前逻辑段实际被内核放置在的各物理页帧
     * 上，从而 在地址空间中通过该逻辑段就能访问这些数据。
     * 
     * 切片 data 中的数据大小不超过当前逻辑段的 总大小，且切片中的数据会被对齐
     * 到逻辑段的开头，然后逐页拷贝到实际的物理页帧。
     */
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start = 0usize;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            // 循环会遍历每一个需要拷贝数据的虚拟页面，
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            // 在数据拷贝完成后调用 step 方法，该 方法来自于 os/src/mm/address.rs
            //  中为 VirtPageNum 实现的 StepOne Trait
            // 每个页面的数据拷贝需要确定源 src 和目标 dst 两个切片并直接使用 
            // copy_from_slice 完成复制
            current_vpn.step();
        }
    }
}

impl MemorySet {
    // 新建一个空的地址空间
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }
    /**
     * 在当前地址空间插入一个新的逻辑段 map_area
     * 如果它是以 Framed 方式映射到 物理内存，
     * 还可以可选地在那些被映射到的物理页帧上写入一些初始化数据 data 
     */
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        // 调用 push ，可以在当前地址空间插入一个 Framed 方式映射到 物理内存的逻辑段
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }
    // new_kernel 可以生成内核的地址空间
    pub fn new_kernel() -> Self;
    // from_elf 则可以应用的 ELF 格式可执行文件 解析出各数据段并对应生成应用的地址空间
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize);
}


impl MemoryArea {
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;

        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
    }
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        match self.map_type {
            MapType::Framed => {
                self.data_frames.remove(&vpn);
            }
            _ => {}
        }
        page_table.unmap(vpn);
    }
}