//! 进程管理

use crate::arch::riscv::*;
use crate::arch::riscv::csr::{SSTATUS_SPIE, SSTATUS_SPP, SSTATUS_SUM};
use crate::kernel::trap::TrapFrame;
use crate::kernel::pgtable;
use crate::kernel::vm;
use crate::kernel::ipc::{Endpoint, IpcResult, Message, MAX_ENDPOINTS};
use crate::println;
use spin::Mutex;

const MAX_TASKS: usize = 16;

/// 进程状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskState {
    Unused,
    Ready,
    Running,
    Sleeping,
    Zombie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitChannel {
    Child(u32),
    UartRx,
    IpcCall(usize),
    IpcRecv(usize),
}

/// 进程控制块
pub struct Task {
    pub pid: u32,
    pub state: TaskState,
    pub trap_frame: usize,
    pub kernel_stack: usize,
    pub user_stack: usize,
    pub entry: usize,
    pub pagetable: &'static mut pgtable::PageTable,
    pub exit_code: i32,
    pub image_base: usize,
    pub image_size: usize,
    pub heap_base: usize,
    pub heap_end: usize,
    pub parent_pid: Option<u32>,
    pub waiting_for: Option<WaitChannel>,
}

pub enum WaitResult {
    Reaped(i32),
    Blocked(*mut TrapFrame),
    Error,
}

/// 进程管理器
pub struct TaskManager {
    /// 当前进程
    current_pid: Option<u32>,
    /// 进程列表
    tasks: [Option<Task>; MAX_TASKS],
    /// 下一个PID
    next_pid: u32,
    /// 可运行进程数
    runnable_count: u32,
    endpoints: [Option<Endpoint>; MAX_ENDPOINTS],
    uart_owner: Option<u32>,
}

/// 全局进程管理器
static TASK_MANAGER: Mutex<Option<TaskManager>> = Mutex::new(None);

impl TaskManager {
    const IDLE_PID: u32 = 0;
    // 用户栈占用最后一页，并在堆与栈之间保留一页不可映射的保护页。
    const HEAP_LIMIT: usize = USER_STACK_TOP - 2 * PAGE_SIZE;

    fn align_up(value: usize) -> Result<usize, ()> {
        value
            .checked_add(PAGE_SIZE - 1)
            .map(|value| value & !(PAGE_SIZE - 1))
            .ok_or(())
    }

    fn free_user_space(
        pagetable: &mut pgtable::PageTable,
        user_stack: usize,
        image_base: usize,
        image_size: usize,
        heap_base: usize,
        heap_end: usize,
    ) {
        let start = image_base & !(PAGE_SIZE - 1);
        let end = (image_base + image_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        for va in (start..end).step_by(PAGE_SIZE) {
            if let Some(pte) = pgtable::walk(pagetable, va, false) {
                if (*pte & PTE_V) != 0 {
                    crate::kernel::page::free(pgtable::pte_to_pa(*pte));
                    *pte = 0;
                }
            }
        }
        let heap_mapped_end = Self::align_up(heap_end).unwrap_or(heap_base);
        for va in (heap_base..heap_mapped_end).step_by(PAGE_SIZE) {
            if let Some(pte) = pgtable::walk(pagetable, va, false) {
                if (*pte & PTE_V) != 0 {
                    crate::kernel::page::free(pgtable::pte_to_pa(*pte));
                    *pte = 0;
                }
            }
        }
        if user_stack != 0 {
            crate::kernel::page::free(user_stack);
        }
        pgtable::free(pagetable);
    }

    fn free_address_space(task: &mut Task) {
        Self::free_user_space(
            task.pagetable,
            task.user_stack,
            task.image_base,
            task.image_size,
            task.heap_base,
            task.heap_end,
        );
    }

    fn free_task(mut task: Task) {
        Self::free_address_space(&mut task);
        crate::kernel::page::free(task.kernel_stack);
    }

    fn reap_detached_zombies(&mut self) {
        let current_pid = self.current_pid;
        for index in 0..MAX_TASKS {
            let reap = self.tasks[index].as_ref().is_some_and(|task| {
                task.state == TaskState::Zombie
                    && task.parent_pid.is_none()
                    && Some(task.pid) != current_pid
            });
            if reap {
                let task = self.tasks[index].take().unwrap();
                println!("task reap: pid={} exit_code={}", task.pid, task.exit_code);
                Self::free_task(task);
            }
        }
    }

    fn init_context(
        kernel_stack: usize,
        pagetable: &pgtable::PageTable,
        frame: &TrapFrame,
    ) -> usize {
        let frame_addr = kernel_stack + PAGE_SIZE - 16 - core::mem::size_of::<TrapFrame>();
        unsafe {
            (frame_addr as *mut TrapFrame).write(frame.clone());
            ((frame_addr + core::mem::size_of::<TrapFrame>()) as *mut usize)
                .write(vm::kernel_satp());
            ((frame_addr + core::mem::size_of::<TrapFrame>() + 8) as *mut usize)
                .write((8usize << 60) | ((pagetable as *const pgtable::PageTable as usize) >> PAGE_SHIFT));
        }
        frame_addr
    }

    /// 创建新的进程管理器
    pub fn new() -> Self {
        Self {
            current_pid: None,
            tasks: core::array::from_fn(|_| None),
            next_pid: 1,
            runnable_count: 0,
            endpoints: core::array::from_fn(|_| None),
            uart_owner: None,
        }
    }

    /// 创建唯一的内核态 idle 任务。PID 0 不占用普通 PID，也不计入 runnable_count。
    pub fn create_idle(&mut self, entry: usize) -> Result<u32, ()> {
        if self.tasks.iter().flatten().any(|task| task.pid == Self::IDLE_PID) {
            return Err(());
        }
        let slot_index = self.tasks.iter().position(|slot| slot.is_none()).ok_or(())?;

        let kernel_stack = crate::kernel::page::alloc().ok_or(())?;
        let pagetable = pgtable::create().ok_or_else(|| {
            crate::kernel::page::free(kernel_stack);
        })?;
        if vm::map_kernel(pagetable).is_err() {
            pgtable::free(pagetable);
            crate::kernel::page::free(kernel_stack);
            return Err(());
        }

        let mut frame = TrapFrame::new();
        frame.sp = kernel_stack + PAGE_SIZE - 16;
        frame.sepc = entry;
        // SPP=1 使 sret 返回 S-mode；SPIE=1 使 idle 中可以响应中断。
        frame.sstatus = SSTATUS_SPP | SSTATUS_SPIE;
        let trap_frame = Self::init_context(kernel_stack, pagetable, &frame);

        let idle = Task {
            pid: Self::IDLE_PID,
            state: TaskState::Ready,
            trap_frame,
            kernel_stack,
            user_stack: 0,
            entry,
            pagetable,
            exit_code: 0,
            image_base: 0,
            image_size: 0,
            heap_base: 0,
            heap_end: 0,
            parent_pid: None,
            waiting_for: None,
        };
        self.tasks[slot_index] = Some(idle);
        println!("task create idle: pid=0 entry=0x{:x}", entry);
        Ok(Self::IDLE_PID)
    }
    
    /// 接管已经加载好的 ELF 地址空间并创建用户进程。
    pub fn create_user_image(
        &mut self,
        pagetable: &'static mut pgtable::PageTable,
        user_stack: usize,
        entry: usize,
        image_base: usize,
        image_size: usize,
    ) -> Result<u32, ()> {
        let heap_base = image_base.checked_add(image_size).ok_or(())?;
        if heap_base > Self::HEAP_LIMIT || heap_base % PAGE_SIZE != 0 {
            Self::free_user_space(
                pagetable,
                user_stack,
                image_base,
                image_size,
                heap_base,
                heap_base,
            );
            return Err(());
        }
        let slot_index = match self.tasks.iter().position(|slot| slot.is_none()) {
            Some(index) => index,
            None => {
                Self::free_user_space(
                    pagetable,
                    user_stack,
                    image_base,
                    image_size,
                    heap_base,
                    heap_base,
                );
                return Err(());
            }
        };
        let kernel_stack = match crate::kernel::page::alloc() {
            Some(stack) => stack,
            None => {
                Self::free_user_space(
                    pagetable,
                    user_stack,
                    image_base,
                    image_size,
                    heap_base,
                    heap_base,
                );
                return Err(());
            }
        };

        let pid = self.next_pid;
        self.next_pid += 1;
        let mut initial_frame = TrapFrame::new();
        initial_frame.sp = USER_STACK_TOP;
        initial_frame.sepc = entry;
        initial_frame.sstatus = SSTATUS_SPIE | SSTATUS_SUM;
        let trap_frame = Self::init_context(kernel_stack, pagetable, &initial_frame);

        let task = Task {
            pid,
            state: TaskState::Ready,
            trap_frame,
            kernel_stack,
            user_stack,
            entry,
            pagetable,
            exit_code: 0,
            image_base,
            image_size,
            heap_base,
            heap_end: heap_base,
            parent_pid: self.current_pid,
            waiting_for: None,
        };

        self.tasks[slot_index] = Some(task);
        self.runnable_count += 1;

        println!("task create user: pid={} entry=0x{:x} user_sp=0x{:x}",
                 pid, entry, USER_STACK_TOP);

        Ok(pid)
    }
    
    /// 获取当前进程
    pub fn current(&self) -> Option<&Task> {
        let pid = self.current_pid?;
        self.tasks
            .iter()
            .filter_map(|task| task.as_ref())
            .find(|task| task.pid == pid)
    }
    
    /// 获取当前进程 (可变)
    pub fn current_mut(&mut self) -> Option<&mut Task> {
        let pid = self.current_pid?;
        self.tasks
            .iter_mut()
            .filter_map(|task| task.as_mut())
            .find(|task| task.pid == pid)
    }
    
    /// 设置当前进程
    pub fn set_current(&mut self, pid: u32) {
        for task in self.tasks.iter_mut().filter_map(|task| task.as_mut()) {
            if task.pid == pid {
                task.state = TaskState::Running;
                self.current_pid = Some(pid);
                break;
            }
        }
    }
    
    /// 调度下一个进程
    pub fn schedule(&mut self, tf: &TrapFrame) -> usize {
        self.reap_detached_zombies();
        let current_index = self.current_pid.and_then(|pid| {
            self.tasks.iter().position(|slot| slot.as_ref().is_some_and(|task| task.pid == pid))
        });
        if let Some(index) = current_index {
            if let Some(current) = self.tasks[index].as_mut() {
                unsafe { (current.trap_frame as *mut TrapFrame).write(tf.clone()); }
                if current.state == TaskState::Running { current.state = TaskState::Ready; }
            }
        }

        let start = current_index.map_or(0, |index| (index + 1) % MAX_TASKS);
        for offset in 0..MAX_TASKS {
            let index = (start + offset) % MAX_TASKS;
            if self.tasks[index].as_ref().is_some_and(|task| {
                task.pid != Self::IDLE_PID && task.state == TaskState::Ready
            }) {
                let task = self.tasks[index].as_mut().unwrap();
                task.state = TaskState::Running;
                self.current_pid = Some(task.pid);
                return task.trap_frame;
            }
        }

        // 普通任务全部阻塞时才运行 PID 0。
        if let Some(idle) = self.tasks.iter_mut().flatten()
            .find(|task| task.pid == Self::IDLE_PID)
        {
            idle.state = TaskState::Running;
            self.current_pid = Some(Self::IDLE_PID);
            return idle.trap_frame;
        }

        // 初始化阶段的保底路径；正常启动后一定存在 PID 0。
        tf as *const TrapFrame as usize
    }

    pub fn fork(&mut self, tf: &mut TrapFrame) -> Result<u32, ()> {
        if self.current_pid == Some(Self::IDLE_PID) {
            return Err(());
        }
        let parent_index = self.tasks.iter().position(|slot| {
            slot.as_ref().is_some_and(|task| Some(task.pid) == self.current_pid)
        }).ok_or(())?;
        let (image_base, image_size, heap_base, heap_end, entry, parent_stack, parent_pt) = {
            let parent = self.tasks[parent_index].as_mut().ok_or(())?;
            (
                parent.image_base,
                parent.image_size,
                parent.heap_base,
                parent.heap_end,
                parent.entry,
                parent.user_stack,
                parent.pagetable as *mut pgtable::PageTable,
            )
        };

        let child_kernel_stack = crate::kernel::page::alloc().ok_or(())?;
        let child_user_stack = crate::kernel::page::alloc().ok_or(())?;
        let child_pt = pgtable::create().ok_or(())?;
        vm::map_kernel(child_pt)?;
        let start = image_base & !(PAGE_SIZE - 1);
        let end = (image_base + image_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        for va in (start..end).step_by(PAGE_SIZE) {
            let source_pte = pgtable::walk(unsafe { &mut *parent_pt }, va, false)
                .map(|pte| *pte).filter(|pte| pte & PTE_V != 0);
            let Some(source_pte) = source_pte else { continue };
            let page = crate::kernel::page::alloc().ok_or(())?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    pgtable::pte_to_pa(source_pte) as *const u8,
                    page as *mut u8,
                    PAGE_SIZE,
                );
            }
            pgtable::map(child_pt, va, page, PAGE_SIZE, source_pte & 0x3fe)?;
        }
        let heap_mapped_end = Self::align_up(heap_end)?;
        for va in (heap_base..heap_mapped_end).step_by(PAGE_SIZE) {
            let source_pte = pgtable::walk(unsafe { &mut *parent_pt }, va, false)
                .map(|pte| *pte)
                .filter(|pte| pte & PTE_V != 0)
                .ok_or(())?;
            let page = crate::kernel::page::alloc().ok_or(())?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    pgtable::pte_to_pa(source_pte) as *const u8,
                    page as *mut u8,
                    PAGE_SIZE,
                );
            }
            pgtable::map(child_pt, va, page, PAGE_SIZE, source_pte & 0x3fe)?;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                parent_stack as *const u8,
                child_user_stack as *mut u8,
                PAGE_SIZE,
            );
        }
        pgtable::map(
            child_pt,
            USER_STACK_TOP - PAGE_SIZE,
            child_user_stack,
            PAGE_SIZE,
            PTE_R | PTE_W | PTE_U,
        )?;

        let pid = self.next_pid;
        self.next_pid += 1;
        let mut child_frame = tf.clone();
        child_frame.a0 = 0;
        let child_tf = Self::init_context(child_kernel_stack, child_pt, &child_frame);
        let child = Task {
            pid,
            state: TaskState::Ready,
            trap_frame: child_tf,
            kernel_stack: child_kernel_stack,
            user_stack: child_user_stack,
            entry,
            pagetable: child_pt,
            exit_code: 0,
            image_base,
            image_size,
            heap_base,
            heap_end,
            parent_pid: self.current_pid,
            waiting_for: None,
        };
        let slot = self.tasks.iter_mut().find(|slot| slot.is_none()).ok_or(())?;
        *slot = Some(child);
        self.runnable_count += 1;
        tf.a0 = pid as usize;
        println!("fork: parent={} child={}", self.current_pid.unwrap_or(0), pid);
        Ok(pid)
    }

    pub fn replace_current(
        &mut self,
        pagetable: &'static mut pgtable::PageTable,
        user_stack: usize,
        entry: usize,
        image_base: usize,
        image_size: usize,
        tf: &mut TrapFrame,
    ) -> Result<(), ()> {
        let heap_base = image_base.checked_add(image_size).ok_or(())?;
        if heap_base > Self::HEAP_LIMIT || heap_base % PAGE_SIZE != 0 {
            return Err(());
        }
        let user_satp = (8usize << 60)
            | ((pagetable as *const pgtable::PageTable as usize) >> PAGE_SHIFT);
        let (
            old_pagetable,
            old_user_stack,
            old_image_base,
            old_image_size,
            old_heap_base,
            old_heap_end,
        ) = {
            let current = self.current_mut().ok_or(())?;
            let old_pagetable = core::mem::replace(&mut current.pagetable, pagetable);
            let old = (
                old_pagetable,
                current.user_stack,
                current.image_base,
                current.image_size,
                current.heap_base,
                current.heap_end,
            );
            current.user_stack = user_stack;
            current.entry = entry;
            current.image_base = image_base;
            current.image_size = image_size;
            current.heap_base = heap_base;
            current.heap_end = heap_base;
            unsafe {
                ((current.trap_frame + core::mem::size_of::<TrapFrame>() + 8) as *mut usize)
                    .write(user_satp);
            }
            old
        };
        tf.sp = USER_STACK_TOP;
        tf.sepc = entry;
        tf.a0 = 0;
        tf.a1 = 0;
        tf.a2 = 0;
        Self::free_user_space(
            old_pagetable,
            old_user_stack,
            old_image_base,
            old_image_size,
            old_heap_base,
            old_heap_end,
        );
        Ok(())
    }

    pub fn sbrk(&mut self, increment: isize) -> Result<usize, ()> {
        if self.current_pid == Some(Self::IDLE_PID) {
            return Err(());
        }
        let task = self.current_mut().ok_or(())?;
        let old_end = task.heap_end;
        let new_end = old_end.checked_add_signed(increment).ok_or(())?;
        if new_end < task.heap_base || new_end > Self::HEAP_LIMIT {
            return Err(());
        }

        let old_mapped_end = Self::align_up(old_end)?;
        let new_mapped_end = Self::align_up(new_end)?;
        if new_mapped_end > old_mapped_end {
            let mut mapped_end = old_mapped_end;
            while mapped_end < new_mapped_end {
                let Some(page) = crate::kernel::page::alloc() else {
                    if mapped_end > old_mapped_end {
                        let _ = pgtable::unmap(
                            task.pagetable,
                            old_mapped_end,
                            mapped_end - old_mapped_end,
                            true,
                        );
                    }
                    return Err(());
                };
                unsafe {
                    core::ptr::write_bytes(page as *mut u8, 0, PAGE_SIZE);
                }
                if pgtable::map(
                    task.pagetable,
                    mapped_end,
                    page,
                    PAGE_SIZE,
                    PTE_R | PTE_W | PTE_U,
                )
                .is_err() {
                    crate::kernel::page::free(page);
                    if mapped_end > old_mapped_end {
                        let _ = pgtable::unmap(
                            task.pagetable,
                            old_mapped_end,
                            mapped_end - old_mapped_end,
                            true,
                        );
                    }
                    return Err(());
                }
                mapped_end += PAGE_SIZE;
            }
        } else if new_mapped_end < old_mapped_end {
            pgtable::unmap(
                task.pagetable,
                new_mapped_end,
                old_mapped_end - new_mapped_end,
                true,
            )?;
        }

        task.heap_end = new_end;
        Ok(old_end)
    }

    pub fn waitpid(&mut self, pid: u32, tf: &TrapFrame) -> WaitResult {
        if self.current_pid == Some(Self::IDLE_PID) {
            return WaitResult::Error;
        }
        let Some(parent_pid) = self.current_pid else { return WaitResult::Error };
        let Some(child_index) = self.tasks.iter().position(|slot| {
            slot.as_ref().is_some_and(|task| task.pid == pid && task.parent_pid == Some(parent_pid))
        }) else {
            return WaitResult::Error;
        };

        if self.tasks[child_index].as_ref().unwrap().state == TaskState::Zombie {
            let task = self.tasks[child_index].take().unwrap();
            let exit_code = task.exit_code;
            println!("task reap: pid={} exit_code={}", task.pid, exit_code);
            Self::free_task(task);
            return WaitResult::Reaped(exit_code);
        }

        let Some(parent) = self.current_mut() else { return WaitResult::Error };
        parent.state = TaskState::Sleeping;
        parent.waiting_for = Some(WaitChannel::Child(pid));
        unsafe { (parent.trap_frame as *mut TrapFrame).write(tf.clone()); }
        self.runnable_count = self.runnable_count.saturating_sub(1);
        WaitResult::Blocked(self.schedule(tf) as *mut TrapFrame)
    }

    fn set_message(frame: &mut TrapFrame, sender: u32, words: [usize; 4]) {
        frame.a0 = sender as usize;
        frame.a1 = words[0];
        frame.a2 = words[1];
        frame.a3 = words[2];
        frame.a4 = words[3];
    }

    pub fn register_endpoint(&mut self, endpoint: usize, owner: u32) -> Result<(), ()> {
        if endpoint >= MAX_ENDPOINTS || self.endpoints[endpoint].is_some() {
            return Err(());
        }
        if !self.tasks.iter().flatten().any(|task| task.pid == owner) {
            return Err(());
        }
        self.endpoints[endpoint] = Some(Endpoint {
            owner,
            waiting_receiver: None,
            pending: crate::kernel::ipc::MsgQueue::new(),
            copy_buf: [0u8; crate::kernel::ipc::IPC_BUF_SIZE],
            copy_len: 0,
            recv_buf_addr: 0,
            recv_buf_capacity: 0,
            has_buffered: false,
        });
        println!("ipc: endpoint={} owner={}", endpoint, owner);
        Ok(())
    }

    pub fn ipc_call(
        &mut self,
        endpoint: usize,
        words: [usize; 4],
        tf: &mut TrapFrame,
    ) -> IpcResult {
        let Some(sender) = self.current_pid else { return IpcResult::Error };
        let (owner, receiver_waiting, queue_full) = match self.endpoints.get(endpoint).and_then(|e| e.as_ref()) {
            Some(ep) => (ep.owner, ep.waiting_receiver == Some(ep.owner), ep.pending.is_full()),
            None => return IpcResult::Error,
        };
        if sender == owner || queue_full {
            return IpcResult::Error;
        }

        let Some(sender_index) = self.tasks.iter().position(|slot| {
            slot.as_ref().is_some_and(|task| task.pid == sender)
        }) else { return IpcResult::Error };
        {
            let task = self.tasks[sender_index].as_mut().unwrap();
            task.state = TaskState::Sleeping;
            task.waiting_for = Some(WaitChannel::IpcCall(endpoint));
            unsafe { (task.trap_frame as *mut TrapFrame).write(tf.clone()); }
        }
        self.runnable_count = self.runnable_count.saturating_sub(1);

        let message = Message { sender, words, buf_len: 0 };
        if receiver_waiting {
            let receiver_index = self.tasks.iter().position(|slot| {
                slot.as_ref().is_some_and(|task| task.pid == owner)
            }).unwrap();
            let receiver = self.tasks[receiver_index].as_mut().unwrap();
            let frame = unsafe { &mut *(receiver.trap_frame as *mut TrapFrame) };
            Self::set_message(frame, sender, words);
            receiver.state = TaskState::Ready;
            receiver.waiting_for = None;
            self.runnable_count += 1;
            self.endpoints[endpoint].as_mut().unwrap().waiting_receiver = None;
        } else {
            self.endpoints[endpoint].as_mut().unwrap().pending.push(message).ok();
        }
        IpcResult::Blocked(self.schedule(tf) as *mut TrapFrame)
    }

    pub fn ipc_recv(&mut self, endpoint: usize, tf: &mut TrapFrame) -> IpcResult {
        let Some(receiver) = self.current_pid else { return IpcResult::Error };
        let Some(ep) = self.endpoints.get_mut(endpoint).and_then(|e| e.as_mut()) else {
            return IpcResult::Error;
        };
        if ep.owner != receiver {
            return IpcResult::Error;
        }
        if let Some(message) = ep.pending.pop() {
            Self::set_message(tf, message.sender, message.words);
            return IpcResult::Continue;
        }
        ep.waiting_receiver = Some(receiver);
        let Some(task) = self.current_mut() else { return IpcResult::Error };
        task.state = TaskState::Sleeping;
        task.waiting_for = Some(WaitChannel::IpcRecv(endpoint));
        unsafe { (task.trap_frame as *mut TrapFrame).write(tf.clone()); }
        self.runnable_count = self.runnable_count.saturating_sub(1);
        IpcResult::Blocked(self.schedule(tf) as *mut TrapFrame)
    }

    pub fn ipc_reply(&mut self, client: u32, words: [usize; 4]) -> Result<(), ()> {
        let replier = self.current_pid.ok_or(())?;
        let Some(index) = self.tasks.iter().position(|slot| {
            slot.as_ref().is_some_and(|task| task.pid == client)
        }) else { return Err(()) };
        let waiting_endpoint = match self.tasks[index].as_ref().unwrap().waiting_for {
            Some(WaitChannel::IpcCall(endpoint)) => endpoint,
            _ => return Err(()),
        };
        if self.endpoints
            .get(waiting_endpoint)
            .and_then(|endpoint| endpoint.as_ref())
            .is_none_or(|endpoint| endpoint.owner != replier)
        {
            return Err(());
        }
        let task = self.tasks[index].as_mut().unwrap();
        if task.state != TaskState::Sleeping {
            return Err(());
        }
        let frame = unsafe { &mut *(task.trap_frame as *mut TrapFrame) };
        frame.a0 = words[0];
        frame.a1 = words[1];
        frame.a2 = words[2];
        frame.a3 = words[3];
        task.state = TaskState::Ready;
        task.waiting_for = None;
        self.runnable_count += 1;
        Ok(())
    }

    /// 带缓冲区的 ipc_reply：回复 words + 将 replier 用户空间的 buf[0..buf_len]
    /// 复制到被阻塞的 caller 用户空间。
    pub fn ipc_reply_buf(
        &mut self,
        client: u32,
        words: [usize; 4],
        user_buf: usize,
        buf_len: usize,
    ) -> Result<(), ()> {
        let replier = self.current_pid.ok_or(())?;
        let Some(index) = self.tasks.iter().position(|slot| {
            slot.as_ref().is_some_and(|task| task.pid == client)
        }) else { return Err(()) };
        let waiting_endpoint = match self.tasks[index].as_ref().unwrap().waiting_for {
            Some(WaitChannel::IpcCall(endpoint)) => endpoint,
            _ => return Err(()),
        };
        if self.endpoints
            .get(waiting_endpoint)
            .and_then(|endpoint| endpoint.as_ref())
            .is_none_or(|endpoint| endpoint.owner != replier)
        {
            return Err(());
        }
        // 获取 replier 和 caller 的页表指针（避免双重 borrow）
        let (replier_pt_opt, caller_pt_raw) = {
            let mut replier_pt = None;
            let mut caller_pt = core::ptr::null_mut();
            for task in self.tasks.iter_mut().flatten() {
                if Some(task.pid) == self.current_pid && buf_len > 0 {
                    replier_pt = Some(task.pagetable as *mut pgtable::PageTable);
                }
                if task.pid == client {
                    caller_pt = task.pagetable as *mut pgtable::PageTable;
                }
            }
            (replier_pt, caller_pt)
        };
        let task = self.tasks[index].as_mut().unwrap();
        if task.state != TaskState::Sleeping {
            return Err(());
        }
        let frame = unsafe { &mut *(task.trap_frame as *mut TrapFrame) };
        let caller_buf_addr = frame.a5;
        let caller_buf_capacity = frame.a6;
        let copy_len = buf_len
            .min(caller_buf_capacity)
            .min(crate::kernel::ipc::IPC_BUF_SIZE);
        frame.a0 = words[0];
        frame.a1 = words[1];
        frame.a2 = words[2];
        frame.a3 = words[3];
        frame.a5 = copy_len; // 返回给 caller 的实际数据长度
        if copy_len > 0 {
            let replier_pt = replier_pt_opt.unwrap();
            let caller_pt = caller_pt_raw;
            for i in 0..copy_len {
                let src_pa = pgtable::virt_to_phys(unsafe { &mut *replier_pt }, user_buf + i)
                    .ok_or(())?;
                let dst_pa = pgtable::virt_to_phys(unsafe { &mut *caller_pt }, caller_buf_addr + i)
                    .ok_or(())?;
                unsafe {
                    core::ptr::write(dst_pa as *mut u8, core::ptr::read(src_pa as *const u8));
                }
            }
        }
        task.state = TaskState::Ready;
        task.waiting_for = None;
        self.runnable_count += 1;
        Ok(())
    }

    /// 从用户空间复制 len 字节到 dst，使用指定的页表。
    fn mem_copy_from_user(
        pagetable: &mut pgtable::PageTable,
        dst: &mut [u8],
        user_src: usize,
        len: usize,
    ) -> Result<(), ()> {
        let len = len.min(dst.len());
        for i in 0..len {
            let pa = pgtable::virt_to_phys(pagetable, user_src + i).ok_or(())?;
            dst[i] = unsafe { core::ptr::read(pa as *const u8) };
        }
        Ok(())
    }

    /// 从 src 复制 len 字节到用户空间，使用指定的页表。
    fn mem_copy_to_user(
        pagetable: &mut pgtable::PageTable,
        user_dst: usize,
        src: &[u8],
        len: usize,
    ) -> Result<(), ()> {
        let len = len.min(src.len());
        for i in 0..len {
            let pa = pgtable::virt_to_phys(pagetable, user_dst + i).ok_or(())?;
            unsafe { core::ptr::write(pa as *mut u8, src[i]); }
        }
        Ok(())
    }

    /// 缓冲区版 ipc_call：将 words 和 user_buf[0..buf_len] 一起发送。
    pub fn ipc_call_buf(
        &mut self,
        endpoint: usize,
        words: [usize; 4],
        user_buf: usize,
        buf_len: usize,
        tf: &mut TrapFrame,
    ) -> IpcResult {
        use crate::kernel::ipc::IPC_BUF_SIZE;

        let Some(sender) = self.current_pid else { return IpcResult::Error };
        let ep = match self.endpoints.get(endpoint).and_then(|e| e.as_ref()) {
            Some(ep) => ep,
            None => return IpcResult::Error,
        };
        let (owner, receiver_waiting, queue_full) =
            (ep.owner, ep.waiting_receiver == Some(ep.owner), ep.pending.is_full());
        // 队列满或者已有缓冲型消息等待，拒绝新的缓冲型请求。
        // 如果接收者正阻塞等待，可以用 copy_buf 直接传递，不算入队。
        let buffered_blocked = ep.has_buffered && !receiver_waiting;
        if sender == owner || queue_full || buffered_blocked {
            return IpcResult::Error;
        }
        if buf_len > IPC_BUF_SIZE {
            return IpcResult::Error;
        }

        // 从发送方用户空间复制缓冲区数据到 endpoint 的 copy_buf
        if buf_len > 0 {
            // 获取发送方页表指针以避免 borrow 冲突
            let pt_ptr = match self.current_mut() {
                Some(task) => task.pagetable as *mut pgtable::PageTable,
                None => return IpcResult::Error,
            };
            let ep_buf = &mut self.endpoints[endpoint].as_mut().unwrap().copy_buf[..buf_len];
            if Self::mem_copy_from_user(unsafe { &mut *pt_ptr }, ep_buf, user_buf, buf_len).is_err() {
                return IpcResult::Error;
            }
            self.endpoints[endpoint].as_mut().unwrap().copy_len = buf_len;
        } else {
            self.endpoints[endpoint].as_mut().unwrap().copy_len = 0;
        }

        let Some(sender_index) = self.tasks.iter().position(|slot| {
            slot.as_ref().is_some_and(|task| task.pid == sender)
        }) else { return IpcResult::Error };
        {
            let task = self.tasks[sender_index].as_mut().unwrap();
            task.state = TaskState::Sleeping;
            task.waiting_for = Some(WaitChannel::IpcCall(endpoint));
            unsafe { (task.trap_frame as *mut TrapFrame).write(tf.clone()); }
        }
        self.runnable_count = self.runnable_count.saturating_sub(1);

        let message = Message { sender, words, buf_len: buf_len as u16 };
        if receiver_waiting {
            let receiver_index = self.tasks.iter().position(|slot| {
                slot.as_ref().is_some_and(|task| task.pid == owner)
            }).unwrap();
            let receiver = self.tasks[receiver_index].as_mut().unwrap();
            let frame = unsafe { &mut *(receiver.trap_frame as *mut TrapFrame) };
            Self::set_message(frame, sender, words);
            // 如果接收方是通过 recv_buf 阻塞的，将缓冲区数据复制到接收方
            let recv_addr = self.endpoints[endpoint].as_ref().unwrap().recv_buf_addr;
            let recv_cap = self.endpoints[endpoint].as_ref().unwrap().recv_buf_capacity;
            if buf_len > 0 && recv_addr != 0 {
                let copy_len = buf_len.min(recv_cap);
                let recv_pt = receiver.pagetable as *mut pgtable::PageTable;
                let ep_buf = &self.endpoints[endpoint].as_ref().unwrap().copy_buf[..copy_len];
                if Self::mem_copy_to_user(unsafe { &mut *recv_pt }, recv_addr, ep_buf, copy_len).is_ok() {
                    frame.a5 = copy_len;
                } else {
                    frame.a5 = 0;
                }
            } else {
                frame.a5 = 0;
            }
            self.endpoints[endpoint].as_mut().unwrap().recv_buf_addr = 0;
            self.endpoints[endpoint].as_mut().unwrap().recv_buf_capacity = 0;
            receiver.state = TaskState::Ready;
            receiver.waiting_for = None;
            self.runnable_count += 1;
            self.endpoints[endpoint].as_mut().unwrap().waiting_receiver = None;
        } else {
            if buf_len > 0 {
                self.endpoints[endpoint].as_mut().unwrap().has_buffered = true;
            }
            self.endpoints[endpoint].as_mut().unwrap().pending.push(message).ok();
        }
        IpcResult::Blocked(self.schedule(tf) as *mut TrapFrame)
    }

    /// 缓冲区版 ipc_recv：接收消息后将附带的缓冲区数据复制到 user_buf[0..capacity]。
    pub fn ipc_recv_buf(
        &mut self,
        endpoint: usize,
        user_buf: usize,
        capacity: usize,
        tf: &mut TrapFrame,
    ) -> IpcResult {
        let Some(receiver) = self.current_pid else { return IpcResult::Error };
        let Some(ep) = self.endpoints.get_mut(endpoint).and_then(|e| e.as_mut()) else {
            return IpcResult::Error;
        };
        if ep.owner != receiver {
            return IpcResult::Error;
        }
        if let Some(message) = ep.pending.pop() {
            Self::set_message(tf, message.sender, message.words);
            if message.buf_len > 0 {
                self.endpoints[endpoint].as_mut().unwrap().has_buffered = false;
                let copy_len = (message.buf_len as usize).min(capacity);
                let pt_ptr = match self.current_mut() {
                    Some(task) => task.pagetable as *mut pgtable::PageTable,
                    None => { tf.a5 = 0; return IpcResult::Continue; }
                };
                let ep_buf = &self.endpoints[endpoint].as_ref().unwrap().copy_buf[..copy_len];
                if Self::mem_copy_to_user(unsafe { &mut *pt_ptr }, user_buf, ep_buf, copy_len).is_err() {
                    tf.a5 = 0;
                } else {
                    tf.a5 = copy_len;
                }
            } else {
                tf.a5 = 0;
            }
            return IpcResult::Continue;
        }
        // 阻塞等待：记录接收方的用户缓冲区地址，供 call_buf 唤醒时使用
        self.endpoints[endpoint].as_mut().unwrap().waiting_receiver = Some(receiver);
        self.endpoints[endpoint].as_mut().unwrap().recv_buf_addr = user_buf;
        self.endpoints[endpoint].as_mut().unwrap().recv_buf_capacity = capacity;
        let Some(task) = self.current_mut() else { return IpcResult::Error };
        task.state = TaskState::Sleeping;
        task.waiting_for = Some(WaitChannel::IpcRecv(endpoint));
        unsafe { (task.trap_frame as *mut TrapFrame).write(tf.clone()); }
        self.runnable_count = self.runnable_count.saturating_sub(1);
        IpcResult::Blocked(self.schedule(tf) as *mut TrapFrame)
    }
    
    /// 退出当前进程
    pub fn exit_current(&mut self, code: i32) -> Option<usize> {
        if self.current_pid == Some(Self::IDLE_PID) {
            return self.current().map(|task| task.trap_frame);
        }
        let mut exited_pid = None;
        let mut parent_pid = None;

        if let Some(current) = self.current_mut() {
            current.state = TaskState::Zombie;
            current.exit_code = code;
            exited_pid = Some(current.pid);
            parent_pid = current.parent_pid;
        }

        if let Some(pid) = exited_pid {
            self.runnable_count -= 1;
            println!("task exit: pid={} code={}", pid, code);

            // 父进程退出后，其子进程成为孤儿；Zombie 会在安全时机回收。
            for child in self.tasks.iter_mut().filter_map(|task| task.as_mut()) {
                if child.parent_pid == Some(pid) {
                    child.parent_pid = None;
                }
            }
        }

        // 如果父进程正阻塞在 waitpid 上，写回退出码并唤醒它。
        if let (Some(child_pid), Some(parent_pid)) = (exited_pid, parent_pid) {
            let parent_index = self.tasks.iter().position(|slot| {
                slot.as_ref().is_some_and(|task| task.pid == parent_pid)
            });
            if let Some(index) = parent_index {
                let waiting = self.tasks[index].as_ref().is_some_and(|parent| {
                    parent.state == TaskState::Sleeping && parent.waiting_for == Some(WaitChannel::Child(child_pid))
                });
                if waiting {
                    let parent = self.tasks[index].as_mut().unwrap();
                    parent.state = TaskState::Ready;
                    parent.waiting_for = None;
                    unsafe { (*(parent.trap_frame as *mut TrapFrame)).a0 = code as usize; }
                    self.runnable_count += 1;
                    if let Some(child) = self.tasks.iter_mut().filter_map(|task| task.as_mut())
                        .find(|task| task.pid == child_pid)
                    {
                        child.parent_pid = None;
                    }
                }
            }
        }
        
        // 调度下一个进程
        self.current_pid = None;
        
        // 查找下一个可运行进程
        for task in self.tasks.iter_mut().filter_map(|task| task.as_mut()) {
            if task.pid != Self::IDLE_PID && task.state == TaskState::Ready {
                task.state = TaskState::Running;
                self.current_pid = Some(task.pid);
                return Some(task.trap_frame);
            }
        }

        if let Some(idle) = self.tasks.iter_mut().flatten()
            .find(|task| task.pid == Self::IDLE_PID)
        {
            idle.state = TaskState::Running;
            self.current_pid = Some(Self::IDLE_PID);
            return Some(idle.trap_frame);
        }
        
        None
    }
    
    /// 列出所有进程
    pub fn list_all(&self) {
        println!("PID    TYPE   STATE        ENTRY      STACK_TOP ");
        println!("------ ------ ------------ ---------- ----------");
        
        for task in self.tasks.iter().filter_map(|task| task.as_ref()) {
            let state = match task.state {
                TaskState::Unused => "UNUSED",
                TaskState::Ready => "READY",
                TaskState::Running => "RUNNING",
                TaskState::Sleeping => "SLEEPING",
                TaskState::Zombie => "ZOMBIE",
            };
            
            let (kind, stack_top) = if task.pid == Self::IDLE_PID {
                ("IDLE", task.kernel_stack + PAGE_SIZE - 16)
            } else {
                ("USER", USER_STACK_TOP)
            };
            println!("{:<6} {:<6} {:<12} 0x{:08x} 0x{:08x}",
                     task.pid, kind, state, task.entry, stack_top);
        }
        
        println!("total: {} runnable", self.runnable_count);
    }
}

/// 初始化进程管理
pub fn init() {
    *TASK_MANAGER.lock() = Some(TaskManager::new());
}

/// 接管 ELF 加载器创建的地址空间并注册用户进程。
pub fn create_user_image(
    pagetable: &'static mut pgtable::PageTable,
    user_stack: usize,
    entry: usize,
    image_base: usize,
    image_size: usize,
) -> Result<u32, ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.create_user_image(
        pagetable,
        user_stack,
        entry,
        image_base,
        image_size,
    )
}

/// 创建固定 PID 0 的内核态 idle 任务。
pub fn create_idle(entry: usize) -> Result<u32, ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.create_idle(entry)
}

/// 设置当前进程
pub fn set_current(pid: u32) {
    TASK_MANAGER.lock().as_mut().unwrap().set_current(pid);
}

/// 获取当前进程PID
pub fn current_pid() -> u32 {
    TASK_MANAGER.lock()
        .as_ref()
        .and_then(|m| m.current())
        .map(|t| t.pid)
        .unwrap_or(0)
}

/// 获取进程的内核栈顶
pub fn kernel_stack_top(pid: u32) -> Option<usize> {
    TASK_MANAGER
        .lock()
        .as_ref()
        .and_then(|manager| {
            manager
                .tasks
                .iter()
                .filter_map(|task| task.as_ref())
                .find(|task| task.pid == pid)
                .map(|task| task.kernel_stack + PAGE_SIZE)
        })
}

/// 获取任务页表对应的 SATP 值。
pub fn task_satp(pid: u32) -> Option<usize> {
    TASK_MANAGER
        .lock()
        .as_ref()
        .and_then(|manager| {
            manager
                .tasks
                .iter()
                .filter_map(|task| task.as_ref())
                .find(|task| task.pid == pid)
                .map(|task| {
                    (8usize << 60)
                        | ((task.pagetable as *const pgtable::PageTable as usize) >> PAGE_SHIFT)
                })
        })
}

/// 将指定的已注册用户任务作为第一个任务启动。
///
/// 后续上下文切换统一由 trap 返回路径完成。
pub unsafe fn enter_task(pid: u32) -> ! {
    let (entry, kernel_stack_top, user_satp) = {
        let mut guard = TASK_MANAGER.lock();
        let manager = guard.as_mut().expect("task manager not initialized");
        let task = manager.tasks.iter_mut().flatten()
            .find(|task| task.pid == pid)
            .expect("task not found");
        task.state = TaskState::Running;
        manager.current_pid = Some(pid);
        (
            task.entry,
            task.kernel_stack + PAGE_SIZE,
            (8usize << 60)
                | ((task.pagetable as *const pgtable::PageTable as usize) >> PAGE_SHIFT),
        )
    };

    enter_user(
        USER_STACK_TOP,
        entry,
        kernel_stack_top - 16,
        vm::kernel_satp(),
        user_satp,
    );
    core::hint::unreachable_unchecked()
}

/// 调度下一个进程
pub fn schedule(tf: &TrapFrame) -> *mut TrapFrame {
    TASK_MANAGER.lock().as_mut().unwrap().schedule(tf) as *mut TrapFrame
}

/// 退出当前进程
pub fn exit_current(code: i32) -> *mut TrapFrame {
    if let Some(next) = TASK_MANAGER
        .lock()
        .as_mut()
        .and_then(|manager| manager.exit_current(code))
    {
        return next as *mut TrapFrame;
    }

    crate::arch::riscv::sbi::shutdown();

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

pub fn fork(tf: &mut TrapFrame) -> Result<u32, ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.fork(tf)
}

pub fn sbrk(increment: isize) -> Result<usize, ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.sbrk(increment)
}

pub fn waitpid(pid: u32, tf: &TrapFrame) -> WaitResult {
    TASK_MANAGER
        .lock()
        .as_mut()
        .map(|manager| manager.waitpid(pid, tf))
        .unwrap_or(WaitResult::Error)
}

pub fn register_endpoint(endpoint: usize, owner: u32) -> Result<(), ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.register_endpoint(endpoint, owner)
}

pub fn ipc_call(endpoint: usize, words: [usize; 4], tf: &mut TrapFrame) -> IpcResult {
    TASK_MANAGER
        .lock()
        .as_mut()
        .map(|manager| manager.ipc_call(endpoint, words, tf))
        .unwrap_or(IpcResult::Error)
}

pub fn ipc_recv(endpoint: usize, tf: &mut TrapFrame) -> IpcResult {
    TASK_MANAGER
        .lock()
        .as_mut()
        .map(|manager| manager.ipc_recv(endpoint, tf))
        .unwrap_or(IpcResult::Error)
}

pub fn ipc_reply(client: u32, words: [usize; 4]) -> Result<(), ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.ipc_reply(client, words)
}

pub fn ipc_call_buf(
    endpoint: usize,
    words: [usize; 4],
    user_buf: usize,
    buf_len: usize,
    tf: &mut TrapFrame,
) -> IpcResult {
    TASK_MANAGER.lock().as_mut().unwrap().ipc_call_buf(endpoint, words, user_buf, buf_len, tf)
}

pub fn ipc_recv_buf(
    endpoint: usize,
    user_buf: usize,
    capacity: usize,
    tf: &mut TrapFrame,
) -> IpcResult {
    TASK_MANAGER.lock().as_mut().unwrap().ipc_recv_buf(endpoint, user_buf, capacity, tf)
}

pub fn ipc_reply_buf(
    client: u32,
    words: [usize; 4],
    user_buf: usize,
    buf_len: usize,
) -> Result<(), ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.ipc_reply_buf(client, words, user_buf, buf_len)
}

pub fn grant_uart(pid: u32) -> Result<(), ()> {
    let mut guard = TASK_MANAGER.lock();
    let manager = guard.as_mut().ok_or(())?;
    if manager.uart_owner.is_some() {
        return Err(());
    }
    let task = manager.tasks.iter_mut().flatten().find(|task| task.pid == pid).ok_or(())?;
    pgtable::set_flags(
        task.pagetable,
        0x1000_0000,
        PAGE_SIZE,
        PTE_R | PTE_W | PTE_U,
    )?;
    manager.uart_owner = Some(pid);
    Ok(())
}

pub fn replace_current(
    pagetable: &'static mut pgtable::PageTable,
    user_stack: usize,
    entry: usize,
    image_base: usize,
    image_size: usize,
    tf: &mut TrapFrame,
) -> Result<(), ()> {
    TASK_MANAGER.lock().as_mut().ok_or(())?.replace_current(
        pagetable, user_stack, entry, image_base, image_size, tf,
    )
}

pub fn translate_user(va: usize) -> Option<usize> {
    TASK_MANAGER.lock().as_mut()?.current_mut().and_then(|task| {
        pgtable::virt_to_phys(task.pagetable, va)
    })
}

/// 列出所有进程
pub fn list_all() {
    TASK_MANAGER.lock().as_ref().unwrap().list_all();
}

pub fn wait_uart_irq(tf: &TrapFrame) -> Result<*mut TrapFrame, ()> {
    let mut guard = TASK_MANAGER.lock();
    let manager = guard.as_mut().ok_or(())?;

    if manager.current_pid == Some(0) || manager.current_pid != manager.uart_owner {
        return Err(());
    }

    if let Some(current) = manager.current_mut() {
        current.state = TaskState::Sleeping;
        current.waiting_for = Some(WaitChannel::UartRx);

        unsafe {
            (current.trap_frame as *mut TrapFrame).write(tf.clone());
        }

        manager.runnable_count =
            manager.runnable_count.saturating_sub(1);
    } else {
        return Err(());
    }

    // 单核上系统调用处理期间 S-mode 中断关闭。先使能 IRQ 再切走，
    // 即使字符已在检查和 ecall 之间到达，PLIC 的电平中断也不会丢失。
    crate::drivers::plic::enable(crate::drivers::plic::UART0_IRQ);
    Ok(manager.schedule(tf) as *mut TrapFrame)
}

pub fn wake_uart() -> bool {
    let mut guard = TASK_MANAGER.lock();
    let Some(manager) = guard.as_mut() else { return false };
    let mut woke = false;

    for task in manager.tasks.iter_mut().flatten() {
        if task.state == TaskState::Sleeping
            && task.waiting_for == Some(WaitChannel::UartRx)
        {
            task.state = TaskState::Ready;
            task.waiting_for = None;
            unsafe {
                (*(task.trap_frame as *mut TrapFrame)).a0 = 0;
            }
            manager.runnable_count += 1;
            woke = true;
        }
    }
    woke
}

/// 外部汇编函数
extern "C" {
    pub fn enter_user(
        user_stack_top: usize,
        user_entry: usize,
        trap_stack: usize,
        kernel_satp: usize,
        user_satp: usize,
    );
}
