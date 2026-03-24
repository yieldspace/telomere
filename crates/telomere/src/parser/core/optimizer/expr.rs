use std::hash::{Hash, Hasher};

use crate::common::ValType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ExprId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ExprOriginKind {
    EntryStack,
    EntryLocal,
    InstrResult,
    SyntheticConst,
    BlockParam,
    MemoryValue,
    GlobalValue,
    TableValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ExprOrigin {
    pub(crate) block_id: usize,
    pub(crate) ordinal: usize,
    pub(crate) kind: ExprOriginKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ConstValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl PartialEq for ConstValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::I32(lhs), Self::I32(rhs)) => lhs == rhs,
            (Self::I64(lhs), Self::I64(rhs)) => lhs == rhs,
            (Self::F32(lhs), Self::F32(rhs)) => lhs.to_bits() == rhs.to_bits(),
            (Self::F64(lhs), Self::F64(rhs)) => lhs.to_bits() == rhs.to_bits(),
            _ => false,
        }
    }
}

impl Eq for ConstValue {}

impl Hash for ConstValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::I32(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::I64(value) => {
                1u8.hash(state);
                value.hash(state);
            }
            Self::F32(value) => {
                2u8.hash(state);
                value.to_bits().hash(state);
            }
            Self::F64(value) => {
                3u8.hash(state);
                value.to_bits().hash(state);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct LocalSlot {
    pub(crate) addr: u32,
    pub(crate) size: u32,
}

impl LocalSlot {
    pub(crate) fn new(addr: u32, size: u32) -> Self {
        Self { addr, size }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum AliasSpace {
    Memory,
    Global,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum AliasAddress {
    Const(u32),
    Origin(ExprOrigin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AliasKey {
    pub(crate) space: AliasSpace,
    pub(crate) index: u32,
    pub(crate) width: u8,
    pub(crate) address: AliasAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EffectBarrier {
    Control,
    Memory,
    Global,
    Table,
    Call,
    TrapSensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct HeapVersion {
    pub(crate) memory: u32,
    pub(crate) global: u32,
    pub(crate) table: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PureOpKind {
    I32Eqz,
    I64Eqz,
    I32Add,
    I32Sub,
    I32Mul,
    I32And,
    I32Or,
    I32Xor,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I64Add,
    I64Sub,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ValueKey {
    Unary {
        op: PureOpKind,
        input: ExprOrigin,
    },
    Binary {
        op: PureOpKind,
        lhs: ExprOrigin,
        rhs: ExprOrigin,
    },
    MemoryLoad(AliasKey),
    GlobalGet {
        slot: LocalSlot,
    },
    TableGet {
        tableidx: u32,
        index: ExprOrigin,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ExprState {
    pub(crate) ty: ValType,
    pub(crate) origin: ExprOrigin,
    pub(crate) const_value: Option<ConstValue>,
    pub(crate) key: Option<ValueKey>,
    pub(crate) producer_record: Option<usize>,
    pub(crate) ref_count: usize,
    pub(crate) removable: bool,
}

pub(crate) type EffectEpoch = usize;
