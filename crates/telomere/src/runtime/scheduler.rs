use std::collections::VecDeque;

use crate::{
    common::{gc::MemoryPool, ExecuteContext, GcRef, Instr, LocalReference, MemArg},
    Stack, Store, VMResult,
};

use super::{
    memory_effect::{AtomicFlag, Effect, Operation, ReadOperationHandler, Target, WriteOperation},
    vm::traps::TRAPS_MEMORY_INDEX_OUT_OF_RANGE,
};

fn compute_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(
        memarg.offset.checked_add(offset).map(|v| v as usize),
        || VMResult::MemoryIndexOutOfRange,
    )
}

#[derive(PartialEq, Eq)]
pub(crate) enum ReadyFlag {
    Ready,
    NonReady,
}
pub(crate) struct Task {
    pub task_id: u32,
    pub stack: Stack,
    pub local_reference: LocalReference,
    pub pending_effects: u32,
    pub ready_flag: ReadyFlag,
    pub fp: *const Instr,
}
pub(crate) struct CompletedTask {
    pub task_id: u32,
    pub stack: Stack,
    pub result: VMResult<()>,
}
pub(crate) struct Scheduler<'a> {
    tasks: VecDeque<Task>,
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
        let start = vm_try!(compute_offset(memarg, offset));
        let end = vm_try!(VMResult::from_option(
            start.checked_add(size as usize),
            || VMResult::MemoryIndexOutOfRange
        ));
        self.effects.push_back(Effect {
            task_id,
            target: Target::Memory(addr, start..end),
            atomic: AtomicFlag::NonAtomic,
            operation: Operation::Read(handler),
        });
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
        let size = write_operation_size(&operation);
        let start = vm_try!(compute_offset(memarg, offset));
        let end = vm_try!(VMResult::from_option(
            start.checked_add(size),
            || VMResult::MemoryIndexOutOfRange
        ));
        vm_try!(VMResult::from_option(
            unsafe { gc.get_memory(addr).get_mut(start as usize..end as usize) },
            || VMResult::MemoryIndexOutOfRange
        ));
        self.effects.push_back(Effect {
            task_id,
            target: Target::Memory(addr, start..end),
            atomic: AtomicFlag::NonAtomic,
            operation: Operation::Write(operation),
        });
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
        }
    }
    pub fn push(&mut self, task: Task) {
        if task.ready_flag == ReadyFlag::Ready {
            self.ready_count += 1;
        }
        self.tasks.push_back(task);
    }
    unsafe fn handle_effect(&mut self, gc: &mut MemoryPool, effect: Effect) {
        let task = self
            .tasks
            .iter_mut()
            .find(|v| v.task_id == effect.task_id)
            .unwrap();

        match effect {
            Effect {
                task_id: _,
                target: Target::Memory(addr, range),
                atomic: AtomicFlag::NonAtomic,
                operation: Operation::Read(handler),
            } => {
                let data = unsafe {
                    gc.get_memory(addr)
                        .get(range.start..range.end)
                };
                if let Some(data) = data {
                    task.fp = handler(&mut task.stack, data, task.fp);
                } else {
                    task.fp = TRAPS_MEMORY_INDEX_OUT_OF_RANGE.as_ptr();
                }

                task.pending_effects -= 1;
                if task.pending_effects == 0 {
                    task.ready_flag = ReadyFlag::Ready;
                    self.ready_count += 1;
                }
            }
            Effect {
                task_id: _,
                target: Target::Memory(addr, range),
                atomic: AtomicFlag::NonAtomic,
                operation: Operation::Write(operation),
            } => {
                let dst = unsafe {
                    gc.get_memory(addr)
                        .get_mut(range.start..range.end)
                };
                if let Some(dst) = dst {
                    dst.copy_from_slice(operation.get());
                } else {
                    unreachable!()
                }
                task.pending_effects -= 1;
                if task.pending_effects == 0 {
                    task.ready_flag = ReadyFlag::Ready;
                    self.ready_count += 1;
                }
            }
            _ => todo!(),
        }
    }
    pub unsafe fn run_with_ref(&mut self, gc: &mut MemoryPool) {
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
                        if cont.is_null() {
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
                            self.completed_tasks.push(CompletedTask {
                                task_id,
                                stack,
                                result: VMResult::Success(()),
                            })
                        }
                    }
                    other => self.completed_tasks.push(CompletedTask {
                        task_id,
                        stack,
                        result: other,
                    }),
                }
            }
            self.processing_effect(gc);
        }
    }
    unsafe fn processing_effect(&mut self, gc: &mut MemoryPool) {
        while let Some(effect) = self.effects.pop_front() {
            self.handle_effect(gc, effect);
        }
    }
}
