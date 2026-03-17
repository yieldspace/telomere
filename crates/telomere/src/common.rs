#[macro_use]
mod vm_result;
use std::{fmt::Display, future::Future, pin::Pin, sync::Arc};

use custom_section::NameSubSection;

use gc::FunctionInstanceData;
use gc::GcRootHandle;
use gc::InstanceData;
use gc::MemoryPool;
pub use vm_result::VMResult;
mod memory;
pub use memory::{MemArg, Memory};
pub(crate) mod stack;
pub use stack::{LocalReference, Stack};
mod registry;
pub use registry::Registry;
pub(crate) mod store;
pub(crate) use store::ModuleInstance;
pub(crate) mod gc;
pub use gc::GcRef;

pub use store::{Store, StoreState};

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemType(pub Limits);
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
pub(crate) struct StablePc(usize);
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
        gc: &MemoryPool,
        stack: &Stack,
        local_reference: LocalReference,
        ptr: *const Instr,
    ) -> Self {
        Self::relative_index_for_ptr(gc, stack, local_reference, ptr)
            .map(Self::from_relative_index)
            .unwrap_or_else(|| Self::from_stable_ptr(ptr))
    }

    pub(crate) fn resolve(
        self,
        gc: &MemoryPool,
        stack: &Stack,
        local_reference: LocalReference,
    ) -> *const Instr {
        match self.relative_index() {
            Some(index) => {
                let (base, len) = Self::current_frame_code_range(gc, stack, local_reference)
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
        gc: &MemoryPool,
        stack: &Stack,
        local_reference: LocalReference,
    ) -> Option<(*const Instr, usize)> {
        let frame_size = local_reference.local_size as usize;
        if frame_size < std::mem::size_of::<crate::common::stack::CallStackInfo>() {
            return None;
        }
        let code_addr = stack.code_addr(&local_reference);
        let funcinst = unsafe { gc.get_func(code_addr) };
        if funcinst.is_host_func() {
            return None;
        }
        let (_locals, code_offset) = funcinst.locals_and_code_offset(gc);
        let total_words = gc.read_header(funcinst.body).word_size() as usize;
        let code_words = total_words.checked_sub(code_offset)?;
        let instr_len = code_words / word_size::<Instr>();
        Some((
            unsafe { gc.get_value::<Instr>(funcinst.body, code_offset) },
            instr_len,
        ))
    }

    fn relative_index_for_ptr(
        gc: &MemoryPool,
        stack: &Stack,
        local_reference: LocalReference,
        ptr: *const Instr,
    ) -> Option<usize> {
        let (base, instr_len) = Self::current_frame_code_range(gc, stack, local_reference)?;
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
    pub store: &'a Store,
    pub gc: &'a mut MemoryPool,
    pub effect: EffectSupplier<'a>,
    pub cont: *const Instr,
    pub task_id: u32,
}
impl ExecuteContext<'_> {
    pub fn func(&self) -> &FunctionInstanceData {
        let code_addr = self.stack.code_addr(&self.local_reference());
        unsafe { self.gc.get_func(code_addr) }
    }
    pub fn func_by_addr(&self, addr: GcRef) -> &FunctionInstanceData {
        unsafe { self.gc.get_func(addr) }
    }
    pub(crate) fn code(&self) -> *const Instr {
        let func = self.func();
        let (_local_data, offset) = func.locals_and_code_offset(self.gc);
        unsafe { self.gc.get_value::<Instr>(func.body, offset) }
    }
    pub fn module(&self) -> &ModuleInstance {
        unsafe { self.gc.get_module(self.instance().module_addr) }
    }
    pub fn instance_addr(&self) -> GcRef {
        self.func().instance_addr
    }
    pub fn instance_id(&self) -> u32 {
        self.instance().instance_id
    }
    pub fn instance(&self) -> &InstanceData {
        unsafe { &*self.gc.get_instance_unchecked(self.instance_addr()) }
    }
    pub fn local_reference(&self) -> LocalReference {
        self.local_reference
    }
    pub fn memory_addr(&self) -> Option<GcRef> {
        self.instance().mems.as_slice(self.gc).first().copied()
    }
    pub fn memory(&mut self) -> Option<&mut Memory> {
        self.memory_addr().map(|v| unsafe { self.gc.get_memory(v) })
    }
    pub fn return_slot(&mut self) -> ReturnSlot {
        let local_ref = self.local_reference();
        ReturnSlot(unsafe { self.stack.local_area_mut_ptr(&local_ref) })
    }
}

#[derive(Debug, Clone)]
pub struct InstanceHandle(pub(crate) Arc<GcRootHandle>);
impl InstanceHandle {
    pub(crate) fn get_gc_ref_with_pool(
        &self,
        store: &Store,
        pool_ref: &MemoryPool,
    ) -> Option<GcRef> {
        if !store.matches_identity(&self.0.store_identity) {
            return None;
        }
        Some(pool_ref.read_root_slot(self.0.slot.get()))
    }

    pub(crate) fn from_root_slot(store: &Store, slot: u32) -> Self {
        Self(Arc::new(GcRootHandle::new(
            slot,
            store.gc_weak(),
            store.identity_weak(),
        )))
    }
}

#[cfg(test)]
mod instance_handle_tests {
    use super::{
        gc::{Header, ObjectType},
        GcRef, InstanceHandle, LocalReference, StablePc, Stack, Store, VMResult, WasmValue,
    };
    use crate::{
        aliasing, get_global, instantiate, run_module_function, IoReadBinaryReader, Module,
        Registry, ResultValue, WasmParser,
    };
    use futures::executor::block_on;

    fn parse_module(wat: &str) -> Module {
        let source = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(&source[..]);
        let mut parser = WasmParser::new(&mut reader);
        parser.parse_module().expect("module must parse")
    }

    async fn instantiate_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
        instantiate(parse_module(wat), store, registry)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn dropped_instance_handle_releases_root_slot() {
        let store = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (global (export "g") i32 (i32.const 42)))
            "#,
            &store,
            &registry,
        )
        .await;
        let slot = handle.0.slot.get();
        assert_ne!(store.lock_gc().read_root_slot(slot), GcRef(0));

        drop(handle);

        assert_eq!(store.lock_gc().read_root_slot(slot), GcRef(0));
    }

    #[tokio::test]
    async fn dropped_instance_handle_releases_root_slot_while_store_gc_is_active() {
        let store = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (global (export "g") i32 (i32.const 42)))
            "#,
            &store,
            &registry,
        )
        .await;
        let slot = handle.0.slot.get();
        let gc = store.lock_gc();
        assert_ne!(gc.read_root_slot(slot), GcRef(0));

        drop(handle);

        assert_eq!(gc.read_root_slot(slot), GcRef(0));
    }

    #[tokio::test]
    async fn foreign_store_handle_resolution_fails_closed() {
        let store_a = Store::new();
        let store_b = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (global (export "g") i32 (i32.const 42)))
            "#,
            &store_a,
            &registry,
        )
        .await;

        assert!(matches!(
            get_global(&handle, &store_b, "g"),
            VMResult::Unlinkable
        ));
        assert!(matches!(
            get_global(&handle, &store_a, "g"),
            VMResult::Success(WasmValue::I32(42))
        ));
    }

    #[tokio::test]
    async fn same_thread_reentrant_get_global_fails_closed() {
        let store = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (global (export "g") i32 (i32.const 42)))
            "#,
            &store,
            &registry,
        )
        .await;
        let _gc = store.lock_gc();

        assert!(matches!(
            get_global(&handle, &store, "g"),
            VMResult::Unlinkable
        ));
    }

    #[tokio::test]
    async fn same_thread_reentrant_run_module_function_fails_closed() {
        let store = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (func (export "f") (result i32) (i32.const 42)))
            "#,
            &store,
            &registry,
        )
        .await;
        let _gc = store.lock_gc();

        assert!(matches!(
            block_on(run_module_function(
                &handle,
                &store,
                "f",
                &ResultValue::new(vec![]),
            )),
            VMResult::Unlinkable
        ));
    }

    #[tokio::test]
    async fn failed_instantiate_does_not_leak_root_slots() {
        let store = Store::new();
        let registry = Registry::new();
        let module = parse_module(
            r#"
            (module
              (import "env" "missing" (func)))
            "#,
        );

        for _ in 0..3 {
            assert!(matches!(
                instantiate(module.clone(), &store, &registry).await,
                VMResult::Unlinkable
            ));
        }

        assert_eq!(store.lock_gc().reserve_root_slot(), 1);
    }

    #[test]
    fn failed_aliasing_does_not_leak_root_slots() {
        let store = Store::new();
        let registry = Registry::new();
        let triplets = [("missing".to_owned(), "x".to_owned(), "y".to_owned())];

        for _ in 0..3 {
            assert!(matches!(
                aliasing(&registry, &triplets, &store),
                VMResult::Unlinkable
            ));
        }

        assert_eq!(store.lock_gc().reserve_root_slot(), 1);
    }

    #[tokio::test]
    async fn stable_pc_relative_continuation_survives_memory_pool_growth() {
        let store = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (func (export "f") (result i32)
                i32.const 42))
            "#,
            &store,
            &registry,
        )
        .await;
        let mut gc = store.lock_gc();
        let instance_gc_ref = handle.get_gc_ref_with_pool(&store, &gc).unwrap();
        let instance = unsafe { &*gc.get_instance_unchecked(instance_gc_ref) };
        let func_addr = instance.funcs.as_slice(&gc)[0];
        let func = unsafe { gc.get_func(func_addr) };
        let (locals, code_offset) = func.locals_and_code_offset(&gc);
        let mut stack = Stack::new(256);
        let local_reference = stack
            .function_call(
                0,
                locals.byte_size(),
                func_addr,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &crate::runtime::vm::VM_END as *const super::Instr,
                &gc,
            )
            .unwrap();
        let raw = unsafe { gc.get_value::<super::Instr>(func.body, code_offset) };
        let first_instr = unsafe { *raw };
        let stable = StablePc::from_raw_in_frame(&gc, &stack, local_reference, raw);

        let mut reallocated = false;
        while !reallocated {
            let old_ptr = gc.memory.as_ptr();
            gc.allocate(Header::new(ObjectType::Raw, 4096).initialized());
            reallocated = gc.memory.as_ptr() != old_ptr;
        }

        let resolved = stable.resolve(&gc, &stack, local_reference);
        assert_eq!(unsafe { first_instr.op as usize }, unsafe {
            (*resolved).op as usize
        });
        assert_eq!(
            stable,
            StablePc::from_raw_in_frame(&gc, &stack, local_reference, resolved)
        );
    }

    #[tokio::test]
    async fn stack_return_continuation_survives_memory_pool_growth() {
        let store = Store::new();
        let registry = Registry::new();
        let handle = instantiate_wat(
            r#"
            (module
              (func (export "f") (result i32)
                i32.const 1
                drop
                i32.const 42))
            "#,
            &store,
            &registry,
        )
        .await;
        let mut gc = store.lock_gc();
        let instance_gc_ref = handle.get_gc_ref_with_pool(&store, &gc).unwrap();
        let instance = unsafe { &*gc.get_instance_unchecked(instance_gc_ref) };
        let func_addr = instance.funcs.as_slice(&gc)[0];
        let func = unsafe { gc.get_func(func_addr) };
        let (locals, code_offset) = func.locals_and_code_offset(&gc);
        let mut stack = Stack::new(256);
        let caller_reference = stack
            .function_call(
                0,
                locals.byte_size(),
                func_addr,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &crate::runtime::vm::VM_END as *const super::Instr,
                &gc,
            )
            .unwrap();
        let caller_entry = unsafe { gc.get_value::<super::Instr>(func.body, code_offset) };
        let return_addr = unsafe { caller_entry.add(2) };
        let callee_reference = stack
            .function_call(0, 0, func_addr, caller_reference, return_addr, &gc)
            .unwrap();
        let expected = unsafe { *return_addr };

        let mut reallocated = false;
        while !reallocated {
            let old_ptr = gc.memory.as_ptr();
            gc.allocate(Header::new(ObjectType::Raw, 4096).initialized());
            reallocated = gc.memory.as_ptr() != old_ptr;
        }

        let (prev_local_reference, resolved_return_addr) =
            stack.function_return(&callee_reference, 0, &gc);
        let prev_local_top = prev_local_reference.local_top;
        let prev_local_size = prev_local_reference.local_size;
        let caller_local_top = caller_reference.local_top;
        let caller_local_size = caller_reference.local_size;
        assert_eq!(prev_local_top, caller_local_top);
        assert_eq!(prev_local_size, caller_local_size);
        assert_eq!(unsafe { expected.op as usize }, unsafe {
            (*resolved_return_addr).op as usize
        });
    }

    #[test]
    fn stable_pc_static_continuation_round_trips() {
        let stack = Stack::new(0);
        let gc = crate::common::gc::MemoryPool::new();
        let pc = StablePc::from_stable_ptr(&crate::runtime::vm::VM_END as *const super::Instr);
        assert_eq!(
            pc.resolve(
                &gc,
                &stack,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                }
            ),
            &crate::runtime::vm::VM_END as *const super::Instr
        );
    }
}

pub fn execute_elem_init_const_expr(
    gc: &mut MemoryPool,
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
            let mut buf = [0u8; 4];
            buf.copy_from_slice(unsafe { gc.get_global(addr) });
            VMResult::Success(GcRef(u32::from_le_bytes(buf)))
        }
        unknown => todo!("{unknown:?}"),
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
