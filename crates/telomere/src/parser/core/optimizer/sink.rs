use crate::common::{Instr, Op, Operand};

#[derive(Clone)]
pub(crate) struct RecordEmit {
    pub(crate) source_start: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<Operand>,
}

impl RecordEmit {
    pub(crate) fn len(&self) -> usize {
        1 + self.operands.len()
    }
}

pub(crate) fn flatten_records(records: &[RecordEmit]) -> Vec<Instr> {
    let mut instrs = Vec::with_capacity(records.iter().map(RecordEmit::len).sum());
    for record in records {
        instrs.push(Instr { op: record.op });
        for operand in &record.operands {
            instrs.push(Instr { operand: *operand });
        }
    }
    instrs
}
