use std::{fmt, sync::Arc};

use crate::{
    common::{
        AsyncHostFuture, ResultType, ResultValue, SharedMemoryObject, SharedWaitRegistration,
        StablePc,
    },
    VMResult,
};

pub struct HostCallPending {
    pub task_id: u32,
    pub future: AsyncHostFuture,
    pub fp: StablePc,
    pub result_types: ResultType,
    pub result_slot: usize,
}

pub struct MemoryWaitPending {
    pub task_id: u32,
    pub shared: Arc<SharedMemoryObject>,
    pub wait: SharedWaitRegistration,
    pub timeout_ns: i64,
    pub fp: StablePc,
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

    pub(crate) fn pending_code(&self) -> Option<crate::common::formal::PendingCode> {
        match self {
            Self::HostCall(_) => Some(crate::common::formal::PendingCode::HostCall),
            Self::MemoryWait(_) => Some(crate::common::formal::PendingCode::Wait),
            Self::WasmAsync(_) => None,
        }
    }
}

impl fmt::Debug for PendingOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostCall(op) => f
                .debug_struct("PendingOp::HostCall")
                .field("task_id", &op.task_id)
                .field("fp", &op.fp)
                .field("result_types", &op.result_types)
                .field("result_slot", &op.result_slot)
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
        fp: StablePc,
    },
    ResumeWithI32 {
        fp: StablePc,
        value: i32,
    },
    HostCall {
        fp: StablePc,
        result_types: ResultType,
        result_slot: usize,
        result: VMResult<ResultValue>,
    },
    #[allow(dead_code)]
    WasmAsync,
}
