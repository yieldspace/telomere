#![allow(private_interfaces)]

#[macro_use]
mod vm_result;
use std::{fmt::Display, future::Future, pin::Pin};

use custom_section::NameSubSection;

pub use vm_result::VMResult;
mod memory;
pub use memory::{AtomicRmwOp, LocalMemoryObject, MemArg, Memory, SharedMemoryObject};
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
pub use store::{InstanceHandle, MemoryHandle, Store, StoreState};
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
pub(crate) struct StablePc(usize);
impl StablePc {
    const RELATIVE_TAG: usize = 1;

    pub(crate) fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

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
    pub stack: &'a mut Stack,
    pub local_reference: LocalReference,
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
