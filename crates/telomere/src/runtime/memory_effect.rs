use std::{fmt, sync::Arc};

use crate::{
    common::{AsyncHostFuture, SharedMemoryObject, SharedWaitRegistration},
    VMResult,
};

pub struct HostCallPending {
    pub task_id: u32,
    pub future: AsyncHostFuture,
}

pub struct MemoryWaitPending {
    pub task_id: u32,
    pub shared: Arc<SharedMemoryObject>,
    pub wait: SharedWaitRegistration,
    pub timeout_ns: i64,
    pub fp: usize,
}

#[derive(Debug)]
pub struct WasmAsyncPending {
    pub task_id: u32,
}

pub enum PendingOp {
    HostCall(HostCallPending),
    MemoryWait(MemoryWaitPending),
    #[allow(dead_code)]
    WasmAsync(WasmAsyncPending),
}

impl PendingOp {
    pub fn task_id(&self) -> u32 {
        match self {
            Self::HostCall(op) => op.task_id,
            Self::MemoryWait(op) => op.task_id,
            Self::WasmAsync(op) => op.task_id,
        }
    }
}

impl fmt::Debug for PendingOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostCall(op) => f
                .debug_struct("PendingOp::HostCall")
                .field("task_id", &op.task_id)
                .finish(),
            Self::MemoryWait(op) => f
                .debug_struct("PendingOp::MemoryWait")
                .field("task_id", &op.task_id)
                .field("timeout_ns", &op.timeout_ns)
                .field("fp", &op.fp)
                .finish(),
            Self::WasmAsync(op) => f.debug_tuple("PendingOp::WasmAsync").field(op).finish(),
        }
    }
}

#[derive(Debug)]
pub struct Completion {
    pub task_id: u32,
    pub payload: CompletionPayload,
}

#[derive(Debug)]
pub enum CompletionPayload {
    #[allow(dead_code)]
    Resume {
        fp: usize,
    },
    ResumeWithI32 {
        fp: usize,
        value: i32,
    },
    HostCall {
        result: VMResult<*const crate::common::Instr>,
    },
    #[allow(dead_code)]
    WasmAsync,
}
