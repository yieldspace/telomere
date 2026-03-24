use std::{fmt, sync::Arc};

use crate::{
    common::{
        AsyncHostFuture, SafepointMetadataCache, SharedMemoryObject, SharedWaitRegistration,
        StablePc,
    },
    VMResult,
};

#[allow(private_interfaces)]
pub struct HostCallPending {
    pub task_id: u32,
    pub future: AsyncHostFuture,
    pub safepoint: SafepointMetadataCache,
}

impl HostCallPending {
    pub async fn into_completion(self) -> Completion {
        Completion {
            task_id: self.task_id,
            payload: CompletionPayload::HostCall {
                result: self.future.await,
                safepoint: self.safepoint,
            },
        }
    }
}

pub struct MemoryWaitPending {
    pub(crate) task_id: u32,
    pub(crate) shared: Arc<SharedMemoryObject>,
    pub(crate) wait: SharedWaitRegistration,
    pub(crate) timeout_ns: i64,
    pub(crate) fp: StablePc,
    pub(crate) safepoint: SafepointMetadataCache,
}

impl MemoryWaitPending {
    pub async fn into_completion(self) -> Completion {
        let value = self.wait.wait_result(self.shared, self.timeout_ns).await;
        Completion {
            task_id: self.task_id,
            payload: CompletionPayload::ResumeWithI32 {
                fp: self.fp,
                value,
                safepoint: self.safepoint,
            },
        }
    }
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
#[allow(private_interfaces)]
pub enum CompletionPayload {
    #[allow(dead_code)]
    Resume {
        fp: StablePc,
        safepoint: SafepointMetadataCache,
    },
    ResumeWithI32 {
        fp: StablePc,
        value: i32,
        safepoint: SafepointMetadataCache,
    },
    HostCall {
        result: VMResult<*const crate::common::Instr>,
        safepoint: SafepointMetadataCache,
    },
    #[allow(dead_code)]
    WasmAsync,
}
