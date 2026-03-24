#![allow(private_interfaces)]

#[macro_use]
mod vm_result;
mod continuation;
mod control_flow;
mod execute_context;
mod frame_layout;
mod precomputed_sites;
mod superop_kinds;
use std::{fmt::Display, future::Future, pin::Pin, sync::Arc};

use custom_section::NameSubSection;

pub use vm_result::VMResult;
mod memory;
pub use memory::{
    AtomicRmwOp, LocalMemoryObject, MemArg, Memory, MemoryInitError, SharedMemoryObject,
};
pub use memory::{AtomicWaitResult, SharedWaitRegistration};
pub(crate) mod stack;
pub(crate) use continuation::StablePc;
pub(crate) use control_flow::{
    structured_jump_rewrite, ControlFlowMetadataKind, ControlFlowMetadataSite,
    SafepointMetadataCache, StackMapSite, StackMapSourceSite, StructuredJumpRewriteKind,
    UnwindSiteMetadata, UnwindSourceSite,
};
pub use execute_context::ExecuteContext;
pub(crate) use frame_layout::{FrameLayoutColdMetadata, FrameLayoutHeader, FrameLayoutMetadata};
pub(crate) use frame_layout::{LocalSlotLayout, RefSlotRun};
pub(crate) use precomputed_sites::{
    DerivedRuntimeMetadata, PrecomputedBlockReturnSite, PrecomputedCallFrame,
    PrecomputedDirectCallSite, PrecomputedFunctionReturnSite, PrecomputedImportCallSite,
    PrecomputedIndirectCallSite, PrecomputedLoopSite, PrecomputedWaitSite,
};
pub(crate) use stack::CallFrameCache;
pub use stack::{LocalReference, Stack};
pub(crate) use superop_kinds::{
    FloatCompareKind, FloatScalarKind, I32ScalarKind, I64ScalarKind, IntCompareKind, Load4Kind,
    Load8Kind, Store4Kind, Store8Kind,
};
mod registry;
pub use registry::Registry;
mod object_ref;
pub(crate) mod store;
pub use object_ref::ObjectRef;
pub(crate) use store::{FunctionInstanceData, InstanceData, ModuleInstance, StoreInner};
pub use store::{InstanceHandle, MemoryHandle, Store, StoreState};
use store::{InstanceMemorySlot, LocalMemoryId, SharedMemoryId};

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

    pub fn stack_byte_size(&self) -> u32 {
        self.iter().map(|value| value.stack_size().u32()).sum()
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

    pub fn param_stack_byte_size(&self) -> u32 {
        self.0.stack_byte_size()
    }

    pub fn result_stack_byte_size(&self) -> u32 {
        self.1.stack_byte_size()
    }

    pub(crate) fn identity(&self) -> FuncTypeIdentity {
        FuncTypeIdentity::from_func_type(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FuncTypeIdentity {
    Packed(u128),
    Heap {
        params: Arc<[ValType]>,
        results: Arc<[ValType]>,
    },
}

impl FuncTypeIdentity {
    const LEN_BITS: u32 = 6;
    const TYPE_BITS: u32 = 4;
    const HEADER_BITS: u32 = Self::LEN_BITS * 2;
    const MAX_PACKED_ARITY: usize = ((u128::BITS - Self::HEADER_BITS) / Self::TYPE_BITS) as usize;

    fn from_func_type(ty: &FuncType) -> Self {
        let total_arity = ty.0 .0.len() + ty.1 .0.len();
        if total_arity > Self::MAX_PACKED_ARITY {
            return Self::Heap {
                params: Arc::from(ty.0 .0.as_slice()),
                results: Arc::from(ty.1 .0.as_slice()),
            };
        }

        let mut encoded = ty.0 .0.len() as u128;
        encoded |= (ty.1 .0.len() as u128) << Self::LEN_BITS;

        let mut shift = Self::HEADER_BITS;
        for value in ty.0.iter().chain(ty.1.iter()) {
            encoded |= (Self::encode_valtype(*value) as u128) << shift;
            shift += Self::TYPE_BITS;
        }
        Self::Packed(encoded)
    }

    const fn encode_valtype(value: ValType) -> u8 {
        match value {
            ValType::I32 => 0,
            ValType::I64 => 1,
            ValType::F32 => 2,
            ValType::F64 => 3,
            ValType::V128 => 4,
            ValType::FuncRef => 5,
            ValType::ExternRef => 6,
        }
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
    pub(crate) frame_layout: Arc<FrameLayoutMetadata>,
    pub(crate) control_flow_metadata: Arc<[ControlFlowMetadataSite]>,
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
    pub fn return_shape(&self, types: &TypeSection) -> Option<ReturnShape> {
        match self {
            BlockType::TypeIdx(idx) => {
                let ty = types.get(*idx)?;
                Some(shape_for_types(ty.1.iter().copied()))
            }
            BlockType::ValType(ty) => Some(ReturnShape::from_size(ty.stack_size().u32())),
            BlockType::Void => Some(ReturnShape::Empty),
        }
    }

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

    pub fn param_shape(&self, types: &TypeSection) -> Option<ReturnShape> {
        match self {
            BlockType::TypeIdx(idx) => {
                let ty = types.get(*idx)?;
                Some(shape_for_types(ty.0.iter().copied()))
            }
            BlockType::ValType(_ty) => Some(ReturnShape::Empty),
            BlockType::Void => Some(ReturnShape::Empty),
        }
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

fn shape_for_types(values: impl IntoIterator<Item = ValType>) -> ReturnShape {
    let mut iter = values.into_iter();
    match (iter.next(), iter.next()) {
        (None, _) => ReturnShape::Empty,
        (Some(first), None) => ReturnShape::from_size(first.stack_size().u32()),
        _ => ReturnShape::Generic,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoopParam {
    pub stack_top: u32,
    meta: u32,
}
#[derive(Debug, Clone, Copy)]
pub struct BlockReturn {
    pub stack_top: u32,
    meta: u32,
}
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedLoopParam {
    pub dst_from_local_top: u32,
    meta: u32,
}
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedBlockReturn {
    pub dst_from_local_top: u32,
    meta: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ReturnShape {
    Empty = 0,
    Scalar4 = 1,
    Scalar8 = 2,
    Generic = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum StackMapSafepointKind {
    Call = 0,
    CallImport = 1,
    CallIndirect = 2,
    ReturnCall = 3,
    ReturnCallImport = 4,
    ReturnCallIndirect = 5,
    Return = 6,
    Loop = 7,
    BlockReturn = 8,
    FunctionReturn = 9,
    MemoryWait = 10,
}

impl ReturnShape {
    const SHAPE_SHIFT: u32 = 30;
    const SIZE_MASK: u32 = (1 << Self::SHAPE_SHIFT) - 1;

    pub(crate) const fn from_size(size: u32) -> Self {
        match size {
            0 => Self::Empty,
            4 => Self::Scalar4,
            8 => Self::Scalar8,
            _ => Self::Generic,
        }
    }

    pub(crate) const fn encode_meta(size: u32, shape: ReturnShape) -> u32 {
        (size & Self::SIZE_MASK) | ((shape as u32) << Self::SHAPE_SHIFT)
    }

    #[allow(dead_code)]
    pub(crate) const fn decode_meta(meta: u32) -> Self {
        match meta >> Self::SHAPE_SHIFT {
            0 => Self::Empty,
            1 => Self::Scalar4,
            2 => Self::Scalar8,
            _ => Self::Generic,
        }
    }

    pub(crate) const fn size_from_meta(meta: u32) -> u32 {
        meta & Self::SIZE_MASK
    }
}

impl LoopParam {
    pub(crate) const fn with_shape(stack_top: u32, param_size: u32, shape: ReturnShape) -> Self {
        Self {
            stack_top,
            meta: ReturnShape::encode_meta(param_size, shape),
        }
    }

    pub(crate) const fn param_size(self) -> u32 {
        ReturnShape::size_from_meta(self.meta)
    }

    #[allow(dead_code)]
    pub(crate) const fn param_shape(self) -> ReturnShape {
        ReturnShape::decode_meta(self.meta)
    }
}

#[allow(dead_code)]
impl PrecomputedLoopParam {
    pub(crate) const fn with_shape(
        dst_from_local_top: u32,
        param_size: u32,
        shape: ReturnShape,
    ) -> Self {
        Self {
            dst_from_local_top,
            meta: ReturnShape::encode_meta(param_size, shape),
        }
    }

    pub(crate) const fn param_size(self) -> u32 {
        ReturnShape::size_from_meta(self.meta)
    }

    #[allow(dead_code)]
    pub(crate) const fn param_shape(self) -> ReturnShape {
        ReturnShape::decode_meta(self.meta)
    }
}

impl BlockReturn {
    pub(crate) const fn with_shape(stack_top: u32, return_size: u32, shape: ReturnShape) -> Self {
        Self {
            stack_top,
            meta: ReturnShape::encode_meta(return_size, shape),
        }
    }

    pub(crate) const fn return_size(self) -> u32 {
        ReturnShape::size_from_meta(self.meta)
    }

    pub(crate) const fn return_shape(self) -> ReturnShape {
        ReturnShape::decode_meta(self.meta)
    }
}

#[allow(dead_code)]
impl PrecomputedBlockReturn {
    pub(crate) const fn with_shape(
        dst_from_local_top: u32,
        return_size: u32,
        shape: ReturnShape,
    ) -> Self {
        Self {
            dst_from_local_top,
            meta: ReturnShape::encode_meta(return_size, shape),
        }
    }

    pub(crate) const fn return_size(self) -> u32 {
        ReturnShape::size_from_meta(self.meta)
    }

    #[allow(dead_code)]
    pub(crate) const fn return_shape(self) -> ReturnShape {
        ReturnShape::decode_meta(self.meta)
    }
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
    pub code_ptr: usize,
    pub drop_size: u32,
    pub local_addr: u32,
    pub select: u32,
    pub memarg: MemArg,
    pub block_return: BlockReturn,
    pub loop_param: LoopParam,
    pub precomputed_block_return: PrecomputedBlockReturn,
    pub precomputed_loop_param: PrecomputedLoopParam,
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
pub const PAGE_SIZE_MAX: usize = 4 * 1024 * 1024 * 1024 / PAGE_SIZE;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalGroupLayout {
    pub local_end_exclusive: u32,
    pub local_count: u32,
    pub val_type: ValType,
    pub offset_from_local_top: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalReassignTable(pub(crate) Vec<LocalGroupLayout>);
#[derive(Default, Debug, Clone)]
pub struct LocalsData {
    count_i32: u32,
    count_f32: u32,
    count_func_ref: u32,
    count_extern_ref: u32,
    count_i64: u32,
    count_f64: u32,
    count_v128: u32,
}
impl LocalsData {
    pub fn byte_size(&self) -> usize {
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
        } = self;
        (*count_i32 as usize
            + *count_f32 as usize
            + *count_extern_ref as usize
            + *count_func_ref as usize)
            + (*count_i64 as usize + *count_f64 as usize) * 2
            + *count_v128 as usize * 4
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
        let mut res = vec![];
        for Locals { n, t } in locals {
            index = index
                .checked_add(*n)
                .ok_or(WasmParserError::TooManyLocals)?;
            match t {
                ValType::I32 => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::I32,
                        offset_from_local_top: count_i32
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    });
                    count_i32 = count_i32
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::F32 => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::F32,
                        offset_from_local_top: (self.count_i32 + count_f32)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    });
                    count_f32 = count_f32
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::FuncRef => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::FuncRef,
                        offset_from_local_top: (self.count_i32 + self.count_f32 + count_func_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    });
                    count_func_ref = count_func_ref
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::ExternRef => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::ExternRef,
                        offset_from_local_top: (self.count_i32
                            + self.count_f32
                            + self.count_func_ref
                            + count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?,
                    });
                    count_extern_ref = count_extern_ref
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::I64 => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::I64,
                        offset_from_local_top: (self.count_i32
                            + self.count_f32
                            + self.count_func_ref
                            + self.count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?
                            + count_i64
                                .checked_mul(8)
                                .ok_or(WasmParserError::TooManyLocals)?,
                    });
                    count_i64 = count_i64
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::F64 => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::F64,
                        offset_from_local_top: (self.count_i32
                            + self.count_f32
                            + self.count_func_ref
                            + self.count_extern_ref)
                            .checked_mul(4)
                            .ok_or(WasmParserError::TooManyLocals)?
                            + (self.count_i64 + count_f64)
                                .checked_mul(8)
                                .ok_or(WasmParserError::TooManyLocals)?,
                    });
                    count_f64 = count_f64
                        .checked_add(*n)
                        .ok_or(WasmParserError::TooManyLocals)?;
                }
                ValType::V128 => {
                    res.push(LocalGroupLayout {
                        local_end_exclusive: index,
                        local_count: *n,
                        val_type: ValType::V128,
                        offset_from_local_top: (self.count_i32
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
                    });
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
