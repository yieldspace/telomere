use super::families::{is_integer_scalar, local_get, same_width, scalar_matches_const};
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ProducerSeed {
    Local {
        width: ValueSize,
        local_addr: u32,
    },
    LocalImmScalar {
        width: ValueSize,
        src_local: u32,
        imm: TypedConst,
        op: TypedScalarOp,
    },
    LocalLocalScalar {
        width: ValueSize,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        op: TypedScalarOp,
    },
    LocalAddrLoad {
        width: ValueSize,
        local_addr: u32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    LocalImmAddrLoad {
        width: ValueSize,
        local_addr: u32,
        imm: i32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    ConstAddrLoad {
        width: ValueSize,
        start: u32,
        op: TypedLoadOp,
    },
}

impl ProducerSeed {
    pub(super) fn width(self) -> ValueSize {
        match self {
            Self::Local { width, .. }
            | Self::LocalImmScalar { width, .. }
            | Self::LocalLocalScalar { width, .. }
            | Self::LocalAddrLoad { width, .. }
            | Self::LocalImmAddrLoad { width, .. }
            | Self::ConstAddrLoad { width, .. } => width,
        }
    }
}

pub(super) struct ProducerSeedMatch {
    pub(super) seed: ProducerSeed,
    pub(super) consumed: usize,
}

pub(super) fn is_seed_load(op: TypedLoadOp) -> bool {
    matches!(
        op,
        TypedLoadOp::Bits4(
            Load4Kind::I32
                | Load4Kind::I32Load8S
                | Load4Kind::I32Load8U
                | Load4Kind::I32Load16S
                | Load4Kind::I32Load16U
                | Load4Kind::F32
        ) | TypedLoadOp::Bits8(
            Load8Kind::I64
                | Load8Kind::I64Load8S
                | Load8Kind::I64Load8U
                | Load8Kind::I64Load16S
                | Load8Kind::I64Load16U
                | Load8Kind::I64Load32S
                | Load8Kind::I64Load32U
                | Load8Kind::F64
        )
    )
}

pub(super) fn is_float_compare(op: TypedCompareOp) -> bool {
    matches!(op, TypedCompareOp::F32(_) | TypedCompareOp::F64(_))
}

pub(super) fn is_float_load_seed_for_compare(
    seed: ProducerSeed,
    compare_op: TypedCompareOp,
) -> bool {
    matches!(
        (seed, compare_op),
        (
            ProducerSeed::LocalAddrLoad {
                op: TypedLoadOp::Bits4(Load4Kind::F32),
                ..
            } | ProducerSeed::LocalImmAddrLoad {
                op: TypedLoadOp::Bits4(Load4Kind::F32),
                ..
            } | ProducerSeed::ConstAddrLoad {
                op: TypedLoadOp::Bits4(Load4Kind::F32),
                ..
            },
            TypedCompareOp::F32(_),
        ) | (
            ProducerSeed::LocalAddrLoad {
                op: TypedLoadOp::Bits8(Load8Kind::F64),
                ..
            } | ProducerSeed::LocalImmAddrLoad {
                op: TypedLoadOp::Bits8(Load8Kind::F64),
                ..
            } | ProducerSeed::ConstAddrLoad {
                op: TypedLoadOp::Bits8(Load8Kind::F64),
                ..
            },
            TypedCompareOp::F64(_),
        )
    )
}

pub(super) fn match_producer_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    match_local_imm_addr_load_seed(decoded, index)
        .or_else(|| match_const_addr_load_seed(decoded, index))
        .or_else(|| match_local_addr_load_seed(decoded, index))
        .or_else(|| match_local_imm_scalar_seed(decoded, index))
        .or_else(|| match_local_local_scalar_seed(decoded, index))
        .or_else(|| match_local_seed(decoded, index))
}

fn match_local_seed(decoded: &[DecodedInstruction], index: usize) -> Option<ProducerSeedMatch> {
    let first = decoded.get(index)?;
    let (width, local_addr) = local_get(first.kind)?;
    if !matches!(width, ValueSize::Byte4 | ValueSize::Byte8) {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::Local { width, local_addr },
        consumed: 1,
    })
}

fn match_local_imm_scalar_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    let (width, src_local) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    if !matches!(width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(width, imm.width())
        || !same_width(width, op.width())
        || !scalar_matches_const(op, imm)
        || !is_integer_scalar(op)
    {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalImmScalar {
            width,
            src_local,
            imm,
            op,
        },
        consumed: 3,
    })
}

fn match_local_local_scalar_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    if !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !is_integer_scalar(op)
    {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalLocalScalar {
            width: lhs_width,
            lhs_local_addr,
            rhs_local_addr,
            op,
        },
        consumed: 3,
    })
}

fn match_local_addr_load_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    let (addr_width, local_addr) = local_get(first.kind)?;
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !is_seed_load(op) {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalAddrLoad {
            width: op.width(),
            local_addr,
            memarg,
            op,
        },
        consumed: 2,
    })
}

fn match_local_imm_addr_load_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    let (addr_width, local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !matches!(
        third.kind,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Add))
    ) {
        return None;
    }
    let (op, memarg) = match fourth.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !is_seed_load(op) {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalImmAddrLoad {
            width: op.width(),
            local_addr,
            imm,
            memarg,
            op,
        },
        consumed: 4,
    })
}

fn match_const_addr_load_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    let addr = match first.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !is_seed_load(op) {
        return None;
    }
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };
    Some(ProducerSeedMatch {
        seed: ProducerSeed::ConstAddrLoad {
            width: op.width(),
            start,
            op,
        },
        consumed: 2,
    })
}

pub(super) fn has_nontrivial_seed(seed_match: &ProducerSeedMatch) -> bool {
    seed_match.consumed > 1
}
