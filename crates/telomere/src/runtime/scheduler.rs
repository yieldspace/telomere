use super::{
    memory_effect::{
        AtomicFlag, Effect, MemoryEffect, Operation, ReadOperationHandler, Target, WriteOperation,
    },
    vm::traps::TRAPS_MEMORY_INDEX_OUT_OF_RANGE,
};
use crate::{
    common::{gc::MemoryPool, ExecuteContext, GcRef, Instr, LocalReference, MemArg},
    Stack, Store, VMResult,
};
use futures::{future::FusedFuture, stream::FuturesUnordered};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Poll, Waker},
};

fn compute_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(
        memarg.offset.checked_add(offset).map(|v| v as usize),
        || VMResult::MemoryIndexOutOfRange,
    )
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
    pub fp: *const Instr,
}

#[derive(Debug)]
pub(crate) struct CompletedTask {
    pub stack: Stack,
    pub result: VMResult<()>,
}
pub(crate) struct AsyncResult {
    pub task_id: u32,
    pub fp: *const Instr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncRunError {
    AsyncPending,
    Stalled,
}

pub(crate) struct Notify {
    ready: AtomicBool,
    waker: Waker,
}
impl Notify {
    pub fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            waker: Waker::noop().clone(),
        }
    }
    fn wake(&self) {
        trace!("wake");

        self.ready.store(true, std::sync::atomic::Ordering::Release);
        self.waker.wake_by_ref();
    }
    fn receiver(&mut self) -> NotificationReceiver<'_> {
        NotificationReceiver { notify: self }
    }
}

struct NotificationReceiver<'a> {
    notify: &'a mut Notify,
}
impl Future for NotificationReceiver<'_> {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self
            .notify
            .ready
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(true) => {
                trace!("ready");
                Poll::Ready(())
            }
            Err(false) => {
                trace!("pending");
                self.notify.waker.clone_from(cx.waker());
                Poll::Pending
            }
            _ => unreachable!(),
        }
    }
}
impl FusedFuture for NotificationReceiver<'_> {
    fn is_terminated(&self) -> bool {
        false
    }
}
pub(crate) struct Scheduler<'a> {
    tasks: VecDeque<Task>,
    notify: Notify,
    async_tasks: FuturesUnordered<Pin<Box<dyn Future<Output = AsyncResult>>>>,
    pub(crate) completed_tasks: Vec<CompletedTask>,
    pub(crate) store: &'a mut Store,
    effects: VecDeque<Effect>,
    ready_count: u32,
}

pub struct EffectSupplier<'a> {
    pending_effects: &'a mut u32,
    effects: &'a mut VecDeque<Effect>,
}
impl EffectSupplier<'_> {
    pub(crate) fn get_pending_count(&self) -> u32 {
        *self.pending_effects
    }
}
fn write_operation_size(op: &WriteOperation) -> usize {
    match op {
        WriteOperation::Write1(_) => 1,
        WriteOperation::Write2(_) => 2,
        WriteOperation::Write4(_) => 4,
        WriteOperation::Write8(_) => 8,
        WriteOperation::Write16(_) => 16,
    }
}
impl EffectSupplier<'_> {
    pub(crate) fn push_non_atomic_memory_read_effect(
        &mut self,
        task_id: u32,
        addr: GcRef,
        memarg: MemArg,
        offset: u32,
        size: u32,
        handler: ReadOperationHandler,
    ) -> VMResult<()> {
        trace!("push_non_atomic_memory_read_effect");
        let start = vm_try!(compute_offset(memarg, offset));
        let end = vm_try!(VMResult::from_option(
            start.checked_add(size as usize),
            || VMResult::MemoryIndexOutOfRange
        ));
        self.effects.push_back(Effect::MemoryEffect(MemoryEffect {
            task_id,
            target: Target::Memory(addr, start..end),
            atomic: AtomicFlag::NonAtomic,
            operation: Operation::Read(handler),
        }));
        *self.pending_effects += 1;
        VMResult::Success(())
    }
    pub(crate) fn push_non_atomic_memory_write_effect(
        &mut self,
        task_id: u32,
        addr: GcRef,
        memarg: MemArg,
        offset: u32,
        gc: &mut MemoryPool,
        operation: WriteOperation,
    ) -> VMResult<()> {
        trace!("push_non_atomic_memory_write_effect");
        let size = write_operation_size(&operation);
        let start = vm_try!(compute_offset(memarg, offset));
        let end = vm_try!(VMResult::from_option(start.checked_add(size), || {
            VMResult::MemoryIndexOutOfRange
        }));
        vm_try!(VMResult::from_option(
            unsafe { gc.get_memory(addr).get_mut(start as usize..end as usize) },
            || VMResult::MemoryIndexOutOfRange
        ));
        self.effects.push_back(Effect::MemoryEffect(MemoryEffect {
            task_id,
            target: Target::Memory(addr, start..end),
            atomic: AtomicFlag::NonAtomic,
            operation: Operation::Write(operation),
        }));
        *self.pending_effects += 1;
        VMResult::Success(())
    }
}

impl<'a> Scheduler<'a> {
    pub fn new(store: &'a mut Store) -> Self {
        Self {
            tasks: VecDeque::new(),
            completed_tasks: vec![],
            store,
            effects: VecDeque::new(),
            ready_count: 0,
            notify: Notify::new(),
            async_tasks: FuturesUnordered::new(),
        }
    }
    pub fn push(&mut self, task: Task) {
        let is_ready = task.ready_flag == ReadyFlag::Ready;
        self.tasks.push_back(task);
        if is_ready {
            self.ready_count += 1;
            self.notify.wake();
        }
    }
    unsafe fn handle_memory_effect(&mut self, gc: &mut MemoryPool, effect: MemoryEffect) {
        trace!("{:?}", self.tasks);
        let task = self
            .tasks
            .iter_mut()
            .find(|v| v.task_id == effect.task_id)
            .unwrap();

        match effect {
            MemoryEffect {
                task_id: _,
                target: Target::Memory(addr, range),
                atomic: AtomicFlag::NonAtomic,
                operation: Operation::Read(handler),
            } => {
                let data = unsafe { gc.get_memory(addr).get(range.start..range.end) };
                if let Some(data) = data {
                    task.fp = handler(&mut task.stack, data, task.fp);
                } else {
                    task.fp = TRAPS_MEMORY_INDEX_OUT_OF_RANGE.as_ptr();
                }

                task.pending_effects -= 1;
                if task.pending_effects == 0 {
                    task.ready_flag = ReadyFlag::Ready;
                    self.ready_count += 1;
                    self.notify.wake();
                }
            }
            MemoryEffect {
                task_id: _,
                target: Target::Memory(addr, range),
                atomic: AtomicFlag::NonAtomic,
                operation: Operation::Write(operation),
            } => {
                let dst = unsafe { gc.get_memory(addr).get_mut(range.start..range.end) };
                if let Some(dst) = dst {
                    dst.copy_from_slice(operation.get());
                } else {
                    unreachable!()
                }
                task.pending_effects -= 1;
                if task.pending_effects == 0 {
                    task.ready_flag = ReadyFlag::Ready;
                    self.ready_count += 1;
                    self.notify.wake();
                }
            }
        }
    }
    #[cfg(feature = "async-runtime")]
    unsafe fn handle_async_effect_call(&mut self, effect: super::memory_effect::AsyncEffect) {
        use crate::runtime::memory_effect::AsyncEffectOperation;

        use super::memory_effect::AsyncEffect;
        let AsyncEffect { task_id, operation } = effect;
        let task = self
            .tasks
            .iter_mut()
            .find(|v| v.task_id == effect.task_id)
            .unwrap();
        match operation {
            AsyncEffectOperation::Call(sup) => {
                self.async_tasks.push(sup(task_id, task.fp));
            }
        }
    }
    fn handle_async_return(&mut self, ret: AsyncResult) {
        let task = self
            .tasks
            .iter_mut()
            .find(|v| v.task_id == ret.task_id)
            .unwrap();
        task.pending_effects -= 1;
        task.ready_flag = ReadyFlag::Ready;
        self.ready_count += 1;
        self.notify.wake();
        task.fp = ret.fp;
    }
    #[cfg(feature = "async-runtime")]
    async fn await_executation(&mut self) {
        use futures::{select_biased, StreamExt};
        trace!("await_executation");
        loop {
            select_biased! {
                fut = self.async_tasks.select_next_some() => {
                    self.handle_async_return(fut);
                }
                _ = self.notify.receiver() => {
                    break;
                }
            }
        }
    }

    pub async fn run(&mut self) {
        let gc = self.store.gc.clone();

        while !self.tasks.is_empty() {
            self.await_executation().await;
            let mut gc = gc.borrow_mut();
            let gc = &mut gc;
            while self.ready_count != 0 {
                trace!("task ready count: {:?}", self.ready_count);

                let task = self.tasks.pop_front().unwrap();
                if task.ready_flag == ReadyFlag::NonReady {
                    self.tasks.push_back(task);
                    continue;
                }
                self.ready_count -= 1;
                let Task {
                    local_reference,
                    fp,
                    mut stack,
                    task_id,
                    mut pending_effects,
                    ..
                } = task;

                let mut ec = ExecuteContext {
                    gc,
                    local_reference,
                    stack: &mut stack,
                    store: self.store,
                    effect: EffectSupplier {
                        pending_effects: &mut pending_effects,
                        effects: &mut self.effects,
                    },
                    cont: fp,
                    task_id,
                };
                let res = unsafe { ((*fp).op)(fp.offset(1), &mut ec) };
                let cont = ec.cont;
                let local_reference = ec.local_reference;
                match res {
                    VMResult::Success(()) => {
                        if !cont.is_null() {
                            trace!("continue task: {}", ec.task_id);
                            let new_task = Task {
                                local_reference,
                                fp: cont,
                                ready_flag: ReadyFlag::NonReady,
                                task_id,
                                stack,
                                pending_effects,
                            };
                            self.tasks.push_back(new_task);
                        } else {
                            trace!("complte task: {}", ec.task_id);
                            self.completed_tasks.push(CompletedTask {
                                stack,
                                result: VMResult::Success(()),
                            })
                        }
                    }
                    other => {
                        trace!("trap task: {}", ec.task_id);
                        self.completed_tasks.push(CompletedTask {
                            stack,
                            result: other,
                        })
                    }
                }
            }
            self.processing_effect(gc);
        }
    }

    pub fn run_sync_with_gc(&mut self, gc: &mut MemoryPool) -> Result<(), SyncRunError> {
        while !self.tasks.is_empty() {
            while self.ready_count != 0 {
                trace!("task ready count: {:?}", self.ready_count);

                let task = self.tasks.pop_front().unwrap();
                if task.ready_flag == ReadyFlag::NonReady {
                    self.tasks.push_back(task);
                    continue;
                }
                self.ready_count -= 1;
                let Task {
                    local_reference,
                    fp,
                    mut stack,
                    task_id,
                    mut pending_effects,
                    ..
                } = task;

                let mut ec = ExecuteContext {
                    gc,
                    local_reference,
                    stack: &mut stack,
                    store: self.store,
                    effect: EffectSupplier {
                        pending_effects: &mut pending_effects,
                        effects: &mut self.effects,
                    },
                    cont: fp,
                    task_id,
                };
                let res = unsafe { ((*fp).op)(fp.offset(1), &mut ec) };
                let cont = ec.cont;
                let local_reference = ec.local_reference;
                match res {
                    VMResult::Success(()) => {
                        if !cont.is_null() {
                            trace!("continue task: {}", ec.task_id);
                            let new_task = Task {
                                local_reference,
                                fp: cont,
                                ready_flag: ReadyFlag::NonReady,
                                task_id,
                                stack,
                                pending_effects,
                            };
                            self.tasks.push_back(new_task);
                        } else {
                            trace!("complte task: {}", ec.task_id);
                            self.completed_tasks.push(CompletedTask {
                                stack,
                                result: VMResult::Success(()),
                            })
                        }
                    }
                    other => {
                        trace!("trap task: {}", ec.task_id);
                        self.completed_tasks.push(CompletedTask {
                            stack,
                            result: other,
                        })
                    }
                }
            }
            self.processing_effect(gc);
            if self.tasks.is_empty() {
                break;
            }
            if self.ready_count != 0 {
                continue;
            }
            #[cfg(feature = "async-runtime")]
            if !self.async_tasks.is_empty() {
                return Err(SyncRunError::AsyncPending);
            }
            if self.effects.is_empty() {
                return Err(SyncRunError::Stalled);
            }
        }
        Ok(())
    }

    fn processing_effect(&mut self, gc: &mut MemoryPool) {
        while let Some(effect) = self.effects.pop_front() {
            match effect {
                Effect::MemoryEffect(effect) => {
                    unsafe { self.handle_memory_effect(gc, effect) };
                }
                #[cfg(feature = "async-runtime")]
                Effect::AsyncEffect(effect) => {
                    unsafe { self.handle_async_effect_call(effect) };
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::{
        common::{ExecuteContext, Instr, LocalReference},
        runtime::memory_effect::{AsyncEffect, AsyncEffectOperation, Effect},
        Stack, Store, VMResult,
    };

    use super::{AsyncResult, ReadyFlag, Scheduler, Task};
    fn async_end(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
        ctx.cont = std::ptr::null();
        trace!("ok");
        VMResult::Success(())
    }
    async fn example_func(task_id: u32, fp: *const Instr) -> AsyncResult {
        AsyncResult { task_id, fp }
    }
    #[tokio::test]
    async fn test_async() {
        let mut store = Store::new();
        let mut scheduler = Scheduler::new(&mut store);
        {
            scheduler.push(Task {
                task_id: 0,
                stack: Stack::new(256),
                local_reference: LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                pending_effects: 1,
                ready_flag: ReadyFlag::NonReady,
                fp: &Instr { op: async_end },
            });
            scheduler
                .effects
                .push_back(Effect::AsyncEffect(AsyncEffect {
                    operation: AsyncEffectOperation::Call(|a, b| Box::pin(example_func(a, b))),
                    task_id: 0,
                }));
            scheduler.notify.wake();
        }
        scheduler.run().await;
    }
}
