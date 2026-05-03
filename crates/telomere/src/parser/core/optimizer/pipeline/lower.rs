use super::select::KernelFunction;
use crate::common::{LoweredOperand, Op};

#[derive(Debug, Clone)]
pub(crate) struct LoweredKernelFunction {
    pub(crate) blocks: Vec<LoweredKernelBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredKernelBlock {
    pub(crate) block_id: usize,
    pub(crate) label: usize,
    pub(crate) ops: Vec<LoweredKernelOp>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredKernelOp {
    pub(crate) label: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<LoweredOperand>,
    pub(crate) family: &'static str,
}

pub(crate) fn lower(kernel: KernelFunction) -> LoweredKernelFunction {
    LoweredKernelFunction {
        blocks: kernel
            .blocks
            .into_iter()
            .map(|block| LoweredKernelBlock {
                block_id: block.block_id,
                label: block.label,
                ops: block
                    .ops
                    .into_iter()
                    .map(|op| LoweredKernelOp {
                        label: op.label,
                        op: op.op,
                        operands: op.operands,
                        family: op.family,
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub(crate) fn verify(lowered: &LoweredKernelFunction) -> bool {
    !lowered.blocks.is_empty()
        && lowered.blocks.iter().enumerate().all(|(expected, block)| {
            block.block_id == expected
                && block.label == expected
                && !block.ops.is_empty()
                && block.ops.iter().all(|op| !op.family.is_empty())
        })
}
