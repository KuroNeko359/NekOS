//! 微内核同步 IPC ABI。

use crate::kernel::trap::TrapFrame;

pub const MAX_ENDPOINTS: usize = 8;
pub const CONSOLE_ENDPOINT: usize = 1;
pub const CONSOLE_WRITE: usize = 1;
pub const CONSOLE_READ: usize = 2;
pub const FS_ENDPOINT: usize = 2;

/// 每个 endpoint 最多可排队的待处理消息数。
pub const MSG_QUEUE_SIZE: usize = 8;

/// IPC 缓冲区大小（每个 endpoint 一份，用于缓冲型消息）。
pub const IPC_BUF_SIZE: usize = 4096;

#[derive(Clone, Copy)]
pub struct Message {
    pub sender: u32,
    pub words: [usize; 4],
    /// 如果 > 0，表示该消息附带了缓冲区数据，长度为 buf_len。
    /// 实际数据存放在 Endpoint.copy_buf 中。
    pub buf_len: u16,
}

/// 固定大小的环形消息队列。
pub struct MsgQueue {
    buf: [Message; MSG_QUEUE_SIZE],
    head: usize,
    tail: usize,
    count: usize,
}

impl MsgQueue {
    pub const fn new() -> Self {
        Self {
            buf: [Message { sender: 0, words: [0; 4], buf_len: 0 }; MSG_QUEUE_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    pub fn is_full(&self) -> bool {
        self.count == MSG_QUEUE_SIZE
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 入队一条消息。队列满时返回 Err。
    pub fn push(&mut self, msg: Message) -> Result<(), ()> {
        if self.is_full() {
            return Err(());
        }
        self.buf[self.tail] = msg;
        self.tail = (self.tail + 1) % MSG_QUEUE_SIZE;
        self.count += 1;
        Ok(())
    }

    /// 出队一条消息。队列空时返回 None。
    pub fn pop(&mut self) -> Option<Message> {
        if self.is_empty() {
            return None;
        }
        let msg = self.buf[self.head];
        self.head = (self.head + 1) % MSG_QUEUE_SIZE;
        self.count -= 1;
        Some(msg)
    }

    /// 查看队首消息（不移除）。
    pub fn peek(&self) -> Option<Message> {
        if self.is_empty() {
            None
        } else {
            Some(self.buf[self.head])
        }
    }

    /// 队列中的消息数量。
    pub fn len(&self) -> usize {
        self.count
    }
}

pub struct Endpoint {
    pub owner: u32,
    pub waiting_receiver: Option<u32>,
    pub pending: MsgQueue,
    /// 缓冲型消息的暂存区（同一时刻最多一条缓冲消息在途）。
    pub copy_buf: [u8; IPC_BUF_SIZE],
    pub copy_len: usize,
    /// 缓冲型 recv 阻塞时，接收方的用户态缓冲区地址和容量。
    pub recv_buf_addr: usize,
    pub recv_buf_capacity: usize,
    /// 队列中是否已有缓冲型消息（共享 copy_buf，同一时刻只能有一条）。
    pub has_buffered: bool,
}

pub enum IpcResult {
    Continue,
    Blocked(*mut TrapFrame),
    Error,
}

pub fn register(endpoint: usize, owner: u32) -> Result<(), ()> {
    crate::kernel::task::register_endpoint(endpoint, owner)
}

pub fn call(endpoint: usize, words: [usize; 4], tf: &mut TrapFrame) -> IpcResult {
    crate::kernel::task::ipc_call(endpoint, words, tf)
}

pub fn recv(endpoint: usize, tf: &mut TrapFrame) -> IpcResult {
    crate::kernel::task::ipc_recv(endpoint, tf)
}

pub fn reply(client: u32, words: [usize; 4]) -> Result<(), ()> {
    crate::kernel::task::ipc_reply(client, words)
}

pub fn call_buf(
    endpoint: usize,
    words: [usize; 4],
    user_buf: usize,
    buf_len: usize,
    tf: &mut TrapFrame,
) -> IpcResult {
    crate::kernel::task::ipc_call_buf(endpoint, words, user_buf, buf_len, tf)
}

pub fn recv_buf(
    endpoint: usize,
    user_buf: usize,
    capacity: usize,
    tf: &mut TrapFrame,
) -> IpcResult {
    crate::kernel::task::ipc_recv_buf(endpoint, user_buf, capacity, tf)
}
