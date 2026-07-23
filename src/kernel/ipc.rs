//! 微内核同步 IPC ABI。

use crate::kernel::trap::TrapFrame;

pub const MAX_ENDPOINTS: usize = 8;
pub const CONSOLE_ENDPOINT: usize = 1;

pub const CONSOLE_WRITE: usize = 1;
pub const CONSOLE_READ: usize = 2;

#[derive(Clone, Copy)]
pub struct Message {
    pub sender: u32,
    pub words: [usize; 4],
}

#[derive(Clone, Copy)]
pub struct Endpoint {
    pub owner: u32,
    pub waiting_receiver: Option<u32>,
    pub pending: Option<Message>,
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
