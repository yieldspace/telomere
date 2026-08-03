#![allow(private_interfaces)]

#[macro_use]
mod vm_result;
use std::{fmt::Display, future::Future, pin::Pin, sync::Arc};

use custom_section::NameSubSection;
use smallvec::SmallVec;

pub use vm_result::VMResult;
pub(crate) mod memory;
pub use memory::{
    AtomicRmwOp, LocalMemoryObject, MemArg, Memory, MemoryInitError, MemoryMappingOperation,
    SharedMemoryObject,
};
#[cfg(feature = "threads")]
pub use memory::{AtomicWaitResult, SharedWaitRegistration};
pub(crate) mod stack;
use stack::CachedMemoryKind;
pub(crate) use stack::CallFrameCache;
pub use stack::{LocalReference, Stack};
mod registry;
pub use registry::Registry;
mod object_ref;
pub(crate) mod store;
pub use object_ref::ObjectRef;
pub(crate) use store::{FunctionInstanceData, InstanceData, ModuleInstance, StoreInner};
pub use store::{
    InstanceHandle, JitConfig, MemoryConfig, MemoryHandle, RuntimeConfig, Store, StoreState,
};
use store::{InstanceMemorySlot, LocalMemoryId, SharedMemoryId};

use crate::runtime::scheduler::EffectSupplier;
use crate::WasmParserError;
pub mod custom_section;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct FuncIdx(pub u32);
impl Display for FuncIdx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct TableIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct MemIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct GlobalIdx(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum ValueSize {
    Byte4,
    Byte8,
    Byte16,
}
impl ValueSize {
    pub fn u32(&self) -> u32 {
        match self {
            Self::Byte4 => 4,
            Self::Byte8 => 8,
            Self::Byte16 => 16,
        }
    }
    pub fn usize(&self) -> usize {
        match self {
            Self::Byte4 => 4,
            Self::Byte8 => 8,
            Self::Byte16 => 16,
        }
    }
}

impl ValType {
    pub fn stack_size(&self) -> ValueSize {
        match self {
            ValType::ExternRef => ValueSize::Byte4,
            ValType::F32 => ValueSize::Byte4,
            ValType::F64 => ValueSize::Byte8,
            ValType::FuncRef => ValueSize::Byte4,
            ValType::I32 => ValueSize::Byte4,
            ValType::I64 => ValueSize::Byte8,
            ValType::V128 => ValueSize::Byte16,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultType(pub Vec<ValType>);
impl ResultType {
    pub fn stack_pop_iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter().rev()
    }
    pub fn iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter()
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResultValue(Vec<WasmValue>);
impl ResultValue {
    pub fn new(args: Vec<WasmValue>) -> Self {
        Self(args)
    }
    pub fn iter(&self) -> impl Iterator<Item = &WasmValue> + use<'_> {
        self.0.iter()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableType {
    pub reftype: RefType,
    pub limits: Limits,
}
#[derive(Debug)]
pub struct Table(pub TableType);
impl Table {
    pub fn new(tt: TableType) -> Self {
        Self(tt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncType(pub ResultType, pub ResultType);
impl FuncType {
    pub fn new(param: Vec<ValType>, result: Vec<ValType>) -> Self {
        Self(ResultType(param), ResultType(result))
    }
}
#[derive(Debug, Clone)]
pub struct TypeSection(pub Vec<FuncType>);
impl TypeSection {
    pub fn get(&self, idx: TypeIdx) -> Option<&FuncType> {
        self.0.get(idx.0 as usize)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportDesc {
    TypeIdx(TypeIdx),
    TableType(TableType),
    MemType(MemType),
    GlobalType(GlobalType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub desc: ImportDesc,
}
#[derive(Debug, Clone)]
pub struct ImportSection(pub Vec<Import>);

#[derive(Debug, Clone)]
pub struct FunctionSection(pub Vec<TypeIdx>);

#[derive(Debug, Clone)]
pub struct ExportSection(pub Vec<Export>);
impl ExportSection {
    pub fn find(&self, name: &str) -> Option<ExportDesc> {
        self.0.iter().find(|it| it.0 == name).map(|it| it.1)
    }
}
pub type HostFunction = fn(ctx: &mut ExecuteContext) -> VMResult<*const Instr>;
pub type AsyncHostFuture = Pin<Box<dyn Future<Output = VMResult<*const Instr>> + 'static>>;
pub type AsyncHostFunction = fn(&mut ExecuteContext<'_>) -> AsyncHostFuture;

pub struct ReturnSlot(*mut u8);
unsafe impl Send for ReturnSlot {}
impl ReturnSlot {
    pub fn write(&self, data: &[u8]) {
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), self.0, data.len()) };
    }
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.0
    }
}
#[derive(Clone)]
pub enum FunctionBody {
    Wasm(Func),
    Host(HostFunction),
}
#[derive(Clone)]
pub struct CodeSection(pub Vec<FunctionBody>);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min: u32,
    pub max: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemType {
    pub limits: Limits,
    pub shared: bool,
}
impl MemType {
    pub const fn new(limits: Limits, shared: bool) -> Self {
        Self { limits, shared }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefType {
    FuncRef,
    ExternRef,
}
impl From<RefType> for ValType {
    fn from(val: RefType) -> Self {
        match val {
            RefType::ExternRef => ValType::ExternRef,
            RefType::FuncRef => ValType::FuncRef,
        }
    }
}
#[derive(Debug, Clone)]
pub enum ElemMode {
    Passive,
    Active(TableIdx, Vec<ConstExpr>),
    Declarative,
}
#[derive(Debug, Clone)]
pub enum ElemInit {
    FuncIdx(Vec<u32>),
    ConstExpr(Vec<Vec<ConstExpr>>),
}
#[derive(Debug, Clone)]
pub struct Elem {
    pub kind: RefType,
    pub init: ElemInit,
    pub mode: ElemMode,
}
#[derive(Debug, Clone)]
pub struct ElementSection(pub Vec<Elem>);
#[derive(Debug, Clone)]
pub enum DataMode {
    Passive,
    Active(MemIdx, Vec<ConstExpr>),
}
#[derive(Debug, Clone)]
pub struct Data {
    pub init: Vec<u8>,
    pub mode: DataMode,
}
pub enum DataCountVerifier {
    OnePass(u32),
    Lazy { max_data_idx: Option<u32> },
}

#[derive(Debug, Clone)]
pub struct DataSection(pub Vec<Data>);
#[derive(Clone)]
pub struct Module {
    pub fts: TypeSection,
    pub functions: Vec<TypeIdx>,
    pub imports: ImportSection,
    pub mems: Vec<MemType>,
    pub globals: Vec<GlobalType>,
    pub global_init: Vec<ConstExpr>,
    pub exs: ExportSection,
    pub tables: Vec<TableType>,
    pub elems: ElementSection,
    pub codes: CodeSection,
    pub data: DataSection,
    pub start: Option<FuncIdx>,
    pub name: Option<NameSubSection>,
}
pub struct HostFunctionDefinition {
    pub name: Option<String>,
    pub signature: FuncType,
    pub fp: HostFunction,
}
pub struct AsyncHostFunctionDefinition {
    pub name: Option<String>,
    pub signature: FuncType,
    pub fp: AsyncHostFunction,
}
pub struct NativeModule {
    pub functions: Vec<HostFunctionDefinition>,
}
pub struct AsyncNativeModule {
    pub functions: Vec<AsyncHostFunctionDefinition>,
}
pub const TABLE_UNINITIALIZED: u32 = 0x00;
#[derive(Debug, Clone)]
pub struct TableInstance(pub TableType, pub Vec<u32>);
impl TableInstance {
    pub fn new(tt: TableType) -> Self {
        Self(tt, vec![TABLE_UNINITIALIZED; tt.limits.min as usize])
    }
}
#[derive(Clone)]
pub struct Instance {
    pub module_addr: ObjectRef,
    pub instance_id: u32,
    //  -> addr
    pub memory: Vec<ObjectRef>,
    // idx -> addr
    pub globals: Vec<ObjectRef>,
    // idx -> addr
    pub funcs: Vec<ObjectRef>,
    // idx -> addr
    pub tables: Vec<ObjectRef>,
}
#[derive(Debug, Clone)]
pub struct Locals {
    pub n: u32,
    pub t: ValType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mut {
    Const = 0,
    Var = 1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalType(pub ValType, pub Mut);
#[derive(Debug, Clone)]
pub struct Global(pub GlobalType, pub Vec<ConstExpr>);
#[derive(Clone)]
pub struct Func {
    pub locals: LocalsData,
    pub expr: Vec<Instr>,
    pub op_lens: Vec<u16>,
    pub(crate) lowered: Arc<LoweredFunction>,
}
impl Func {
    pub fn local_size(&self) -> usize {
        self.locals.byte_size()
    }
}
#[derive(Debug, Clone, Copy)]
pub enum ExportDesc {
    Func(FuncIdx),
    Table(TableIdx),
    Mem(MemIdx),
    Global(GlobalIdx),
}
#[derive(Debug, Clone)]
pub struct Export(pub String, pub ExportDesc);
#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum BlockType {
    Void,
    ValType(ValType),
    TypeIdx(TypeIdx),
}
impl BlockType {
    pub fn return_size(&self, types: &TypeSection) -> Option<u32> {
        let return_size = match self {
            BlockType::TypeIdx(idx) => {
                let ty = types.get(*idx)?;

                ty.1.iter().map(|v| v.stack_size().u32()).sum()
            }
            BlockType::ValType(ty) => ty.stack_size().u32(),
            BlockType::Void => 0,
        };
        Some(return_size)
    }
    pub fn param_size(&self, types: &TypeSection) -> Option<u32> {
        let param_size = match self {
            BlockType::TypeIdx(idx) => {
                let ty = types.get(*idx)?;

                ty.0.iter().map(|v| v.stack_size().u32()).sum()
            }
            BlockType::ValType(_ty) => 0,
            BlockType::Void => 0,
        };
        Some(param_size)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct LoopParam {
    pub stack_top: u32,
    pub param_size: u32,
}
#[derive(Debug, Clone, Copy)]
pub struct BlockReturn {
    pub stack_top: u32,
    pub return_size: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectCallTarget {
    pub funcidx: u32,
    pub funcaddr: u32,
}
impl DirectCallTarget {
    pub const fn from_funcidx(funcidx: u32) -> Self {
        Self {
            funcidx,
            funcaddr: 0,
        }
    }

    pub const fn with_funcaddr(self, funcaddr: ObjectRef) -> Self {
        Self {
            funcidx: self.funcidx,
            funcaddr: funcaddr.0,
        }
    }

    pub const fn resolved_funcaddr(self) -> Option<ObjectRef> {
        if self.funcaddr == 0 {
            None
        } else {
            Some(ObjectRef(self.funcaddr))
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallRecipeRef {
    pub funcidx: u32,
    recipe_slot_plus_one: u32,
}

impl CallRecipeRef {
    pub const fn from_funcidx(funcidx: u32) -> Self {
        Self {
            funcidx,
            recipe_slot_plus_one: 0,
        }
    }

    pub const fn with_recipe_slot(self, recipe_slot: u32) -> Self {
        Self {
            funcidx: self.funcidx,
            recipe_slot_plus_one: match recipe_slot.checked_add(1) {
                Some(value) => value,
                None => panic!("call recipe slot overflow"),
            },
        }
    }

    pub const fn resolved_recipe_slot(self) -> Option<u32> {
        if self.recipe_slot_plus_one == 0 {
            None
        } else {
            Some(self.recipe_slot_plus_one - 1)
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalFastRhsShape {
    Local = 0,
    Const = 1,
}

impl LocalFastRhsShape {
    const fn from_encoded(encoded: u32) -> Option<Self> {
        match encoded & 1 {
            0 => Some(Self::Local),
            1 => Some(Self::Const),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalFastConstKind {
    I32,
    I64,
    F32,
    F64,
}

macro_rules! define_local_fast_kind {
    ($name:ident { $($variant:ident => $const_kind:ident),+ $(,)? }) => {
        #[repr(u32)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum $name {
            $($variant,)+
        }

        impl $name {
            #[allow(dead_code)]
            pub(crate) const fn const_kind(self) -> LocalFastConstKind {
                match self {
                    $(Self::$variant => LocalFastConstKind::$const_kind,)+
                }
            }

            const fn from_index(index: u32) -> Option<Self> {
                match index {
                    $(x if x == Self::$variant as u32 => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

define_local_fast_kind!(LocalBinop32Op {
    I32Add => I32,
    I32Sub => I32,
    I32Mul => I32,
    I32And => I32,
    I32Or => I32,
    I32Xor => I32,
    I32Shl => I32,
    I32ShrS => I32,
    I32ShrU => I32,
    I32Rotl => I32,
    I32Rotr => I32,
    F32Add => F32,
    F32Sub => F32,
    F32Mul => F32,
    F32Div => F32,
});

define_local_fast_kind!(LocalBinop64Op {
    I64Add => I64,
    I64Sub => I64,
    I64Mul => I64,
    I64And => I64,
    I64Or => I64,
    I64Xor => I64,
    I64Shl => I64,
    I64ShrS => I64,
    I64ShrU => I64,
    I64Rotl => I64,
    I64Rotr => I64,
    F64Add => F64,
    F64Sub => F64,
    F64Mul => F64,
    F64Div => F64,
});

define_local_fast_kind!(LocalCmp32Op {
    I32Eq => I32,
    I32Ne => I32,
    I32LtS => I32,
    I32LtU => I32,
    I32GtS => I32,
    I32GtU => I32,
    I32LeS => I32,
    I32LeU => I32,
    I32GeS => I32,
    I32GeU => I32,
    F32Eq => F32,
    F32Ne => F32,
    F32Lt => F32,
    F32Gt => F32,
    F32Le => F32,
    F32Ge => F32,
});

define_local_fast_kind!(LocalCmp64Op {
    I64Eq => I64,
    I64Ne => I64,
    I64LtS => I64,
    I64LtU => I64,
    I64GtS => I64,
    I64GtU => I64,
    I64LeS => I64,
    I64LeU => I64,
    I64GeS => I64,
    I64GeU => I64,
    F64Eq => F64,
    F64Ne => F64,
    F64Lt => F64,
    F64Gt => F64,
    F64Le => F64,
    F64Ge => F64,
});

define_local_fast_kind!(LocalUnary32Op {
    I32Clz => I32,
    I32Ctz => I32,
    I32Popcnt => I32,
    F32Abs => F32,
    F32Neg => F32,
    F32Sqrt => F32,
    F32Ceil => F32,
    F32Floor => F32,
    F32Trunc => F32,
    F32Nearest => F32,
});

define_local_fast_kind!(LocalUnary64Op {
    I64Clz => I64,
    I64Ctz => I64,
    I64Popcnt => I64,
    F64Abs => F64,
    F64Neg => F64,
    F64Sqrt => F64,
    F64Ceil => F64,
    F64Floor => F64,
    F64Trunc => F64,
    F64Nearest => F64,
});

pub(crate) const fn encode_local_binop32_kind(
    op: LocalBinop32Op,
    rhs_shape: LocalFastRhsShape,
) -> u32 {
    (op as u32) << 1 | rhs_shape as u32
}

pub(crate) const fn encode_local_binop64_kind(
    op: LocalBinop64Op,
    rhs_shape: LocalFastRhsShape,
) -> u32 {
    (op as u32) << 1 | rhs_shape as u32
}

pub(crate) const fn encode_local_cmp32_kind(op: LocalCmp32Op, rhs_shape: LocalFastRhsShape) -> u32 {
    (op as u32) << 1 | rhs_shape as u32
}

pub(crate) const fn encode_local_cmp64_kind(op: LocalCmp64Op, rhs_shape: LocalFastRhsShape) -> u32 {
    (op as u32) << 1 | rhs_shape as u32
}

pub(crate) const fn encode_local_unary32_kind(op: LocalUnary32Op) -> u32 {
    op as u32
}

pub(crate) const fn encode_local_unary64_kind(op: LocalUnary64Op) -> u32 {
    op as u32
}

pub(crate) fn decode_local_binop32_kind(kind: u32) -> Option<(LocalBinop32Op, LocalFastRhsShape)> {
    Some((
        LocalBinop32Op::from_index(kind >> 1)?,
        LocalFastRhsShape::from_encoded(kind)?,
    ))
}

pub(crate) fn decode_local_binop64_kind(kind: u32) -> Option<(LocalBinop64Op, LocalFastRhsShape)> {
    Some((
        LocalBinop64Op::from_index(kind >> 1)?,
        LocalFastRhsShape::from_encoded(kind)?,
    ))
}

pub(crate) fn decode_local_cmp32_kind(kind: u32) -> Option<(LocalCmp32Op, LocalFastRhsShape)> {
    Some((
        LocalCmp32Op::from_index(kind >> 1)?,
        LocalFastRhsShape::from_encoded(kind)?,
    ))
}

pub(crate) fn decode_local_cmp64_kind(kind: u32) -> Option<(LocalCmp64Op, LocalFastRhsShape)> {
    Some((
        LocalCmp64Op::from_index(kind >> 1)?,
        LocalFastRhsShape::from_encoded(kind)?,
    ))
}

pub(crate) fn decode_local_unary32_kind(kind: u32) -> Option<LocalUnary32Op> {
    LocalUnary32Op::from_index(kind)
}

pub(crate) fn decode_local_unary64_kind(kind: u32) -> Option<LocalUnary64Op> {
    LocalUnary64Op::from_index(kind)
}

#[derive(Clone, Copy)]
pub union Operand {
    pub i32: i32,
    pub u32: u32,
    pub i64: i64,
    pub u64: u64,
    pub f32: f32,
    pub f64: f64,

    pub jump_addr: u32,
    pub drop_size: u32,
    pub local_addr: u32,
    pub select: u32,
    pub call_recipe_ref: CallRecipeRef,
    pub direct_call_target: DirectCallTarget,
    pub memarg: MemArg,
    pub block_return: BlockReturn,
    pub loop_param: LoopParam,
    pub encoded: [u8; 8],
    pub start_host_function: HostFunction,
}

pub type Op = unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>;
#[derive(Clone, Copy)]
pub union Instr {
    pub op: Op,
    pub operand: Operand,
}
unsafe impl Send for Instr {}
unsafe impl Sync for Instr {}

#[derive(Clone)]
pub(crate) struct MaterializedFunction {
    pub(crate) instrs: Vec<Instr>,
    pub(crate) op_lens: Vec<u16>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct LoweredFunction {
    pub(crate) code: Vec<LoweredOp>,
    pub(crate) const_pool: Vec<[u8; 8]>,
    pub(crate) call_recipes: Vec<CallRecipeRef>,
    pub(crate) jump_table: Vec<LoweredJumpTarget>,
    pub(crate) block_map: Vec<LoweredBlockMap>,
    pub(crate) materialized_preview: Option<MaterializedFunction>,
}

impl LoweredFunction {
    pub(crate) fn from_materialized(mut instrs: Vec<Instr>, mut op_lens: Vec<u16>) -> Self {
        instrs.shrink_to_fit();
        op_lens.shrink_to_fit();
        let mut code = Vec::with_capacity(op_lens.len());
        let mut cursor = 0usize;
        for len in &op_lens {
            let width = usize::from(*len);
            let op = unsafe { instrs[cursor].op };
            let operands = (1..width)
                .map(|offset| {
                    lower_materialized_operand(op, offset, unsafe {
                        instrs[cursor + offset].operand
                    })
                })
                .collect();
            code.push(LoweredOp {
                label: None,
                op,
                operands,
            });
            cursor += width;
        }
        debug_assert_eq!(cursor, instrs.len());
        Self {
            code,
            const_pool: Vec::new(),
            call_recipes: Vec::new(),
            jump_table: Vec::new(),
            block_map: Vec::new(),
            materialized_preview: Some(MaterializedFunction { instrs, op_lens }),
        }
    }

    pub(crate) fn materialize(&self) -> MaterializedFunction {
        self.materialize_inner(None)
    }

    pub(crate) fn materialize_with_recipe_slots(
        &self,
        recipe_slots: &[u32],
    ) -> MaterializedFunction {
        if let Some(preview) = &self.materialized_preview {
            let mut preview = preview.clone();
            resolve_direct_call_operands_in_materialized(
                &mut preview.instrs,
                &preview.op_lens,
                recipe_slots,
            );
            return preview;
        }
        self.materialize_inner(Some(recipe_slots))
    }

    fn materialize_inner(&self, recipe_slots: Option<&[u32]>) -> MaterializedFunction {
        if recipe_slots.is_none() {
            if let Some(preview) = &self.materialized_preview {
                return preview.clone();
            }
        }
        let mut label_to_addr = vec![0usize; self.block_map.len()];
        let mut cursor = 0usize;
        for op in &self.code {
            if let Some(label) = op.label {
                if label >= label_to_addr.len() {
                    label_to_addr.resize(label + 1, 0);
                }
                label_to_addr[label] = cursor;
            }
            cursor += 1 + op.operands.len();
        }
        let mut instrs = Vec::with_capacity(cursor);
        let mut op_lens = Vec::with_capacity(self.code.len());
        for op in &self.code {
            instrs.push(Instr { op: op.op });
            for operand in &op.operands {
                instrs.push(Instr {
                    operand: operand.materialize(&label_to_addr, &self.const_pool, recipe_slots),
                });
            }
            op_lens.push(
                u16::try_from(1 + op.operands.len())
                    .expect("lowered instruction length exceeds u16::MAX"),
            );
        }
        MaterializedFunction { instrs, op_lens }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredOp {
    pub(crate) label: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<LoweredOperand>,
}

#[derive(Debug, Clone)]
pub(crate) enum LoweredOperand {
    Raw([u8; 8]),
    ConstPoolRef(u32),
    JumpTarget(usize),
    CallRecipeRef(CallRecipeRef),
}

impl LoweredOperand {
    fn materialize(
        &self,
        label_to_addr: &[usize],
        const_pool: &[[u8; 8]],
        recipe_slots: Option<&[u32]>,
    ) -> Operand {
        match self {
            Self::Raw(encoded) => Operand { encoded: *encoded },
            Self::ConstPoolRef(index) => Operand {
                encoded: const_pool[*index as usize],
            },
            Self::JumpTarget(label) => Operand {
                jump_addr: u32::try_from(label_to_addr[*label])
                    .expect("lowered jump target exceeds u32::MAX"),
            },
            Self::CallRecipeRef(target) => {
                let resolved = recipe_slots
                    .and_then(|slots| slots.get(target.funcidx as usize).copied())
                    .map(|slot| target.with_recipe_slot(slot))
                    .unwrap_or(*target);
                Operand {
                    call_recipe_ref: resolved,
                }
            }
        }
    }
}

fn lower_materialized_operand(op: Op, offset: usize, operand: Operand) -> LoweredOperand {
    if offset == 1 && is_direct_call_op(op) {
        return LoweredOperand::CallRecipeRef(unsafe { operand.call_recipe_ref });
    }
    LoweredOperand::Raw(unsafe { operand.encoded })
}

fn is_direct_call_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, crate::runtime::vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, crate::runtime::vm::op_call_i32_crc16_update16 as Op)
        || std::ptr::fn_addr_eq(
            op,
            crate::runtime::vm::op_call_i32_numeric_token_state_transition as Op,
        )
        || std::ptr::fn_addr_eq(op, crate::runtime::vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, crate::runtime::vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, crate::runtime::vm::op_return_call_import as Op)
}

fn resolve_direct_call_operands_in_materialized(
    instrs: &mut [Instr],
    op_lens: &[u16],
    recipe_slots: &[u32],
) {
    let mut cursor = 0usize;
    for len in op_lens {
        let op = unsafe { instrs[cursor].op };
        if is_direct_call_op(op) {
            let target = unsafe { instrs[cursor + 1].operand.call_recipe_ref };
            if let Some(recipe_slot) = recipe_slots.get(target.funcidx as usize).copied() {
                instrs[cursor + 1] = Instr {
                    operand: Operand {
                        call_recipe_ref: target.with_recipe_slot(recipe_slot),
                    },
                };
            }
        }
        cursor += usize::from(*len);
    }
    debug_assert_eq!(cursor, instrs.len());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoweredJumpTarget {
    pub(crate) label: usize,
    pub(crate) block_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoweredBlockMap {
    pub(crate) block_id: usize,
    pub(crate) label: usize,
    pub(crate) code_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct StablePc(usize);
impl StablePc {
    const RELATIVE_TAG: usize = 1;

    pub(crate) fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[cfg(feature = "threads")]
    pub(crate) fn raw(self) -> usize {
        self.0
    }

    pub(crate) fn from_stable_ptr(ptr: *const Instr) -> Self {
        let ptr = ptr as usize;
        debug_assert_eq!(ptr & Self::RELATIVE_TAG, 0);
        Self(ptr)
    }

    pub(crate) fn from_relative_index(index: usize) -> Self {
        Self((index << 1) | Self::RELATIVE_TAG)
    }

    pub(crate) fn from_raw_in_frame(
        runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
        ptr: *const Instr,
    ) -> Self {
        Self::relative_index_for_ptr(runtime, stack, local_reference, ptr)
            .map(Self::from_relative_index)
            .unwrap_or_else(|| Self::from_stable_ptr(ptr))
    }

    pub(crate) fn resolve(
        self,
        runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
    ) -> *const Instr {
        match self.relative_index() {
            Some(index) => {
                let (base, len) = Self::current_frame_code_range(runtime, stack, local_reference)
                    .expect("relative continuation must resolve against a wasm frame");
                debug_assert!(index < len);
                unsafe { base.add(index) }
            }
            None => self.0 as *const Instr,
        }
    }

    pub(crate) fn resolve_optional(
        self,
        runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
    ) -> Option<*const Instr> {
        match self.relative_index() {
            Some(index) => {
                let (base, len) = Self::current_frame_code_range(runtime, stack, local_reference)?;
                debug_assert!(index < len);
                Some(unsafe { base.add(index) })
            }
            None => Some(self.0 as *const Instr),
        }
    }

    fn relative_index(self) -> Option<usize> {
        (self.0 & Self::RELATIVE_TAG == Self::RELATIVE_TAG).then_some(self.0 >> 1)
    }

    fn current_frame_code_range(
        runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
    ) -> Option<(*const Instr, usize)> {
        let frame_size = local_reference.local_size as usize;
        if frame_size < std::mem::size_of::<crate::common::stack::CallStackInfo>() {
            return None;
        }
        let code_base = stack.code_base(&local_reference);
        if code_base.is_null() {
            return None;
        }
        let code_addr = stack.code_addr(&local_reference);
        let funcinst = runtime.get_func(code_addr);
        let code = funcinst.code()?;
        Some((code_base, code.len()))
    }

    fn relative_index_for_ptr(
        runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
        ptr: *const Instr,
    ) -> Option<usize> {
        let (base, instr_len) = Self::current_frame_code_range(runtime, stack, local_reference)?;
        let instr_size = std::mem::size_of::<Instr>();
        let base_addr = base as usize;
        let ptr_addr = ptr as usize;
        let byte_len = instr_len.checked_mul(instr_size)?;
        let end_addr = base_addr.checked_add(byte_len)?;
        if !(base_addr..end_addr).contains(&ptr_addr) {
            return None;
        }
        let delta = ptr_addr - base_addr;
        if delta % instr_size != 0 {
            return None;
        }
        Some(delta / instr_size)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(u128),
    FuncRef(u32),
    ExternRef(u32),
}
#[derive(Debug, Clone, Copy)]
pub enum ConstExpr {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(u128),
    FuncRef(u32),
    RefNull(RefType),
    GlobalGet(u32),
}
impl ConstExpr {
    pub fn to_offset(&self) -> u32 {
        match self {
            Self::I32(v) => *v as u32,
            Self::I64(v) => *v as u32,
            v => unreachable!("{:?}", v),
        }
    }
}
pub const PAGE_SIZE: usize = 64 * 1024;
pub const PAGE_SIZE_MAX: usize = (4_u64 * 1024 * 1024 * 1024 / PAGE_SIZE as u64) as usize;

pub struct ExecuteContext<'a> {
    pub stack: &'a mut Stack,
    // Read by JIT-generated code via offset_of!; the Rust dead_code lint cannot see that use.
    #[allow(dead_code)]
    pub(crate) stack_memory_ptr: *mut u8,
    #[allow(dead_code)]
    pub(crate) stack_memory_len: usize,
    #[allow(dead_code)]
    pub(crate) stack_top_ptr: *mut usize,
    pub local_reference: LocalReference,
    pub(crate) local_base_ptr: *mut u8,
    pub(crate) default_local_memory_ptr: *mut Memory,
    pub(crate) current_instance_globals_ptr: *const ObjectRef,
    pub(crate) global_values_ptr: *mut store::GlobalValue,
    pub(crate) current_frame: CallFrameCache,
    pub store: &'a Store,
    pub gc: &'a mut StoreInner,
    pub effect: EffectSupplier<'a>,
    pub cont: *const Instr,
    pub task_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotMemoryKind {
    None,
    Local,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecuteContextSnapshot {
    pub(crate) default_memory: SnapshotMemoryKind,
    pub(crate) caller_memory: SnapshotMemoryKind,
    pub(crate) cont_addr: usize,
    pub(crate) task_id: u32,
}

impl ExecuteContextSnapshot {
    pub(crate) fn has_default_memory(self) -> bool {
        !matches!(self.default_memory, SnapshotMemoryKind::None)
    }
}

fn snapshot_memory_kind(kind: CachedMemoryKind) -> SnapshotMemoryKind {
    match kind {
        CachedMemoryKind::None => SnapshotMemoryKind::None,
        CachedMemoryKind::Local => SnapshotMemoryKind::Local,
        CachedMemoryKind::Shared => SnapshotMemoryKind::Shared,
    }
}

impl ExecuteContext<'_> {
    fn refresh_default_local_memory_ptr(&mut self) {
        self.default_local_memory_ptr = match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_memory_mut(unsafe {
                    LocalMemoryId::from_raw_unchecked(self.current_frame.memory0_raw)
                })
                .memory_mut() as *mut Memory,
            CachedMemoryKind::None | CachedMemoryKind::Shared => std::ptr::null_mut(),
        };
    }

    fn refresh_jit_global_ptrs(&mut self) {
        self.current_instance_globals_ptr = if self.current_frame.code_addr.is_null() {
            std::ptr::null()
        } else {
            self.gc
                .jit_instance_global_addrs_ptr(self.current_frame.instance)
        };
        self.global_values_ptr = self.gc.jit_global_values_ptr();
    }

    fn refresh_frame_cached_ptrs(&mut self) {
        self.refresh_default_local_memory_ptr();
        self.refresh_jit_global_ptrs();
    }

    pub(crate) fn snapshot(&self) -> ExecuteContextSnapshot {
        let default_memory = snapshot_memory_kind(self.current_frame.memory0_kind);
        let caller_memory = self
            .caller_frame_cache()
            .map(|frame| snapshot_memory_kind(frame.memory0_kind))
            .unwrap_or(SnapshotMemoryKind::None);
        ExecuteContextSnapshot {
            default_memory,
            caller_memory,
            cont_addr: self.cont as usize,
            task_id: self.task_id,
        }
    }

    pub fn set_local_reference(&mut self, local_reference: LocalReference) {
        self.local_reference = local_reference;
        self.local_base_ptr = unsafe { self.stack.local_area_mut_ptr(&local_reference) };
        if local_reference.local_size as usize
            >= std::mem::size_of::<crate::common::stack::CallStackInfo>()
        {
            self.current_frame = self.stack.frame_cache(&local_reference);
        }
        self.refresh_frame_cached_ptrs();
    }

    #[inline(always)]
    pub(crate) fn set_local_reference_with_frame(
        &mut self,
        local_reference: LocalReference,
        frame: CallFrameCache,
    ) {
        self.local_reference = local_reference;
        self.local_base_ptr = unsafe { self.stack.local_area_mut_ptr(&local_reference) };
        self.current_frame = frame;
        self.refresh_frame_cached_ptrs();
    }

    #[inline(always)]
    fn caller_frame_cache(&self) -> Option<CallFrameCache> {
        let caller = self.caller_local_reference()?;
        Some(self.stack.frame_cache(&caller))
    }

    pub fn func(&self) -> &FunctionInstanceData {
        self.gc.get_func(self.current_frame.code_addr)
    }
    pub fn func_by_addr(&self, addr: ObjectRef) -> &FunctionInstanceData {
        self.gc.get_func(addr)
    }
    pub(crate) fn code(&self) -> *const Instr {
        let code = self.current_frame.code_base;
        debug_assert!(!code.is_null(), "wasm frame must have a code base");
        code
    }
    pub fn module(&self) -> &ModuleInstance {
        self.gc.get_module(self.instance().module_addr)
    }
    pub fn instance_addr(&self) -> ObjectRef {
        self.gc.object_ref_for_instance(self.current_frame.instance)
    }
    pub fn instance_id(&self) -> u32 {
        self.instance().instance_id
    }
    pub fn instance(&self) -> &InstanceData {
        self.gc.instance(self.current_frame.instance)
    }
    pub fn local_reference(&self) -> LocalReference {
        self.local_reference
    }
    pub fn memory_addr(&self) -> Option<MemoryHandle> {
        self.current_frame.memory0_handle()
    }

    /// # Safety
    /// The active frame must have a default local memory and the cached pointer must have been
    /// refreshed from the current frame after the last frame transition.
    pub unsafe fn default_local_memory_unchecked(&self) -> &Memory {
        debug_assert!(!self.default_local_memory_ptr.is_null());
        unsafe { &*self.default_local_memory_ptr }
    }

    /// # Safety
    /// The active frame must have a default local memory and the cached pointer must have been
    /// refreshed from the current frame after the last frame transition.
    pub unsafe fn default_local_memory_mut_unchecked(&mut self) -> &mut Memory {
        debug_assert!(!self.default_local_memory_ptr.is_null());
        unsafe { &mut *self.default_local_memory_ptr }
    }
    #[inline(always)]
    fn memory_slot_at(&self, memidx: u32) -> Option<InstanceMemorySlot> {
        self.instance().memory_slots.get(memidx as usize).copied()
    }
    #[inline(always)]
    /// Returns the cached default local-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - The active frame must have a default memory and its cached kind must be `Local`.
    /// - Callers must only use the returned id while `self.current_frame` remains the active frame.
    pub unsafe fn default_local_memory_id_unchecked(&self) -> LocalMemoryId {
        debug_assert_eq!(self.current_frame.memory0_kind, CachedMemoryKind::Local);
        unsafe { LocalMemoryId::from_raw_unchecked(self.current_frame.memory0_raw) }
    }
    #[inline(always)]
    /// Returns the cached default shared-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - The active frame must have a default memory and its cached kind must be `Shared`.
    /// - Callers must only use the returned id while `self.current_frame` remains the active frame.
    pub unsafe fn default_shared_memory_id_unchecked(&self) -> SharedMemoryId {
        debug_assert_eq!(self.current_frame.memory0_kind, CachedMemoryKind::Shared);
        unsafe { SharedMemoryId::from_raw_unchecked(self.current_frame.memory0_raw) }
    }
    #[inline(always)]
    /// Returns the cached caller local-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - A caller frame must exist and its cached default memory kind must be `Local`.
    /// - Callers must only use the returned id while that caller frame remains valid.
    pub unsafe fn caller_local_memory_id_unchecked(&self) -> LocalMemoryId {
        let frame = self
            .caller_frame_cache()
            .expect("caller frame cache required for caller local memory");
        debug_assert_eq!(frame.memory0_kind, CachedMemoryKind::Local);
        unsafe { LocalMemoryId::from_raw_unchecked(frame.memory0_raw) }
    }
    #[inline(always)]
    /// Returns the cached caller shared-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - A caller frame must exist and its cached default memory kind must be `Shared`.
    /// - Callers must only use the returned id while that caller frame remains valid.
    pub unsafe fn caller_shared_memory_id_unchecked(&self) -> SharedMemoryId {
        let frame = self
            .caller_frame_cache()
            .expect("caller frame cache required for caller shared memory");
        debug_assert_eq!(frame.memory0_kind, CachedMemoryKind::Shared);
        unsafe { SharedMemoryId::from_raw_unchecked(frame.memory0_raw) }
    }
    #[inline(always)]
    /// Returns the typed local-memory id for `memidx` without decoding a tagged handle.
    ///
    /// # Safety
    /// - `memidx` must be in-bounds for the active instance memory list.
    /// - The memory at `memidx` must be local.
    pub unsafe fn local_memory_id_at_unchecked(&self, memidx: u32) -> LocalMemoryId {
        let slot = unsafe { self.memory_slot_at(memidx).unwrap_unchecked() };
        debug_assert!(matches!(slot, InstanceMemorySlot::Local(_)));
        match slot {
            InstanceMemorySlot::Local(id) => id,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }
    #[inline(always)]
    /// Returns the typed shared-memory id for `memidx` without decoding a tagged handle.
    ///
    /// # Safety
    /// - `memidx` must be in-bounds for the active instance memory list.
    /// - The memory at `memidx` must be shared.
    pub unsafe fn shared_memory_id_at_unchecked(&self, memidx: u32) -> SharedMemoryId {
        let slot = unsafe { self.memory_slot_at(memidx).unwrap_unchecked() };
        debug_assert!(matches!(slot, InstanceMemorySlot::Shared(_)));
        match slot {
            InstanceMemorySlot::Shared(id) => id,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }
    pub fn local_memory(&mut self) -> Option<&mut LocalMemoryObject> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => Some(
                self.gc
                    .local_memory_mut(unsafe { self.default_local_memory_id_unchecked() }),
            ),
            CachedMemoryKind::None | CachedMemoryKind::Shared => None,
        }
    }
    pub fn memory(&mut self) -> Option<&mut Memory> {
        self.local_memory().map(LocalMemoryObject::memory_mut)
    }

    #[inline(always)]
    pub fn memory_handle_result(&self) -> VMResult<MemoryHandle> {
        VMResult::from_option(self.current_frame.memory0_handle(), || {
            VMResult::MemoryIndexOutOfRange
        })
    }

    #[inline(always)]
    pub fn read_memory_u8_array<const N: usize>(&mut self, offset: usize) -> VMResult<[u8; N]> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u8_array::<N>(handle, offset)
    }

    #[inline(always)]
    pub fn push_memory_to_stack<const N: usize>(&mut self, offset: usize) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc
            .push_memory_to_stack::<N>(handle, self.stack, offset)
    }

    #[inline(always)]
    pub fn read_memory_u8(&mut self, offset: usize) -> VMResult<u8> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u8_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i8(&mut self, offset: usize) -> VMResult<i8> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i8_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_u16(&mut self, offset: usize) -> VMResult<u16> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u16_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i16(&mut self, offset: usize) -> VMResult<i16> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i16_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_u32(&mut self, offset: usize) -> VMResult<u32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u32_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i32(&mut self, offset: usize) -> VMResult<i32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i32_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_u64(&mut self, offset: usize) -> VMResult<u64> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u64_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i64(&mut self, offset: usize) -> VMResult<i64> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i64_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_f32(&mut self, offset: usize) -> VMResult<f32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_f32_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_f64(&mut self, offset: usize) -> VMResult<f64> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_f64_at(handle, offset)
    }

    #[inline(always)]
    pub fn write_memory_bytes(&mut self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.write_bytes(handle, offset, bytes)
    }

    #[inline(always)]
    pub fn grow_memory(&mut self, page_size_delta: u32) -> VMResult<i32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.grow_memory(handle, page_size_delta)
    }

    #[inline(always)]
    pub fn copy_memory(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.copy_memory(handle, dst, src, len)
    }

    #[inline(always)]
    pub fn fill_memory(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.fill_memory(handle, ptr, len, data)
    }

    pub fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        let handle = self.current_frame.memory0_handle()?;
        let addr = self.gc.object_ref_for_memory_handle(handle);
        Some(self.gc.with_memory_by_addr(addr, f))
    }
    pub fn memory_page_size(&self) -> Option<u32> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::None => None,
            CachedMemoryKind::Local => Some(
                self.gc
                    .local_memory(unsafe { self.default_local_memory_id_unchecked() })
                    .page_size(),
            ),
            CachedMemoryKind::Shared => Some(
                self.gc
                    .shared_memory(unsafe { self.default_shared_memory_id_unchecked() })
                    .page_size(),
            ),
        }
    }
    pub fn caller_local_reference(&self) -> Option<LocalReference> {
        (self.local_reference.local_size != 0)
            .then(|| self.stack.previous_local_reference(&self.local_reference))
            .filter(|reference| reference.local_size != 0)
    }
    pub fn caller_memory_addr(&self) -> Option<MemoryHandle> {
        self.caller_frame_cache()?.memory0_handle()
    }
    pub fn caller_local_memory(&mut self) -> Option<&mut LocalMemoryObject> {
        let frame = self.caller_frame_cache()?;
        match frame.memory0_kind {
            CachedMemoryKind::Local => Some(
                self.gc
                    .local_memory_mut(unsafe { self.caller_local_memory_id_unchecked() }),
            ),
            CachedMemoryKind::None | CachedMemoryKind::Shared => None,
        }
    }
    pub fn caller_memory(&mut self) -> Option<&mut Memory> {
        self.caller_local_memory()
            .map(LocalMemoryObject::memory_mut)
    }
    pub fn with_caller_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        let handle = self.caller_memory_addr()?;
        let addr = self.gc.object_ref_for_memory_handle(handle);
        Some(self.gc.with_memory_by_addr(addr, f))
    }
    /// Returns the in-place result area at the start of the active frame.
    ///
    /// Its capacity is the larger of the parameter area and the result area;
    /// host-call frame construction reserves any additional bytes needed before
    /// the frame's call-stack metadata.
    pub fn return_slot(&mut self) -> ReturnSlot {
        let local_ref = self.local_reference();
        ReturnSlot(unsafe { self.stack.local_area_mut_ptr(&local_ref) })
    }
}

pub fn execute_elem_init_const_expr(
    runtime: &mut StoreInner,
    globals: &[ObjectRef],
    funcs: &[ObjectRef],
    exprs: &[ConstExpr],
    expected: RefType,
) -> VMResult<ObjectRef> {
    if exprs.len() != 1 {
        return VMResult::Unlinkable;
    }
    tracing::trace!("execute_elem_init_const_expr: {funcs:?} {exprs:?}");
    match &exprs[0] {
        ConstExpr::FuncRef(idx) => {
            if expected != RefType::FuncRef {
                return VMResult::Unlinkable;
            }

            if let Some(addr) = funcs.get(*idx as usize) {
                VMResult::Success(*addr)
            } else {
                tracing::trace!("InvalidOperand");

                VMResult::InvalidOperand
            }
        }
        ConstExpr::RefNull(RefType::FuncRef) => {
            if expected != RefType::FuncRef {
                return VMResult::Unlinkable;
            }
            VMResult::Success(ObjectRef(0))
        }
        ConstExpr::RefNull(RefType::ExternRef) => {
            if expected != RefType::ExternRef {
                return VMResult::Unlinkable;
            }
            VMResult::Success(ObjectRef(0))
        }
        ConstExpr::GlobalGet(idx) => {
            let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                VMResult::Unlinkable
            }));
            let Ok(buf): Result<[u8; 4], _> = runtime.get_global(addr).try_into() else {
                return VMResult::Unlinkable;
            };
            VMResult::Success(ObjectRef(u32::from_le_bytes(buf)))
        }
        _ => VMResult::Unlinkable,
    }
}
pub const fn word_size<T>() -> usize {
    std::mem::size_of::<T>() / std::mem::size_of::<u32>()
}
#[derive(Debug)]
pub(crate) struct LocalReassignTable(pub(crate) SmallVec<[(u32, ValType, u32); 8]>);
#[derive(Default, Debug, Clone)]
pub struct LocalsData {
    count_i32: u32,
    count_f32: u32,
    count_func_ref: u32,
    count_extern_ref: u32,
    count_i64: u32,
    count_f64: u32,
    count_v128: u32,
    param_bytes: u32,
    temp_bytes: u32,
}
impl LocalsData {
    pub fn byte_size(&self) -> usize {
        self.base_byte_size() + self.temp_bytes as usize
    }
    pub(crate) fn base_byte_size(&self) -> usize {
        self.word_size() * 4
    }
    pub(crate) fn word_size(&self) -> usize {
        let Self {
            count_extern_ref,
            count_f32,
            count_f64,
            count_func_ref,
            count_i32,
            count_i64,
            count_v128,
            param_bytes: _,
            temp_bytes: _,
        } = self;
        (*count_i32 as usize
            + *count_f32 as usize
            + *count_extern_ref as usize
            + *count_func_ref as usize)
            + (*count_i64 as usize + *count_f64 as usize) * 2
            + *count_v128 as usize * 4
    }
    pub(crate) fn set_param_bytes(&mut self, param_bytes: u32) {
        self.param_bytes = param_bytes;
    }
    pub(crate) fn allocate_temp_slot(&mut self, ty: ValType) -> u32 {
        let addr = self.param_bytes + self.base_byte_size() as u32 + self.temp_bytes;
        self.temp_bytes += ty.stack_size().u32();
        addr
    }
    pub(crate) fn create_reassignment_table(
        &self,
        locals: &[Locals],
    ) -> Result<LocalReassignTable, WasmParserError> {
        let mut count_i32 = 0u32;
        let mut count_f32 = 0u32;
        let mut count_func_ref = 0u32;
        let mut count_extern_ref = 0u32;
        let mut count_i64 = 0u32;
        let mut count_f64 = 0u32;
        let mut count_v128 = 0u32;
        let mut index = 0u32;
        let mut res = SmallVec::with_capacity(locals.len());
        for Locals { n, t } in locals {
            index = index
                .checked_add(*n)
                .ok_or(WasmParserError::TooManyLocals)?;
            match t {
                ValType::I32 => {
                    res.push((
                        index,
                        ValType::I32,
                        count_i32
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_i32 = count_i32
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::F32 => {
                    res.push((
                        index,
                        ValType::F32,
                        (self.count_i32 + count_f32)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_f32 = count_f32
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::FuncRef => {
                    res.push((
                        index,
                        ValType::FuncRef,
                        (self.count_i32 + self.count_f32 + count_func_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_func_ref = count_func_ref
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::ExternRef => {
                    res.push((
                        index,
                        ValType::ExternRef,
                        (self.count_i32 + self.count_f32 + self.count_func_ref + count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_extern_ref = count_extern_ref
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::I64 => {
                    res.push((
                        index,
                        ValType::I64,
                        (self.count_i32
                            + self.count_f32
                            + self.count_func_ref
                            + self.count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?
                            + count_i64
                                .checked_mul(8)
                                .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_i64 = count_i64
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::F64 => {
                    res.push((
                        index,
                        ValType::F64,
                        (self.count_i32
                            + self.count_f32
                            + self.count_func_ref
                            + self.count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?
                            + (self.count_i64 + count_f64)
                                .checked_mul(8)
                                .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_f64 = count_f64
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::V128 => {
                    res.push((
                        index,
                        ValType::V128,
                        (self.count_i32
                            + self.count_f32
                            + self.count_func_ref
                            + self.count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?
                            + (self.count_i64 + self.count_f64)
                                .checked_mul(8)
                                .ok_or(WasmParserError::TooManyLocals)?
                            + count_v128
                                .checked_mul(16)
                                .ok_or(WasmParserError::TooManyLocals)?,
                    ));
                    count_v128 = count_v128
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
            }
        }
        Ok(LocalReassignTable(res))
    }
}
impl From<&[Locals]> for LocalsData {
    fn from(value: &[Locals]) -> Self {
        let mut me = Self::default();
        for Locals { n, t } in value {
            let n = *n;
            use ValType::*;
            match t {
                ExternRef => me.count_extern_ref += n,
                I32 => me.count_i32 += n,
                I64 => me.count_i64 += n,
                F32 => me.count_f32 += n,
                F64 => me.count_f64 += n,
                V128 => me.count_v128 += n,
                FuncRef => me.count_func_ref += n,
            }
        }
        me
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_elem_init_const_expr_fail_closes_numeric_const() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let result =
            execute_elem_init_const_expr(&mut gc, &[], &[], &[ConstExpr::I32(7)], RefType::FuncRef);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[test]
    fn execute_elem_init_const_expr_fail_closes_non_ref_global_get() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let global = gc.new_global_data8(42);
        let result = execute_elem_init_const_expr(
            &mut gc,
            &[global],
            &[],
            &[ConstExpr::GlobalGet(0)],
            RefType::ExternRef,
        );
        assert!(matches!(result, VMResult::Unlinkable));
    }
}
