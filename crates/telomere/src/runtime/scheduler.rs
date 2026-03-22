use super::memory_effect::{
    Completion, CompletionPayload, HostCallPending, MemoryWaitPending, PendingOp,
};
use crate::{
    common::{
        CallFrameCache, ExecuteContext, LocalReference, ResultType, ResultValue, StablePc,
        StoreInner, ValType, WasmValue,
    },
    Stack, Store, VMResult,
};
use futures::{future::poll_fn, stream::FuturesUnordered, StreamExt};
use std::{collections::VecDeque, future::Future, pin::Pin, time::Duration};

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
        VMResult::InvalidOperand => VMResult::InvalidOperand,
    }
}

fn result_type_size(ty: &ResultType) -> usize {
    ty.iter().map(|value| value.stack_size().usize()).sum()
}

fn append_typed_value_bytes(buffer: &mut Vec<u8>, ty: ValType, value: &WasmValue) -> VMResult<()> {
    match (ty, value) {
        (ValType::I32, WasmValue::I32(value)) => buffer.extend_from_slice(&value.to_le_bytes()),
        (ValType::I64, WasmValue::I64(value)) => buffer.extend_from_slice(&value.to_le_bytes()),
        (ValType::F32, WasmValue::F32(value)) => {
            buffer.extend_from_slice(&value.to_bits().to_le_bytes())
        }
        (ValType::F64, WasmValue::F64(value)) => {
            buffer.extend_from_slice(&value.to_bits().to_le_bytes())
        }
        (ValType::V128, WasmValue::V128(value)) => buffer.extend_from_slice(&value.to_le_bytes()),
        (ValType::FuncRef, WasmValue::FuncRef(value)) => {
            buffer.extend_from_slice(&value.to_le_bytes())
        }
        (ValType::ExternRef, WasmValue::ExternRef(value)) => {
            buffer.extend_from_slice(&value.to_le_bytes())
        }
        _ => return VMResult::InvalidOperand,
    }
    VMResult::Success(())
}

fn encode_result_values(types: &ResultType, values: &ResultValue) -> VMResult<Vec<u8>> {
    if types.0.len() != values.len() {
        return VMResult::InvalidOperand;
    }
    let mut encoded = Vec::with_capacity(result_type_size(types));
    for (ty, value) in types.iter().zip(values.iter()) {
        vm_try!(append_typed_value_bytes(&mut encoded, *ty, value));
    }
    VMResult::Success(encoded)
}

fn write_marshaled_results(
    stack: &mut Stack,
    slot_offset: usize,
    types: &ResultType,
    values: &ResultValue,
) -> VMResult<()> {
    let encoded = vm_try!(encode_result_values(types, values));
    let slot = LocalReference {
        local_top: slot_offset,
        local_size: encoded.len() as u32,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(
            encoded.as_ptr(),
            stack.local_area_mut_ptr(&slot),
            encoded.len(),
        );
    }
    VMResult::Success(())
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
    pub pending_ops: u32,
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

pub struct PendingOpEmitter<'a> {
    task_id: u32,
    pending_ops: &'a mut u32,
    queue: &'a mut VecDeque<PendingOp>,
}

impl PendingOpEmitter<'_> {
    #[cfg(test)]
    pub(crate) fn from_parts<'a>(
        task_id: u32,
        pending_ops: &'a mut u32,
        queue: &'a mut VecDeque<PendingOp>,
    ) -> PendingOpEmitter<'a> {
        PendingOpEmitter {
            task_id,
            pending_ops,
            queue,
        }
    }

    pub(crate) fn push_pending(&mut self, op: PendingOp) {
        debug_assert_eq!(op.task_id(), self.task_id);
        self.queue.push_back(op);
        *self.pending_ops += 1;
    }

    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }

    pub(crate) fn appended_pending_code(
        &self,
        before_len: usize,
    ) -> Option<Option<crate::common::formal::PendingCode>> {
        match self.queue.len().checked_sub(before_len) {
            Some(0) => Some(None),
            Some(1) => self.queue.get(before_len).map(PendingOp::pending_code),
            Some(_) | None => None,
        }
    }
}

type DriverFuture = Pin<Box<dyn Future<Output = Completion>>>;

pub trait ExecutionDriver {
    fn submit(&mut self, op: PendingOp);
    fn next_completion<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>>;
}

#[derive(Default)]
pub struct TokioDriver {
    inflight: FuturesUnordered<DriverFuture>,
}

impl TokioDriver {
    pub fn new() -> Self {
        Self::default()
    }

    fn submit_host_call(&mut self, op: HostCallPending) {
        self.inflight.push(Box::pin(async move {
            Completion {
                task_id: op.task_id,
                payload: CompletionPayload::HostCall {
                    fp: op.fp,
                    result_types: op.result_types,
                    result_slot: op.result_slot,
                    result: op.future.await,
                },
            }
        }));
    }

    fn submit_memory_wait(&mut self, op: MemoryWaitPending) {
        self.inflight.push(Box::pin(async move {
            let value = if op.timeout_ns < 0 {
                poll_fn(|cx| op.wait.poll_wait(cx)).await;
                op.wait.finish_notified(&op.shared)
            } else {
                let sleep = tokio::time::sleep(Duration::from_nanos(op.timeout_ns as u64));
                tokio::pin!(sleep);
                tokio::select! {
                    _ = poll_fn(|cx| op.wait.poll_wait(cx)) => {
                        op.wait.finish_notified(&op.shared)
                    }
                    _ = &mut sleep => op.wait.finish_timeout(&op.shared),
                }
            };
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

pub(crate) struct ExecutionKernel<'a> {
    tasks: VecDeque<Task>,
    pending_queue: VecDeque<PendingOp>,
    pub(crate) completed_tasks: Vec<CompletedTask>,
    pub(crate) store: &'a Store,
    ready_count: u32,
}

impl<'a> ExecutionKernel<'a> {
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
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    task.pending_ops -= 1;
                    task.fp = fp;
                    if task.pending_ops == 0 {
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
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    let push_result = task.stack.push_i32(value);
                    task.pending_ops -= 1;
                    task.fp = fp;
                    if task.pending_ops == 0 {
                        if let Some(result) = task.terminal_result.take() {
                            complete_result = Some(result);
                        } else {
                            complete_result = Some(vm_result_to_unit(push_result));
                        }
                    } else if push_result.is_err() {
                        task.terminal_result = Some(vm_result_to_unit(push_result));
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
            CompletionPayload::HostCall {
                fp,
                result_types,
                result_slot,
                result,
            } => {
                let mut complete_result = None;
                {
                    let task = self.tasks.get_mut(task_index).unwrap();
                    let host_result = match result {
                        VMResult::Success(values) => vm_result_to_unit(write_marshaled_results(
                            &mut task.stack,
                            result_slot,
                            &result_types,
                            &values,
                        )),
                        other => vm_result_to_unit(other),
                    };
                    task.pending_ops -= 1;
                    task.fp = fp;
                    if task.pending_ops == 0 {
                        if let Some(result) = task.terminal_result.take() {
                            complete_result = Some(result);
                        } else {
                            complete_result = Some(host_result);
                        }
                    } else if host_result.is_err() {
                        task.terminal_result = Some(host_result);
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
                mut pending_ops,
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
                let mut ec = ExecuteContext::new(
                    &mut stack,
                    local_reference,
                    current_frame,
                    self.store,
                    gc,
                    PendingOpEmitter {
                        task_id,
                        pending_ops: &mut pending_ops,
                        queue: &mut self.pending_queue,
                    },
                    fp,
                    task_id,
                );
                let res = unsafe { ((*fp).op)(fp.offset(1), &mut ec) };
                (res, ec.cont(), ec.local_reference())
            };
            match res {
                VMResult::Success(()) => {
                    if !cont.is_null() {
                        self.tasks.push_back(Task {
                            local_reference,
                            fp: StablePc::from_raw_in_frame(gc, &stack, local_reference, cont),
                            ready_flag: if pending_ops == 0 {
                                ReadyFlag::Ready
                            } else {
                                ReadyFlag::NonReady
                            },
                            task_id,
                            stack,
                            pending_ops,
                            terminal_result: None,
                        });
                        if pending_ops == 0 {
                            self.ready_count += 1;
                        }
                    } else if pending_ops == 0 {
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
                            pending_ops,
                            terminal_result: Some(VMResult::Success(())),
                        });
                    }
                }
                other => {
                    if pending_ops == 0 {
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
                            pending_ops,
                            terminal_result: Some(other),
                        });
                    }
                }
            }
        }
    }

    pub async fn run<D: ExecutionDriver>(&mut self, driver: &mut D) {
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
    use std::{collections::VecDeque, future::Future, pin::Pin};

    use crate::{
        common::{LocalReference, ResultValue, StablePc, WasmValue},
        runtime::memory_effect::{Completion, CompletionPayload, PendingOp, WasmAsyncPending},
        Stack, Store, VMResult,
    };

    use super::{
        CompletedTask, ExecutionDriver, ExecutionKernel, ReadyFlag, SyncRunError, Task, TokioDriver,
    };

    struct MockDriver {
        submitted: Vec<PendingOp>,
        completions: VecDeque<Completion>,
    }

    impl MockDriver {
        fn new(completions: VecDeque<Completion>) -> Self {
            Self {
                submitted: vec![],
                completions,
            }
        }
    }

    impl ExecutionDriver for MockDriver {
        fn submit(&mut self, op: PendingOp) {
            self.submitted.push(op);
        }

        fn next_completion<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>> {
            Box::pin(async move { self.completions.pop_front() })
        }
    }

    #[test]
    fn sync_kernel_fail_closes_pending_queue() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let mut kernel = ExecutionKernel::new(&store);
        kernel.push(Task {
            task_id: 1,
            stack: Stack::new(32),
            local_reference: LocalReference {
                local_size: 0,
                local_top: 0,
            },
            pending_ops: 1,
            ready_flag: ReadyFlag::NonReady,
            fp: StablePc::from_relative_index(0),
            terminal_result: None,
        });
        kernel
            .pending_queue
            .push_back(PendingOp::WasmAsync(WasmAsyncPending { task_id: 1 }));
        assert_eq!(
            kernel.run_sync_with_gc(&mut gc),
            Err(SyncRunError::AsyncPending)
        );
    }

    #[test]
    fn host_completion_writes_marshaled_results() {
        let store = Store::new();
        let mut kernel = ExecutionKernel::new(&store);
        let mut stack = Stack::new(32);
        stack.push_u32(0).unwrap();
        kernel.push(Task {
            task_id: 7,
            stack,
            local_reference: LocalReference {
                local_size: 0,
                local_top: 0,
            },
            pending_ops: 1,
            ready_flag: ReadyFlag::NonReady,
            fp: StablePc::from_relative_index(0),
            terminal_result: None,
        });

        kernel.apply_completion(crate::runtime::memory_effect::Completion {
            task_id: 7,
            payload: CompletionPayload::HostCall {
                fp: StablePc::from_relative_index(1),
                result_types: crate::common::FuncType::new(
                    vec![],
                    vec![crate::common::ValType::I32],
                )
                .1,
                result_slot: 0,
                result: VMResult::Success(ResultValue::new(vec![WasmValue::I32(41)])),
            },
        });

        let CompletedTask { stack, result } = kernel.completed_tasks.pop().unwrap();
        assert!(matches!(result, VMResult::Success(())));
        assert_eq!(
            stack.local_bytes(
                &LocalReference {
                    local_top: 0,
                    local_size: 4
                },
                0,
                4
            ),
            &41i32.to_le_bytes()
        );
    }

    #[tokio::test]
    async fn kernel_resumes_with_mock_driver_completion() {
        let store = Store::new();
        let mut kernel = ExecutionKernel::new(&store);
        kernel.push(Task {
            task_id: 11,
            stack: Stack::new(32),
            local_reference: LocalReference {
                local_size: 0,
                local_top: 0,
            },
            pending_ops: 1,
            ready_flag: ReadyFlag::NonReady,
            fp: StablePc::from_relative_index(0),
            terminal_result: None,
        });
        kernel
            .pending_queue
            .push_back(PendingOp::WasmAsync(WasmAsyncPending { task_id: 11 }));
        let mut completions = VecDeque::new();
        completions.push_back(Completion {
            task_id: 11,
            payload: CompletionPayload::ResumeWithI32 {
                fp: StablePc::from_relative_index(3),
                value: 27,
            },
        });
        let mut driver = MockDriver::new(completions);

        kernel.run(&mut driver).await;

        assert_eq!(driver.submitted.len(), 1);
        let CompletedTask { mut stack, result } = kernel.completed_tasks.pop().unwrap();
        assert!(matches!(result, VMResult::Success(())));
        assert_eq!(stack.pop_i32(), 27);
    }

    #[tokio::test]
    async fn tokio_driver_completes_host_call_future() {
        let mut driver = TokioDriver::new();
        driver.submit(PendingOp::HostCall(
            crate::runtime::memory_effect::HostCallPending {
                task_id: 3,
                future: Box::pin(async {
                    VMResult::Success(ResultValue::new(vec![WasmValue::I32(9)]))
                }),
                fp: StablePc::from_relative_index(2),
                result_types: crate::common::FuncType::new(
                    vec![],
                    vec![crate::common::ValType::I32],
                )
                .1,
                result_slot: 0,
            },
        ));
        let completion = driver.next_completion().await.unwrap();
        match completion.payload {
            CompletionPayload::HostCall { result, .. } => {
                assert!(matches!(result, VMResult::Success(_)));
            }
            other => panic!("unexpected completion: {other:?}"),
        }
    }
}
