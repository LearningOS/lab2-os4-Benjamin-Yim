//! Process management syscalls

use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE, KERNEL_STACK_SIZE, MEMORY_END};
use crate::mm::memory_set::{MapArea, MapType, self, MemorySet};
use crate::mm::{VirtAddr, PhysAddr, MapPermission};
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, current_user_token, get_current_task_info, kernel_sys_mmap, kernel_sys_munmap};
use crate::timer::get_time_us;
use crate::mm::page_table::PageTable;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Debug,Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    info!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    // ts to ppa
    let user_token = current_user_token();
    let page_table = PageTable::from_token(user_token);
    let ptr = ts  as usize;
    let va = VirtAddr::from(ptr);
    // 第一次的时候漏掉了
    let page_offset = va.page_offset();
    let vpn = va.floor();
    let ppn = page_table.translate(vpn).unwrap().ppn();
    let pa = PhysAddr::from(PhysAddr::from(ppn).0 | page_offset);
    let us = get_time_us();
    let sec = us / 1_000_000;
    let usec = us % 1_000_000;
    // 向物理地址写数据
    let time_val = pa.0 as *mut TimeVal;
    unsafe{
        *time_val = TimeVal {
            sec,
            usec,
        };
    }
    0
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    if _len == 0{
        return 0;
    }    
    if _start > 268439552 || _start % PAGE_SIZE != 0{
        return  -1;
    }
    if _port &!0x7 != 0 || _port &0x7 == 0{
        return -1;
    }
    let mut permission = MapPermission::U;
    if _port & 1 == 1{
        permission  |= MapPermission::R;
    }
    if _port & 2 == 2{
        permission  |= MapPermission::W;
    }
    if _port & 4 == 4{
        permission  |= MapPermission::X;
    }
    if !kernel_sys_mmap(_start,_len,permission){
        // println!("mmap _start:{}, _len:{},result:{}",_start, _len, -1);
        return -1;
    }
    // println!("mmap _start:{}, _len:{},result:{}",_start, _len, 0);
    0
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    // if _len % PAGE_SIZE != 0{
    //     println!("munmap _start:{}, _len:{} % PAGE_SIZE != 0, result:{} ",VirtAddr::from(_start).floor().0, _len,-1);
    //     return  -1;
    // }
    // if kernel_sys_munmap(_start,_len){
    //     println!("======munmap start:{}, end:{}, result:{}", VirtAddr::from(_start).floor().0, VirtAddr::from(_start+_len).ceil().0,-1);
    //     return -1;
    // }
    // println!("--------munmap start:{}, end:{}, result:{}",VirtAddr::from(_start).floor().0, VirtAddr::from(_start+_len).ceil().0,0);
    // 0
    kernel_sys_munmap(_start,_len)
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let user_token = current_user_token();
    let page_table = PageTable::from_token(user_token);
    let ptr = ti  as usize;
    let va = VirtAddr::from(ptr);
    let page_offset = va.page_offset();
    let vpn = va.floor();
    let ppn = page_table.translate(vpn).unwrap().ppn();
    let pa = PhysAddr::from(PhysAddr::from(ppn).0 | page_offset);
    let current_task = get_current_task_info();
    // 向物理地址写数据
    let task_info = pa.0 as *mut TaskInfo;
    unsafe{
        *task_info = TaskInfo {
            status: current_task.status,
            syscall_times: current_task.syscall_times,
            time: (get_time_us() - current_task.time)/1_000,
        };
    }
    0
}
