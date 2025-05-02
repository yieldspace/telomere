use std::collections::VecDeque;

use crate::{
    common::{gc::MemoryPool, ExecuteContext, Instr, LocalReference},
    Stack, Store, VMResult,
};

use super::memory_effect::Effect;
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
    fn handle_effect(&mut self, effect: Effect) {
        let task = self
            .tasks
            .iter_mut()
            .find(|v| v.task_id == effect.task_id)
            .unwrap();
        /*
        TODO:
        let res = action(&mut task.stack);
        if res.is_err() {
            todo!() // FIXME: handle action result
        }
         */
        //task.ready_flag = ReadyFlag::Ready;
        //self.ready_count += 1;
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
                };
                let res = unsafe { ((*fp).op)(fp.offset(1) as *const Instr, &mut ec) };
                match res {
                    /*VMResult::Continue(fp) => {
                        let new_task = Task {
                            local_reference: ec.local_reference,
                            fp,
                            ready_flag: ReadyFlag::NonReady,
                            task_id,
                            stack,
                        };

                        self.tasks.push_back(new_task);
                    }*/
                    other => self.completed_tasks.push(CompletedTask {
                        task_id,
                        stack,
                        result: other,
                    }),
                }
            }
            self.processing_effect();
        }
    }
    pub fn run(&mut self) {
        let gc = self.store.gc.clone();
        let mut gc = gc.borrow_mut();
        self.run_with_ref(&mut gc);
    }
    fn processing_effect(&mut self) {
        while let Some(effect) = self.effects.pop_front() {
            self.handle_effect(effect);
        }
    }
}
