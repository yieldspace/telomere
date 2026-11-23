use std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

#[cfg(feature = "threads")]
use std::sync::Arc;

#[cfg(feature = "threads")]
use crate::common::{MeteringHandle, SharedMemoryObject, SharedWaitRegistration};
use crate::{
    common::{stack::LocalReference, AsyncHostFuture, Instr, ResultType, WasmValue},
    VMResult,
};

/// The result of a finished host-call future, in interpreter terms, before
/// the scheduler applies it to the suspended task.
///
/// The public-boundary variant deliberately carries the callback's values
/// instead of a resume address: a submitted future is owned by the
/// embedder's driver and may still be polled after the execution that
/// submitted it has been dropped (for example when a driver is reused after
/// a cancelled run), so the future itself must never write into the task's
/// stack. The scheduler owns the stack and performs the return-slot write
/// when it applies the completion.
pub(crate) enum HostCallResult {
    /// Raw internal convention: the future resolved directly to the resume
    /// address (or a trap); any result values were already placed by the
    /// host function itself under the raw calling convention.
    Resume(VMResult<*const Instr>),
    /// Public-boundary convention: the scheduler validates `result`'s
    /// values against `result_types`, writes them into the host frame's
    /// return slot, and resumes at `return_addr`.
    Values {
        result: VMResult<Vec<WasmValue>>,
        return_addr: *const Instr,
        result_types: ResultType,
    },
}

/// The result of a completed asynchronous host call, as handed back to the
/// runtime by an execution driver.
///
/// The payload is the interpreter's internal resume token; drivers only
/// forward it from a finished [`HostCallFuture`] to the runtime through
/// [`Completion`], so the token's type stays behind this public boundary
/// (issue #216).
pub struct HostCallOutcome {
    result: HostCallResult,
}

impl HostCallOutcome {
    pub(crate) fn into_result(self) -> HostCallResult {
        self.result
    }
}

impl fmt::Debug for HostCallOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCallOutcome").finish_non_exhaustive()
    }
}

/// A pending asynchronous host call, exposed to execution drivers.
///
/// The future is opaque: a driver polls it to completion and forwards the
/// resulting [`HostCallOutcome`] back to the runtime as the completion
/// payload, without naming any interpreter-internal type (issue #216).
/// Polling the future never writes into the suspended task's stack — the
/// scheduler performs the return-slot write when it applies the completion —
/// so polling a future whose submitting execution has been dropped is
/// harmless.
pub struct HostCallFuture {
    future: Pin<Box<dyn Future<Output = HostCallResult> + 'static>>,
}

impl HostCallFuture {
    pub(crate) fn from_raw(future: AsyncHostFuture) -> Self {
        Self {
            future: Box::pin(async move { HostCallResult::Resume(future.await) }),
        }
    }

    pub(crate) fn from_public(
        future: Pin<Box<dyn Future<Output = HostCallResult> + 'static>>,
    ) -> Self {
        Self { future }
    }
}

impl Future for HostCallFuture {
    type Output = HostCallOutcome;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.future.as_mut().poll(cx) {
            Poll::Ready(result) => Poll::Ready(HostCallOutcome { result }),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl fmt::Debug for HostCallFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCallFuture").finish_non_exhaustive()
    }
}

pub struct HostCallPending {
    pub task_id: u64,
    pub future: HostCallFuture,
    /// The frame of the host call itself, retained by the runtime so that a
    /// trap completing asynchronously is attributed to the host frame
    /// (#211) even though the frame is finished eagerly for resumption.
    pub(crate) frame_reference: LocalReference,
}

#[cfg(feature = "threads")]
pub struct MemoryWaitPending {
    pub task_id: u64,
    pub shared: Arc<SharedMemoryObject>,
    pub wait: SharedWaitRegistration,
    pub timeout_ns: i64,
    pub fp: usize,
    /// The producing store's metering handle, when metering is enabled.
    ///
    /// Drivers pass this to
    /// [`SharedWaitRegistration::wait_result_interruptible`] so that
    /// [`MeteringHandle::interrupt`] wakes the parked guest (#177). It is
    /// `None` for stores without metering, whose waits can only resolve
    /// by notify or timeout.
    pub interrupt: Option<MeteringHandle>,
}

#[cfg(feature = "unstable-internals")]
#[derive(Debug)]
pub struct WasmAsyncPending {
    pub task_id: u64,
}

pub enum PendingOp {
    HostCall(HostCallPending),
    #[cfg(feature = "threads")]
    MemoryWait(MemoryWaitPending),
    #[cfg(feature = "unstable-internals")]
    #[allow(dead_code)]
    WasmAsync(WasmAsyncPending),
}

impl PendingOp {
    pub fn task_id(&self) -> u64 {
        match self {
            Self::HostCall(op) => op.task_id,
            #[cfg(feature = "threads")]
            Self::MemoryWait(op) => op.task_id,
            #[cfg(feature = "unstable-internals")]
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
            #[cfg(feature = "threads")]
            Self::MemoryWait(op) => f
                .debug_struct("PendingOp::MemoryWait")
                .field("task_id", &op.task_id)
                .field("timeout_ns", &op.timeout_ns)
                .field("fp", &op.fp)
                .finish(),
            #[cfg(feature = "unstable-internals")]
            Self::WasmAsync(op) => f.debug_tuple("PendingOp::WasmAsync").field(op).finish(),
        }
    }
}

#[derive(Debug)]
pub struct Completion {
    pub task_id: u64,
    pub payload: CompletionPayload,
}

#[derive(Debug)]
pub enum CompletionPayload {
    #[allow(dead_code)]
    Resume {
        fp: usize,
    },
    // #202: Shared-memory wait completion is intentionally dormant without threads support.
    #[cfg_attr(not(feature = "threads"), allow(dead_code))]
    ResumeWithI32 {
        fp: usize,
        value: i32,
    },
    /// A pending shared-memory wait was cut short by
    /// [`MeteringHandle::interrupt`] (#177). The task terminates with
    /// [`VMResult::Cancelled`] instead of resuming with a wait result
    /// code, exactly as if it had been stopped at a metering checkpoint.
    #[cfg_attr(not(feature = "threads"), allow(dead_code))]
    Interrupted,
    HostCall {
        result: HostCallOutcome,
    },
    #[allow(dead_code)]
    WasmAsync,
}
