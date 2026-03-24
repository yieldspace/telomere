use crate::common::{Instr, Op, Operand};

#[derive(Clone)]
pub(crate) struct RecordEmit {
    pub(crate) source_start: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<Operand>,
    pub(crate) alive: bool,
}

impl RecordEmit {
    pub(crate) fn len(&self) -> usize {
        1 + self.operands.len()
    }
}

#[derive(Default, Clone)]
pub(crate) struct RewriteSink {
    records: Vec<RecordEmit>,
}

impl RewriteSink {
    pub(crate) fn push(
        &mut self,
        source_start: Option<usize>,
        op: Op,
        operands: Vec<Operand>,
    ) -> usize {
        let idx = self.records.len();
        self.records.push(RecordEmit {
            source_start,
            op,
            operands,
            alive: true,
        });
        idx
    }

    pub(crate) fn remove(&mut self, idx: usize) {
        if let Some(record) = self.records.get_mut(idx) {
            record.alive = false;
        }
    }

    pub(crate) fn last_alive_index(&self) -> Option<usize> {
        self.records.iter().rposition(|record| record.alive)
    }

    pub(crate) fn record_mut(&mut self, idx: usize) -> Option<&mut RecordEmit> {
        self.records.get_mut(idx)
    }

    pub(crate) fn into_live_records(self) -> Vec<RecordEmit> {
        self.records
            .into_iter()
            .filter(|record| record.alive)
            .collect()
    }

    pub(crate) fn flatten(records: &[RecordEmit]) -> Vec<Instr> {
        let mut instrs = Vec::with_capacity(records.iter().map(RecordEmit::len).sum());
        for record in records {
            instrs.push(Instr { op: record.op });
            for operand in &record.operands {
                instrs.push(Instr { operand: *operand });
            }
        }
        instrs
    }
}
