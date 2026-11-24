#[cfg(feature = "threads")]
use super::memory_effect::MemoryWaitPending;
use super::memory_effect::{
    Completion, CompletionPayload, HostCallPending, HostCallResult, PendingOp,
};
use super::trap_context::{capture_trap_context, TrapContext};
use crate::{
    common::{
        stack::CachedMemoryKind, CallFrameCache, ExecuteContext, LocalReference, StablePc, Stack,
        StoreInner,
    },
    Store, VMResult,
};
use futures::{stream::FuturesUnordered, StreamExt};
use std::{collections::VecDeque, future::Future, pin::Pin};

fn vm_result_to_unit<T>(result: VMResult<T>) -> VMResult<()> {
    match result {
        VMResult::Success(_) => VMResult::Success(()),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::MemoryAllocationFailed => VMResult::MemoryAllocationFailed,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::DivideByZero => VMResult::DivideByZero,
        VMResult::IntegerOverflow => VMResult::IntegerOverflow,
        VMResult::InvalidConversion => VMResult::InvalidConversion,
        VMResult::Unimplemented => VMResult::Unimplemented,
        VMResult::FuelExhausted => VMResult::FuelExhausted,
        VMResult::Cancelled => VMResult::Cancelled,
        VMResult::ResourceLimitExceeded => VMResult::ResourceLimitExceeded,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadyFlag {
    Ready,
    NonReady,
}

#[derive(Debug)]
pub(crate) struct Task {
    pub task_id: u64,
    pub stack: Stack,
    pub local_reference: LocalReference,
    pub pending_effects: u32,
    pub ready_flag: ReadyFlag,
    pub fp: StablePc,
    pub terminal_result: Option<VMResult<()>>,
    pub terminal_trap: Option<Box<TrapContext>>,
}

#[derive(Debug)]
pub(crate) struct CompletedTask {
    pub stack: Stack,
    pub result: VMResult<()>,
    pub trap: Option<Box<TrapContext>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncRunError {
    AsyncPending,
    Stalled,
}

/// Source of identities for top-level execution tasks.
///
/// A driver matches completions to tasks purely by `task_id`, and an
/// embedder's driver may retain a completion past the execution that
/// submitted it: when the execution future is cancelled, the scheduler
/// and its tasks are dropped while the driver keeps the submitted
/// future, and a later execution reusing the same (undrained) driver
/// can be handed the stale completion. Root tasks therefore take a
/// fresh identity from this process-wide counter instead of a
/// constant, so a retained completion can never name a task of a
/// later execution and [`Scheduler::apply_completion`] discards it
/// before any stack write or resume.
///
/// The counter is 64 bits wide, so identities are never reissued: a
/// 32-bit counter would wrap after 2^32 allocations in a long-lived
/// process and hand a retained stale completion the identity of a new
/// task, while exhausting 2^64 identities is unreachable in practice
/// (at a sustained 10^9 allocations per second it would take roughly
/// 585 years). There is therefore no wrap or exhaustion path to fail
/// closed on.
static NEXT_TASK_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Allocates a task identity unique across executions in this process.
pub(crate) fn fresh_task_id() -> u64 {
    NEXT_TASK_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Test hook: advances the identity counter to at least `floor`, so
/// boundary behaviour can be exercised without allocating billions of
/// identities. Never moves the counter backwards.
#[cfg(test)]
pub(crate) fn seed_task_id_floor(floor: u64) {
    NEXT_TASK_ID.fetch_max(floor, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) struct EffectSupplier<'a> {
    task_id: u64,
    pending_effects: &'a mut u32,
    queue: &'a mut VecDeque<PendingOp>,
}

impl EffectSupplier<'_> {
    pub fn get_pending_count(&self) -> u32 {
        *self.pending_effects
    }

    #[cfg(test)]
    pub(crate) fn from_parts<'a>(
        task_id: u64,
        pending_effects: &'a mut u32,
        queue: &'a mut VecDeque<PendingOp>,
    ) -> EffectSupplier<'a> {
        EffectSupplier {
            task_id,
            pending_effects,
            queue,
        }
    }

    pub(crate) fn push_pending(&mut self, op: PendingOp) {
        debug_assert_eq!(op.task_id(), self.task_id);
        self.queue.push_back(op);
        *self.pending_effects += 1;
    }
}

type DriverFuture = Pin<Box<dyn Future<Output = Completion>>>;

/// Schedules pending guest work and returns its completions to the runtime.
///
/// Implement this trait to connect asynchronous host functions to an embedder's
/// executor. The driver must eventually return a completion for every submitted
/// operation unless the surrounding guest call is deliberately cancelled.
///
/// Neither this trait nor the built-in [`TokioDriver`] requires a Tokio
/// runtime or any other specific executor: embedders may drive the execution
/// future with any executor (for example `futures::executor::block_on`), and
/// timed shared-memory waits do not depend on an executor-provided timer.
/// Host futures submitted by the embedder keep whatever executor requirements
/// they themselves have.
pub trait ExecutionDriver {
    /// Submits a pending host call or shared-memory wait for asynchronous execution.
    fn submit(&mut self, op: PendingOp);

    /// Resolves to the next completion, or `None` when no more work can complete.
    fn next_completion<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>>;
}

#[derive(Default)]
/// The built-in driver for async host calls and shared-memory waits.
///
/// It polls submitted futures directly. Despite the name it requires no
/// Tokio runtime: the surrounding execution future may be driven by any
/// executor, and timed shared-memory waits arm a detached timer thread
/// instead of an executor timer, so `memory.atomic.wait` timeouts behave
/// the same everywhere.
pub struct TokioDriver {
    inflight: FuturesUnordered<DriverFuture>,
}

impl TokioDriver {
    /// Creates a driver with no submitted operations.
    pub fn new() -> Self {
        Self::default()
    }

    fn submit_host_call(&mut self, op: HostCallPending) {
        self.inflight.push(Box::pin(async move {
            Completion {
                task_id: op.task_id,
                payload: CompletionPayload::HostCall {
                    result: op.future.await,
                },
            }
        }));
    }

    #[cfg(feature = "threads")]
    fn submit_memory_wait(&mut self, op: MemoryWaitPending) {
        self.inflight.push(Box::pin(async move {
            let (task_id, fp) = (op.task_id, op.fp);
            let payload = match op
                .wait
                .wait_result_interruptible(op.shared, op.timeout_ns, op.interrupt)
                .await
            {
                VMResult::Success(value) => CompletionPayload::ResumeWithI32 { fp, value },
                VMResult::Cancelled => CompletionPayload::Interrupted,
                // wait_result_interruptible only ever resolves with a wait
                // result code or cancellation.
                _ => unreachable!("memory wait resolves only with a code or cancellation"),
            };
            Completion { task_id, payload }
        }));
    }
}

impl ExecutionDriver for TokioDriver {
    fn submit(&mut self, op: PendingOp) {
        match op {
            PendingOp::HostCall(op) => self.submit_host_call(op),
            #[cfg(feature = "threads")]
            PendingOp::MemoryWait(op) => self.submit_memory_wait(op),
            #[cfg(feature = "unstable-internals")]
            PendingOp::WasmAsync(op) => {
                // #128: an unimplemented pending op must not abort the
                // host. Report it straight back as a completion; the
                // scheduler terminates the task with
                // VMResult::Unimplemented when it applies it.
                let task_id = op.task_id;
                self.inflight.push(Box::pin(async move {
                    Completion {
                        task_id,
                        payload: CompletionPayload::WasmAsync,
                    }
                }));
            }
        }
    }

    fn next_completion<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>> {
        Box::pin(async move { self.inflight.next().await })
    }
}

pub(crate) struct Scheduler<'a> {
    tasks: VecDeque<Task>,
    pending_queue: VecDeque<PendingOp>,
    pub(crate) completed_tasks: Vec<CompletedTask>,
    pub(crate) store: &'a Store,
    ready_count: u32,
    /// Host frames of in-flight host calls, by task, retained so an
    /// asynchronously completing trap is attributed to the host frame
    /// (#211) rather than to the already-resumed caller frame.
    host_call_frames: std::collections::HashMap<u64, LocalReference>,
}

impl<'a> Scheduler<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self {
            tasks: VecDeque::new(),
            pending_queue: VecDeque::new(),
            completed_tasks: vec![],
            store,
            ready_count: 0,
            host_call_frames: std::collections::HashMap::new(),
        }
    }

    pub fn push(&mut self, task: Task) {
        let is_ready = task.ready_flag == ReadyFlag::Ready;
        self.tasks.push_back(task);
        if is_ready {
            self.ready_count += 1;
        }
    }

    fn submit_pending_ops<D: ExecutionDriver>(&mut self, driver: &mut D) {
        while let Some(op) = self.pending_queue.pop_front() {
            if let PendingOp::HostCall(pending) = &op {
                self.host_call_frames
                    .insert(pending.task_id, pending.frame_reference);
            }
            driver.submit(op);
        }
    }

    fn apply_completion(&mut self, completion: Completion) {
        let Some(task_index) = self
            .tasks
            .iter()
            .position(|task| task.task_id == completion.task_id)
        else {
            // No live task carries this identity: the completion belongs
            // to an earlier execution that shared this driver (task ids
            // are unique per execution; see `fresh_task_id`). Discard
            // the stale outcome without writing or resuming anything.
            return;
        };
        let host_frame = if matches!(completion.payload, CompletionPayload::HostCall { .. }) {
            self.host_call_frames.remove(&completion.task_id)
        } else {
            None
        };
        match completion.payload {
            CompletionPayload::Resume { fp } => {
                let fp = StablePc::from_raw(fp);
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.pending_effects -= 1;
                    task.fp = fp;
                    if task.pending_effects == 0 {
                        if let Some(result) = task.terminal_result.take() {
                            complete_result = Some((result, task.terminal_trap.take()));
                        } else {
                            task.ready_flag = ReadyFlag::Ready;
                            self.ready_count += 1;
                        }
                    }
                }
                if let Some((result, trap)) = complete_result {
                    let task = self.tasks.remove(task_index).unwrap();
                    self.completed_tasks.push(CompletedTask {
                        stack: task.stack,
                        result,
                        trap,
                    });
                }
            }
            CompletionPayload::ResumeWithI32 { fp, value } => {
                let fp = StablePc::from_raw(fp);
                let push_result = {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.stack.push_i32(value)
                };
                let push_result = vm_result_to_unit(push_result);
                let trap = if push_result.is_err() {
                    let runtime = self.store.lock_runtime_or_panic();
                    let task = self.tasks.get(task_index).unwrap();
                    Some(capture_trap_context(
                        &runtime,
                        &task.stack,
                        task.local_reference,
                        None,
                        &push_result,
                        task.task_id,
                    ))
                } else {
                    None
                };
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.pending_effects -= 1;
                    task.fp = fp;
                    if task.pending_effects == 0 {
                        if push_result.is_err() {
                            complete_result = Some((push_result, trap));
                        } else if let Some(result) = task.terminal_result.take() {
                            complete_result = Some((result, task.terminal_trap.take()));
                        } else {
                            task.ready_flag = ReadyFlag::Ready;
                            self.ready_count += 1;
                        }
                    } else if push_result.is_err() {
                        task.terminal_result = Some(push_result);
                        task.terminal_trap = trap;
                    } else {
                        task.ready_flag = ReadyFlag::Ready;
                        self.ready_count += 1;
                    }
                }
                if let Some((result, trap)) = complete_result {
                    let task = self.tasks.remove(task_index).unwrap();
                    self.completed_tasks.push(CompletedTask {
                        stack: task.stack,
                        result,
                        trap,
                    });
                }
            }
            CompletionPayload::HostCall { result } => {
                // A public-boundary host call resolves to its callback's
                // values rather than a resume address. Write the values
                // into the suspended frame's return slot here, where the
                // scheduler owns the task's stack, and reduce the outcome
                // to the raw resume form handled below. The submitted
                // future itself never touches the stack, so a driver may
                // safely keep polling it even after the submitting
                // execution has been dropped.
                let result = match result.into_result() {
                    HostCallResult::Resume(result) => result,
                    HostCallResult::Values {
                        result: VMResult::Success(values),
                        return_addr,
                        result_types,
                    } => match host_frame {
                        Some(frame) => {
                            let task = self.tasks.get_mut(task_index).unwrap();
                            // SAFETY: `frame` is the host frame recorded when
                            // this call was submitted; its return slot is the
                            // start of its local area, and the pointer is used
                            // only for this write while the task is held here.
                            let slot = unsafe { task.stack.local_area_mut_ptr(&frame) };
                            match crate::host::write_result_values(slot, &result_types, &values) {
                                VMResult::Success(_) => VMResult::Success(return_addr),
                                other => other.propagate_failure(),
                            }
                        }
                        None => VMResult::InvalidOperand,
                    },
                    HostCallResult::Values { result: other, .. } => other.propagate_failure(),
                };
                match result {
                    VMResult::Success(fp) => {
                        let runtime = self.store.lock_runtime_or_panic();
                        let fp = {
                            let task = self.tasks.get_mut(task_index).unwrap();
                            StablePc::from_raw_in_frame(
                                &runtime,
                                &task.stack,
                                task.local_reference,
                                fp,
                            )
                        };
                        let mut complete_result = None;
                        {
                            let task = self.tasks.get_mut(task_index).unwrap();
                            task.pending_effects -= 1;
                            task.fp = fp;
                            if task.pending_effects == 0 {
                                if let Some(result) = task.terminal_result.take() {
                                    complete_result = Some((result, task.terminal_trap.take()));
                                } else {
                                    task.ready_flag = ReadyFlag::Ready;
                                    self.ready_count += 1;
                                }
                            }
                        }
                        if let Some((result, trap)) = complete_result {
                            let task = self.tasks.remove(task_index).unwrap();
                            self.completed_tasks.push(CompletedTask {
                                stack: task.stack,
                                result,
                                trap,
                            });
                        }
                    }
                    other => {
                        let result = vm_result_to_unit(other);
                        let trap = {
                            let runtime = self.store.lock_runtime_or_panic();
                            let task = self.tasks.get(task_index).unwrap();
                            capture_trap_context(
                                &runtime,
                                &task.stack,
                                host_frame.unwrap_or(task.local_reference),
                                None,
                                &result,
                                task.task_id,
                            )
                        };
                        let mut complete_result = None;
                        {
                            let task = self.tasks.get_mut(task_index).unwrap();
                            task.pending_effects -= 1;
                            if task.pending_effects == 0 {
                                complete_result = Some((result, trap));
                            } else {
                                task.terminal_result = Some(result);
                                task.terminal_trap = Some(trap);
                            }
                        }
                        if let Some((result, trap)) = complete_result {
                            let task = self.tasks.remove(task_index).unwrap();
                            self.completed_tasks.push(CompletedTask {
                                stack: task.stack,
                                result,
                                trap: Some(trap),
                            });
                        }
                    }
                }
            }
            CompletionPayload::Interrupted => {
                let result = VMResult::Cancelled;
                let trap = {
                    let runtime = self.store.lock_runtime_or_panic();
                    let task = self.tasks.get(task_index).unwrap();
                    capture_trap_context(
                        &runtime,
                        &task.stack,
                        task.local_reference,
                        None,
                        &result,
                        task.task_id,
                    )
                };
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.pending_effects -= 1;
                    if task.pending_effects == 0 {
                        complete_result = Some((result, trap));
                    } else {
                        task.terminal_result = Some(result);
                        task.terminal_trap = Some(trap);
                    }
                }
                if let Some((result, trap)) = complete_result {
                    let task = self.tasks.remove(task_index).unwrap();
                    self.completed_tasks.push(CompletedTask {
                        stack: task.stack,
                        result,
                        trap: Some(trap),
                    });
                }
            }
            CompletionPayload::WasmAsync => {
                // #128: a driver can hand back this payload for an op the
                // runtime does not implement. Terminate the task with
                // Unimplemented (mirroring the Interrupted arm) instead of
                // aborting the host.
                let result = VMResult::Unimplemented;
                let trap = {
                    let runtime = self.store.lock_runtime_or_panic();
                    let task = self.tasks.get(task_index).unwrap();
                    capture_trap_context(
                        &runtime,
                        &task.stack,
                        task.local_reference,
                        None,
                        &result,
                        task.task_id,
                    )
                };
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.pending_effects -= 1;
                    if task.pending_effects == 0 {
                        complete_result = Some((result, trap));
                    } else {
                        task.terminal_result = Some(result);
                        task.terminal_trap = Some(trap);
                    }
                }
                if let Some((result, trap)) = complete_result {
                    let task = self.tasks.remove(task_index).unwrap();
                    self.completed_tasks.push(CompletedTask {
                        stack: task.stack,
                        result,
                        trap: Some(trap),
                    });
                }
            }
        }
    }

    fn run_ready_tasks_with_runtime(&mut self, runtime: &mut StoreInner) {
        while self.ready_count != 0 {
            let task = self.tasks.pop_front().unwrap();
            if task.ready_flag == ReadyFlag::NonReady {
                self.tasks.push_back(task);
                continue;
            }
            self.ready_count -= 1;
            let Task {
                local_reference,
                fp: pc,
                mut stack,
                task_id,
                mut pending_effects,
                ..
            } = task;
            let fp = pc.resolve(runtime, &stack, local_reference);
            let (res, cont, local_reference) = {
                let current_frame = if local_reference.local_size as usize
                    >= std::mem::size_of::<crate::common::stack::CallStackInfo>()
                {
                    stack.frame_cache(&local_reference)
                } else {
                    CallFrameCache::dummy()
                };
                let stack_memory_ptr = stack.jit_memory_ptr();
                let stack_memory_len = stack.jit_memory_len();
                let stack_top_ptr = stack.jit_top_ptr();
                let local_base_ptr = unsafe { stack.local_area_mut_ptr(&local_reference) };
                let current_instance_globals_ptr = if current_frame.code_addr.is_null() {
                    std::ptr::null()
                } else {
                    runtime.jit_instance_global_addrs_ptr(current_frame.instance)
                };
                let global_values_ptr = runtime.jit_global_values_ptr();
                let (default_local_memory_ptr, default_local_memory_base, default_local_memory_len) =
                    match current_frame.memory0_kind {
                        CachedMemoryKind::Local => {
                            let memory = runtime
                                .local_memory_mut(unsafe {
                                    crate::common::store::LocalMemoryId::from_raw_unchecked(
                                        current_frame.memory0_raw,
                                    )
                                })
                                .memory_mut();
                            (
                                memory as *mut crate::common::Memory,
                                memory.data_mut_ptr(),
                                memory.data_size(),
                            )
                        }
                        CachedMemoryKind::None | CachedMemoryKind::Shared => {
                            (std::ptr::null_mut(), std::ptr::null_mut(), 0)
                        }
                    };
                let mut ec = ExecuteContext {
                    gc: runtime,
                    stack_memory_ptr,
                    stack_memory_len,
                    stack_top_ptr,
                    local_reference,
                    local_base_ptr,
                    default_local_memory_ptr,
                    default_local_memory_base,
                    default_local_memory_len,
                    current_instance_globals_ptr,
                    global_values_ptr,
                    current_frame,
                    stack: &mut stack,
                    store: self.store,
                    effect: EffectSupplier {
                        task_id,
                        pending_effects: &mut pending_effects,
                        queue: &mut self.pending_queue,
                    },
                    cont: fp,
                    task_id,
                    budget: if self.store.metering_ref().is_some() {
                        0
                    } else {
                        u64::MAX
                    },
                    reserved: 0,
                    budget_epoch: 0,
                    jit_trap_pc: std::ptr::null(),
                    jit_stop_pc: std::ptr::null(),
                };
                let res = unsafe { ((*fp).op)(fp.offset(1), &mut ec) };
                if let Some(metering) = self.store.metering_ref() {
                    metering.release(ec.reserved, ec.budget, ec.budget_epoch);
                    ec.reserved = 0;
                    ec.budget = 0;
                }
                (res, ec.cont, ec.local_reference)
            };
            match res {
                VMResult::Success(()) => {
                    if !cont.is_null() {
                        self.tasks.push_back(Task {
                            local_reference,
                            fp: StablePc::from_raw_in_frame(runtime, &stack, local_reference, cont),
                            ready_flag: if pending_effects == 0 {
                                ReadyFlag::Ready
                            } else {
                                ReadyFlag::NonReady
                            },
                            task_id,
                            stack,
                            pending_effects,
                            terminal_result: None,
                            terminal_trap: None,
                        });
                        if pending_effects == 0 {
                            self.ready_count += 1;
                        }
                    } else if pending_effects == 0 {
                        self.completed_tasks.push(CompletedTask {
                            stack,
                            result: VMResult::Success(()),
                            trap: None,
                        });
                    } else {
                        self.tasks.push_back(Task {
                            local_reference,
                            fp: pc,
                            ready_flag: ReadyFlag::NonReady,
                            task_id,
                            stack,
                            pending_effects,
                            terminal_result: Some(VMResult::Success(())),
                            terminal_trap: None,
                        });
                    }
                }
                other => {
                    let trap = capture_trap_context(
                        runtime,
                        &stack,
                        local_reference,
                        Some(cont),
                        &other,
                        task_id,
                    );
                    if pending_effects == 0 {
                        self.completed_tasks.push(CompletedTask {
                            stack,
                            result: other,
                            trap: Some(trap),
                        });
                    } else {
                        self.tasks.push_back(Task {
                            local_reference,
                            fp: pc,
                            ready_flag: ReadyFlag::NonReady,
                            task_id,
                            stack,
                            pending_effects,
                            terminal_result: Some(other),
                            terminal_trap: Some(trap),
                        });
                    }
                }
            }
        }
    }

    pub async fn run_with_driver<D: ExecutionDriver>(&mut self, driver: &mut D) {
        while !self.tasks.is_empty() {
            {
                let mut runtime = self.store.lock_runtime_or_panic();
                self.run_ready_tasks_with_runtime(&mut runtime);
            }
            if self.tasks.is_empty() {
                break;
            }
            self.submit_pending_ops(driver);
            if self.ready_count != 0 {
                continue;
            }
            let Some(completion) = driver.next_completion().await else {
                break;
            };
            self.apply_completion(completion);
        }
    }

    pub async fn run(&mut self) {
        let mut driver = TokioDriver::new();
        self.run_with_driver(&mut driver).await;
    }

    pub fn run_sync_with_runtime(&mut self, runtime: &mut StoreInner) -> Result<(), SyncRunError> {
        while !self.tasks.is_empty() {
            self.run_ready_tasks_with_runtime(runtime);
            if self.tasks.is_empty() {
                break;
            }
            if self.ready_count != 0 {
                continue;
            }
            if !self.pending_queue.is_empty() {
                return Err(SyncRunError::AsyncPending);
            }
            return Err(SyncRunError::Stalled);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fresh_task_id, seed_task_id_floor, Completion, CompletionPayload, ExecutionDriver,
        HostCallPending, PendingOp, ReadyFlag, Scheduler, Task, TokioDriver,
    };
    use crate::runtime::memory_effect::{HostCallFuture, HostCallResult};
    use crate::{
        common::{CallFrameCache, LocalReference, StablePc, Stack},
        Store, VMResult,
    };

    #[tokio::test]
    async fn tokio_driver_completes_host_call_future() {
        let mut driver = TokioDriver::new();
        driver.submit(PendingOp::HostCall(HostCallPending {
            task_id: 3,
            future: HostCallFuture::from_raw(Box::pin(async {
                VMResult::Success(std::ptr::null())
            })),
            frame_reference: LocalReference {
                local_top: 0,
                local_size: 0,
            },
        }));
        let completion = driver.next_completion().await.unwrap();
        match completion.payload {
            CompletionPayload::HostCall { result } => {
                assert!(matches!(
                    result.into_result(),
                    HostCallResult::Resume(VMResult::Success(ptr)) if ptr.is_null()
                ));
            }
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    #[test]
    fn resume_i32_stack_overflow_captures_before_completion() {
        let store = Store::new();
        let mut stack = Stack::new(std::mem::size_of::<crate::common::stack::CallStackInfo>());
        let local_reference = match stack.function_call_cached(
            0,
            0,
            0,
            CallFrameCache::dummy(),
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            StablePc::from_raw(0),
        ) {
            VMResult::Success(local_reference) => local_reference,
            other => panic!("frame must exactly fill the test stack: {other:?}"),
        };
        let mut scheduler = Scheduler::new(&store);
        scheduler.push(Task {
            task_id: 7,
            stack,
            local_reference,
            pending_effects: 1,
            ready_flag: ReadyFlag::NonReady,
            fp: StablePc::from_raw(0),
            terminal_result: None,
            terminal_trap: None,
        });

        scheduler.apply_completion(Completion {
            task_id: 7,
            payload: CompletionPayload::ResumeWithI32 { fp: 0, value: 1 },
        });

        let completed = scheduler
            .completed_tasks
            .pop()
            .expect("overflowing completion must finish the task");
        assert!(matches!(completed.result, VMResult::StackOverflow));
        let context = completed
            .trap
            .expect("overflowing completion must carry a trap context");
        assert!(matches!(&context.result, VMResult::StackOverflow));
        assert_eq!(context.task_id, 7);
        assert_eq!(context.frames[0].pc_index, None);
    }

    // #128: the unimplemented WasmAsync pending op must not abort the
    // host; the driver reports it back as a completion instead.
    #[cfg(feature = "unstable-internals")]
    #[tokio::test]
    async fn tokio_driver_reports_wasm_async_op_as_completion() {
        let mut driver = TokioDriver::new();
        driver.submit(PendingOp::WasmAsync(crate::runtime::WasmAsyncPending {
            task_id: 7,
        }));
        let completion = driver
            .next_completion()
            .await
            .expect("wasm async op must produce a completion, not a panic");
        assert_eq!(completion.task_id, 7);
        assert!(matches!(completion.payload, CompletionPayload::WasmAsync));
    }

    // #128: applying a WasmAsync completion terminates the task with
    // Unimplemented instead of aborting the host.
    #[cfg(feature = "unstable-internals")]
    #[test]
    fn wasm_async_completion_terminates_task_with_unimplemented() {
        let store = Store::new();
        let mut stack = Stack::new(std::mem::size_of::<crate::common::stack::CallStackInfo>());
        let local_reference = match stack.function_call_cached(
            0,
            0,
            0,
            CallFrameCache::dummy(),
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            StablePc::from_raw(0),
        ) {
            VMResult::Success(local_reference) => local_reference,
            other => panic!("frame must exactly fill the test stack: {other:?}"),
        };
        let mut scheduler = Scheduler::new(&store);
        scheduler.push(Task {
            task_id: 7,
            stack,
            local_reference,
            pending_effects: 1,
            ready_flag: ReadyFlag::NonReady,
            fp: StablePc::from_raw(0),
            terminal_result: None,
            terminal_trap: None,
        });

        scheduler.apply_completion(Completion {
            task_id: 7,
            payload: CompletionPayload::WasmAsync,
        });

        let completed = scheduler
            .completed_tasks
            .pop()
            .expect("wasm async completion must finish the task");
        assert!(matches!(completed.result, VMResult::Unimplemented));
    }

    #[test]
    fn task_ids_do_not_wrap_at_the_u32_boundary() {
        // Seed the process-wide counter just below the former 32-bit
        // boundary: allocations must cross it without wrapping or
        // reissuing an identity a retained completion could still name.
        // Successive allocations in one thread are strictly increasing
        // even if other tests allocate concurrently, and the seed only
        // ever moves the shared counter forward.
        seed_task_id_floor(u64::from(u32::MAX) - 1);
        let first = fresh_task_id();
        let second = fresh_task_id();
        let third = fresh_task_id();
        assert!(first < second && second < third);
        assert!(third > u64::from(u32::MAX));
    }
}
