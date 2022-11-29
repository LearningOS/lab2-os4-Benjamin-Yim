//! Implementation of [`MapArea`] and [`MemorySet`].

use super::{frame_alloc, FrameTracker};
use super::{PTEFlags, PageTable, PageTableEntry};
use super::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use super::{StepByOne, VPNRange};
use crate::config::{MEMORY_END, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT, USER_STACK_SIZE};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;
use riscv::register::satp;
use spin::Mutex;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// a memory set instance through lazy_static! managing kernel space
    pub static ref KERNEL_SPACE: Arc<Mutex<MemorySet>> =
        Arc::new(Mutex::new(MemorySet::new_kernel()));
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
/// memory set structure, controls virtual-memory space
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}

impl MemorySet {
    // 新建一个空的地址空间
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// Assume that no conflicts.
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
    /// Mention that trampoline is not collected by areas.
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }
    /// Without kernel stacks.
    // new_kernel 可以生成内核的地址空间
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map kernel sections
        info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        info!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        info!("mapping .text section");
        memory_set.push(
            MapArea::new(
                (stext as usize).into(),
                (etext as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            None,
        );
        info!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        );
        info!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        memory_set
    }
    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp and entry point.
    // from_elf 则可以应用的 ELF 格式可执行文件 解析出各数据段并对应生成应用的地址空间
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        // map trampoline
        // 我们将跳板插入到应用地址空间；
        memory_set.map_trampoline();
        // map program headers of elf, with U flag
        // 我们使用外部 crate xmas_elf 来解析传入的应用 ELF 数据并可以轻松取出各个部分
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        // 我们取出 ELF 的魔数来判断 它是不是一个合法的 ELF
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            // 我们可以直接得到 program header 的数目，
            // 然后遍历所有的 program header 并将合适的区域加入 到应用地址空间中
            let ph = elf.program_header(i).unwrap();
            // 确认 program header 的类型是 LOAD ， 这表明它有被内核加载的必要，
            // 此时不必理会其他类型的 program header 。
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                // 通过 ph.virtual_addr() 和 ph.mem_size() 来计算这一区域在应用地址空间中的位置
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                // 确认这一区域访问方式的 限制并将其转换为 MapPermission 类型
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                // max_end_vpn 记录目前涉及到的最大的虚拟页号
                max_end_vpn = map_area.vpn_range.get_end();
                // 当前 program header 数据被存放的位置可以通过 ph.offset() 和 ph.file_size() 来找到
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }
        // map user stack with U flags
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into();
        // guard page
        user_stack_bottom += PAGE_SIZE;
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE;
        // 应用地址空间中映射次高页面来存放 Trap 上下文。
        memory_set.push(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // map TrapContext
        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        // 返回应用地址空间 memory_set ，也同时返回用户栈虚拟地址 user_stack_top
        // 以及从解析 ELF 得到的该应用入口点地址
        (
            memory_set,
            user_stack_top,
            elf.header.pt2.entry_point() as usize,
        )
    }

    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            core::arch::asm!("sfence.vma");
        }
    }
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }
}


/**
 *  逻辑段 MapArea 为单位描述一段连续地址的虚拟内存。所谓逻辑段，
 *  就是指地址区间中的一段实际可用（即 MMU 通过查多级页表 可以正确完成地址转换）
 *  的地址连续的虚拟地址区间，该区间内包含的所有虚拟页面都以一种相同的方式映射到物理页帧，
 *  具有可读/可写/可执行等属性。
 *  VPNRange 描述一段虚拟页号的连续区间，表示该逻辑段在地址区间中的位置和长度
 */
/// map area structure, controls a contiguous piece of virtual memory
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

impl MapArea {
    /**
     * 新建一个逻辑段结构体，注意传入的起始/终止虚拟地址会分别被下取整/上
     * 取整为虚拟页号并传入 迭代器 vpn_range 中
     */    
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
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
    #[allow(unused)]
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        #[allow(clippy::single_match)]
        match self.map_type {
            MapType::Framed => {
                self.data_frames.remove(&vpn);
            }
            _ => {}
        }
        page_table.unmap(vpn);
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
    #[allow(unused)]
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
    /// data: start-aligned but maybe with shorter length
    /// assume that all frames were cleared before
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
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
#[derive(Copy, Clone, PartialEq, Debug)]
/// map type for memory set: identical or framed
pub enum MapType {
    Identical,
    Framed,
}

bitflags! {
    /// map permission corresponding to that in pte: `R W X U`
    pub struct MapPermission: u8 {
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
    }
}

#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.lock();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert!(!kernel_space
        .page_table
        .translate(mid_text.floor())
        .unwrap()
        .writable());
    assert!(!kernel_space
        .page_table
        .translate(mid_rodata.floor())
        .unwrap()
        .writable());
    assert!(!kernel_space
        .page_table
        .translate(mid_data.floor())
        .unwrap()
        .executable());
    info!("remap_test passed!");
}
