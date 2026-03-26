use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut},
};

use crate::common::ValType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ExprId(pub(crate) usize);

pub(crate) type ValueRef = ExprId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BlockArgumentId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EffectOpId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ExprOriginKind {
    EntryStack,
    EntryLocal,
    InstrResult,
    SyntheticConst,
    BlockArgument,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockArgument {
    pub(crate) id: BlockArgumentId,
    pub(crate) block_id: usize,
    pub(crate) ordinal: usize,
    pub(crate) ty: ValType,
    pub(crate) value: ValueRef,
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SlotClass {
    EntryLocal,
    TempLocal,
    SpillLocal,
    VirtualStack,
    ConstPoolRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SlotRef {
    pub(crate) slot: LocalSlot,
    pub(crate) class: SlotClass,
}

impl SlotRef {
    pub(crate) const fn new(slot: LocalSlot, class: SlotClass) -> Self {
        Self { slot, class }
    }

    pub(crate) const fn entry_local(slot: LocalSlot) -> Self {
        Self::new(slot, SlotClass::EntryLocal)
    }

    #[allow(dead_code)]
    pub(crate) const fn temp_local(slot: LocalSlot) -> Self {
        Self::new(slot, SlotClass::TempLocal)
    }

    pub(crate) const fn spill_local(slot: LocalSlot) -> Self {
        Self::new(slot, SlotClass::SpillLocal)
    }

    #[allow(dead_code)]
    pub(crate) const fn virtual_stack(slot: LocalSlot) -> Self {
        Self::new(slot, SlotClass::VirtualStack)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AddressBaseKind {
    EntryLocal(LocalSlot),
    SpillLocal(LocalSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AddressShape {
    pub(crate) base: AddressBaseKind,
    pub(crate) offset_delta: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum LoopValueShape {
    Local4(LocalSlot),
    Local4ConstAdd {
        base: LocalSlot,
        imm: i32,
    },
    Local4Local4Add {
        lhs: LocalSlot,
        rhs: LocalSlot,
    },
    CompareEqz {
        input: Box<LoopValueShape>,
    },
    CompareConstI32 {
        lhs: Box<LoopValueShape>,
        op: PureOpKind,
        imm: i32,
    },
    CompareLocal4 {
        lhs: LocalSlot,
        op: PureOpKind,
        rhs: LocalSlot,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct SlotShape {
    pub(crate) slot: Option<SlotRef>,
    pub(crate) address: Option<AddressShape>,
    pub(crate) loop_value: Option<LoopValueShape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum ProviderClass {
    #[default]
    None,
    LocalLoad,
    Const,
    PureUnary,
    PureBinary,
    EffectResultSpill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum MaterializationCost {
    #[default]
    Unknown,
    Immediate,
    Local,
    Pure,
    Spill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum AliasSpace {
    Memory,
    Global,
    Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum AliasAddress {
    Const(u32),
    Origin(ExprOrigin),
    Unary {
        op: PureOpKind,
        input: Box<AliasAddress>,
    },
    Binary {
        op: PureOpKind,
        lhs: Box<AliasAddress>,
        rhs: Box<AliasAddress>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AliasKey {
    pub(crate) space: AliasSpace,
    pub(crate) index: u32,
    pub(crate) offset: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum PureOpKind {
    I32Eqz,
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I64Eqz,
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
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
    I64Mul,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Abs,
    F32Neg,
    F32Sqrt,
    F32Ceil,
    F32Floor,
    F32Trunc,
    F32Nearest,
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
    F64Abs,
    F64Neg,
    F64Sqrt,
    F64Ceil,
    F64Floor,
    F64Trunc,
    F64Nearest,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ValueDef {
    Const,
    Instr,
    EffectResult(EffectOpId, usize),
    BlockArgument(BlockArgumentId),
    Synthetic,
}

#[derive(Debug, Clone)]
pub(crate) struct ValueNode {
    pub(crate) ty: ValType,
    pub(crate) origin: ExprOrigin,
    pub(crate) def: ValueDef,
    pub(crate) const_value: Option<ConstValue>,
    pub(crate) key: Option<ValueKey>,
    pub(crate) address_shape: Option<AddressShape>,
    pub(crate) loop_value_shape: Option<LoopValueShape>,
    pub(crate) slot_shape: Option<SlotShape>,
    pub(crate) provider_class: ProviderClass,
    pub(crate) materialization_cost: MaterializationCost,
    pub(crate) producer_op: Option<usize>,
    pub(crate) materialized_block: Option<usize>,
    pub(crate) materialized_op: Option<usize>,
    pub(crate) needs_spill: bool,
    pub(crate) use_count: usize,
    pub(crate) ref_count: usize,
    pub(crate) removable: bool,
}

pub(crate) type ExprState = ValueNode;

#[derive(Debug, Default, Clone)]
pub(crate) struct ValueGraph {
    pub(crate) nodes: Vec<ValueNode>,
    pub(crate) block_arguments: Vec<BlockArgument>,
    block_argument_lookup: HashMap<(usize, usize), BlockArgumentId>,
}

impl Deref for ValueGraph {
    type Target = [ValueNode];

    fn deref(&self) -> &Self::Target {
        &self.nodes
    }
}

impl DerefMut for ValueGraph {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.nodes
    }
}

impl ValueNode {
    pub(crate) fn effect_result(&self) -> Option<(EffectOpId, usize)> {
        match self.def {
            ValueDef::EffectResult(id, result_index) => Some((id, result_index)),
            _ => None,
        }
    }

    pub(crate) fn is_effect_result(&self) -> bool {
        matches!(self.def, ValueDef::EffectResult(..))
    }

    pub(crate) fn block_argument(&self) -> Option<BlockArgumentId> {
        match self.def {
            ValueDef::BlockArgument(id) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn is_block_argument(&self) -> bool {
        matches!(self.def, ValueDef::BlockArgument(_))
    }

    pub(crate) fn refresh_optimizer_metadata(&mut self) {
        self.provider_class = if self.const_value.is_some() {
            ProviderClass::Const
        } else if matches!(self.key, Some(ValueKey::Unary { .. })) {
            ProviderClass::PureUnary
        } else if matches!(self.key, Some(ValueKey::Binary { .. })) {
            ProviderClass::PureBinary
        } else if self.is_effect_result()
            && self
                .slot_shape
                .as_ref()
                .and_then(|shape| shape.slot)
                .is_some_and(|slot| slot.class == SlotClass::SpillLocal)
        {
            ProviderClass::EffectResultSpill
        } else if self
            .slot_shape
            .as_ref()
            .is_some_and(|shape| shape.slot.is_some())
        {
            ProviderClass::LocalLoad
        } else {
            ProviderClass::None
        };
        self.materialization_cost = match self.provider_class {
            ProviderClass::Const => MaterializationCost::Immediate,
            ProviderClass::LocalLoad => MaterializationCost::Local,
            ProviderClass::PureUnary | ProviderClass::PureBinary => MaterializationCost::Pure,
            ProviderClass::EffectResultSpill => MaterializationCost::Spill,
            ProviderClass::None => MaterializationCost::Unknown,
        };
    }
}

impl ValueGraph {
    pub(crate) fn existing_block_argument_value(
        &self,
        block_id: usize,
        ordinal: usize,
    ) -> Option<ValueRef> {
        self.block_argument_lookup
            .get(&(block_id, ordinal))
            .copied()
            .map(|id| self.block_arguments[id.0].value)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ensure_block_argument(
        &mut self,
        block_id: usize,
        ordinal: usize,
        ty: ValType,
        const_value: Option<ConstValue>,
        key: Option<ValueKey>,
        address_shape: Option<AddressShape>,
        loop_value_shape: Option<LoopValueShape>,
        slot_shape: Option<SlotShape>,
    ) -> ValueRef {
        if let Some(id) = self
            .block_argument_lookup
            .get(&(block_id, ordinal))
            .copied()
        {
            let value = self.block_arguments[id.0].value;
            let node = &mut self.nodes[value.0];
            node.const_value = const_value;
            node.key = key;
            node.address_shape = address_shape;
            node.loop_value_shape = loop_value_shape;
            node.slot_shape = slot_shape;
            node.refresh_optimizer_metadata();
            return value;
        }

        let id = BlockArgumentId(self.block_arguments.len());
        let value = ExprId(self.nodes.len());
        self.nodes.push(ValueNode {
            ty,
            origin: ExprOrigin {
                block_id,
                ordinal,
                kind: ExprOriginKind::BlockArgument,
            },
            def: ValueDef::BlockArgument(id),
            const_value,
            key,
            address_shape,
            loop_value_shape,
            slot_shape,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        self.nodes[value.0].refresh_optimizer_metadata();
        self.block_arguments.push(BlockArgument {
            id,
            block_id,
            ordinal,
            ty,
            value,
        });
        self.block_argument_lookup.insert((block_id, ordinal), id);
        value
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn block_argument(&self, id: BlockArgumentId) -> Option<&BlockArgument> {
        self.block_arguments.get(id.0)
    }
}

pub(crate) type EffectEpoch = usize;
