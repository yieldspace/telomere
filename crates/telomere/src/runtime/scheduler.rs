use std::collections::VecDeque;

use crate::{
    common::{gc::MemoryPool, ExecuteContext, GcRef, Instr, LocalReference, MemArg},
    Stack, Store, VMResult,
};

use super::memory_effect::{AtomicFlag, Effect, Operation, Target};

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
    effects: &'a mut VecDeque<Effect>,
}
impl EffectSupplier<'_> {
    pub(crate) fn push_non_atomic_memory_read_effect(
        &mut self,
        task_id: u32,
        addr: GcRef,
        memarg: MemArg,
        offset: u32,
        size: u32,
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
            operation: Operation::Read,
        });
        VMResult::Success(())
    }
}
fn special_memory_index_out_of_range(
    _tail_code: *const Instr,
    _ctx: &mut ExecuteContext,
) -> VMResult<()> {
    VMResult::MemoryIndexOutOfRange
}
const MEMORY_INDEX_OUT_OF_RANGE: [Instr; 1] = [Instr {
    op: special_memory_index_out_of_range,
}];
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
    fn handle_effect(&mut self, gc: &mut MemoryPool, effect: Effect) {
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
                operation: Operation::Read,
            } => {
                let data = unsafe {
                    gc.get_memory(addr)
                        .get(range.start as usize..range.end as usize)
                };
                if let Some(data) = data {
                    task.stack.push_slice(data).unwrap();
                } else {
                    task.fp = MEMORY_INDEX_OUT_OF_RANGE.as_ptr();
                }
                task.ready_flag = ReadyFlag::Ready;
                self.ready_count += 1;
            }
            _ => todo!(),
        }
    }
    pub fn run_with_ref(&mut self, gc: &mut MemoryPool) {
        while !self.tasks.is_empty() {
            while self.ready_count != 0 {
                println!("{:?}", self.ready_count);

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
                    ..
                } = task;
                let mut ec = ExecuteContext {
                    gc,
                    local_reference,
                    stack: &mut stack,
                    store: self.store,
                    effect: EffectSupplier {
                        effects: &mut self.effects,
                    },
                    cont: std::ptr::null(),
                    task_id,
                };
                let res = unsafe { ((*fp).op)(fp.offset(1) as *const Instr, &mut ec) };
                match res {
                    VMResult::Success(()) => {
                        if ec.cont != std::ptr::null() {
                            let new_task = Task {
                                local_reference: ec.local_reference,
                                fp: ec.cont,
                                ready_flag: ReadyFlag::NonReady,
                                task_id,
                                stack,
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
    fn processing_effect(&mut self, gc: &mut MemoryPool) {
        while let Some(effect) = self.effects.pop_front() {
            self.handle_effect(gc, effect);
        }
    }
}
