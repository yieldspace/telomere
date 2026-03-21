#![allow(private_interfaces)]

#[macro_use]
mod vm_result;
use std::{cell::OnceCell, fmt::Display, future::Future, pin::Pin, sync::Arc};

use custom_section::NameSubSection;

pub use vm_result::VMResult;
mod memory;
pub use memory::{AtomicRmwOp, LocalMemoryObject, MemArg, Memory, SharedMemoryObject};
pub use memory::{AtomicWaitResult, SharedWaitRegistration};
pub(crate) mod formal;
pub(crate) mod stack;
use stack::local_reference_has_call_stack_info;
use stack::CachedMemoryKind;
use stack::IntoCallFrameCache;
use stack::StackOperation;
pub(crate) use stack::{CallFrameCache, FrameProjection, MemoryHandleProjection};
pub use stack::{LocalReference, Stack};
mod registry;
pub use registry::Registry;
pub(crate) mod store;
pub(crate) use store::{FunctionInstanceData, InstanceData, ModuleInstance, StoreInner};
pub use store::{InstanceHandle, MemoryHandle, Store, StoreState};
use store::{InstanceMemorySlot, LocalMemoryId, SharedMemoryId};
pub(crate) mod gc;
pub use gc::GcRef;

use crate::runtime::scheduler::PendingOpEmitter;
use crate::WasmParserError;
pub mod custom_section;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeIdx(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[inline(always)]
fn read_marshaled_value(bytes: &[u8], ty: ValType) -> Option<WasmValue> {
    match ty {
        ValType::I32 => Some(WasmValue::I32(i32::from_le_bytes(bytes.try_into().ok()?))),
        ValType::I64 => Some(WasmValue::I64(i64::from_le_bytes(bytes.try_into().ok()?))),
        ValType::F32 => Some(WasmValue::F32(f32::from_bits(u32::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        ValType::F64 => Some(WasmValue::F64(f64::from_bits(u64::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        ValType::V128 => Some(WasmValue::V128(u128::from_le_bytes(bytes.try_into().ok()?))),
        ValType::FuncRef => Some(WasmValue::FuncRef(u32::from_le_bytes(
            bytes.try_into().ok()?,
        ))),
        ValType::ExternRef => Some(WasmValue::ExternRef(u32::from_le_bytes(
            bytes.try_into().ok()?,
        ))),
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
pub type AsyncHostFuture = Pin<Box<dyn Future<Output = VMResult<ResultValue>> + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTailCallTarget {
    FuncIdx(FuncIdx),
    FuncRef(GcRef),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostCallControl {
    Return(ResultValue),
    TailCall {
        target: HostTailCallTarget,
        params: ResultValue,
    },
    EndProgram,
}

pub struct HostCallContext<'ctx, 'store> {
    ctx: &'ctx mut ExecuteContext<'store>,
    param_types: &'ctx ResultType,
    result_types: &'ctx ResultType,
    params: OnceCell<ResultValue>,
}

impl<'ctx, 'store> HostCallContext<'ctx, 'store> {
    pub(crate) fn new(
        ctx: &'ctx mut ExecuteContext<'store>,
        param_types: &'ctx ResultType,
        result_types: &'ctx ResultType,
    ) -> Self {
        Self {
            ctx,
            param_types,
            result_types,
            params: OnceCell::new(),
        }
    }

    pub fn params(&self) -> &ResultValue {
        self.params.get_or_init(|| {
            let mut values = Vec::with_capacity(self.param_types.0.len());
            for index in 0..self.param_types.0.len() {
                values.push(
                    self.decode_param(index)
                        .expect("host params must decode from validated local frame"),
                );
            }
            ResultValue::new(values)
        })
    }

    pub fn param(&self, index: usize) -> Option<&WasmValue> {
        self.params().0.get(index)
    }

    pub fn param_i32(&self, index: usize) -> Option<i32> {
        match self.decode_param(index)? {
            WasmValue::I32(value) => Some(value),
            _ => None,
        }
    }

    pub fn param_i64(&self, index: usize) -> Option<i64> {
        match self.decode_param(index)? {
            WasmValue::I64(value) => Some(value),
            _ => None,
        }
    }

    pub fn param_funcref(&self, index: usize) -> Option<u32> {
        match self.decode_param(index)? {
            WasmValue::FuncRef(value) => Some(value),
            _ => None,
        }
    }

    pub fn param_externref(&self, index: usize) -> Option<u32> {
        match self.decode_param(index)? {
            WasmValue::ExternRef(value) => Some(value),
            _ => None,
        }
    }

    pub fn result_types(&self) -> &ResultType {
        self.result_types
    }

    pub fn store(&self) -> &Store {
        self.ctx.store_ref()
    }

    pub fn store_state(&self) -> StoreState {
        self.store().state
    }

    pub fn instance_id(&self) -> u32 {
        self.ctx.instance_id()
    }

    pub fn func_idx(&self) -> u32 {
        self.ctx.func().funcidx
    }

    pub fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.ctx.with_memory(f)
    }

    pub fn with_caller_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.ctx.with_caller_memory(f)
    }

    fn decode_param(&self, index: usize) -> Option<WasmValue> {
        let ty = *self.param_types.0.get(index)?;
        let mut local_addr = 0usize;
        for ty in self.param_types.iter().take(index) {
            local_addr += ty.stack_size().usize();
        }
        let size = ty.stack_size().usize();
        read_marshaled_value(
            self.ctx
                .stack_ref()
                .local_bytes(&self.ctx.local_reference(), local_addr, size),
            ty,
        )
    }
}

pub type HostFunction =
    for<'ctx, 'store> fn(ctx: HostCallContext<'ctx, 'store>) -> VMResult<HostCallControl>;

#[derive(Debug, Clone)]
pub struct AsyncHostCallContext {
    pub params: ResultValue,
    pub result_types: ResultType,
    pub store_state: StoreState,
}

pub type AsyncHostFunction = fn(AsyncHostCallContext) -> AsyncHostFuture;
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
    pub module_addr: GcRef,
    pub instance_id: u32,
    //  -> addr
    pub memory: Vec<GcRef>,
    // idx -> addr
    pub globals: Vec<GcRef>,
    // idx -> addr
    pub funcs: Vec<GcRef>,
    // idx -> addr
    pub tables: Vec<GcRef>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct StablePc(usize);
impl StablePc {
    const RELATIVE_TAG: usize = 1;

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
pub const PAGE_SIZE_MAX: usize = 4 * 1024 * 1024 * 1024 / PAGE_SIZE;

pub struct ExecuteContext<'a> {
    stack: &'a mut Stack,
    local_reference: LocalReference,
    pub(crate) current_frame: CallFrameCache,
    store: &'a Store,
    gc: &'a mut StoreInner,
    pending: PendingOpEmitter<'a>,
    cont: *const Instr,
    task_id: u32,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ExecuteContextSnapshot {
    pub(crate) default_memory: Option<MemoryHandle>,
    pub(crate) caller_memory: Option<MemoryHandle>,
    pub(crate) cont_addr: usize,
    pub(crate) task_id: u32,
    pub(crate) current_frame: CallFrameCache,
    pub(crate) caller_frame: Option<CallFrameCache>,
    pub(crate) active_local: LocalReference,
    pub(crate) caller_local: Option<LocalReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ExecuteContextProjection {
    pub(crate) default_memory: MemoryHandleProjection,
    pub(crate) caller_memory: MemoryHandleProjection,
    pub(crate) cont_addr: usize,
    pub(crate) task_id: u32,
    pub(crate) current_frame: Option<FrameProjection>,
    pub(crate) caller_frame: Option<FrameProjection>,
    pub(crate) active_local: LocalReference,
    pub(crate) caller_local: Option<LocalReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ExecContextTokenProjection {
    pub(crate) current_frame: FrameProjection,
    pub(crate) caller_frame: Option<FrameProjection>,
    pub(crate) cont_addr: usize,
    pub(crate) task_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
struct ProofReadyExecuteContextProjection {
    current_frame: FrameProjection,
    caller_frame: Option<FrameProjection>,
    cont_addr: usize,
    task_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
struct ExecContextTokenProjectionParts {
    current_return_pc: usize,
    current_instance_raw: u32,
    current_default_memory_present: bool,
    current_default_memory_shared: bool,
    current_default_memory_raw: u32,
    current_prev_local_top: usize,
    current_prev_local_size: u32,
    caller_present: bool,
    caller_return_pc: usize,
    caller_instance_raw: u32,
    caller_default_memory_present: bool,
    caller_default_memory_shared: bool,
    caller_default_memory_raw: u32,
    caller_prev_local_top: usize,
    caller_prev_local_size: u32,
    cont_addr: usize,
    task_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct FunctionProjectionParts {
    pub(crate) instance_raw: u32,
    pub(crate) entry_pc: usize,
    pub(crate) param_size: u32,
    pub(crate) local_size: u32,
    pub(crate) type_id: u32,
    pub(crate) default_memory_present: bool,
    pub(crate) default_memory_shared: bool,
    pub(crate) default_memory_raw: u32,
    pub(crate) is_host: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct CoreStepStateProjectionParts {
    pub(crate) stack: stack::StackProjectionParts,
    pub(crate) context: ExecContextTokenProjectionParts,
    pub(crate) tables: Vec<(u32, store::TableProjectionParts)>,
    pub(crate) globals: Vec<(u32, Vec<u8>)>,
    pub(crate) functions: Vec<(u32, FunctionProjectionParts)>,
    pub(crate) local_memories: Vec<(u32, memory::LinearMemoryProjectionParts)>,
    pub(crate) shared_memories: Vec<(u32, memory::SharedMemoryProjectionParts)>,
    pub(crate) data_segments: Vec<(u32, Vec<u8>)>,
    pub(crate) elem_segments: Vec<(u32, Vec<u32>)>,
    pub(crate) frame_metadata_len: usize,
}

pub(crate) struct ExecuteContextFacade<'ctx, 'store> {
    ctx: &'ctx mut ExecuteContext<'store>,
}

impl<'ctx, 'store> ExecuteContextFacade<'ctx, 'store> {
    #[inline(always)]
    pub(crate) fn new(ctx: &'ctx mut ExecuteContext<'store>) -> Self {
        Self { ctx }
    }

    #[inline(always)]
    pub(crate) fn as_ctx(&self) -> &ExecuteContext<'store> {
        self.ctx
    }

    #[inline(always)]
    pub(crate) fn as_ctx_mut(&mut self) -> &mut ExecuteContext<'store> {
        self.ctx
    }

    #[inline(always)]
    pub(crate) fn store_ref(&self) -> &Store {
        self.as_ctx().store_ref()
    }

    #[inline(always)]
    pub(crate) fn instance_id(&self) -> u32 {
        self.as_ctx().instance_id()
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn snapshot(&self) -> ExecuteContextSnapshot {
        self.as_ctx().snapshot()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn projection(&self) -> ExecuteContextProjection {
        self.as_ctx().projection()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn token_projection(&self) -> Option<ExecContextTokenProjection> {
        self.projection().token_projection()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn core_step_state_projection_parts(
        &mut self,
    ) -> Option<CoreStepStateProjectionParts> {
        self.as_ctx_mut().core_step_state_projection_parts()
    }

    #[inline(always)]
    pub(crate) fn has_default_memory(&self) -> bool {
        self.as_ctx().current_frame.memory0_handle().is_some()
    }

    #[inline(always)]
    pub(crate) fn gc_ref(&self) -> &StoreInner {
        self.as_ctx().gc_ref()
    }

    #[inline(always)]
    pub(crate) fn gc_mut(&mut self) -> &mut StoreInner {
        self.as_ctx_mut().gc_mut()
    }

    #[inline(always)]
    pub(crate) fn instance(&self) -> &InstanceData {
        self.as_ctx().instance()
    }

    #[inline(always)]
    pub(crate) fn module(&self) -> &ModuleInstance {
        self.as_ctx().module()
    }

    #[inline(always)]
    pub(crate) fn func(&self) -> &FunctionInstanceData {
        self.as_ctx().func()
    }

    #[inline(always)]
    pub(crate) fn func_by_addr(&self, funcaddr: GcRef) -> &FunctionInstanceData {
        self.as_ctx().func_by_addr(funcaddr)
    }

    #[inline(always)]
    pub(crate) fn stack_ref(&self) -> &Stack {
        self.as_ctx().stack_ref()
    }

    #[inline(always)]
    pub(crate) fn stack_mut(&mut self) -> &mut Stack {
        self.as_ctx_mut().stack_mut()
    }

    #[inline(always)]
    pub(crate) fn pop<T>(&mut self) -> T
    where
        Stack: StackOperation<T>,
    {
        self.stack_mut().pop()
    }

    #[inline(always)]
    pub(crate) fn push<T>(&mut self, value: T) -> VMResult<()>
    where
        Stack: StackOperation<T>,
    {
        self.stack_mut().push(value)
    }

    #[inline(always)]
    pub(crate) fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.as_ctx_mut().pop_u8_array::<N>()
    }

    #[inline(always)]
    pub(crate) fn pop_u32(&mut self) -> u32 {
        self.stack_mut().pop_u32()
    }

    #[inline(always)]
    pub(crate) fn pop_i32(&mut self) -> i32 {
        self.stack_mut().pop_i32()
    }

    #[inline(always)]
    pub(crate) fn pop_u64(&mut self) -> u64 {
        self.stack_mut().pop_u64()
    }

    #[inline(always)]
    pub(crate) fn pop_i64(&mut self) -> i64 {
        self.stack_mut().pop_i64()
    }

    #[inline(always)]
    pub(crate) fn pop_u128(&mut self) -> u128 {
        self.stack_mut().pop_u128()
    }

    #[inline(always)]
    pub(crate) fn pop_u8_array_generic<const N: usize>(&mut self, size: usize) -> [u8; N] {
        self.stack_mut().pop_u8_array_generic::<N>(size)
    }

    #[inline(always)]
    pub(crate) fn push_slice(&mut self, value: &[u8]) -> VMResult<()> {
        self.as_ctx_mut().push_slice(value)
    }

    #[inline(always)]
    pub(crate) fn drop_values(&mut self, size: usize) {
        self.stack_mut().drop(size);
    }

    #[inline(always)]
    pub(crate) fn push_u32(&mut self, value: u32) -> VMResult<()> {
        self.stack_mut().push_u32(value)
    }

    #[inline(always)]
    pub(crate) fn push_i32(&mut self, value: i32) -> VMResult<()> {
        self.stack_mut().push_i32(value)
    }

    #[inline(always)]
    pub(crate) fn push_u64(&mut self, value: u64) -> VMResult<()> {
        self.stack_mut().push_u64(value)
    }

    #[inline(always)]
    pub(crate) fn push_i64(&mut self, value: i64) -> VMResult<()> {
        self.stack_mut().push_i64(value)
    }

    #[inline(always)]
    pub(crate) fn push_u128(&mut self, value: u128) -> VMResult<()> {
        self.stack_mut().push_u128(value)
    }

    #[inline(always)]
    pub(crate) fn push_typed_value(&mut self, ty: ValType, value: &WasmValue) -> VMResult<()> {
        match (ty, value) {
            (ValType::I32, WasmValue::I32(value)) => self.push_i32(*value),
            (ValType::I64, WasmValue::I64(value)) => self.push_i64(*value),
            (ValType::F32, WasmValue::F32(value)) => self.push(*value),
            (ValType::F64, WasmValue::F64(value)) => self.push(*value),
            (ValType::V128, WasmValue::V128(value)) => self.push_u128(*value),
            (ValType::FuncRef, WasmValue::FuncRef(value)) => self.push_u32(*value),
            (ValType::ExternRef, WasmValue::ExternRef(value)) => self.push_u32(*value),
            _ => VMResult::InvalidOperand,
        }
    }

    #[inline(never)]
    pub(crate) fn push_result_values(
        &mut self,
        types: &ResultType,
        values: &ResultValue,
    ) -> VMResult<()> {
        if types.0.len() != values.len() {
            return VMResult::InvalidOperand;
        }
        for (ty, value) in types.iter().zip(values.iter()) {
            vm_try!(self.push_typed_value(*ty, value));
        }
        VMResult::Success(())
    }

    #[inline(never)]
    pub(crate) fn write_marshaled_results(
        &mut self,
        slot_offset: usize,
        types: &ResultType,
        values: &ResultValue,
    ) -> VMResult<()> {
        let local_size = types
            .iter()
            .map(|ty| ty.stack_size().usize())
            .sum::<usize>();
        let slot = LocalReference {
            local_top: slot_offset,
            local_size: local_size as u32,
        };
        if types.0.len() != values.len() {
            return VMResult::InvalidOperand;
        }
        let mut offset = 0usize;
        let dst = unsafe { self.stack_mut().local_area_mut_ptr(&slot) };
        for (ty, value) in types.iter().zip(values.iter()) {
            let size = ty.stack_size().usize();
            unsafe {
                match (ty, value) {
                    (ValType::I32, WasmValue::I32(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    (ValType::I64, WasmValue::I64(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    (ValType::F32, WasmValue::F32(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_bits().to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    (ValType::F64, WasmValue::F64(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_bits().to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    (ValType::V128, WasmValue::V128(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    (ValType::FuncRef, WasmValue::FuncRef(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    (ValType::ExternRef, WasmValue::ExternRef(value)) => {
                        std::ptr::copy_nonoverlapping(
                            value.to_le_bytes().as_ptr(),
                            dst.add(offset),
                            size,
                        );
                    }
                    _ => return VMResult::InvalidOperand,
                }
            }
            offset += size;
        }
        VMResult::Success(())
    }

    #[inline(always)]
    pub(crate) fn local_get(&mut self, local_addr: usize, size: usize) -> VMResult<()> {
        self.as_ctx_mut().local_get(local_addr, size)
    }

    #[inline(always)]
    pub(crate) fn local_set(&mut self, local_addr: usize, size: usize) {
        self.as_ctx_mut().local_set(local_addr, size);
    }

    #[inline(always)]
    pub(crate) fn local_tee(&mut self, local_addr: usize, size: usize) {
        self.as_ctx_mut().local_tee(local_addr, size);
    }

    #[inline(always)]
    pub(crate) fn select(&mut self, size: usize, cond: u32) -> VMResult<()> {
        let a = self.pop_u8_array_generic::<8>(size);
        let b = self.pop_u8_array_generic::<8>(size);
        let value = if cond == 0 { a } else { b };
        self.push_slice(&value[0..size])
    }

    #[inline(always)]
    pub(crate) fn local_reference(&self) -> LocalReference {
        self.as_ctx().local_reference()
    }

    #[inline(always)]
    pub(crate) fn pop_branch_cond(&mut self) -> bool {
        self.pop::<u32>() != 0
    }

    #[inline(always)]
    pub(crate) fn pending_mut(&mut self) -> &mut PendingOpEmitter<'store> {
        self.as_ctx_mut().pending_mut()
    }

    #[inline(always)]
    pub(crate) fn task_id(&self) -> u32 {
        self.as_ctx().task_id()
    }

    #[inline(always)]
    pub(crate) fn code(&self) -> *const Instr {
        self.as_ctx().code()
    }

    #[inline(always)]
    pub(crate) fn set_cont(&mut self, cont: *const Instr) {
        self.as_ctx_mut().set_cont(cont)
    }

    #[inline(always)]
    pub(crate) fn clear_cont(&mut self) {
        self.as_ctx_mut().clear_cont();
    }

    #[inline(always)]
    pub(crate) fn clear_continuation(&mut self) {
        self.clear_cont();
    }

    #[inline(always)]
    pub(crate) fn clone_shared_memory(
        &self,
        id: store::SharedMemoryId,
    ) -> std::sync::Arc<SharedMemoryObject> {
        self.as_ctx().clone_shared_memory(id)
    }

    #[inline(always)]
    pub(crate) fn with_shared_memory_ref<T>(
        &self,
        id: store::SharedMemoryId,
        f: impl FnOnce(&SharedMemoryObject) -> T,
    ) -> T {
        f(self.gc_ref().shared_memory(id))
    }

    #[inline(always)]
    pub(crate) fn stable_pc_from_raw_in_frame(&self, pc: *const Instr) -> StablePc {
        StablePc::from_raw_in_frame(self.gc_ref(), self.stack_ref(), self.local_reference(), pc)
    }

    #[inline(always)]
    pub(crate) fn resolve_branch_target(&self, addr: u32) -> *const Instr {
        self.jump_target(addr)
    }

    #[inline(always)]
    pub(crate) fn enter_function_call(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: CallFrameCache,
        return_addr: *const Instr,
    ) -> VMResult<()> {
        self.as_ctx_mut()
            .enter_function_call(param_size, local_size, frame, return_addr)
    }

    #[inline(always)]
    pub(crate) fn enter_function_return_call(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: CallFrameCache,
    ) -> VMResult<()> {
        self.as_ctx_mut()
            .enter_function_return_call(param_size, local_size, frame)
    }

    #[inline(always)]
    pub(crate) unsafe fn default_local_memory_id_unchecked(&self) -> store::LocalMemoryId {
        unsafe { self.as_ctx().default_local_memory_id_unchecked() }
    }

    #[inline(always)]
    pub(crate) unsafe fn default_shared_memory_id_unchecked(&self) -> store::SharedMemoryId {
        unsafe { self.as_ctx().default_shared_memory_id_unchecked() }
    }

    #[inline(always)]
    pub(crate) unsafe fn local_memory_id_at_unchecked(&self, memidx: u32) -> store::LocalMemoryId {
        unsafe { self.as_ctx().local_memory_id_at_unchecked(memidx) }
    }

    #[inline(always)]
    pub(crate) unsafe fn shared_memory_id_at_unchecked(
        &self,
        memidx: u32,
    ) -> store::SharedMemoryId {
        unsafe { self.as_ctx().shared_memory_id_at_unchecked(memidx) }
    }

    #[inline(always)]
    pub(crate) fn read_memory_u8_array<const N: usize>(
        &mut self,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        self.as_ctx_mut().read_memory_u8_array::<N>(offset)
    }

    #[inline(always)]
    pub(crate) fn push_memory_to_stack<const N: usize>(&mut self, offset: usize) -> VMResult<()> {
        self.as_ctx_mut().push_memory_to_stack::<N>(offset)
    }

    #[inline(always)]
    pub(crate) fn read_memory_u8(&mut self, offset: usize) -> VMResult<u8> {
        self.as_ctx_mut().read_memory_u8(offset)
    }

    #[inline(always)]
    pub(crate) fn read_memory_i8(&mut self, offset: usize) -> VMResult<i8> {
        self.as_ctx_mut().read_memory_i8(offset)
    }

    #[inline(always)]
    pub(crate) fn read_memory_u16(&mut self, offset: usize) -> VMResult<u16> {
        self.as_ctx_mut().read_memory_u16(offset)
    }

    #[inline(always)]
    pub(crate) fn read_memory_i16(&mut self, offset: usize) -> VMResult<i16> {
        self.as_ctx_mut().read_memory_i16(offset)
    }

    #[inline(always)]
    pub(crate) fn read_memory_u32(&mut self, offset: usize) -> VMResult<u32> {
        self.as_ctx_mut().read_memory_u32(offset)
    }

    #[inline(always)]
    pub(crate) fn read_memory_i32(&mut self, offset: usize) -> VMResult<i32> {
        self.as_ctx_mut().read_memory_i32(offset)
    }

    #[inline(always)]
    pub(crate) fn write_memory_bytes(&mut self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        self.as_ctx_mut().write_memory_bytes(offset, bytes)
    }

    #[inline(always)]
    pub(crate) unsafe fn push_memory_to_stack_local_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .push_memory_to_stack_local_indexed::<N>(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn push_memory_to_stack_shared_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .push_memory_to_stack_shared_indexed::<N>(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_array_local_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        unsafe {
            self.as_ctx_mut()
                .read_u8_array_local_indexed::<N>(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_array_shared_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        unsafe {
            self.as_ctx_mut()
                .read_u8_array_shared_indexed::<N>(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u8> {
        unsafe { self.as_ctx_mut().read_u8_at_local_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u8> {
        unsafe { self.as_ctx_mut().read_u8_at_shared_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i8_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i8> {
        unsafe { self.as_ctx_mut().read_i8_at_local_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i8_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i8> {
        unsafe { self.as_ctx_mut().read_i8_at_shared_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u16_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u16> {
        unsafe { self.as_ctx_mut().read_u16_at_local_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u16_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u16> {
        unsafe { self.as_ctx_mut().read_u16_at_shared_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i16_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i16> {
        unsafe { self.as_ctx_mut().read_i16_at_local_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i16_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i16> {
        unsafe { self.as_ctx_mut().read_i16_at_shared_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u32_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u32> {
        unsafe { self.as_ctx_mut().read_u32_at_local_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u32_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u32> {
        unsafe { self.as_ctx_mut().read_u32_at_shared_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i32_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i32> {
        unsafe { self.as_ctx_mut().read_i32_at_local_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i32_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i32> {
        unsafe { self.as_ctx_mut().read_i32_at_shared_indexed(memidx, offset) }
    }

    #[inline(always)]
    pub(crate) unsafe fn write_memory_bytes_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .write_memory_bytes_local_indexed(memidx, offset, bytes)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn write_memory_bytes_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .write_memory_bytes_shared_indexed(memidx, offset, bytes)
        }
    }

    #[inline(always)]
    pub(crate) fn grow_memory(&mut self, page_size_delta: u32) -> VMResult<i32> {
        self.as_ctx_mut().grow_memory(page_size_delta)
    }

    #[inline(always)]
    pub(crate) unsafe fn grow_memory_local_indexed(
        &mut self,
        memidx: u32,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        unsafe {
            self.as_ctx_mut()
                .grow_memory_local_indexed(memidx, page_size_delta)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn grow_memory_shared_indexed(
        &mut self,
        memidx: u32,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        unsafe {
            self.as_ctx_mut()
                .grow_memory_shared_indexed(memidx, page_size_delta)
        }
    }

    #[inline(always)]
    pub(crate) fn memory_page_size(&self) -> Option<u32> {
        self.as_ctx().memory_page_size()
    }

    #[inline(always)]
    pub(crate) unsafe fn memory_page_size_local_indexed(&self, memidx: u32) -> u32 {
        unsafe { self.as_ctx().memory_page_size_local_indexed(memidx) }
    }

    #[inline(always)]
    pub(crate) unsafe fn memory_page_size_shared_indexed(&self, memidx: u32) -> u32 {
        unsafe { self.as_ctx().memory_page_size_shared_indexed(memidx) }
    }

    #[inline(always)]
    pub(crate) fn local_atomic_fence(&mut self) {
        self.as_ctx_mut().local_atomic_fence()
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_fence(&mut self) {
        self.as_ctx_mut().shared_atomic_fence()
    }

    #[inline(always)]
    pub(crate) fn local_atomic_load_u8(&mut self, offset: usize) -> VMResult<u8> {
        self.as_ctx_mut().local_atomic_load_u8(offset)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_load_u16(&mut self, offset: usize) -> VMResult<u16> {
        self.as_ctx_mut().local_atomic_load_u16(offset)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_load_u32(&mut self, offset: usize) -> VMResult<u32> {
        self.as_ctx_mut().local_atomic_load_u32(offset)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_load_u64(&mut self, offset: usize) -> VMResult<u64> {
        self.as_ctx_mut().local_atomic_load_u64(offset)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_load_u8(&mut self, offset: usize) -> VMResult<u8> {
        self.as_ctx_mut().shared_atomic_load_u8(offset)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_load_u16(&mut self, offset: usize) -> VMResult<u16> {
        self.as_ctx_mut().shared_atomic_load_u16(offset)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_load_u32(&mut self, offset: usize) -> VMResult<u32> {
        self.as_ctx_mut().shared_atomic_load_u32(offset)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_load_u64(&mut self, offset: usize) -> VMResult<u64> {
        self.as_ctx_mut().shared_atomic_load_u64(offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_load_u8(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u8> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_load_u8(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_load_u16(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u16> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_load_u16(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_load_u32(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u32> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_load_u32(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_load_u64(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u64> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_load_u64(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_load_u8(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u8> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_load_u8(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_load_u16(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u16> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_load_u16(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_load_u32(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u32> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_load_u32(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_load_u64(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u64> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_load_u64(memidx, offset)
        }
    }

    #[inline(always)]
    pub(crate) fn local_atomic_store_u8(&mut self, offset: usize, value: u8) -> VMResult<()> {
        self.as_ctx_mut().local_atomic_store_u8(offset, value)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_store_u16(&mut self, offset: usize, value: u16) -> VMResult<()> {
        self.as_ctx_mut().local_atomic_store_u16(offset, value)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_store_u32(&mut self, offset: usize, value: u32) -> VMResult<()> {
        self.as_ctx_mut().local_atomic_store_u32(offset, value)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_store_u64(&mut self, offset: usize, value: u64) -> VMResult<()> {
        self.as_ctx_mut().local_atomic_store_u64(offset, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_store_u8(&mut self, offset: usize, value: u8) -> VMResult<()> {
        self.as_ctx_mut().shared_atomic_store_u8(offset, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_store_u16(&mut self, offset: usize, value: u16) -> VMResult<()> {
        self.as_ctx_mut().shared_atomic_store_u16(offset, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_store_u32(&mut self, offset: usize, value: u32) -> VMResult<()> {
        self.as_ctx_mut().shared_atomic_store_u32(offset, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_store_u64(&mut self, offset: usize, value: u64) -> VMResult<()> {
        self.as_ctx_mut().shared_atomic_store_u64(offset, value)
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_store_u8(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u8,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_store_u8(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_store_u16(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u16,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_store_u16(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_store_u32(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u32,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_store_u32(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_store_u64(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u64,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_store_u64(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_store_u8(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u8,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_store_u8(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_store_u16(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u16,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_store_u16(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_store_u32(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u32,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_store_u32(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_store_u64(
        &mut self,
        memidx: u32,
        offset: usize,
        value: u64,
    ) -> VMResult<()> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_store_u64(memidx, offset, value)
        }
    }

    #[inline(always)]
    pub(crate) fn local_atomic_rmw_u8(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        self.as_ctx_mut().local_atomic_rmw_u8(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_rmw_u16(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.as_ctx_mut().local_atomic_rmw_u16(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_rmw_u32(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.as_ctx_mut().local_atomic_rmw_u32(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_rmw_u64(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.as_ctx_mut().local_atomic_rmw_u64(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_rmw_u8(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        self.as_ctx_mut().shared_atomic_rmw_u8(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_rmw_u16(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.as_ctx_mut().shared_atomic_rmw_u16(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_rmw_u32(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.as_ctx_mut().shared_atomic_rmw_u32(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_rmw_u64(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.as_ctx_mut().shared_atomic_rmw_u64(offset, op, value)
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_rmw_u8(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_rmw_u8(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_rmw_u16(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_rmw_u16(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_rmw_u32(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_rmw_u32(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_rmw_u64(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_rmw_u64(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_rmw_u8(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_rmw_u8(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_rmw_u16(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_rmw_u16(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_rmw_u32(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_rmw_u32(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_rmw_u64(
        &mut self,
        memidx: u32,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_rmw_u64(memidx, offset, op, value)
        }
    }

    #[inline(always)]
    pub(crate) fn local_atomic_cmpxchg_u8(
        &mut self,
        offset: usize,
        current: u8,
        new: u8,
    ) -> VMResult<u8> {
        self.as_ctx_mut()
            .local_atomic_cmpxchg_u8(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_cmpxchg_u16(
        &mut self,
        offset: usize,
        current: u16,
        new: u16,
    ) -> VMResult<u16> {
        self.as_ctx_mut()
            .local_atomic_cmpxchg_u16(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_cmpxchg_u32(
        &mut self,
        offset: usize,
        current: u32,
        new: u32,
    ) -> VMResult<u32> {
        self.as_ctx_mut()
            .local_atomic_cmpxchg_u32(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn local_atomic_cmpxchg_u64(
        &mut self,
        offset: usize,
        current: u64,
        new: u64,
    ) -> VMResult<u64> {
        self.as_ctx_mut()
            .local_atomic_cmpxchg_u64(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_cmpxchg_u8(
        &mut self,
        offset: usize,
        current: u8,
        new: u8,
    ) -> VMResult<u8> {
        self.as_ctx_mut()
            .shared_atomic_cmpxchg_u8(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_cmpxchg_u16(
        &mut self,
        offset: usize,
        current: u16,
        new: u16,
    ) -> VMResult<u16> {
        self.as_ctx_mut()
            .shared_atomic_cmpxchg_u16(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_cmpxchg_u32(
        &mut self,
        offset: usize,
        current: u32,
        new: u32,
    ) -> VMResult<u32> {
        self.as_ctx_mut()
            .shared_atomic_cmpxchg_u32(offset, current, new)
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_cmpxchg_u64(
        &mut self,
        offset: usize,
        current: u64,
        new: u64,
    ) -> VMResult<u64> {
        self.as_ctx_mut()
            .shared_atomic_cmpxchg_u64(offset, current, new)
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_cmpxchg_u8(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u8,
        new: u8,
    ) -> VMResult<u8> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_cmpxchg_u8(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_cmpxchg_u16(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u16,
        new: u16,
    ) -> VMResult<u16> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_cmpxchg_u16(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_cmpxchg_u32(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u32,
        new: u32,
    ) -> VMResult<u32> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_cmpxchg_u32(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_local_atomic_cmpxchg_u64(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u64,
        new: u64,
    ) -> VMResult<u64> {
        unsafe {
            self.as_ctx_mut()
                .indexed_local_atomic_cmpxchg_u64(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_cmpxchg_u8(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u8,
        new: u8,
    ) -> VMResult<u8> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_cmpxchg_u8(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_cmpxchg_u16(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u16,
        new: u16,
    ) -> VMResult<u16> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_cmpxchg_u16(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_cmpxchg_u32(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u32,
        new: u32,
    ) -> VMResult<u32> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_cmpxchg_u32(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn indexed_shared_atomic_cmpxchg_u64(
        &mut self,
        memidx: u32,
        offset: usize,
        current: u64,
        new: u64,
    ) -> VMResult<u64> {
        unsafe {
            self.as_ctx_mut()
                .indexed_shared_atomic_cmpxchg_u64(memidx, offset, current, new)
        }
    }

    #[inline(always)]
    pub(crate) fn function_return_in_place(&mut self, return_size: usize) -> (*const Instr, usize) {
        self.as_ctx_mut().function_return_in_place(return_size)
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn function_return(&mut self, return_size: usize) -> *const Instr {
        self.as_ctx_mut().function_return(return_size)
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn finish_function_return(&mut self, return_size: usize) -> *const Instr {
        self.function_return(return_size)
    }

    #[inline(always)]
    pub(crate) fn block_return(&mut self, stack_top: usize, return_size: usize) {
        self.as_ctx_mut().block_return(stack_top, return_size);
    }

    #[inline(always)]
    pub(crate) fn apply_block_return(&mut self, stack_top: usize, return_size: usize) {
        self.block_return(stack_top, return_size);
    }

    #[inline(always)]
    pub(crate) fn end_program(&mut self) {
        self.as_ctx_mut().end_program();
    }

    #[inline(always)]
    pub(crate) fn global_addr(&self, global_idx: usize) -> GcRef {
        self.instance().globals.as_slice()[global_idx]
    }

    #[inline(always)]
    pub(crate) fn push_global_bytes<const SIZE: usize>(
        &mut self,
        global_idx: usize,
    ) -> VMResult<()> {
        let addr = self.global_addr(global_idx);
        let bytes = self.gc_ref().get_global(addr).to_vec();
        debug_assert_eq!(bytes.len(), SIZE);
        self.push_slice(&bytes)
    }

    #[inline(always)]
    pub(crate) fn write_global_bytes<const SIZE: usize>(
        &mut self,
        global_idx: usize,
        value: [u8; SIZE],
    ) {
        let addr = self.global_addr(global_idx);
        self.gc_mut().get_global_mut(addr).copy_from_slice(&value);
    }

    #[inline(always)]
    pub(crate) fn push_ref(&mut self, value: u32) -> VMResult<()> {
        self.push(value)
    }

    #[inline(always)]
    pub(crate) fn pop_ref(&mut self) -> u32 {
        self.pop::<u32>()
    }

    #[inline(always)]
    pub(crate) fn ref_func_value(&self, funcidx: u32) -> u32 {
        self.instance().funcs.as_slice()[funcidx as usize].get()
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn instance_func_addr(&self, funcidx: u32) -> Option<GcRef> {
        self.instance()
            .funcs
            .as_slice()
            .get(funcidx as usize)
            .copied()
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn gc_ref_for_instance(&self, instance: store::InstanceId) -> GcRef {
        self.gc_ref().gc_ref_for_instance(instance)
    }

    #[inline(always)]
    pub(crate) fn function_type(&self, func: &FunctionInstanceData) -> &FuncType {
        let gc = self.gc_ref();
        let instance = gc.instance(func.instance);
        let module = gc.get_module(instance.module_addr);
        &module.function_types[func.typeidx.0 as usize]
    }

    #[inline(always)]
    pub(crate) fn function_type_by_addr(&self, funcaddr: GcRef) -> &FuncType {
        self.function_type(self.func_by_addr(funcaddr))
    }

    #[inline(always)]
    pub(crate) fn current_function_type(&self) -> &FuncType {
        self.function_type(self.func())
    }

    #[inline(always)]
    pub(crate) fn module_function_type(&self, typeidx: u32) -> Option<&FuncType> {
        self.module().function_types.get(typeidx as usize)
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn call_frame_for_funcaddr(&self, funcaddr: GcRef) -> CallFrameCache {
        let funcinst = self.func_by_addr(funcaddr);
        let instance = self.gc_ref().instance(funcinst.instance);
        let memory0 = instance
            .memory_slots
            .first()
            .copied()
            .and_then(|slot| slot.handle());
        CallFrameCache::from_parts(funcaddr, funcinst, memory0)
    }

    #[inline(always)]
    pub(crate) fn run_sync_host_function(
        &mut self,
        fp: HostFunction,
        param_types: &ResultType,
        result_types: &ResultType,
    ) -> VMResult<HostCallControl> {
        fp(HostCallContext::new(
            self.as_ctx_mut(),
            param_types,
            result_types,
        ))
    }

    #[inline(always)]
    pub(crate) fn table_addr(&self, table_idx: usize) -> GcRef {
        self.instance().tables.as_slice()[table_idx]
    }

    #[inline(always)]
    pub(crate) fn table_reftype(&mut self, table_addr: GcRef) -> RefType {
        self.gc_mut().get_table(table_addr).0.reftype
    }

    #[inline(always)]
    pub(crate) fn table_len(&mut self, table_addr: GcRef) -> usize {
        self.gc_mut().get_table(table_addr).1.len()
    }

    #[inline(always)]
    pub(crate) fn table_get_value(&mut self, table_addr: GcRef, index: usize) -> Option<u32> {
        self.gc_mut().get_table(table_addr).1.get(index).copied()
    }

    #[inline(always)]
    pub(crate) fn table_set_value(
        &mut self,
        table_addr: GcRef,
        index: usize,
        value: u32,
    ) -> VMResult<()> {
        let table = &mut self.gc_mut().get_table(table_addr).1;
        let slot = vm_try!(VMResult::from_option(table.get_mut(index), || {
            VMResult::TableIndexOutOfRange
        }));
        *slot = value;
        VMResult::Success(())
    }

    #[inline(never)]
    pub(crate) fn table_copy(
        &mut self,
        dst_table_addr: GcRef,
        src_table_addr: GcRef,
        dst: usize,
        src: usize,
        len: usize,
    ) -> VMResult<()> {
        let src_values = {
            let src_table = &self.gc_mut().get_table(src_table_addr).1;
            vm_try!(VMResult::from_option(src_table.get(src..src + len), || {
                VMResult::TableIndexOutOfRange
            }))
            .to_vec()
        };
        let dst_table = &mut self.gc_mut().get_table(dst_table_addr).1;
        let dst_slice = vm_try!(VMResult::from_option(
            dst_table.get_mut(dst..dst + len),
            || { VMResult::TableIndexOutOfRange }
        ));
        dst_slice.copy_from_slice(&src_values);
        VMResult::Success(())
    }

    #[inline(always)]
    pub(crate) fn table_grow(&mut self, table_addr: GcRef, n: i32, value: u32) -> u32 {
        let table_inst = &mut self.gc_mut().get_table(table_addr);
        let sz = table_inst.1.len();
        if n < 0 {
            return (-1i32) as u32;
        }
        let new_len = sz + n as usize;
        match table_inst.0.limits.max {
            Some(max) if max as usize >= new_len => {
                table_inst.1.resize(new_len, value);
                sz as u32
            }
            None => {
                table_inst.1.resize(new_len, value);
                sz as u32
            }
            Some(_) => (-1i32) as u32,
        }
    }

    #[inline(always)]
    pub(crate) fn table_fill(
        &mut self,
        table_addr: GcRef,
        start: usize,
        len: usize,
        value: u32,
    ) -> VMResult<()> {
        let table = &mut self.gc_mut().get_table(table_addr).1;
        let slice = vm_try!(VMResult::from_option(
            table.get_mut(start..start + len),
            || { VMResult::TableIndexOutOfRange }
        ));
        slice.fill(value);
        VMResult::Success(())
    }

    #[inline(always)]
    pub(crate) fn table_write_slice(
        &mut self,
        table_addr: GcRef,
        start: usize,
        values: &[u32],
    ) -> VMResult<()> {
        let table = &mut self.gc_mut().get_table(table_addr).1;
        let slice = vm_try!(VMResult::from_option(
            table.get_mut(start..start + values.len()),
            || { VMResult::TableIndexOutOfRange }
        ));
        slice.copy_from_slice(values);
        VMResult::Success(())
    }

    #[inline(never)]
    pub(crate) fn eval_elem_init_exprs(
        &mut self,
        exprs: &[Vec<ConstExpr>],
        globals: &[GcRef],
        funcs: &[GcRef],
        reftype: RefType,
    ) -> VMResult<Vec<u32>> {
        let mut values = Vec::with_capacity(exprs.len());
        for expr in exprs {
            let res = vm_try!(execute_elem_init_const_expr(
                self.gc_mut(),
                globals,
                funcs,
                expr,
                reftype,
            ));
            values.push(res.get());
        }
        VMResult::Success(values)
    }

    #[inline(always)]
    pub(crate) fn instance_globals_snapshot(&self) -> Vec<GcRef> {
        self.instance().globals.as_slice().to_vec()
    }

    #[inline(always)]
    pub(crate) fn instance_funcs_snapshot(&self) -> Vec<GcRef> {
        self.instance().funcs.as_slice().to_vec()
    }

    #[inline(always)]
    pub(crate) fn instance_func_addrs_snapshot(&self) -> Vec<u32> {
        self.instance()
            .funcs
            .as_slice()
            .iter()
            .map(|func| func.get())
            .collect()
    }

    #[inline(always)]
    pub(crate) fn elem_init(&self, elem_idx: u32) -> Option<ElemInit> {
        self.store_ref()
            .lock_segments()
            .elems
            .get(&(self.instance_id(), elem_idx))
            .map(|elem| elem.init.clone())
    }

    #[inline(always)]
    pub(crate) fn drop_elem_segment(&self, elem_idx: u32) {
        let _ = self
            .store_ref()
            .lock_segments()
            .elems
            .remove(&(self.instance_id(), elem_idx));
    }

    #[inline(always)]
    pub(crate) fn data_segment_bytes(
        &self,
        idx: u32,
        src: u32,
        len: u32,
    ) -> VMResult<Option<Vec<u8>>> {
        let copied = {
            let segments = self.store_ref().lock_segments();
            let data = segments.data.get(&(self.instance_id(), idx));
            if data.is_none() && len == 0 && src == 0 {
                None
            } else {
                let data = vm_try!(VMResult::from_option(data, || {
                    VMResult::MemoryIndexOutOfRange
                }));
                let src_last = vm_try!(VMResult::from_option(src.checked_add(len), || {
                    VMResult::MemoryIndexOutOfRange
                })) as usize;
                let data = vm_try!(VMResult::from_option(
                    data.init.get(src as usize..src_last),
                    || { VMResult::MemoryIndexOutOfRange }
                ));
                Some(data.to_vec())
            }
        };
        VMResult::Success(copied)
    }

    #[inline(always)]
    pub(crate) fn drop_data_segment(&self, idx: u32) {
        let _ = self
            .store_ref()
            .lock_segments()
            .data
            .remove(&(self.instance_id(), idx));
    }

    #[inline(always)]
    pub(crate) fn write_local_memory_bytes_by_id(
        &mut self,
        memory: store::LocalMemoryId,
        dst: u32,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.gc_mut().local_write_bytes(memory, dst as usize, bytes)
    }

    #[inline(always)]
    pub(crate) fn write_shared_memory_bytes_by_id(
        &mut self,
        memory: store::SharedMemoryId,
        dst: u32,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.gc_mut()
            .shared_write_bytes(memory, dst as usize, bytes)
    }

    #[inline(always)]
    pub(crate) fn copy_local_memory_by_id(
        &mut self,
        memory: store::LocalMemoryId,
        dst: u32,
        src: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc_mut().local_copy_memory(memory, dst, src, len)
    }

    #[inline(always)]
    pub(crate) fn copy_shared_memory_by_id(
        &mut self,
        memory: store::SharedMemoryId,
        dst: u32,
        src: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc_mut().shared_copy_memory(memory, dst, src, len)
    }

    #[inline(always)]
    pub(crate) fn fill_local_memory_by_id(
        &mut self,
        memory: store::LocalMemoryId,
        ptr: u32,
        len: u32,
        data: u32,
    ) -> VMResult<()> {
        self.gc_mut().local_fill_memory(memory, ptr, len, data)
    }

    #[inline(always)]
    pub(crate) fn fill_shared_memory_by_id(
        &mut self,
        memory: store::SharedMemoryId,
        ptr: u32,
        len: u32,
        data: u32,
    ) -> VMResult<()> {
        self.gc_mut().shared_fill_memory(memory, ptr, len, data)
    }

    #[inline(always)]
    pub(crate) fn copy_memory_local_to_local_by_id(
        &mut self,
        dst: store::LocalMemoryId,
        src: store::LocalMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc_mut()
            .copy_memory_local_to_local(dst, src, dst_offset, src_offset, len)
    }

    #[inline(always)]
    pub(crate) fn copy_memory_shared_to_local_by_id(
        &mut self,
        dst: store::LocalMemoryId,
        src: store::SharedMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc_mut()
            .copy_memory_shared_to_local(dst, src, dst_offset, src_offset, len)
    }

    #[inline(always)]
    pub(crate) fn copy_memory_local_to_shared_by_id(
        &mut self,
        dst: store::SharedMemoryId,
        src: store::LocalMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc_mut()
            .copy_memory_local_to_shared(dst, src, dst_offset, src_offset, len)
    }

    #[inline(always)]
    pub(crate) fn copy_memory_shared_to_shared_by_id(
        &mut self,
        dst: store::SharedMemoryId,
        src: store::SharedMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc_mut()
            .copy_memory_shared_to_shared(dst, src, dst_offset, src_offset, len)
    }

    #[inline(always)]
    pub(crate) fn jump_target(&self, addr: u32) -> *const Instr {
        unsafe { self.code().offset(addr as isize) }
    }
}

impl ExecuteContextSnapshot {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn has_default_memory(self) -> bool {
        self.default_memory.is_some()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn projection(
        self,
        stack: &Stack,
        runtime: &StoreInner,
    ) -> ExecuteContextProjection {
        let current_frame = local_reference_has_call_stack_info(self.active_local)
            .then(|| stack.frame_projection(&self.active_local, runtime));
        let caller_frame = self.caller_local.and_then(|reference| {
            local_reference_has_call_stack_info(reference)
                .then(|| stack.frame_projection(&reference, runtime))
        });
        let default_memory = current_frame
            .as_ref()
            .map(|frame| frame.default_memory)
            .unwrap_or_else(|| MemoryHandleProjection::from_handle(self.default_memory));
        let caller_memory = caller_frame
            .as_ref()
            .map(|frame| frame.default_memory)
            .unwrap_or_else(|| MemoryHandleProjection::from_handle(self.caller_memory));
        ExecuteContextProjection {
            default_memory,
            caller_memory,
            cont_addr: self.cont_addr,
            task_id: self.task_id,
            current_frame,
            caller_frame,
            active_local: self.active_local,
            caller_local: self.caller_local,
        }
    }
}

impl ExecuteContextProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    fn proof_ready(&self) -> Option<ProofReadyExecuteContextProjection> {
        let current_frame = self.current_frame.clone()?;
        if current_frame.local_ref != self.active_local {
            return None;
        }
        if self.default_memory != current_frame.default_memory {
            return None;
        }
        let expected_caller_memory = self
            .caller_frame
            .as_ref()
            .map(|frame| frame.default_memory)
            .unwrap_or_else(|| MemoryHandleProjection::from_handle(None));
        if self.caller_memory != expected_caller_memory {
            return None;
        }

        match (&self.caller_frame, self.caller_local) {
            (Some(caller_frame), Some(caller_local))
                if caller_frame.local_ref == caller_local
                    && current_frame.prev_local == caller_frame.local_ref => {}
            (None, None)
                if current_frame.prev_local.local_size == 0
                    && current_frame.prev_local.local_top == 0 => {}
            _ => return None,
        }

        Some(ProofReadyExecuteContextProjection {
            current_frame,
            caller_frame: self.caller_frame.clone(),
            cont_addr: self.cont_addr,
            task_id: self.task_id,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn token_projection(&self) -> Option<ExecContextTokenProjection> {
        Some(self.proof_ready()?.token_projection())
    }
}

impl ProofReadyExecuteContextProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    fn token_projection(&self) -> ExecContextTokenProjection {
        ExecContextTokenProjection {
            current_frame: self.current_frame.clone(),
            caller_frame: self.caller_frame.clone(),
            cont_addr: self.cont_addr,
            task_id: self.task_id,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn formal_builder_parts(&self) -> ExecContextTokenProjectionParts {
        let current = self.current_frame.formal_builder_parts();
        let caller = self
            .caller_frame
            .as_ref()
            .map(FrameProjection::formal_builder_parts);
        ExecContextTokenProjectionParts {
            current_return_pc: current.return_pc,
            current_instance_raw: current.instance_raw,
            current_default_memory_present: current.default_memory_present,
            current_default_memory_shared: current.default_memory_shared,
            current_default_memory_raw: current.default_memory_raw,
            current_prev_local_top: current.prev_local_top,
            current_prev_local_size: current.prev_local_size,
            caller_present: caller.is_some(),
            caller_return_pc: caller.map(|frame| frame.return_pc).unwrap_or(0),
            caller_instance_raw: caller.map(|frame| frame.instance_raw).unwrap_or(0),
            caller_default_memory_present: caller
                .map(|frame| frame.default_memory_present)
                .unwrap_or(false),
            caller_default_memory_shared: caller
                .map(|frame| frame.default_memory_shared)
                .unwrap_or(false),
            caller_default_memory_raw: caller.map(|frame| frame.default_memory_raw).unwrap_or(0),
            caller_prev_local_top: caller.map(|frame| frame.prev_local_top).unwrap_or(0),
            caller_prev_local_size: caller.map(|frame| frame.prev_local_size).unwrap_or(0),
            cont_addr: self.cont_addr,
            task_id: self.task_id,
        }
    }
}

impl ExecContextTokenProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    fn formal_builder_parts(&self) -> ExecContextTokenProjectionParts {
        let current = self.current_frame.formal_builder_parts();
        let caller = self
            .caller_frame
            .as_ref()
            .map(FrameProjection::formal_builder_parts);
        ExecContextTokenProjectionParts {
            current_return_pc: current.return_pc,
            current_instance_raw: current.instance_raw,
            current_default_memory_present: current.default_memory_present,
            current_default_memory_shared: current.default_memory_shared,
            current_default_memory_raw: current.default_memory_raw,
            current_prev_local_top: current.prev_local_top,
            current_prev_local_size: current.prev_local_size,
            caller_present: caller.is_some(),
            caller_return_pc: caller.map(|frame| frame.return_pc).unwrap_or(0),
            caller_instance_raw: caller.map(|frame| frame.instance_raw).unwrap_or(0),
            caller_default_memory_present: caller
                .map(|frame| frame.default_memory_present)
                .unwrap_or(false),
            caller_default_memory_shared: caller
                .map(|frame| frame.default_memory_shared)
                .unwrap_or(false),
            caller_default_memory_raw: caller.map(|frame| frame.default_memory_raw).unwrap_or(0),
            caller_prev_local_top: caller.map(|frame| frame.prev_local_top).unwrap_or(0),
            caller_prev_local_size: caller.map(|frame| frame.prev_local_size).unwrap_or(0),
            cont_addr: self.cont_addr,
            task_id: self.task_id,
        }
    }
}

macro_rules! define_execute_context_atomic_load {
    ($default_local:ident, $default_shared:ident, $indexed_local:ident, $indexed_shared:ident, $store_local:ident, $store_shared:ident, $ty:ty) => {
        #[inline(always)]
        pub(crate) fn $default_local(&mut self, offset: usize) -> VMResult<$ty> {
            self.gc
                .$store_local(unsafe { self.default_local_memory_id_unchecked() }, offset)
        }

        #[inline(always)]
        pub(crate) fn $default_shared(&mut self, offset: usize) -> VMResult<$ty> {
            self.gc
                .$store_shared(unsafe { self.default_shared_memory_id_unchecked() }, offset)
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_local(
            &mut self,
            memidx: u32,
            offset: usize,
        ) -> VMResult<$ty> {
            self.gc
                .$store_local(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_shared(
            &mut self,
            memidx: u32,
            offset: usize,
        ) -> VMResult<$ty> {
            self.gc.$store_shared(
                unsafe { self.shared_memory_id_at_unchecked(memidx) },
                offset,
            )
        }
    };
}

macro_rules! define_execute_context_atomic_store {
    ($default_local:ident, $default_shared:ident, $indexed_local:ident, $indexed_shared:ident, $store_local:ident, $store_shared:ident, $ty:ty) => {
        #[inline(always)]
        pub(crate) fn $default_local(&mut self, offset: usize, value: $ty) -> VMResult<()> {
            self.gc.$store_local(
                unsafe { self.default_local_memory_id_unchecked() },
                offset,
                value,
            )
        }

        #[inline(always)]
        pub(crate) fn $default_shared(&mut self, offset: usize, value: $ty) -> VMResult<()> {
            self.gc.$store_shared(
                unsafe { self.default_shared_memory_id_unchecked() },
                offset,
                value,
            )
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_local(
            &mut self,
            memidx: u32,
            offset: usize,
            value: $ty,
        ) -> VMResult<()> {
            self.gc.$store_local(
                unsafe { self.local_memory_id_at_unchecked(memidx) },
                offset,
                value,
            )
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_shared(
            &mut self,
            memidx: u32,
            offset: usize,
            value: $ty,
        ) -> VMResult<()> {
            self.gc.$store_shared(
                unsafe { self.shared_memory_id_at_unchecked(memidx) },
                offset,
                value,
            )
        }
    };
}

macro_rules! define_execute_context_atomic_rmw {
    ($default_local:ident, $default_shared:ident, $indexed_local:ident, $indexed_shared:ident, $store_local:ident, $store_shared:ident, $ty:ty) => {
        #[inline(always)]
        pub(crate) fn $default_local(
            &mut self,
            offset: usize,
            op: AtomicRmwOp,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_local(
                unsafe { self.default_local_memory_id_unchecked() },
                offset,
                op,
                value,
            )
        }

        #[inline(always)]
        pub(crate) fn $default_shared(
            &mut self,
            offset: usize,
            op: AtomicRmwOp,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_shared(
                unsafe { self.default_shared_memory_id_unchecked() },
                offset,
                op,
                value,
            )
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_local(
            &mut self,
            memidx: u32,
            offset: usize,
            op: AtomicRmwOp,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_local(
                unsafe { self.local_memory_id_at_unchecked(memidx) },
                offset,
                op,
                value,
            )
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_shared(
            &mut self,
            memidx: u32,
            offset: usize,
            op: AtomicRmwOp,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_shared(
                unsafe { self.shared_memory_id_at_unchecked(memidx) },
                offset,
                op,
                value,
            )
        }
    };
}

macro_rules! define_execute_context_atomic_cmpxchg {
    ($default_local:ident, $default_shared:ident, $indexed_local:ident, $indexed_shared:ident, $store_local:ident, $store_shared:ident, $ty:ty) => {
        #[inline(always)]
        pub(crate) fn $default_local(
            &mut self,
            offset: usize,
            expected: $ty,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_local(
                unsafe { self.default_local_memory_id_unchecked() },
                offset,
                expected,
                value,
            )
        }

        #[inline(always)]
        pub(crate) fn $default_shared(
            &mut self,
            offset: usize,
            expected: $ty,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_shared(
                unsafe { self.default_shared_memory_id_unchecked() },
                offset,
                expected,
                value,
            )
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_local(
            &mut self,
            memidx: u32,
            offset: usize,
            expected: $ty,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_local(
                unsafe { self.local_memory_id_at_unchecked(memidx) },
                offset,
                expected,
                value,
            )
        }

        #[inline(always)]
        pub(crate) unsafe fn $indexed_shared(
            &mut self,
            memidx: u32,
            offset: usize,
            expected: $ty,
            value: $ty,
        ) -> VMResult<$ty> {
            self.gc.$store_shared(
                unsafe { self.shared_memory_id_at_unchecked(memidx) },
                offset,
                expected,
                value,
            )
        }
    };
}

impl<'a> ExecuteContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        stack: &'a mut Stack,
        local_reference: LocalReference,
        current_frame: CallFrameCache,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending: PendingOpEmitter<'a>,
        cont: *const Instr,
        task_id: u32,
    ) -> ExecuteContext<'a> {
        let current_frame = if local_reference_has_call_stack_info(local_reference) {
            let canonical = stack.frame_cache(&local_reference);
            #[cfg(not(test))]
            debug_assert!(
                current_frame.subset_matches(canonical),
                "ExecuteContext::new received stale frame cache for a real stack frame",
            );
            canonical
        } else {
            current_frame
        };
        ExecuteContext {
            stack,
            local_reference,
            current_frame,
            store,
            gc,
            pending,
            cont,
            task_id,
        }
    }

    pub(crate) fn snapshot(&self) -> ExecuteContextSnapshot {
        let default_memory = self.current_frame.memory0_handle();
        let caller_local = self.caller_local_reference();
        let caller_frame = caller_local.map(|reference| self.stack.frame_cache(&reference));
        let caller_memory = caller_frame.and_then(|frame| frame.memory0_handle());
        ExecuteContextSnapshot {
            default_memory,
            caller_memory,
            cont_addr: self.cont as usize,
            task_id: self.task_id,
            current_frame: self.current_frame,
            caller_frame,
            active_local: self.local_reference,
            caller_local,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn projection(&self) -> ExecuteContextProjection {
        self.snapshot().projection(self.stack, self.gc)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn function_projection_parts(&self, funcaddr: GcRef) -> FunctionProjectionParts {
        let func = self.gc.get_func(funcaddr);
        let instance = self.gc.instance(func.instance);
        let default_memory = MemoryHandleProjection::from_handle(
            instance
                .memory_slots
                .first()
                .copied()
                .and_then(|slot| slot.handle()),
        );
        FunctionProjectionParts {
            instance_raw: func.instance.raw(),
            // Formal call semantics treat wasm entry as the first instruction in the body.
            entry_pc: 0,
            param_size: func.param_size,
            local_size: func.local_size,
            type_id: func.typeidx.0,
            default_memory_present: default_memory.present,
            default_memory_shared: default_memory.shared,
            default_memory_raw: default_memory.raw,
            is_host: func.is_host_func(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn core_step_state_projection_parts(
        &mut self,
    ) -> Option<CoreStepStateProjectionParts> {
        let stack_projection = self.stack.projection(&self.local_reference, self.gc);
        if !stack_projection.proof_ready() {
            return None;
        }
        let context_projection = self.projection().token_projection()?;

        let table_addrs = self.instance().tables.as_slice().to_vec();
        let global_addrs = self.instance().globals.as_slice().to_vec();
        let func_addrs = self.instance().funcs.as_slice().to_vec();
        let memory_slots = self.instance().memory_slots.clone();

        let tables = table_addrs
            .into_iter()
            .map(|addr| {
                (
                    addr.get(),
                    self.gc.table_projection(addr).formal_builder_parts(),
                )
            })
            .collect();
        let globals = global_addrs
            .into_iter()
            .map(|addr| (addr.get(), self.gc.global_projection(addr).bytes))
            .collect();
        let functions = func_addrs
            .into_iter()
            .map(|addr| (addr.get(), self.function_projection_parts(addr)))
            .collect();

        let mut local_memories = Vec::new();
        let mut shared_memories = Vec::new();
        for slot in memory_slots {
            match slot.handle() {
                Some(MemoryHandle::Local(id))
                    if !local_memories.iter().any(|(raw, _)| *raw == id.raw()) =>
                {
                    local_memories.push((
                        id.raw(),
                        self.gc.local_memory(id).projection().formal_builder_parts(),
                    ));
                }
                Some(MemoryHandle::Shared(id))
                    if !shared_memories.iter().any(|(raw, _)| *raw == id.raw()) =>
                {
                    shared_memories.push((
                        id.raw(),
                        self.gc
                            .shared_memory(id)
                            .projection()
                            .formal_builder_parts(),
                    ));
                }
                _ => {}
            }
        }
        local_memories.sort_by_key(|(raw, _)| *raw);
        shared_memories.sort_by_key(|(raw, _)| *raw);

        let instance_id = self.instance_id();
        let segments = self.store.lock_segments();
        let mut data_segments = segments
            .data
            .iter()
            .filter(|((owner, _), _)| *owner == instance_id)
            .map(|((_, idx), data)| (*idx, data.init.clone()))
            .collect::<Vec<_>>();
        let mut elem_segments = segments
            .elems
            .iter()
            .filter(|((owner, _), _)| *owner == instance_id)
            .map(|((_, idx), elem)| {
                let init = match &elem.init {
                    ElemInit::FuncIdx(values) => values.clone(),
                    ElemInit::ConstExpr(_) => Vec::new(),
                };
                (*idx, init)
            })
            .collect::<Vec<_>>();
        drop(segments);
        data_segments.sort_by_key(|(idx, _)| *idx);
        elem_segments.sort_by_key(|(idx, _)| *idx);

        Some(CoreStepStateProjectionParts {
            stack: stack_projection.formal_builder_parts(),
            context: context_projection.formal_builder_parts(),
            tables,
            globals,
            functions,
            local_memories,
            shared_memories,
            data_segments,
            elem_segments,
            frame_metadata_len: stack::call_stack_metadata_len(),
        })
    }

    pub(crate) fn stack_ref(&self) -> &Stack {
        self.stack
    }

    pub(crate) fn stack_mut(&mut self) -> &mut Stack {
        self.stack
    }

    pub(crate) fn store_ref(&self) -> &Store {
        self.store
    }

    pub(crate) fn gc_ref(&self) -> &StoreInner {
        self.gc
    }

    pub(crate) fn gc_mut(&mut self) -> &mut StoreInner {
        self.gc
    }

    pub(crate) fn pending_mut(&mut self) -> &mut PendingOpEmitter<'a> {
        &mut self.pending
    }

    pub(crate) fn cont(&self) -> *const Instr {
        self.cont
    }

    pub(crate) fn set_cont(&mut self, cont: *const Instr) {
        self.cont = cont;
    }

    pub(crate) fn clear_cont(&mut self) {
        self.cont = std::ptr::null();
    }

    pub(crate) fn task_id(&self) -> u32 {
        self.task_id
    }

    pub fn set_local_reference(&mut self, local_reference: LocalReference) {
        self.local_reference = local_reference;
        if local_reference_has_call_stack_info(local_reference) {
            self.current_frame = self.stack.frame_cache(&local_reference);
        }
    }

    #[inline(always)]
    fn caller_frame_cache(&self) -> Option<CallFrameCache> {
        let caller = self.caller_local_reference()?;
        Some(self.stack.frame_cache(&caller))
    }

    pub fn func(&self) -> &FunctionInstanceData {
        self.gc.get_func(self.current_frame.code_addr)
    }
    pub fn func_by_addr(&self, addr: GcRef) -> &FunctionInstanceData {
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
    pub fn instance_addr(&self) -> GcRef {
        self.gc.gc_ref_for_instance(self.current_frame.instance)
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

    pub(crate) fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.stack.pop_u8_array::<N>()
    }

    pub(crate) fn push_slice(&mut self, value: &[u8]) -> VMResult<()> {
        self.stack.push_slice(value)
    }

    pub(crate) fn local_get(&mut self, local_addr: usize, size: usize) -> VMResult<()> {
        self.stack
            .local_get(&self.local_reference(), local_addr, size)
    }

    pub(crate) fn local_set(&mut self, local_addr: usize, size: usize) {
        self.stack
            .local_set(&self.local_reference(), local_addr, size);
    }

    pub(crate) fn local_tee(&mut self, local_addr: usize, size: usize) {
        self.stack
            .local_tee(&self.local_reference(), local_addr, size);
    }

    pub(crate) fn enter_function_call<F: IntoCallFrameCache>(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: F,
        return_addr: *const Instr,
    ) -> VMResult<()> {
        let local_reference = vm_try!(self.stack.function_call(
            param_size,
            local_size,
            frame,
            self.local_reference,
            return_addr,
            self.gc,
        ));
        self.set_local_reference(local_reference);
        VMResult::Success(())
    }

    pub(crate) fn enter_function_return_call<F: IntoCallFrameCache>(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: F,
    ) -> VMResult<()> {
        let frame = frame.into_call_frame_cache(self.gc);
        let local_reference = vm_try!(self.stack.function_return_call(
            &self.local_reference,
            param_size,
            local_size,
            frame,
        ));
        self.set_local_reference(local_reference);
        VMResult::Success(())
    }

    pub(crate) fn function_return_in_place(&mut self, return_size: usize) -> (*const Instr, usize) {
        let result_slot = self.local_reference.local_top;
        let (prev_local_reference, return_addr) =
            self.stack
                .function_return_in_place(&self.local_reference, return_size, self.gc);
        self.set_local_reference(prev_local_reference);
        (return_addr, result_slot)
    }

    pub(crate) fn function_return(&mut self, return_size: usize) -> *const Instr {
        let (prev_local_reference, return_addr) =
            self.stack
                .function_return(&self.local_reference, return_size, self.gc);
        self.set_local_reference(prev_local_reference);
        return_addr
    }

    pub(crate) fn block_return(&mut self, stack_top: usize, return_size: usize) {
        self.stack
            .block_return(&self.local_reference(), stack_top, return_size);
    }

    pub(crate) fn end_program(&mut self) {
        let mut local_reference = self.local_reference;
        while local_reference.local_size != 0 {
            let (prev_local_ref, _return_addr) =
                self.stack
                    .function_return_in_place(&local_reference, 0, self.gc);
            local_reference = prev_local_ref;
        }
        self.set_local_reference(local_reference);
        self.clear_cont();
    }
    pub fn memory_addr(&self) -> Option<MemoryHandle> {
        self.current_frame.memory0_handle()
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
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self.gc.local_read_u8_array::<N>(
                unsafe { self.default_local_memory_id_unchecked() },
                offset,
            ),
            CachedMemoryKind::Shared => self.gc.shared_read_u8_array::<N>(
                unsafe { self.default_shared_memory_id_unchecked() },
                offset,
            ),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn push_memory_to_stack<const N: usize>(&mut self, offset: usize) -> VMResult<()> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self.gc.local_push_memory_to_stack::<N>(
                unsafe { self.default_local_memory_id_unchecked() },
                self.stack,
                offset,
            ),
            CachedMemoryKind::Shared => self.gc.shared_push_memory_to_stack::<N>(
                unsafe { self.default_shared_memory_id_unchecked() },
                self.stack,
                offset,
            ),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_u8(&mut self, offset: usize) -> VMResult<u8> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_u8_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_u8_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_i8(&mut self, offset: usize) -> VMResult<i8> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_i8_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_i8_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_u16(&mut self, offset: usize) -> VMResult<u16> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_u16_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_u16_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_i16(&mut self, offset: usize) -> VMResult<i16> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_i16_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_i16_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_u32(&mut self, offset: usize) -> VMResult<u32> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_u32_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_u32_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_i32(&mut self, offset: usize) -> VMResult<i32> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_i32_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_i32_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_u64(&mut self, offset: usize) -> VMResult<u64> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_u64_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_u64_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_i64(&mut self, offset: usize) -> VMResult<i64> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_i64_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_i64_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_f32(&mut self, offset: usize) -> VMResult<f32> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_f32_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_f32_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn read_memory_f64(&mut self, offset: usize) -> VMResult<f64> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self
                .gc
                .local_read_f64_at(unsafe { self.default_local_memory_id_unchecked() }, offset),
            CachedMemoryKind::Shared => self
                .gc
                .shared_read_f64_at(unsafe { self.default_shared_memory_id_unchecked() }, offset),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn write_memory_bytes(&mut self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self.gc.local_write_bytes(
                unsafe { self.default_local_memory_id_unchecked() },
                offset,
                bytes,
            ),
            CachedMemoryKind::Shared => self.gc.shared_write_bytes(
                unsafe { self.default_shared_memory_id_unchecked() },
                offset,
                bytes,
            ),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn push_memory_to_stack_local_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<()> {
        self.gc.local_push_memory_to_stack::<N>(
            unsafe { self.local_memory_id_at_unchecked(memidx) },
            self.stack,
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn push_memory_to_stack_shared_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<()> {
        self.gc.shared_push_memory_to_stack::<N>(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            self.stack,
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_array_local_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        self.gc
            .local_read_u8_array::<N>(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_array_shared_indexed<const N: usize>(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        self.gc.shared_read_u8_array::<N>(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u8> {
        self.gc
            .local_read_u8_at(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u8_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u8> {
        self.gc.shared_read_u8_at(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i8_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i8> {
        self.gc
            .local_read_i8_at(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i8_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i8> {
        self.gc.shared_read_i8_at(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u16_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u16> {
        self.gc
            .local_read_u16_at(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u16_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u16> {
        self.gc.shared_read_u16_at(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i16_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i16> {
        self.gc
            .local_read_i16_at(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i16_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i16> {
        self.gc.shared_read_i16_at(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u32_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u32> {
        self.gc
            .local_read_u32_at(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_u32_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<u32> {
        self.gc.shared_read_u32_at(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i32_at_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i32> {
        self.gc
            .local_read_i32_at(unsafe { self.local_memory_id_at_unchecked(memidx) }, offset)
    }

    #[inline(always)]
    pub(crate) unsafe fn read_i32_at_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
    ) -> VMResult<i32> {
        self.gc.shared_read_i32_at(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn write_memory_bytes_local_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.gc.local_write_bytes(
            unsafe { self.local_memory_id_at_unchecked(memidx) },
            offset,
            bytes,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn write_memory_bytes_shared_indexed(
        &mut self,
        memidx: u32,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.gc.shared_write_bytes(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            offset,
            bytes,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn grow_memory_local_indexed(
        &mut self,
        memidx: u32,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        self.gc.local_grow_memory(
            unsafe { self.local_memory_id_at_unchecked(memidx) },
            page_size_delta,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn grow_memory_shared_indexed(
        &mut self,
        memidx: u32,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        self.gc.shared_grow_memory(
            unsafe { self.shared_memory_id_at_unchecked(memidx) },
            page_size_delta,
        )
    }

    #[inline(always)]
    pub(crate) unsafe fn memory_page_size_local_indexed(&self, memidx: u32) -> u32 {
        self.gc
            .local_memory(unsafe { self.local_memory_id_at_unchecked(memidx) })
            .page_size()
    }

    #[inline(always)]
    pub(crate) unsafe fn memory_page_size_shared_indexed(&self, memidx: u32) -> u32 {
        self.gc
            .shared_memory(unsafe { self.shared_memory_id_at_unchecked(memidx) })
            .page_size()
    }

    define_execute_context_atomic_load!(
        local_atomic_load_u8,
        shared_atomic_load_u8,
        indexed_local_atomic_load_u8,
        indexed_shared_atomic_load_u8,
        local_atomic_load_u8,
        shared_atomic_load_u8,
        u8
    );
    define_execute_context_atomic_load!(
        local_atomic_load_u16,
        shared_atomic_load_u16,
        indexed_local_atomic_load_u16,
        indexed_shared_atomic_load_u16,
        local_atomic_load_u16,
        shared_atomic_load_u16,
        u16
    );
    define_execute_context_atomic_load!(
        local_atomic_load_u32,
        shared_atomic_load_u32,
        indexed_local_atomic_load_u32,
        indexed_shared_atomic_load_u32,
        local_atomic_load_u32,
        shared_atomic_load_u32,
        u32
    );
    define_execute_context_atomic_load!(
        local_atomic_load_u64,
        shared_atomic_load_u64,
        indexed_local_atomic_load_u64,
        indexed_shared_atomic_load_u64,
        local_atomic_load_u64,
        shared_atomic_load_u64,
        u64
    );

    define_execute_context_atomic_store!(
        local_atomic_store_u8,
        shared_atomic_store_u8,
        indexed_local_atomic_store_u8,
        indexed_shared_atomic_store_u8,
        local_atomic_store_u8,
        shared_atomic_store_u8,
        u8
    );
    define_execute_context_atomic_store!(
        local_atomic_store_u16,
        shared_atomic_store_u16,
        indexed_local_atomic_store_u16,
        indexed_shared_atomic_store_u16,
        local_atomic_store_u16,
        shared_atomic_store_u16,
        u16
    );
    define_execute_context_atomic_store!(
        local_atomic_store_u32,
        shared_atomic_store_u32,
        indexed_local_atomic_store_u32,
        indexed_shared_atomic_store_u32,
        local_atomic_store_u32,
        shared_atomic_store_u32,
        u32
    );
    define_execute_context_atomic_store!(
        local_atomic_store_u64,
        shared_atomic_store_u64,
        indexed_local_atomic_store_u64,
        indexed_shared_atomic_store_u64,
        local_atomic_store_u64,
        shared_atomic_store_u64,
        u64
    );

    define_execute_context_atomic_rmw!(
        local_atomic_rmw_u8,
        shared_atomic_rmw_u8,
        indexed_local_atomic_rmw_u8,
        indexed_shared_atomic_rmw_u8,
        local_atomic_rmw_u8,
        shared_atomic_rmw_u8,
        u8
    );
    define_execute_context_atomic_rmw!(
        local_atomic_rmw_u16,
        shared_atomic_rmw_u16,
        indexed_local_atomic_rmw_u16,
        indexed_shared_atomic_rmw_u16,
        local_atomic_rmw_u16,
        shared_atomic_rmw_u16,
        u16
    );
    define_execute_context_atomic_rmw!(
        local_atomic_rmw_u32,
        shared_atomic_rmw_u32,
        indexed_local_atomic_rmw_u32,
        indexed_shared_atomic_rmw_u32,
        local_atomic_rmw_u32,
        shared_atomic_rmw_u32,
        u32
    );
    define_execute_context_atomic_rmw!(
        local_atomic_rmw_u64,
        shared_atomic_rmw_u64,
        indexed_local_atomic_rmw_u64,
        indexed_shared_atomic_rmw_u64,
        local_atomic_rmw_u64,
        shared_atomic_rmw_u64,
        u64
    );

    define_execute_context_atomic_cmpxchg!(
        local_atomic_cmpxchg_u8,
        shared_atomic_cmpxchg_u8,
        indexed_local_atomic_cmpxchg_u8,
        indexed_shared_atomic_cmpxchg_u8,
        local_atomic_cmpxchg_u8,
        shared_atomic_cmpxchg_u8,
        u8
    );
    define_execute_context_atomic_cmpxchg!(
        local_atomic_cmpxchg_u16,
        shared_atomic_cmpxchg_u16,
        indexed_local_atomic_cmpxchg_u16,
        indexed_shared_atomic_cmpxchg_u16,
        local_atomic_cmpxchg_u16,
        shared_atomic_cmpxchg_u16,
        u16
    );
    define_execute_context_atomic_cmpxchg!(
        local_atomic_cmpxchg_u32,
        shared_atomic_cmpxchg_u32,
        indexed_local_atomic_cmpxchg_u32,
        indexed_shared_atomic_cmpxchg_u32,
        local_atomic_cmpxchg_u32,
        shared_atomic_cmpxchg_u32,
        u32
    );
    define_execute_context_atomic_cmpxchg!(
        local_atomic_cmpxchg_u64,
        shared_atomic_cmpxchg_u64,
        indexed_local_atomic_cmpxchg_u64,
        indexed_shared_atomic_cmpxchg_u64,
        local_atomic_cmpxchg_u64,
        shared_atomic_cmpxchg_u64,
        u64
    );

    #[inline(always)]
    pub(crate) fn local_atomic_fence(&mut self) {
        self.gc
            .local_atomic_fence(unsafe { self.default_local_memory_id_unchecked() });
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_fence(&mut self) {
        self.gc
            .shared_atomic_fence(unsafe { self.default_shared_memory_id_unchecked() });
    }

    #[inline(always)]
    pub(crate) fn clone_shared_memory(&self, id: store::SharedMemoryId) -> Arc<SharedMemoryObject> {
        self.gc.clone_shared_memory(id)
    }

    #[inline(always)]
    pub fn grow_memory(&mut self, page_size_delta: u32) -> VMResult<i32> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self.gc.local_grow_memory(
                unsafe { self.default_local_memory_id_unchecked() },
                page_size_delta,
            ),
            CachedMemoryKind::Shared => self.gc.shared_grow_memory(
                unsafe { self.default_shared_memory_id_unchecked() },
                page_size_delta,
            ),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn copy_memory(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self.gc.local_copy_memory(
                unsafe { self.default_local_memory_id_unchecked() },
                dst,
                src,
                len,
            ),
            CachedMemoryKind::Shared => self.gc.shared_copy_memory(
                unsafe { self.default_shared_memory_id_unchecked() },
                dst,
                src,
                len,
            ),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    #[inline(always)]
    pub fn fill_memory(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => self.gc.local_fill_memory(
                unsafe { self.default_local_memory_id_unchecked() },
                ptr,
                len,
                data,
            ),
            CachedMemoryKind::Shared => self.gc.shared_fill_memory(
                unsafe { self.default_shared_memory_id_unchecked() },
                ptr,
                len,
                data,
            ),
            CachedMemoryKind::None => VMResult::MemoryIndexOutOfRange,
        }
    }

    pub fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        let handle = self.current_frame.memory0_handle()?;
        let addr = self.gc.gc_ref_for_memory_handle(handle);
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
        let addr = self.gc.gc_ref_for_memory_handle(handle);
        Some(self.gc.with_memory_by_addr(addr, f))
    }
}

pub fn execute_elem_init_const_expr(
    runtime: &mut StoreInner,
    globals: &[GcRef],
    funcs: &[GcRef],
    exprs: &[ConstExpr],
    expected: RefType,
) -> VMResult<GcRef> {
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
            VMResult::Success(GcRef(0))
        }
        ConstExpr::RefNull(RefType::ExternRef) => {
            if expected != RefType::ExternRef {
                return VMResult::Unlinkable;
            }
            VMResult::Success(GcRef(0))
        }
        ConstExpr::GlobalGet(idx) => {
            let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                VMResult::Unlinkable
            }));
            let Ok(buf): Result<[u8; 4], _> = runtime.get_global(addr).try_into() else {
                return VMResult::Unlinkable;
            };
            VMResult::Success(GcRef(u32::from_le_bytes(buf)))
        }
        _ => VMResult::Unlinkable,
    }
}
pub const fn word_size<T>() -> usize {
    std::mem::size_of::<T>() / std::mem::size_of::<u32>()
}
#[derive(Debug)]
pub(crate) struct LocalReassignTable(pub(crate) Vec<(u32, ValType, u32)>);
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
    use crate::runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter};
    use std::collections::VecDeque;
    use std::sync::Arc;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance: store::InstanceId::from_index(0),
            memory0_kind: kind,
            memory0_raw: raw,
        }
    }

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

    #[test]
    fn execute_context_projection_canonicalizes_stale_tail_call_cache() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };

        let root = stack
            .function_call(
                0,
                0,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let callee = stack
            .function_call(
                0,
                0,
                frame(CachedMemoryKind::Shared, 2),
                root,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let tail = stack
            .function_return_call(&callee, 0, 0, frame(CachedMemoryKind::Local, 3))
            .unwrap();

        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops: VecDeque<PendingOp> = VecDeque::new();
        let program = [crate::runtime::vm::VM_END, crate::runtime::vm::VM_END];
        let mut ctx = ExecuteContext::new(
            &mut stack,
            callee,
            frame(CachedMemoryKind::Shared, 2),
            &store,
            &mut gc,
            PendingOpEmitter::from_parts(77, &mut pending_effects, &mut pending_ops),
            program.as_ptr(),
            77,
        );

        let before = ctx.snapshot();
        let before_projection = ctx.projection();
        let before_proof = before_projection.proof_ready().unwrap();
        let before_facade_projection = {
            let facade = ExecuteContextFacade::new(&mut ctx);
            facade.projection()
        };
        let before_facade_token = {
            let facade = ExecuteContextFacade::new(&mut ctx);
            facade.token_projection().unwrap()
        };
        let callee_local_top = callee.local_top;
        let callee_local_size = callee.local_size;
        let root_local_top = root.local_top;
        let root_local_size = root.local_size;
        assert_eq!(
            before.default_memory,
            Some(MemoryHandle::Local(store::LocalMemoryId::from_raw(3)))
        );
        assert!(!frame(CachedMemoryKind::Shared, 2).subset_matches(before.current_frame));
        assert_eq!(
            before.caller_memory,
            Some(MemoryHandle::Local(store::LocalMemoryId::from_raw(1)))
        );
        assert_eq!(ctx.memory_addr(), before.default_memory);
        assert_eq!(before.current_frame.memory0_handle(), before.default_memory);
        assert_eq!(
            before.caller_frame.unwrap().memory0_handle(),
            before.caller_memory
        );
        let before_active_local = before.active_local;
        let before_caller_local = before.caller_local.unwrap();
        let before_active_local_top = before_active_local.local_top;
        let before_active_local_size = before_active_local.local_size;
        let before_caller_local_top = before_caller_local.local_top;
        let before_caller_local_size = before_caller_local.local_size;
        assert_eq!(before_active_local_top, callee_local_top);
        assert_eq!(before_active_local_size, callee_local_size);
        assert_eq!(before_caller_local_top, root_local_top);
        assert_eq!(before_caller_local_size, root_local_size);
        assert!(before.has_default_memory());
        assert_eq!(before.cont_addr, program.as_ptr() as usize);
        assert_eq!(before.task_id, 77);
        assert_eq!(
            before_projection.default_memory,
            MemoryHandleProjection::from_handle(before.default_memory)
        );
        assert_eq!(
            before_projection.caller_memory,
            MemoryHandleProjection::from_handle(before.caller_memory)
        );
        assert_eq!(before_projection.cont_addr, before.cont_addr);
        assert_eq!(before_projection.task_id, before.task_id);
        assert_eq!(before_projection.active_local, callee);
        assert_eq!(before_projection.caller_local, Some(root));
        assert_eq!(
            before_projection.current_frame.as_ref().unwrap().local_ref,
            callee
        );
        assert_eq!(
            before_projection
                .current_frame
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            before_projection.caller_frame.as_ref().unwrap().local_ref,
            root
        );
        assert_eq!(
            before_projection
                .caller_frame
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        let before_token = before_projection.token_projection().unwrap();
        let before_parts = before_proof.formal_builder_parts();
        assert_eq!(before_facade_projection, before_projection);
        assert_eq!(before_facade_token, before_token);
        assert_eq!(before_parts.current_default_memory_raw, 3);
        assert!(!before_parts.current_default_memory_shared);
        assert!(before_parts.caller_present);
        assert_eq!(before_parts.caller_default_memory_raw, 1);
        assert_eq!(before_parts.cont_addr, before.cont_addr);
        assert_eq!(before_parts.task_id, before.task_id);
        assert_eq!(
            before_token.current_frame.default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            before_token.caller_frame.unwrap().default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        assert_eq!(before_token.cont_addr, before.cont_addr);
        assert_eq!(before_token.task_id, before.task_id);

        ctx.set_local_reference(tail);
        ctx.set_cont(unsafe { program.as_ptr().add(1) });

        let after = ctx.snapshot();
        let after_projection = ctx.projection();
        let after_proof = after_projection.proof_ready().unwrap();
        let after_facade_projection = {
            let facade = ExecuteContextFacade::new(&mut ctx);
            facade.projection()
        };
        let after_facade_token = {
            let facade = ExecuteContextFacade::new(&mut ctx);
            facade.token_projection().unwrap()
        };
        let tail_local_top = tail.local_top;
        let tail_local_size = tail.local_size;
        assert_eq!(
            after.default_memory,
            Some(MemoryHandle::Local(store::LocalMemoryId::from_raw(3)))
        );
        assert_eq!(
            after.caller_memory,
            Some(MemoryHandle::Local(store::LocalMemoryId::from_raw(1)))
        );
        assert_eq!(after.current_frame.memory0_handle(), after.default_memory);
        assert_eq!(
            after.caller_frame.unwrap().memory0_handle(),
            after.caller_memory
        );
        let after_active_local = after.active_local;
        let after_caller_local = after.caller_local.unwrap();
        let after_active_local_top = after_active_local.local_top;
        let after_active_local_size = after_active_local.local_size;
        let after_caller_local_top = after_caller_local.local_top;
        let after_caller_local_size = after_caller_local.local_size;
        assert_eq!(after_active_local_top, tail_local_top);
        assert_eq!(after_active_local_size, tail_local_size);
        assert_eq!(after_caller_local_top, root_local_top);
        assert_eq!(after_caller_local_size, root_local_size);
        assert_eq!(ctx.memory_addr(), after.default_memory);
        assert!(after.has_default_memory());
        assert_eq!(after.cont_addr, unsafe { program.as_ptr().add(1) } as usize);
        assert_eq!(after.task_id, 77);
        assert_eq!(
            after_projection.default_memory,
            MemoryHandleProjection::from_handle(after.default_memory)
        );
        assert_eq!(
            after_projection.caller_memory,
            MemoryHandleProjection::from_handle(after.caller_memory)
        );
        assert_eq!(after_projection.cont_addr, after.cont_addr);
        assert_eq!(after_projection.task_id, after.task_id);
        assert_eq!(
            after_projection.current_frame.as_ref().unwrap().local_ref,
            tail
        );
        assert_eq!(
            after_projection
                .current_frame
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            after_projection.caller_frame.as_ref().unwrap().local_ref,
            root
        );
        assert_eq!(
            after_projection
                .caller_frame
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        let after_token = after_projection.token_projection().unwrap();
        let after_parts = after_proof.formal_builder_parts();
        assert_eq!(after_facade_projection, after_projection);
        assert_eq!(after_facade_token, after_token);
        assert_eq!(after_parts.current_default_memory_raw, 3);
        assert_eq!(after_parts.caller_default_memory_raw, 1);
        assert_eq!(after_parts.cont_addr, after.cont_addr);
        assert_eq!(after_parts.task_id, after.task_id);
        assert_eq!(
            after_token.current_frame.default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            after_token.caller_frame.unwrap().default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        assert_eq!(after_token.cont_addr, after.cont_addr);
        assert_eq!(after_token.task_id, after.task_id);
    }

    #[test]
    fn execute_context_projection_rejects_synthetic_context_for_token_builder() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops: VecDeque<PendingOp> = VecDeque::new();
        let mut stack = Stack::new(32);
        let ctx = ExecuteContext::new(
            &mut stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            frame(CachedMemoryKind::Local, 7),
            &store,
            &mut gc,
            PendingOpEmitter::from_parts(11, &mut pending_effects, &mut pending_ops),
            std::ptr::null(),
            11,
        );

        let projection = ctx.projection();
        assert_eq!(
            projection.default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(7),
            )))
        );
        assert_eq!(projection.current_frame, None);
        assert_eq!(projection.caller_frame, None);
        assert!(projection.proof_ready().is_none());
        assert!(projection.token_projection().is_none());
    }

    #[test]
    fn facade_local_helpers_round_trip_locals_and_select() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops: VecDeque<PendingOp> = VecDeque::new();
        let mut stack = Stack::new(128);
        stack.push_u32(10).unwrap();
        stack.push_u32(20).unwrap();

        let locals = stack
            .function_call(
                8,
                0,
                frame(CachedMemoryKind::Local, 1),
                LocalReference {
                    local_top: 0,
                    local_size: 0,
                },
                std::ptr::null(),
                &gc,
            )
            .unwrap();

        let mut ctx = ExecuteContext::new(
            &mut stack,
            locals,
            frame(CachedMemoryKind::Local, 1),
            &store,
            &mut gc,
            PendingOpEmitter::from_parts(19, &mut pending_effects, &mut pending_ops),
            std::ptr::null(),
            19,
        );

        let mut facade = ExecuteContextFacade::new(&mut ctx);
        facade.local_get(0, 4).unwrap();
        assert_eq!(facade.pop_u32(), 10);

        facade.push_u32(33).unwrap();
        facade.local_set(0, 4);
        facade.local_get(0, 4).unwrap();
        assert_eq!(facade.pop_u32(), 33);

        facade.push_u32(44).unwrap();
        facade.local_tee(4, 4);
        assert_eq!(facade.pop_u32(), 44);
        facade.local_get(4, 4).unwrap();
        assert_eq!(facade.pop_u32(), 44);

        facade.push_u32(111).unwrap();
        facade.push_u32(222).unwrap();
        facade.select(4, 0).unwrap();
        assert_eq!(facade.pop_u32(), 222);

        assert!(facade.projection().token_projection().is_some());
        assert!(facade.token_projection().is_some());
    }

    #[test]
    fn facade_global_and_ref_helpers_match_instance_storage() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops: VecDeque<PendingOp> = VecDeque::new();
        let mut stack = Stack::new(64);

        let global = gc.new_global_data4(0x1122_3344);
        let instance = gc.alloc_instance(InstanceData {
            instance_id: 7,
            module_addr: GcRef(0),
            globals: vec![global],
            funcs: vec![GcRef(0x1234_5678)],
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });

        let mut ctx = ExecuteContext::new(
            &mut stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            CallFrameCache {
                code_addr: GcRef(0),
                code_base: std::ptr::null(),
                instance,
                memory0_kind: CachedMemoryKind::None,
                memory0_raw: 0,
            },
            &store,
            &mut gc,
            PendingOpEmitter::from_parts(23, &mut pending_effects, &mut pending_ops),
            std::ptr::null(),
            23,
        );

        let mut facade = ExecuteContextFacade::new(&mut ctx);
        facade.push_global_bytes::<4>(0).unwrap();
        assert_eq!(facade.pop_u32(), 0x1122_3344);

        facade.write_global_bytes(0, 0xaabb_ccdd_u32.to_le_bytes());
        facade.push_global_bytes::<4>(0).unwrap();
        assert_eq!(facade.pop_u32(), 0xaabb_ccdd);

        facade.push_ref(0x0102_0304).unwrap();
        assert_eq!(facade.pop_ref(), 0x0102_0304);
        assert_eq!(facade.ref_func_value(0), 0x1234_5678);

        assert_eq!(
            gc.global_projection(global).bytes,
            0xaabb_ccdd_u32.to_le_bytes().to_vec()
        );
    }

    #[test]
    fn core_step_state_projection_parts_capture_runtime_state() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops: VecDeque<PendingOp> = VecDeque::new();

        let local = match gc.alloc_local_memory(LocalMemoryObject::new(1, 2)) {
            MemoryHandle::Local(id) => id,
            MemoryHandle::Shared(_) => panic!("expected local memory"),
        };
        let shared = match gc.alloc_shared_memory(SharedMemoryObject::new(1, 2)) {
            MemoryHandle::Shared(id) => id,
            MemoryHandle::Local(_) => panic!("expected shared memory"),
        };
        gc.local_write_bytes(local, 0, &[1, 2, 3, 4]).unwrap();
        gc.shared_write_bytes(shared, 0, &[5, 6, 7, 8]).unwrap();

        let global = gc.new_global_data4(0x1122_3344);
        let table = gc.new_table(TableType {
            reftype: RefType::FuncRef,
            limits: Limits {
                min: 0,
                max: Some(2),
            },
        });
        let instance = store::InstanceId::from_index(0);
        let func = gc.new_func(&store::FunctionInstanceData {
            instance,
            funcidx: 0,
            typeidx: TypeIdx(0),
            param_size: 4,
            local_size: 8,
            body: store::FunctionBody::Wasm {
                locals: LocalsData::default(),
                code: Arc::<[Instr]>::from(vec![crate::runtime::vm::VM_END]),
            },
        });
        gc.get_table(table).1.push(func.get());

        let _instance_addr = gc.new_instance(&InstanceData {
            instance_id: 9,
            module_addr: GcRef(0),
            globals: vec![global],
            funcs: vec![func],
            tables: vec![table],
            mems: Vec::new(),
            memory_slots: vec![
                store::InstanceMemorySlot::Local(local),
                store::InstanceMemorySlot::Shared(shared),
            ],
        });

        {
            let mut segments = store.lock_segments();
            segments.data.insert(
                (9, 0),
                Data {
                    init: vec![9, 8, 7],
                    mode: DataMode::Passive,
                },
            );
            segments.elems.insert(
                (9, 0),
                Elem {
                    kind: RefType::FuncRef,
                    init: ElemInit::FuncIdx(vec![func.get()]),
                    mode: ElemMode::Passive,
                },
            );
        }

        let mut stack = Stack::new(256);
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let root = stack
            .function_call(
                0,
                0,
                CallFrameCache {
                    code_addr: func,
                    code_base: std::ptr::null(),
                    instance,
                    memory0_kind: CachedMemoryKind::Local,
                    memory0_raw: local.raw(),
                },
                empty,
                std::ptr::null(),
                &gc,
            )
            .unwrap();
        let callee = stack
            .function_call(
                0,
                0,
                CallFrameCache {
                    code_addr: func,
                    code_base: std::ptr::null(),
                    instance,
                    memory0_kind: CachedMemoryKind::Shared,
                    memory0_raw: shared.raw(),
                },
                root,
                std::ptr::null(),
                &gc,
            )
            .unwrap();

        let program = [crate::runtime::vm::VM_END, crate::runtime::vm::VM_END];
        let mut ctx = ExecuteContext::new(
            &mut stack,
            callee,
            CallFrameCache {
                code_addr: func,
                code_base: std::ptr::null(),
                instance,
                memory0_kind: CachedMemoryKind::Shared,
                memory0_raw: shared.raw(),
            },
            &store,
            &mut gc,
            PendingOpEmitter::from_parts(77, &mut pending_effects, &mut pending_ops),
            unsafe { program.as_ptr().add(1) },
            77,
        );

        let parts = ExecuteContextFacade::new(&mut ctx)
            .core_step_state_projection_parts()
            .unwrap();
        assert_eq!(parts.context.task_id, 77);
        assert_eq!(parts.context.cont_addr, unsafe { program.as_ptr().add(1) }
            as usize);
        assert_eq!(parts.functions.len(), 1);
        assert_eq!(parts.functions[0].0, func.get());
        assert_eq!(parts.functions[0].1.param_size, 4);
        assert_eq!(parts.functions[0].1.local_size, 8);
        assert!(!parts.functions[0].1.is_host);
        assert_eq!(parts.functions[0].1.default_memory_raw, local.raw());
        assert_eq!(parts.tables.len(), 1);
        assert_eq!(parts.tables[0].0, table.get());
        assert_eq!(parts.tables[0].1.elements, vec![func.get()]);
        assert_eq!(
            parts.globals,
            vec![(global.get(), 0x1122_3344_u32.to_le_bytes().to_vec())]
        );
        assert_eq!(parts.local_memories.len(), 1);
        assert_eq!(parts.local_memories[0].0, local.raw());
        assert_eq!(&parts.local_memories[0].1.bytes[0..4], &[1, 2, 3, 4]);
        assert_eq!(parts.shared_memories.len(), 1);
        assert_eq!(parts.shared_memories[0].0, shared.raw());
        assert_eq!(
            &parts.shared_memories[0].1.memory.bytes[0..4],
            &[5, 6, 7, 8]
        );
        assert_eq!(parts.data_segments, vec![(0, vec![9, 8, 7])]);
        assert_eq!(parts.elem_segments, vec![(0, vec![func.get()])]);
        assert_eq!(parts.frame_metadata_len, stack::call_stack_metadata_len());
    }
}
