#![allow(private_interfaces)]

#[macro_use]
mod vm_result;
use std::{fmt::Display, future::Future, pin::Pin, sync::Arc};

use custom_section::NameSubSection;

pub use vm_result::VMResult;
mod memory;
pub use memory::{AtomicRmwOp, LocalMemoryObject, MemArg, Memory, SharedMemoryObject};
pub use memory::{AtomicWaitResult, SharedWaitRegistration};
pub(crate) mod formal;
pub(crate) mod stack;
use stack::CachedMemoryKind;
use stack::IntoCallFrameCache;
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
    facade: ExecuteContextFacade<'ctx, 'store>,
    params: ResultValue,
    result_types: ResultType,
}

impl<'ctx, 'store> HostCallContext<'ctx, 'store> {
    pub(crate) fn new(
        ctx: &'ctx mut ExecuteContext<'store>,
        params: ResultValue,
        result_types: ResultType,
    ) -> Self {
        Self {
            facade: ExecuteContextFacade { ctx },
            params,
            result_types,
        }
    }

    pub fn params(&self) -> &ResultValue {
        &self.params
    }

    pub fn param(&self, index: usize) -> Option<&WasmValue> {
        self.params.0.get(index)
    }

    pub fn result_types(&self) -> &ResultType {
        &self.result_types
    }

    pub fn store(&self) -> &Store {
        self.facade.store_ref()
    }

    pub fn store_state(&self) -> StoreState {
        self.store().state
    }

    pub fn instance_id(&self) -> u32 {
        self.facade.instance_id()
    }

    pub fn func_idx(&self) -> u32 {
        self.facade.func_idx()
    }

    pub fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.facade.with_memory(f)
    }

    pub fn with_caller_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.facade.with_caller_memory(f)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct FrameCacheProjection {
    pub(crate) instance_raw: u32,
    pub(crate) default_memory: MemoryHandleProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ExecuteContextProjection {
    pub(crate) default_memory: MemoryHandleProjection,
    pub(crate) caller_memory: MemoryHandleProjection,
    pub(crate) cont_addr: usize,
    pub(crate) task_id: u32,
    pub(crate) current_frame: FrameCacheProjection,
    pub(crate) caller_frame: Option<FrameCacheProjection>,
    pub(crate) active_frame_slot: Option<FrameProjection>,
    pub(crate) caller_frame_slot: Option<FrameProjection>,
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

pub(crate) struct ExecuteContextFacade<'ctx, 'store> {
    ctx: &'ctx mut ExecuteContext<'store>,
}

impl<'ctx, 'store> ExecuteContextFacade<'ctx, 'store> {
    pub(crate) fn store_ref(&self) -> &Store {
        self.ctx.store_ref()
    }

    pub(crate) fn instance_id(&self) -> u32 {
        self.ctx.instance_id()
    }

    pub(crate) fn func_idx(&self) -> u32 {
        self.ctx.func().funcidx
    }

    pub(crate) fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.ctx.with_memory(f)
    }

    pub(crate) fn with_caller_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.ctx.with_caller_memory(f)
    }
}

impl ExecuteContextSnapshot {
    pub(crate) fn has_default_memory(self) -> bool {
        self.default_memory.is_some()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn projection(
        self,
        stack: &Stack,
        runtime: &StoreInner,
    ) -> ExecuteContextProjection {
        let current_frame = FrameCacheProjection {
            instance_raw: self.current_frame.instance.raw(),
            default_memory: self.current_frame.projection_memory(),
        };
        let caller_frame = self.caller_frame.map(|frame| FrameCacheProjection {
            instance_raw: frame.instance.raw(),
            default_memory: frame.projection_memory(),
        });
        let active_frame_slot = (self.active_local.local_size as usize
            >= std::mem::size_of::<crate::common::stack::CallStackInfo>())
        .then(|| stack.frame_projection(&self.active_local, runtime));
        let caller_frame_slot = self.caller_local.and_then(|reference| {
            (reference.local_size as usize
                >= std::mem::size_of::<crate::common::stack::CallStackInfo>())
            .then(|| stack.frame_projection(&reference, runtime))
        });
        ExecuteContextProjection {
            default_memory: MemoryHandleProjection::from_handle(self.default_memory),
            caller_memory: MemoryHandleProjection::from_handle(self.caller_memory),
            cont_addr: self.cont_addr,
            task_id: self.task_id,
            current_frame,
            caller_frame,
            active_frame_slot,
            caller_frame_slot,
            active_local: self.active_local,
            caller_local: self.caller_local,
        }
    }
}

impl ExecuteContextProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn token_projection(&self) -> Option<ExecContextTokenProjection> {
        Some(ExecContextTokenProjection {
            current_frame: self.active_frame_slot.clone()?,
            caller_frame: self.caller_frame_slot.clone(),
            cont_addr: self.cont_addr,
            task_id: self.task_id,
        })
    }
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
        if local_reference.local_size as usize
            >= std::mem::size_of::<crate::common::stack::CallStackInfo>()
        {
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
    pub(crate) fn memory_handle_at_result(&self, memidx: u32) -> VMResult<MemoryHandle> {
        VMResult::from_option(
            self.memory_slot_at(memidx).and_then(|slot| slot.handle()),
            || VMResult::MemoryIndexOutOfRange,
        )
    }

    #[inline(always)]
    pub(crate) fn push_memory_to_stack_handle<const N: usize>(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<()> {
        self.gc
            .push_memory_to_stack::<N>(handle, self.stack, offset)
    }

    #[inline(always)]
    pub(crate) fn read_u8_array_handle<const N: usize>(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        self.gc.read_u8_array::<N>(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn read_u8_at_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u8> {
        self.gc.read_u8_at(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn read_i8_at_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<i8> {
        self.gc.read_i8_at(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn read_u16_at_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u16> {
        self.gc.read_u16_at(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn read_i16_at_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<i16> {
        self.gc.read_i16_at(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn read_u32_at_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u32> {
        self.gc.read_u32_at(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn read_i32_at_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<i32> {
        self.gc.read_i32_at(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn write_memory_bytes_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.gc.write_bytes(handle, offset, bytes)
    }

    #[inline(always)]
    pub(crate) fn grow_memory_handle(
        &mut self,
        handle: MemoryHandle,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        self.gc.grow_memory(handle, page_size_delta)
    }

    #[inline(always)]
    pub(crate) fn copy_memory_handle(
        &mut self,
        handle: MemoryHandle,
        dst: u32,
        src: u32,
        len: u32,
    ) -> VMResult<()> {
        self.gc.copy_memory(handle, dst, src, len)
    }

    #[inline(always)]
    pub(crate) fn copy_memory_between_handles(
        &mut self,
        dst: MemoryHandle,
        src: MemoryHandle,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        match (dst, src) {
            (MemoryHandle::Local(dst), MemoryHandle::Local(src)) => self
                .gc
                .copy_memory_local_to_local(dst, src, dst_offset, src_offset, len),
            (MemoryHandle::Local(dst), MemoryHandle::Shared(src)) => self
                .gc
                .copy_memory_shared_to_local(dst, src, dst_offset, src_offset, len),
            (MemoryHandle::Shared(dst), MemoryHandle::Local(src)) => self
                .gc
                .copy_memory_local_to_shared(dst, src, dst_offset, src_offset, len),
            (MemoryHandle::Shared(dst), MemoryHandle::Shared(src)) => self
                .gc
                .copy_memory_shared_to_shared(dst, src, dst_offset, src_offset, len),
        }
    }

    #[inline(always)]
    pub(crate) fn fill_memory_handle(
        &mut self,
        handle: MemoryHandle,
        ptr: u32,
        len: u32,
        data: u32,
    ) -> VMResult<()> {
        self.gc.fill_memory(handle, ptr, len, data)
    }

    #[inline(always)]
    pub(crate) fn memory_page_size_handle(&self, handle: MemoryHandle) -> u32 {
        match handle {
            MemoryHandle::Local(id) => self.gc.local_memory(id).page_size(),
            MemoryHandle::Shared(id) => self.gc.shared_memory(id).page_size(),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u8_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u8> {
        self.gc.atomic_load_u8(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u16_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u16> {
        self.gc.atomic_load_u16(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u32_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u32> {
        self.gc.atomic_load_u32(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u64_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u64> {
        self.gc.atomic_load_u64(handle, offset)
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u8_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u8,
    ) -> VMResult<()> {
        self.gc.atomic_store_u8(handle, offset, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u16_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u16,
    ) -> VMResult<()> {
        self.gc.atomic_store_u16(handle, offset, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u32_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u32,
    ) -> VMResult<()> {
        self.gc.atomic_store_u32(handle, offset, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u64_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u64,
    ) -> VMResult<()> {
        self.gc.atomic_store_u64(handle, offset, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u8_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        self.gc.atomic_rmw_u8(handle, offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u16_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.gc.atomic_rmw_u16(handle, offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u32_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.gc.atomic_rmw_u32(handle, offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u64_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.gc.atomic_rmw_u64(handle, offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u8_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u8,
        value: u8,
    ) -> VMResult<u8> {
        self.gc.atomic_cmpxchg_u8(handle, offset, expected, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u16_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u16,
        value: u16,
    ) -> VMResult<u16> {
        self.gc.atomic_cmpxchg_u16(handle, offset, expected, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u32_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u32,
        value: u32,
    ) -> VMResult<u32> {
        self.gc.atomic_cmpxchg_u32(handle, offset, expected, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u64_handle(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u64,
        value: u64,
    ) -> VMResult<u64> {
        self.gc.atomic_cmpxchg_u64(handle, offset, expected, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_fence_handle(&mut self, handle: MemoryHandle) {
        self.gc.atomic_fence(handle);
    }

    pub(crate) fn local_atomic_load_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u8> {
        self.atomic_load_u8_handle(handle, offset)
    }

    pub(crate) fn local_atomic_load_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u16> {
        self.atomic_load_u16_handle(handle, offset)
    }

    pub(crate) fn local_atomic_load_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u32> {
        self.atomic_load_u32_handle(handle, offset)
    }

    pub(crate) fn local_atomic_load_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u64> {
        self.atomic_load_u64_handle(handle, offset)
    }

    pub(crate) fn shared_atomic_load_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u8> {
        self.atomic_load_u8_handle(handle, offset)
    }

    pub(crate) fn shared_atomic_load_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u16> {
        self.atomic_load_u16_handle(handle, offset)
    }

    pub(crate) fn shared_atomic_load_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u32> {
        self.atomic_load_u32_handle(handle, offset)
    }

    pub(crate) fn shared_atomic_load_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<u64> {
        self.atomic_load_u64_handle(handle, offset)
    }

    pub(crate) fn local_atomic_store_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u8,
    ) -> VMResult<()> {
        self.atomic_store_u8_handle(handle, offset, value)
    }

    pub(crate) fn local_atomic_store_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u16,
    ) -> VMResult<()> {
        self.atomic_store_u16_handle(handle, offset, value)
    }

    pub(crate) fn local_atomic_store_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u32,
    ) -> VMResult<()> {
        self.atomic_store_u32_handle(handle, offset, value)
    }

    pub(crate) fn local_atomic_store_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u64,
    ) -> VMResult<()> {
        self.atomic_store_u64_handle(handle, offset, value)
    }

    pub(crate) fn shared_atomic_store_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u8,
    ) -> VMResult<()> {
        self.atomic_store_u8_handle(handle, offset, value)
    }

    pub(crate) fn shared_atomic_store_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u16,
    ) -> VMResult<()> {
        self.atomic_store_u16_handle(handle, offset, value)
    }

    pub(crate) fn shared_atomic_store_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u32,
    ) -> VMResult<()> {
        self.atomic_store_u32_handle(handle, offset, value)
    }

    pub(crate) fn shared_atomic_store_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u64,
    ) -> VMResult<()> {
        self.atomic_store_u64_handle(handle, offset, value)
    }

    pub(crate) fn local_atomic_rmw_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        self.atomic_rmw_u8_handle(handle, offset, op, value)
    }

    pub(crate) fn local_atomic_rmw_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.atomic_rmw_u16_handle(handle, offset, op, value)
    }

    pub(crate) fn local_atomic_rmw_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.atomic_rmw_u32_handle(handle, offset, op, value)
    }

    pub(crate) fn local_atomic_rmw_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.atomic_rmw_u64_handle(handle, offset, op, value)
    }

    pub(crate) fn shared_atomic_rmw_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        self.atomic_rmw_u8_handle(handle, offset, op, value)
    }

    pub(crate) fn shared_atomic_rmw_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.atomic_rmw_u16_handle(handle, offset, op, value)
    }

    pub(crate) fn shared_atomic_rmw_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.atomic_rmw_u32_handle(handle, offset, op, value)
    }

    pub(crate) fn shared_atomic_rmw_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.atomic_rmw_u64_handle(handle, offset, op, value)
    }

    pub(crate) fn local_atomic_cmpxchg_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u8,
        value: u8,
    ) -> VMResult<u8> {
        self.atomic_cmpxchg_u8_handle(handle, offset, expected, value)
    }

    pub(crate) fn local_atomic_cmpxchg_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u16,
        value: u16,
    ) -> VMResult<u16> {
        self.atomic_cmpxchg_u16_handle(handle, offset, expected, value)
    }

    pub(crate) fn local_atomic_cmpxchg_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u32,
        value: u32,
    ) -> VMResult<u32> {
        self.atomic_cmpxchg_u32_handle(handle, offset, expected, value)
    }

    pub(crate) fn local_atomic_cmpxchg_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u64,
        value: u64,
    ) -> VMResult<u64> {
        self.atomic_cmpxchg_u64_handle(handle, offset, expected, value)
    }

    pub(crate) fn shared_atomic_cmpxchg_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u8,
        value: u8,
    ) -> VMResult<u8> {
        self.atomic_cmpxchg_u8_handle(handle, offset, expected, value)
    }

    pub(crate) fn shared_atomic_cmpxchg_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u16,
        value: u16,
    ) -> VMResult<u16> {
        self.atomic_cmpxchg_u16_handle(handle, offset, expected, value)
    }

    pub(crate) fn shared_atomic_cmpxchg_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u32,
        value: u32,
    ) -> VMResult<u32> {
        self.atomic_cmpxchg_u32_handle(handle, offset, expected, value)
    }

    pub(crate) fn shared_atomic_cmpxchg_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u64,
        value: u64,
    ) -> VMResult<u64> {
        self.atomic_cmpxchg_u64_handle(handle, offset, expected, value)
    }

    pub(crate) fn local_atomic_fence(&mut self, handle: MemoryHandle) {
        self.atomic_fence_handle(handle);
    }

    pub(crate) fn shared_atomic_fence(&mut self, handle: MemoryHandle) {
        self.atomic_fence_handle(handle);
    }

    #[inline(always)]
    pub(crate) fn clone_shared_memory(&self, id: store::SharedMemoryId) -> Arc<SharedMemoryObject> {
        self.gc.clone_shared_memory(id)
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
    fn execute_context_snapshot_tracks_tail_call_memory_cache() {
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
        let callee_local_top = callee.local_top;
        let callee_local_size = callee.local_size;
        let root_local_top = root.local_top;
        let root_local_size = root.local_size;
        assert_eq!(
            before.default_memory,
            Some(MemoryHandle::Shared(store::SharedMemoryId::from_raw(2)))
        );
        assert_eq!(
            before.caller_memory,
            Some(MemoryHandle::Local(store::LocalMemoryId::from_raw(1)))
        );
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
        assert_eq!(
            before_projection.current_frame.default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Shared(
                store::SharedMemoryId::from_raw(2),
            )))
        );
        assert_eq!(
            before_projection.caller_frame.unwrap().default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        assert_eq!(
            before_projection
                .active_frame_slot
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            before_projection
                .caller_frame_slot
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        let before_token = before_projection.token_projection().unwrap();
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
            after_projection.current_frame.default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            after_projection.caller_frame.unwrap().default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        assert_eq!(
            after_projection
                .active_frame_slot
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(3),
            )))
        );
        assert_eq!(
            after_projection
                .caller_frame_slot
                .as_ref()
                .unwrap()
                .default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                store::LocalMemoryId::from_raw(1),
            )))
        );
        let after_token = after_projection.token_projection().unwrap();
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
}
