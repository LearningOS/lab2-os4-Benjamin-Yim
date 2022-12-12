//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use core::borrow::{Borrow, BorrowMut};

use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE};
use crate::loader::{get_app_data, get_num_app};
use crate::mm::memory_set::{MapType, MapArea};
use crate::mm::{MapPermission, VirtAddr, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::syscall;
use crate::syscall::process::TaskInfo;
use crate::timer::get_time_us;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
pub use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        info!("init TASK_MANAGER");
        let num_app = get_num_app();
        info!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        next_task.time = get_time_us();
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn sys_mmap(&self,start: usize, len: usize, permission: MapPermission) -> bool{
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;
        let start_vpn = VirtAddr::from(start).floor();
        let end_vpn = VirtAddr::from(start+len).ceil();
        let areas: &Vec<MapArea> =  inner.tasks[current_task].memory_set.areas.borrow();
        for ele in  areas{
            // 判断是否在范围内
        //    if start_vpn <= ele.vpn_range.get_start()  && ele.vpn_range.get_end() <= end_vpn {
        //         return false;
        //    }
           let start = ele.vpn_range.get_start();
            let end = ele.vpn_range.get_end();
            if start_vpn < end && end_vpn > start {
                return false;
            }
        }
        // {
        //     let mut start = start_vpn.0;
        //     while start < end_vpn.0{
        //         if inner.tasks[current_task].memory_set.range(start, start+1){
        //             return false;
        //         }
        //         start+=1usize;
        //     }
        // }
        // let mut start_va = start;
        // let end_vpn = start + len;
        // while start_va < end_vpn {
        //     inner.tasks[current_task].memory_set.insert_framed_area(VirtAddr::from(start_va) ,VirtAddr::from(start_va+PAGE_SIZE),permission);
        //     start_va += PAGE_SIZE;
        // }
        // println!("insert_framed_area start:{} end:{}",VirtAddr::from(start).floor().0 ,VirtAddr::from(start+len).ceil().0);
        inner.tasks[current_task].memory_set.insert_framed_area(start_vpn.into() ,end_vpn.into(),permission);
        // 拆分每页
        // let mut start = start_vpn.0;
        // while start < end_vpn.0{
        //     inner.tasks[current_task].memory_set.insert_framed_area(VirtPageNum::from(start).into() ,VirtPageNum::from(start+1).into() ,permission);
        //     start+=1usize;
        // }
        true
    }

    #[allow(clippy::mut_from_ref)]
    fn sys_munmap(&self,start: usize, len: usize) -> isize{

        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;

        let memory_set = &mut inner.tasks[current_task].memory_set;
        memory_set.remove(start, len)


        // let start_vpn = VirtAddr(start).floor();
        // let end_vpn = VirtAddr(start+len).ceil();

        // let mut start_index = start_vpn.0;

        // let mut exsit = 0;
        // for _ in 0..max{
        //     for item in 0..inner.tasks[current_task].memory_set.areas.len(){
        //         let memory_set = &mut inner.tasks[current_task].memory_set;
        //         println!("range start:{} end:{},the start:{} end:{},len:{}",memory_set.areas[item].vpn_range.get_start().0,memory_set.areas[item].vpn_range.get_end().0, start_vpn.0,end_vpn.0,len);
        //         if VirtPageNum::from(start_index) == memory_set.areas[item].vpn_range.get_start()  && memory_set.areas[item].vpn_range.get_end() == VirtPageNum::from(start_index+1) {
        //             exsit += 1;
        //         }
        //     }
        //     start_index+=1;
        // }
        
        // if exsit == 0{
        //     println!("no exist so return false=>the start:{} end:{},len:{}",start_vpn.0,end_vpn.0,len);
        //     return false;
        // }
        // println!(" exist so return true=>the start:{} end:{},len:{},exsit:{}",start_vpn.0,end_vpn.0,len,exsit);

        // start_index = start_vpn.0;
        // for _ in 0..max{
        //     for item in 0..inner.tasks[current_task].memory_set.areas.len(){
        //         let memory_set = &mut inner.tasks[current_task].memory_set;
        //         if item >= memory_set.areas.len(){
        //                 continue;
        //         }
        //         if VirtPageNum::from(start_index) == memory_set.areas[item].vpn_range.get_start()  && memory_set.areas[item].vpn_range.get_end() == VirtPageNum::from(start_index+1) {
        //             println!("removing start:{} end:{}",memory_set.areas[item].vpn_range.get_start().0,memory_set.areas[item].vpn_range.get_end().0);
        //             memory_set.areas[item].unmap(&mut memory_set.page_table);
        //             memory_set.areas.remove(item);
        //         }
        //     }
        //     start_index+=1;
        // }
        // for item in 0..inner.tasks[current_task].memory_set.areas.len(){
        //     let memory_set = &mut inner.tasks[current_task].memory_set;
        //     println!("remove after range start:{} end:{}",memory_set.areas[item].vpn_range.get_start().0,memory_set.areas[item].vpn_range.get_end().0);
        // }
        // true
    }


    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn get_current_task_info(&self) -> syscall::process::TaskInfo {
        let inner = self.inner.exclusive_access();
         syscall::process::TaskInfo{
            status: inner.tasks[inner.current_task].task_status.clone(),
            syscall_times:inner.tasks[inner.current_task].syscall_times.clone(),
            time: inner.tasks[inner.current_task].time,
         }
    }

    fn inc_current_task_syscall(&self,syscall_id: usize){
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;
        inner.tasks[current_task].syscall_times[syscall_id]+=1;
    }
    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            if inner.tasks[next].time == 0 {
                inner.tasks[next].time = get_time_us();
            }
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}


/// Get the current 'Running' task's trap contexts.
pub fn get_current_task_info() -> TaskInfo {
    TASK_MANAGER.get_current_task_info()
}

/// Get the current 'Running' task's trap contexts.
pub fn inc_current_task_syscall(syscall_id: usize) {
    TASK_MANAGER.inc_current_task_syscall(syscall_id)
}

/// Get the current 'Running' task's trap contexts.
pub fn kernel_sys_mmap(start: usize, len: usize, port: MapPermission) -> bool {
    TASK_MANAGER.sys_mmap(start,len,port)
}


pub fn kernel_sys_munmap(_start: usize, _len: usize) -> isize{
    // 不小心把 _len 写错 _start 排查 3 小时
    TASK_MANAGER.sys_munmap(_start,_len)
}