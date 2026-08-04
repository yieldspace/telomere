#[cfg(feature = "threads")]
use super::memory_effect::MemoryWaitPending;
use super::memory_effect::{Completion, CompletionPayload, HostCallPending, PendingOp};
use crate::{
    common::{
        stack::CachedMemoryKind, CallFrameCache, ExecuteContext, LocalReference, StablePc,
        StoreInner,
    },
    Stack, Store, VMResult,
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
        VMResult::Unimplemented => VMResult::Unimplemented,
        VMResult::FuelExhausted => VMResult::FuelExhausted,
        VMResult::Cancelled => VMResult::Cancelled,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadyFlag {
    Ready,
    NonReady,
}

#[derive(Debug)]
pub(crate) struct Task {
    pub task_id: u32,
    pub stack: Stack,
    pub local_reference: LocalReference,
    pub pending_effects: u32,
    pub ready_flag: ReadyFlag,
    pub fp: StablePc,
    pub terminal_result: Option<VMResult<()>>,
}

#[derive(Debug)]
pub(crate) struct CompletedTask {
    pub stack: Stack,
    pub result: VMResult<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncRunError {
    AsyncPending,
    Stalled,
}

pub struct EffectSupplier<'a> {
    task_id: u32,
    pending_effects: &'a mut u32,
    queue: &'a mut VecDeque<PendingOp>,
}

impl EffectSupplier<'_> {
    pub fn get_pending_count(&self) -> u32 {
        *self.pending_effects
    }

    #[cfg(test)]
    pub(crate) fn from_parts<'a>(
        task_id: u32,
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
pub trait ExecutionDriver {
    /// Submits a pending host call or shared-memory wait for asynchronous execution.
    fn submit(&mut self, op: PendingOp);

    /// Resolves to the next completion, or `None` when no more work can complete.
    fn next_completion<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>>;
}

#[derive(Default)]
/// The built-in driver for async host calls and shared-memory waits.
///
/// It polls submitted futures directly, so callers must invoke the surrounding
/// execution future from a Tokio runtime or another compatible async context.
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
            let value = op.wait.wait_result(op.shared, op.timeout_ns).await;
            Completion {
                task_id: op.task_id,
                payload: CompletionPayload::ResumeWithI32 { fp: op.fp, value },
            }
        }));
    }
}

impl ExecutionDriver for TokioDriver {
    fn submit(&mut self, op: PendingOp) {
        match op {
            PendingOp::HostCall(op) => self.submit_host_call(op),
            #[cfg(feature = "threads")]
            PendingOp::MemoryWait(op) => self.submit_memory_wait(op),
            PendingOp::WasmAsync(op) => {
                panic!(
                    "Wasm async pending op is not implemented yet for task {}",
                    op.task_id
                )
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
}

impl<'a> Scheduler<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self {
            tasks: VecDeque::new(),
            pending_queue: VecDeque::new(),
            completed_tasks: vec![],
            store,
            ready_count: 0,
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
            driver.submit(op);
        }
    }

    fn apply_completion(&mut self, completion: Completion) {
        let Some(task_index) = self
            .tasks
            .iter()
            .position(|task| task.task_id == completion.task_id)
        else {
            return;
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
                            complete_result = Some(result);
                        } else {
                            task.ready_flag = ReadyFlag::Ready;
                            self.ready_count += 1;
                        }
                    }
                }
                if let Some(result) = complete_result {
                    let task = self.tasks.remove(task_index).unwrap();
                    self.completed_tasks.push(CompletedTask {
                        stack: task.stack,
                        result,
                    });
                }
            }
            CompletionPayload::ResumeWithI32 { fp, value } => {
                let fp = StablePc::from_raw(fp);
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    let push_result = task.stack.push_i32(value);
                    task.pending_effects -= 1;
                    task.fp = fp;
                    let push_result = vm_result_to_unit(push_result);
                    if task.pending_effects == 0 {
                        if push_result.is_err() {
                            complete_result = Some(push_result);
                        } else if let Some(result) = task.terminal_result.take() {
                            complete_result = Some(result);
                        } else {
                            task.ready_flag = ReadyFlag::Ready;
                            self.ready_count += 1;
                        }
                    } else if push_result.is_err() {
                        task.terminal_result = Some(push_result);
                    } else {
                        task.ready_flag = ReadyFlag::Ready;
                        self.ready_count += 1;
                    }
                }
                if let Some(result) = complete_result {
                    let task = self.tasks.remove(task_index).unwrap();
                    self.completed_tasks.push(CompletedTask {
                        stack: task.stack,
                        result,
                    });
                }
            }
            CompletionPayload::HostCall { result } => match result {
                VMResult::Success(fp) => {
                    let gc = self.store.lock_gc();
                    let fp = {
                        let task = self.tasks.get_mut(task_index).unwrap();
                        StablePc::from_raw_in_frame(&gc, &task.stack, task.local_reference, fp)
                    };
                    let mut complete_result = None;
                    {
                        let task = self.tasks.get_mut(task_index).unwrap();
                        task.pending_effects -= 1;
                        task.fp = fp;
                        if task.pending_effects == 0 {
                            if let Some(result) = task.terminal_result.take() {
                                complete_result = Some(result);
                            } else {
                                task.ready_flag = ReadyFlag::Ready;
                                self.ready_count += 1;
                            }
                        }
                    }
                    if let Some(result) = complete_result {
                        let task = self.tasks.remove(task_index).unwrap();
                        self.completed_tasks.push(CompletedTask {
                            stack: task.stack,
                            result,
                        });
                    }
                }
                other => {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.pending_effects -= 1;
                    if task.pending_effects == 0 {
                        let task = self.tasks.remove(task_index).unwrap();
                        self.completed_tasks.push(CompletedTask {
                            stack: task.stack,
                            result: vm_result_to_unit(other),
                        });
                    } else {
                        task.terminal_result = Some(vm_result_to_unit(other));
                    }
                }
            },
            CompletionPayload::WasmAsync => {
                panic!("Wasm async completion is not implemented yet")
            }
        }
    }

    fn run_ready_tasks_with_gc(&mut self, gc: &mut StoreInner) {
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
            let fp = pc.resolve(gc, &stack, local_reference);
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
                    gc.jit_instance_global_addrs_ptr(current_frame.instance)
                };
                let global_values_ptr = gc.jit_global_values_ptr();
                let default_local_memory_ptr = match current_frame.memory0_kind {
                    CachedMemoryKind::Local => {
                        gc.local_memory_mut(unsafe {
                            crate::common::store::LocalMemoryId::from_raw_unchecked(
                                current_frame.memory0_raw,
                            )
                        })
                        .memory_mut() as *mut crate::common::Memory
                    }
                    CachedMemoryKind::None | CachedMemoryKind::Shared => std::ptr::null_mut(),
                };
                let mut ec = ExecuteContext {
                    gc,
                    stack_memory_ptr,
                    stack_memory_len,
                    stack_top_ptr,
                    local_reference,
                    local_base_ptr,
                    default_local_memory_ptr,
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
                            fp: StablePc::from_raw_in_frame(gc, &stack, local_reference, cont),
                            ready_flag: if pending_effects == 0 {
                                ReadyFlag::Ready
                            } else {
                                ReadyFlag::NonReady
                            },
                            task_id,
                            stack,
                            pending_effects,
                            terminal_result: None,
                        });
                        if pending_effects == 0 {
                            self.ready_count += 1;
                        }
                    } else if pending_effects == 0 {
                        self.completed_tasks.push(CompletedTask {
                            stack,
                            result: VMResult::Success(()),
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
                        });
                    }
                }
                other => {
                    if pending_effects == 0 {
                        self.completed_tasks.push(CompletedTask {
                            stack,
                            result: other,
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
                        });
                    }
                }
            }
        }
    }

    pub async fn run_with_driver<D: ExecutionDriver>(&mut self, driver: &mut D) {
        while !self.tasks.is_empty() {
            {
                let mut gc = self.store.lock_gc();
                self.run_ready_tasks_with_gc(&mut gc);
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

    pub fn run_sync_with_gc(&mut self, gc: &mut StoreInner) -> Result<(), SyncRunError> {
        while !self.tasks.is_empty() {
            self.run_ready_tasks_with_gc(gc);
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
    use super::{CompletionPayload, ExecutionDriver, HostCallPending, PendingOp, TokioDriver};
    use crate::VMResult;

    #[tokio::test]
    async fn tokio_driver_completes_host_call_future() {
        let mut driver = TokioDriver::new();
        driver.submit(PendingOp::HostCall(HostCallPending {
            task_id: 3,
            future: Box::pin(async { VMResult::Success(std::ptr::null()) }),
        }));
        let completion = driver.next_completion().await.unwrap();
        match completion.payload {
            CompletionPayload::HostCall { result } => {
                assert!(matches!(result, VMResult::Success(ptr) if ptr.is_null()));
            }
            other => panic!("unexpected completion: {other:?}"),
        }
    }
}
