#![allow(dead_code, private_interfaces)]

use super::{
    memory::{AtomicRmwOp, LocalMemoryObject, SharedMemoryObject},
    object_ref::ObjectRef,
    AsyncHostFunction, CallFrameCache, Data, Elem, ExportSection, FuncType, GlobalType,
    HostFunction, Instr, LocalsData, MemType, Stack, TableType, TypeIdx, VMResult,
};
use parking_lot::{Mutex, MutexGuard};
use std::{
    cell::RefCell,
    collections::HashMap,
    fmt,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Weak,
    },
};

thread_local! {
    static ACTIVE_STORE_RUNTIME: RefCell<Vec<(*const (), *mut StoreInner)>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_STORE_REENTRY: RefCell<Vec<*const ()>> = const { RefCell::new(Vec::new()) };
}

macro_rules! define_store_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $name(NonZeroU32);

        impl $name {
            pub(crate) fn from_index(index: usize) -> Self {
                let raw = u32::try_from(index + 1).expect("store arena index overflow");
                Self(NonZeroU32::new(raw).expect("store arena ids are non-zero"))
            }

            pub(crate) fn from_raw(raw: u32) -> Self {
                Self(NonZeroU32::new(raw).expect("store arena ids are non-zero"))
            }

            pub(crate) unsafe fn from_raw_unchecked(raw: u32) -> Self {
                Self(unsafe { NonZeroU32::new_unchecked(raw) })
            }

            pub(crate) fn index(self) -> usize {
                self.0.get() as usize - 1
            }

            pub(crate) fn raw(self) -> u32 {
                self.0.get()
            }
        }
    };
}

define_store_id!(ModuleId);
define_store_id!(InstanceId);
define_store_id!(FuncId);
define_store_id!(GlobalId);
define_store_id!(TableId);
define_store_id!(LocalMemoryId);
define_store_id!(SharedMemoryId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryHandle {
    Local(LocalMemoryId),
    Shared(SharedMemoryId),
}

impl MemoryHandle {
    pub(crate) fn is_shared(self) -> bool {
        matches!(self, Self::Shared(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceMemorySlot {
    None,
    Local(LocalMemoryId),
    Shared(SharedMemoryId),
}

impl InstanceMemorySlot {
    pub(crate) fn from_handle(handle: Option<MemoryHandle>) -> Self {
        match handle {
            Some(MemoryHandle::Local(id)) => Self::Local(id),
            Some(MemoryHandle::Shared(id)) => Self::Shared(id),
            None => Self::None,
        }
    }

    pub(crate) fn from_object_ref(store: &StoreInner, addr: ObjectRef) -> Self {
        if addr.is_null() {
            Self::None
        } else {
            Self::from_handle(Some(store.memory_handle(addr)))
        }
    }

    pub(crate) fn handle(self) -> Option<MemoryHandle> {
        match self {
            Self::None => None,
            Self::Local(id) => Some(MemoryHandle::Local(id)),
            Self::Shared(id) => Some(MemoryHandle::Shared(id)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInstance {
    pub exports: ExportSection,
    pub tables: Vec<TableType>,
    pub globals: Vec<GlobalType>,
    pub functions: Vec<TypeIdx>,
    pub function_types: Vec<FuncType>,
    pub mems: Vec<MemType>,
}

#[derive(Debug, Clone)]
pub struct InstanceData {
    pub instance_id: u32,
    pub module_addr: ObjectRef,
    pub globals: Vec<ObjectRef>,
    pub funcs: Vec<ObjectRef>,
    pub tables: Vec<ObjectRef>,
    pub mems: Vec<ObjectRef>,
    pub memory_slots: Vec<InstanceMemorySlot>,
}

#[derive(Clone)]
pub(crate) enum FunctionBody {
    Wasm {
        locals: LocalsData,
        code: Arc<[Instr]>,
    },
    Host(HostFunction),
    AsyncHost(AsyncHostFunction),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CallDispatchTarget {
    Wasm { local_size: u32 },
    Host(HostFunction),
    AsyncHost(AsyncHostFunction),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CallRecipe {
    pub(crate) frame: CallFrameCache,
    pub(crate) param_size: u32,
    pub(crate) local_size: u32,
    pub(crate) return_arity: u32,
    pub(crate) target: CallDispatchTarget,
}

pub(crate) type CallDispatchCache = CallRecipe;

impl fmt::Debug for FunctionBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wasm { locals, code } => f
                .debug_struct("Wasm")
                .field("locals", locals)
                .field("code_len", &code.len())
                .finish(),
            Self::Host(_) => f.write_str("Host(..)"),
            Self::AsyncHost(_) => f.write_str("AsyncHost(..)"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FunctionInstanceData {
    pub instance: InstanceId,
    pub funcidx: u32,
    pub body: FunctionBody,
}

impl FunctionInstanceData {
    pub fn is_host_func(&self) -> bool {
        matches!(
            self.body,
            FunctionBody::Host(_) | FunctionBody::AsyncHost(_)
        )
    }

    pub fn is_async_host_func(&self) -> bool {
        matches!(self.body, FunctionBody::AsyncHost(_))
    }

    pub fn locals(&self) -> LocalsData {
        match &self.body {
            FunctionBody::Wasm { locals, .. } => locals.clone(),
            FunctionBody::Host(_) | FunctionBody::AsyncHost(_) => LocalsData::default(),
        }
    }

    pub(crate) fn code(&self) -> Option<&[Instr]> {
        match &self.body {
            FunctionBody::Wasm { code, .. } => Some(code.as_ref()),
            FunctionBody::Host(_) | FunctionBody::AsyncHost(_) => None,
        }
    }

    pub fn code_pointer(&self) -> Option<*const Instr> {
        self.code().map(|code| code.as_ptr())
    }

    pub(crate) fn locals_and_code_offset<R>(&self, _runtime: &R) -> (LocalsData, usize) {
        (self.locals(), 0)
    }

    pub fn host_code_pointer(&self) -> HostFunction {
        match self.body {
            FunctionBody::Host(fp) => fp,
            FunctionBody::Wasm { .. } | FunctionBody::AsyncHost(_) => {
                unreachable!("host code pointer requested for non-host function")
            }
        }
    }

    pub fn async_host_code_pointer(&self) -> AsyncHostFunction {
        match self.body {
            FunctionBody::AsyncHost(fp) => fp,
            FunctionBody::Wasm { .. } | FunctionBody::Host(_) => {
                unreachable!("async host code pointer requested for non-async function")
            }
        }
    }

    pub(crate) fn replace_host_code_pointer(&mut self, fp: HostFunction) {
        self.body = FunctionBody::Host(fp);
    }

    pub(crate) fn replace_async_host_code_pointer(&mut self, fp: AsyncHostFunction) {
        self.body = FunctionBody::AsyncHost(fp);
    }
}

#[derive(Debug, Clone)]
pub(crate) enum GlobalValue {
    Bytes4([u8; 4]),
    Bytes8([u8; 8]),
    Bytes16([u8; 16]),
    Ref(u32),
}

impl GlobalValue {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Bytes4(bytes) => bytes,
            Self::Bytes8(bytes) => bytes,
            Self::Bytes16(bytes) => bytes,
            Self::Ref(raw) => unsafe {
                std::slice::from_raw_parts(raw as *const u32 as *const u8, 4)
            },
        }
    }

    pub(crate) fn as_bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Bytes4(bytes) => bytes,
            Self::Bytes8(bytes) => bytes,
            Self::Bytes16(bytes) => bytes,
            Self::Ref(raw) => unsafe {
                std::slice::from_raw_parts_mut(raw as *mut u32 as *mut u8, 4)
            },
        }
    }
}

pub(crate) fn func_ref_raw(func: FuncId) -> u32 {
    func.raw()
}

pub(crate) fn raw_to_func_id(raw: u32) -> Option<FuncId> {
    NonZeroU32::new(raw).map(FuncId)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum ObjectKind {
    Module = 1,
    Instance = 2,
    Function = 3,
    Table = 4,
    Global = 5,
    LocalMemory = 6,
    SharedMemory = 7,
}

const OBJECT_KIND_SHIFT: u32 = 29;
const OBJECT_INDEX_MASK: u32 = (1 << OBJECT_KIND_SHIFT) - 1;

fn encode_object_ref(kind: ObjectKind, raw: u32) -> ObjectRef {
    ObjectRef(((kind as u32) << OBJECT_KIND_SHIFT) | raw)
}

fn decode_object_ref(addr: ObjectRef) -> (ObjectKind, usize) {
    let raw = addr.get();
    let kind = match raw >> OBJECT_KIND_SHIFT {
        1 => ObjectKind::Module,
        2 => ObjectKind::Instance,
        3 => ObjectKind::Function,
        4 => ObjectKind::Table,
        5 => ObjectKind::Global,
        6 => ObjectKind::LocalMemory,
        7 => ObjectKind::SharedMemory,
        _ => panic!("invalid runtime object ref: {raw}"),
    };
    let index = (raw & OBJECT_INDEX_MASK)
        .checked_sub(1)
        .expect("runtime object refs are 1-based") as usize;
    (kind, index)
}

#[derive(Default)]
pub struct StoreSegments {
    pub data: HashMap<(u32, u32), Data>,
    pub elems: HashMap<(u32, u32), Elem>,
}

#[derive(Default)]
pub struct StoreInner {
    modules: Vec<ModuleInstance>,
    instances: Vec<InstanceData>,
    funcs: Vec<FunctionInstanceData>,
    call_recipes: Vec<Option<CallRecipe>>,
    tables: Vec<super::TableInstance>,
    globals: Vec<GlobalValue>,
    local_memories: Vec<LocalMemoryObject>,
    shared_memories: Vec<Arc<SharedMemoryObject>>,
    pub(crate) segments: StoreSegments,
    next_instance_id: u32,
}

macro_rules! define_store_local_shared_const_read {
    ($local:ident, $shared:ident, $method:ident) => {
        #[inline(always)]
        pub(crate) fn $local<const N: usize>(
            &mut self,
            id: LocalMemoryId,
            offset: usize,
        ) -> VMResult<[u8; N]> {
            self.local_memory(id).$method::<N>(offset)
        }

        #[inline(always)]
        pub(crate) fn $shared<const N: usize>(
            &mut self,
            id: SharedMemoryId,
            offset: usize,
        ) -> VMResult<[u8; N]> {
            self.shared_memory(id).$method::<N>(offset)
        }
    };
}

macro_rules! define_store_local_shared_const_stack {
    ($local:ident, $shared:ident, $method:ident) => {
        #[inline(always)]
        pub(crate) fn $local<const N: usize>(
            &mut self,
            id: LocalMemoryId,
            stack: &mut Stack,
            offset: usize,
        ) -> VMResult<()> {
            self.local_memory(id).$method::<N>(stack, offset)
        }

        #[inline(always)]
        pub(crate) fn $shared<const N: usize>(
            &mut self,
            id: SharedMemoryId,
            stack: &mut Stack,
            offset: usize,
        ) -> VMResult<()> {
            self.shared_memory(id).$method::<N>(stack, offset)
        }
    };
}

macro_rules! define_store_local_shared_read {
    ($local:ident, $shared:ident, $method:ident, $ret:ty) => {
        #[inline(always)]
        pub(crate) fn $local(&mut self, id: LocalMemoryId, offset: usize) -> VMResult<$ret> {
            self.local_memory(id).$method(offset)
        }

        #[inline(always)]
        pub(crate) fn $shared(&mut self, id: SharedMemoryId, offset: usize) -> VMResult<$ret> {
            self.shared_memory(id).$method(offset)
        }
    };
}

macro_rules! define_store_local_shared_store {
    ($local:ident, $shared:ident, $method:ident, $arg:ty) => {
        #[inline(always)]
        pub(crate) fn $local(
            &mut self,
            id: LocalMemoryId,
            offset: usize,
            value: $arg,
        ) -> VMResult<()> {
            self.local_memory_mut(id).$method(offset, value)
        }

        #[inline(always)]
        pub(crate) fn $shared(
            &mut self,
            id: SharedMemoryId,
            offset: usize,
            value: $arg,
        ) -> VMResult<()> {
            self.shared_memory(id).$method(offset, value)
        }
    };
}

macro_rules! define_store_local_shared_rmw {
    ($local:ident, $shared:ident, $method:ident, $arg:ty, $ret:ty) => {
        #[inline(always)]
        pub(crate) fn $local(
            &mut self,
            id: LocalMemoryId,
            offset: usize,
            op: AtomicRmwOp,
            value: $arg,
        ) -> VMResult<$ret> {
            self.local_memory_mut(id).$method(offset, op, value)
        }

        #[inline(always)]
        pub(crate) fn $shared(
            &mut self,
            id: SharedMemoryId,
            offset: usize,
            op: AtomicRmwOp,
            value: $arg,
        ) -> VMResult<$ret> {
            self.shared_memory(id).$method(offset, op, value)
        }
    };
}

macro_rules! define_store_local_shared_cmpxchg {
    ($local:ident, $shared:ident, $method:ident, $arg:ty, $ret:ty) => {
        #[inline(always)]
        pub(crate) fn $local(
            &mut self,
            id: LocalMemoryId,
            offset: usize,
            expected: $arg,
            value: $arg,
        ) -> VMResult<$ret> {
            self.local_memory_mut(id).$method(offset, expected, value)
        }

        #[inline(always)]
        pub(crate) fn $shared(
            &mut self,
            id: SharedMemoryId,
            offset: usize,
            expected: $arg,
            value: $arg,
        ) -> VMResult<$ret> {
            self.shared_memory(id).$method(offset, expected, value)
        }
    };
}

impl StoreInner {
    pub(crate) fn new() -> Self {
        Self {
            next_instance_id: 1,
            ..Self::default()
        }
    }

    pub(crate) fn new_instance_id(&mut self) -> u32 {
        let id = self.next_instance_id;
        self.next_instance_id = self
            .next_instance_id
            .checked_add(1)
            .expect("instance id overflow");
        id
    }

    pub(crate) fn alloc_module(&mut self, module: ModuleInstance) -> ModuleId {
        let id = ModuleId::from_index(self.modules.len());
        self.modules.push(module);
        id
    }

    pub(crate) fn module(&self, id: ModuleId) -> &ModuleInstance {
        &self.modules[id.index()]
    }

    pub(crate) fn new_module(&mut self, module: ModuleInstance) -> ObjectRef {
        let id = self.alloc_module(module);
        encode_object_ref(ObjectKind::Module, id.raw())
    }

    pub(crate) fn get_module(&self, addr: ObjectRef) -> &ModuleInstance {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Module);
        &self.modules[index]
    }

    pub(crate) fn alloc_instance(&mut self, instance: InstanceData) -> InstanceId {
        let id = InstanceId::from_index(self.instances.len());
        self.instances.push(instance);
        id
    }

    pub(crate) fn instance(&self, id: InstanceId) -> &InstanceData {
        &self.instances[id.index()]
    }

    pub(crate) fn new_instance(&mut self, instance: &InstanceData) -> ObjectRef {
        let id = InstanceId::from_index(self.instances.len());
        self.instances.push(instance.clone());
        encode_object_ref(ObjectKind::Instance, id.raw())
    }

    pub(crate) fn object_ref_for_instance(&self, id: InstanceId) -> ObjectRef {
        encode_object_ref(ObjectKind::Instance, id.raw())
    }

    pub(crate) unsafe fn get_instance_unchecked(&self, addr: ObjectRef) -> *const InstanceData {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Instance);
        &self.instances[index] as *const InstanceData
    }

    pub(crate) fn get_instance(&self, addr: ObjectRef) -> &InstanceData {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Instance);
        &self.instances[index]
    }

    pub(crate) unsafe fn place_instance_unchecked(
        &mut self,
        addr: ObjectRef,
        instance: &super::Instance,
    ) {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Instance);
        let memory_slots = instance
            .memory
            .iter()
            .copied()
            .map(|addr| InstanceMemorySlot::from_object_ref(self, addr))
            .collect();
        self.instances[index] = InstanceData {
            instance_id: instance.instance_id,
            module_addr: instance.module_addr,
            globals: instance.globals.clone(),
            funcs: instance.funcs.clone(),
            tables: instance.tables.clone(),
            mems: instance.memory.clone(),
            memory_slots,
        };
    }

    pub(crate) fn alloc_func(&mut self, func: FunctionInstanceData) -> FuncId {
        let id = FuncId::from_index(self.funcs.len());
        self.funcs.push(func);
        self.call_recipes.push(None);
        id
    }

    pub(crate) fn func(&self, id: FuncId) -> &FunctionInstanceData {
        &self.funcs[id.index()]
    }

    pub(crate) fn get_func(&self, addr: ObjectRef) -> &FunctionInstanceData {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Function);
        &self.funcs[index]
    }

    pub(crate) fn func_mut(&mut self, id: FuncId) -> &mut FunctionInstanceData {
        &mut self.funcs[id.index()]
    }

    pub(crate) fn get_func_mut(&mut self, addr: ObjectRef) -> &mut FunctionInstanceData {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Function);
        &mut self.funcs[index]
    }

    pub(crate) fn new_func(&mut self, func: &FunctionInstanceData) -> ObjectRef {
        let id = self.alloc_func(func.clone());
        encode_object_ref(ObjectKind::Function, id.raw())
    }

    fn call_recipe_slot_for_func_addr(&self, addr: ObjectRef) -> usize {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Function);
        index
    }

    pub(crate) fn call_recipe_slot_for_func(&self, addr: ObjectRef) -> u32 {
        u32::try_from(self.call_recipe_slot_for_func_addr(addr))
            .expect("call recipe slot exceeds u32::MAX")
    }

    pub(crate) fn call_recipe(&self, slot: u32) -> Option<CallRecipe> {
        self.call_recipes.get(slot as usize).copied().flatten()
    }

    pub(crate) fn set_call_recipe_for_func(&mut self, addr: ObjectRef, recipe: CallRecipe) {
        let slot = self.call_recipe_slot_for_func_addr(addr);
        self.call_recipes[slot] = Some(recipe);
    }

    pub(crate) fn build_call_recipe(&self, funcaddr: ObjectRef) -> CallRecipe {
        let funcinst = self.get_func(funcaddr);
        let (target, code_base, local_size) = match &funcinst.body {
            FunctionBody::Wasm { locals, code } => (
                CallDispatchTarget::Wasm {
                    local_size: locals.byte_size() as u32,
                },
                code.as_ptr(),
                locals.byte_size() as u32,
            ),
            FunctionBody::Host(fp) => (CallDispatchTarget::Host(*fp), std::ptr::null(), 0),
            FunctionBody::AsyncHost(fp) => {
                (CallDispatchTarget::AsyncHost(*fp), std::ptr::null(), 0)
            }
        };
        let instance_data = self.instance(funcinst.instance);
        let memory0 = instance_data
            .memory_slots
            .first()
            .copied()
            .unwrap_or(InstanceMemorySlot::None);
        let module = self.get_module(instance_data.module_addr);
        let typeidx = module.functions[funcinst.funcidx as usize];
        let functype = &module.function_types[typeidx.0 as usize];
        let param_size = functype.0.iter().map(|ty| ty.stack_size().u32()).sum();
        let return_arity = u32::try_from(functype.1 .0.len()).expect("return arity exceeds u32");
        CallRecipe {
            frame: CallFrameCache::from_cached_parts(
                funcaddr,
                funcinst.instance,
                code_base,
                memory0.handle(),
            ),
            param_size,
            local_size,
            return_arity,
            target,
        }
    }

    pub(crate) fn ensure_call_recipe_for_func(&mut self, funcaddr: ObjectRef) -> CallRecipe {
        let slot = self.call_recipe_slot_for_func_addr(funcaddr);
        if let Some(recipe) = self.call_recipes[slot] {
            return recipe;
        }
        let recipe = self.build_call_recipe(funcaddr);
        self.call_recipes[slot] = Some(recipe);
        recipe
    }

    pub(crate) fn alloc_table(&mut self, table: super::TableInstance) -> TableId {
        let id = TableId::from_index(self.tables.len());
        self.tables.push(table);
        id
    }

    pub(crate) fn table(&self, id: TableId) -> &super::TableInstance {
        &self.tables[id.index()]
    }

    pub(crate) fn new_table(&mut self, table_type: TableType) -> ObjectRef {
        let id = self.alloc_table(super::TableInstance::new(table_type));
        encode_object_ref(ObjectKind::Table, id.raw())
    }

    pub(crate) fn get_table(&mut self, addr: ObjectRef) -> &mut super::TableInstance {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Table);
        &mut self.tables[index]
    }

    pub(crate) fn table_mut(&mut self, id: TableId) -> &mut super::TableInstance {
        &mut self.tables[id.index()]
    }

    pub(crate) fn alloc_global(&mut self, global: GlobalValue) -> GlobalId {
        let id = GlobalId::from_index(self.globals.len());
        self.globals.push(global);
        id
    }

    pub(crate) fn global(&self, id: GlobalId) -> &GlobalValue {
        &self.globals[id.index()]
    }

    pub(crate) fn new_global_ref(&mut self, global_ref: ObjectRef) -> ObjectRef {
        let id = self.alloc_global(GlobalValue::Ref(global_ref.get()));
        encode_object_ref(ObjectKind::Global, id.raw())
    }

    pub(crate) fn new_global_data4(&mut self, data: u32) -> ObjectRef {
        let id = self.alloc_global(GlobalValue::Bytes4(data.to_le_bytes()));
        encode_object_ref(ObjectKind::Global, id.raw())
    }

    pub(crate) fn new_global_data8(&mut self, data: u64) -> ObjectRef {
        let id = self.alloc_global(GlobalValue::Bytes8(data.to_le_bytes()));
        encode_object_ref(ObjectKind::Global, id.raw())
    }

    pub(crate) fn new_global_data16(&mut self, data: u128) -> ObjectRef {
        let id = self.alloc_global(GlobalValue::Bytes16(data.to_le_bytes()));
        encode_object_ref(ObjectKind::Global, id.raw())
    }

    pub(crate) fn get_global(&self, addr: ObjectRef) -> &[u8] {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Global);
        self.globals[index].as_bytes()
    }

    pub(crate) fn global_mut(&mut self, id: GlobalId) -> &mut GlobalValue {
        &mut self.globals[id.index()]
    }

    pub(crate) fn get_global_mut(&mut self, addr: ObjectRef) -> &mut [u8] {
        let (kind, index) = decode_object_ref(addr);
        assert_eq!(kind, ObjectKind::Global);
        self.globals[index].as_bytes_mut()
    }

    pub(crate) fn copy_object(&mut self, item: ObjectRef) -> ObjectRef {
        let (kind, index) = decode_object_ref(item);
        assert_eq!(kind, ObjectKind::Global);
        let copied = self.globals[index].clone();
        let id = self.alloc_global(copied);
        encode_object_ref(ObjectKind::Global, id.raw())
    }

    pub(crate) fn alloc_local_memory(&mut self, memory: LocalMemoryObject) -> MemoryHandle {
        let id = LocalMemoryId::from_index(self.local_memories.len());
        self.local_memories.push(memory);
        MemoryHandle::Local(id)
    }

    pub(crate) fn new_memory(&mut self, page_count: u32, max_page_size: u32) -> ObjectRef {
        let handle = self.alloc_local_memory(
            LocalMemoryObject::new(page_count, max_page_size)
                .expect("validated local memory bounds must satisfy page_count <= max_page_size"),
        );
        self.object_ref_for_memory_handle(handle)
    }

    pub(crate) fn alloc_shared_memory(&mut self, memory: Arc<SharedMemoryObject>) -> MemoryHandle {
        let id = SharedMemoryId::from_index(self.shared_memories.len());
        self.shared_memories.push(memory);
        MemoryHandle::Shared(id)
    }

    pub(crate) fn new_shared_memory(&mut self, page_count: u32, max_page_size: u32) -> ObjectRef {
        let handle = self.alloc_shared_memory(
            SharedMemoryObject::new(page_count, max_page_size)
                .expect("validated shared memory bounds must satisfy page_count <= max_page_size"),
        );
        self.object_ref_for_memory_handle(handle)
    }

    pub(crate) fn memory_page_size(&self, handle: MemoryHandle) -> u32 {
        match handle {
            MemoryHandle::Local(id) => self.local_memories[id.index()].page_size(),
            MemoryHandle::Shared(id) => self.shared_memories[id.index()].page_size(),
        }
    }

    define_store_local_shared_const_read!(local_read_u8_array, shared_read_u8_array, read_u8_array);
    define_store_local_shared_const_stack!(
        local_push_memory_to_stack,
        shared_push_memory_to_stack,
        push_to_stack
    );
    define_store_local_shared_read!(local_read_u8_at, shared_read_u8_at, read_u8_at, u8);
    define_store_local_shared_read!(local_read_i8_at, shared_read_i8_at, read_i8_at, i8);
    define_store_local_shared_read!(local_read_u16_at, shared_read_u16_at, read_u16_at, u16);
    define_store_local_shared_read!(local_read_i16_at, shared_read_i16_at, read_i16_at, i16);
    define_store_local_shared_read!(local_read_u32_at, shared_read_u32_at, read_u32_at, u32);
    define_store_local_shared_read!(local_read_i32_at, shared_read_i32_at, read_i32_at, i32);
    define_store_local_shared_read!(local_read_u64_at, shared_read_u64_at, read_u64_at, u64);
    define_store_local_shared_read!(local_read_i64_at, shared_read_i64_at, read_i64_at, i64);
    define_store_local_shared_read!(local_read_f32_at, shared_read_f32_at, read_f32_at, f32);
    define_store_local_shared_read!(local_read_f64_at, shared_read_f64_at, read_f64_at, f64);
    define_store_local_shared_read!(
        local_atomic_load_u8,
        shared_atomic_load_u8,
        atomic_load_u8,
        u8
    );
    define_store_local_shared_read!(
        local_atomic_load_u16,
        shared_atomic_load_u16,
        atomic_load_u16,
        u16
    );
    define_store_local_shared_read!(
        local_atomic_load_u32,
        shared_atomic_load_u32,
        atomic_load_u32,
        u32
    );
    define_store_local_shared_read!(
        local_atomic_load_u64,
        shared_atomic_load_u64,
        atomic_load_u64,
        u64
    );
    define_store_local_shared_store!(
        local_atomic_store_u8,
        shared_atomic_store_u8,
        atomic_store_u8,
        u8
    );
    define_store_local_shared_store!(
        local_atomic_store_u16,
        shared_atomic_store_u16,
        atomic_store_u16,
        u16
    );
    define_store_local_shared_store!(
        local_atomic_store_u32,
        shared_atomic_store_u32,
        atomic_store_u32,
        u32
    );
    define_store_local_shared_store!(
        local_atomic_store_u64,
        shared_atomic_store_u64,
        atomic_store_u64,
        u64
    );
    define_store_local_shared_rmw!(
        local_atomic_rmw_u8,
        shared_atomic_rmw_u8,
        atomic_rmw_u8,
        u8,
        u8
    );
    define_store_local_shared_rmw!(
        local_atomic_rmw_u16,
        shared_atomic_rmw_u16,
        atomic_rmw_u16,
        u16,
        u16
    );
    define_store_local_shared_rmw!(
        local_atomic_rmw_u32,
        shared_atomic_rmw_u32,
        atomic_rmw_u32,
        u32,
        u32
    );
    define_store_local_shared_rmw!(
        local_atomic_rmw_u64,
        shared_atomic_rmw_u64,
        atomic_rmw_u64,
        u64,
        u64
    );
    define_store_local_shared_cmpxchg!(
        local_atomic_cmpxchg_u8,
        shared_atomic_cmpxchg_u8,
        atomic_cmpxchg_u8,
        u8,
        u8
    );
    define_store_local_shared_cmpxchg!(
        local_atomic_cmpxchg_u16,
        shared_atomic_cmpxchg_u16,
        atomic_cmpxchg_u16,
        u16,
        u16
    );
    define_store_local_shared_cmpxchg!(
        local_atomic_cmpxchg_u32,
        shared_atomic_cmpxchg_u32,
        atomic_cmpxchg_u32,
        u32,
        u32
    );
    define_store_local_shared_cmpxchg!(
        local_atomic_cmpxchg_u64,
        shared_atomic_cmpxchg_u64,
        atomic_cmpxchg_u64,
        u64,
        u64
    );

    #[inline(always)]
    pub(crate) fn local_atomic_fence(&mut self, id: LocalMemoryId) {
        self.local_memory(id).atomic_fence();
    }

    #[inline(always)]
    pub(crate) fn shared_atomic_fence(&mut self, id: SharedMemoryId) {
        self.shared_memory(id).atomic_fence();
    }

    #[inline(always)]
    pub(crate) fn local_write_bytes(
        &mut self,
        id: LocalMemoryId,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.local_memory_mut(id).write_bytes(offset, bytes)
    }

    #[inline(always)]
    pub(crate) fn shared_write_bytes(
        &mut self,
        id: SharedMemoryId,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        self.shared_memory(id).write_bytes(offset, bytes)
    }

    #[inline(always)]
    pub(crate) fn local_grow_memory(
        &mut self,
        id: LocalMemoryId,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        self.local_memory_mut(id).grow(page_size_delta)
    }

    #[inline(always)]
    pub(crate) fn shared_grow_memory(
        &mut self,
        id: SharedMemoryId,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        self.shared_memory(id).grow(page_size_delta)
    }

    #[inline(always)]
    pub(crate) fn local_copy_memory(
        &mut self,
        id: LocalMemoryId,
        dst: u32,
        src: u32,
        len: u32,
    ) -> VMResult<()> {
        self.local_memory_mut(id).copy(dst, src, len)
    }

    #[inline(always)]
    pub(crate) fn shared_copy_memory(
        &mut self,
        id: SharedMemoryId,
        dst: u32,
        src: u32,
        len: u32,
    ) -> VMResult<()> {
        self.shared_memory(id).copy(dst, src, len)
    }

    #[inline(always)]
    pub(crate) fn local_fill_memory(
        &mut self,
        id: LocalMemoryId,
        ptr: u32,
        len: u32,
        data: u32,
    ) -> VMResult<()> {
        self.local_memory_mut(id).fill(ptr, len, data)
    }

    #[inline(always)]
    pub(crate) fn shared_fill_memory(
        &mut self,
        id: SharedMemoryId,
        ptr: u32,
        len: u32,
        data: u32,
    ) -> VMResult<()> {
        self.shared_memory(id).fill(ptr, len, data)
    }

    #[inline(never)]
    fn local_read_bytes_to_vec(
        &self,
        id: LocalMemoryId,
        offset: u32,
        len: u32,
    ) -> VMResult<Vec<u8>> {
        let start = offset as usize;
        let end = vm_try!(VMResult::from_option(
            start.checked_add(len as usize),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        let bytes = vm_try!(VMResult::from_option(
            self.local_memory(id).memory().get(start..end),
            || VMResult::MemoryIndexOutOfRange
        ));
        VMResult::Success(bytes.to_vec())
    }

    #[inline(never)]
    fn shared_read_bytes_to_vec(
        &self,
        id: SharedMemoryId,
        offset: u32,
        len: u32,
    ) -> VMResult<Vec<u8>> {
        let start = offset as usize;
        let end = vm_try!(VMResult::from_option(
            start.checked_add(len as usize),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        self.shared_memory(id).with_memory(|memory| {
            VMResult::Success(
                vm_try!(VMResult::from_option(memory.get(start..end), || {
                    VMResult::MemoryIndexOutOfRange
                }))
                .to_vec(),
            )
        })
    }

    #[inline(never)]
    pub(crate) fn copy_memory_local_to_local(
        &mut self,
        dst: LocalMemoryId,
        src: LocalMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        if dst == src {
            return self.local_copy_memory(dst, dst_offset, src_offset, len);
        }
        let bytes = vm_try!(self.local_read_bytes_to_vec(src, src_offset, len));
        self.local_write_bytes(dst, dst_offset as usize, &bytes)
    }

    #[inline(never)]
    pub(crate) fn copy_memory_local_to_shared(
        &mut self,
        dst: SharedMemoryId,
        src: LocalMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        let bytes = vm_try!(self.local_read_bytes_to_vec(src, src_offset, len));
        self.shared_write_bytes(dst, dst_offset as usize, &bytes)
    }

    #[inline(never)]
    pub(crate) fn copy_memory_shared_to_local(
        &mut self,
        dst: LocalMemoryId,
        src: SharedMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        let bytes = vm_try!(self.shared_read_bytes_to_vec(src, src_offset, len));
        self.local_write_bytes(dst, dst_offset as usize, &bytes)
    }

    #[inline(never)]
    pub(crate) fn copy_memory_shared_to_shared(
        &mut self,
        dst: SharedMemoryId,
        src: SharedMemoryId,
        dst_offset: u32,
        src_offset: u32,
        len: u32,
    ) -> VMResult<()> {
        if dst == src {
            return self.shared_copy_memory(dst, dst_offset, src_offset, len);
        }
        let bytes = vm_try!(self.shared_read_bytes_to_vec(src, src_offset, len));
        self.shared_write_bytes(dst, dst_offset as usize, &bytes)
    }

    #[inline(always)]
    pub(crate) fn read_u8_array<const N: usize>(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
    ) -> VMResult<[u8; N]> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_u8_array::<N>(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_u8_array::<N>(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn push_memory_to_stack<const N: usize>(
        &mut self,
        handle: MemoryHandle,
        stack: &mut Stack,
        offset: usize,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_push_memory_to_stack::<N>(id, stack, offset),
            MemoryHandle::Shared(id) => self.shared_push_memory_to_stack::<N>(id, stack, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_u8_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u8> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_u8_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_u8_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_i8_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<i8> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_i8_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_i8_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_u16_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u16> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_u16_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_u16_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_i16_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<i16> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_i16_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_i16_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_u32_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u32> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_u32_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_u32_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_i32_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<i32> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_i32_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_i32_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_u64_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u64> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_u64_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_u64_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_i64_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<i64> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_i64_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_i64_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_f32_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<f32> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_f32_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_f32_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn read_f64_at(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<f64> {
        match handle {
            MemoryHandle::Local(id) => self.local_read_f64_at(id, offset),
            MemoryHandle::Shared(id) => self.shared_read_f64_at(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u8(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u8> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_load_u8(id, offset),
            MemoryHandle::Shared(id) => self.shared_atomic_load_u8(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u16(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u16> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_load_u16(id, offset),
            MemoryHandle::Shared(id) => self.shared_atomic_load_u16(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u32(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u32> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_load_u32(id, offset),
            MemoryHandle::Shared(id) => self.shared_atomic_load_u32(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_load_u64(&mut self, handle: MemoryHandle, offset: usize) -> VMResult<u64> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_load_u64(id, offset),
            MemoryHandle::Shared(id) => self.shared_atomic_load_u64(id, offset),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u8,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_store_u8(id, offset, value),
            MemoryHandle::Shared(id) => self.shared_atomic_store_u8(id, offset, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u16,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_store_u16(id, offset, value),
            MemoryHandle::Shared(id) => self.shared_atomic_store_u16(id, offset, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u32,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_store_u32(id, offset, value),
            MemoryHandle::Shared(id) => self.shared_atomic_store_u32(id, offset, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_store_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        value: u64,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_store_u64(id, offset, value),
            MemoryHandle::Shared(id) => self.shared_atomic_store_u64(id, offset, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_rmw_u8(id, offset, op, value),
            MemoryHandle::Shared(id) => self.shared_atomic_rmw_u8(id, offset, op, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_rmw_u16(id, offset, op, value),
            MemoryHandle::Shared(id) => self.shared_atomic_rmw_u16(id, offset, op, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_rmw_u32(id, offset, op, value),
            MemoryHandle::Shared(id) => self.shared_atomic_rmw_u32(id, offset, op, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_rmw_u64(id, offset, op, value),
            MemoryHandle::Shared(id) => self.shared_atomic_rmw_u64(id, offset, op, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u8(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u8,
        value: u8,
    ) -> VMResult<u8> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_cmpxchg_u8(id, offset, expected, value),
            MemoryHandle::Shared(id) => self.shared_atomic_cmpxchg_u8(id, offset, expected, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u16(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u16,
        value: u16,
    ) -> VMResult<u16> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_cmpxchg_u16(id, offset, expected, value),
            MemoryHandle::Shared(id) => self.shared_atomic_cmpxchg_u16(id, offset, expected, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u32(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u32,
        value: u32,
    ) -> VMResult<u32> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_cmpxchg_u32(id, offset, expected, value),
            MemoryHandle::Shared(id) => self.shared_atomic_cmpxchg_u32(id, offset, expected, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_cmpxchg_u64(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        expected: u64,
        value: u64,
    ) -> VMResult<u64> {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_cmpxchg_u64(id, offset, expected, value),
            MemoryHandle::Shared(id) => self.shared_atomic_cmpxchg_u64(id, offset, expected, value),
        }
    }

    #[inline(always)]
    pub(crate) fn atomic_fence(&mut self, handle: MemoryHandle) {
        match handle {
            MemoryHandle::Local(id) => self.local_atomic_fence(id),
            MemoryHandle::Shared(id) => self.shared_atomic_fence(id),
        }
    }

    #[inline(always)]
    pub(crate) fn write_bytes(
        &mut self,
        handle: MemoryHandle,
        offset: usize,
        bytes: &[u8],
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_write_bytes(id, offset, bytes),
            MemoryHandle::Shared(id) => self.shared_write_bytes(id, offset, bytes),
        }
    }

    #[inline(always)]
    pub(crate) fn grow_memory(
        &mut self,
        handle: MemoryHandle,
        page_size_delta: u32,
    ) -> VMResult<i32> {
        match handle {
            MemoryHandle::Local(id) => self.local_grow_memory(id, page_size_delta),
            MemoryHandle::Shared(id) => self.shared_grow_memory(id, page_size_delta),
        }
    }

    #[inline(always)]
    pub(crate) fn copy_memory(
        &mut self,
        handle: MemoryHandle,
        dst: u32,
        src: u32,
        len: u32,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_copy_memory(id, dst, src, len),
            MemoryHandle::Shared(id) => self.shared_copy_memory(id, dst, src, len),
        }
    }

    #[inline(always)]
    pub(crate) fn fill_memory(
        &mut self,
        handle: MemoryHandle,
        ptr: u32,
        len: u32,
        data: u32,
    ) -> VMResult<()> {
        match handle {
            MemoryHandle::Local(id) => self.local_fill_memory(id, ptr, len, data),
            MemoryHandle::Shared(id) => self.shared_fill_memory(id, ptr, len, data),
        }
    }

    pub(crate) fn memory_handle(&self, addr: ObjectRef) -> MemoryHandle {
        let (kind, index) = decode_object_ref(addr);
        match kind {
            ObjectKind::LocalMemory => MemoryHandle::Local(LocalMemoryId::from_index(index)),
            ObjectKind::SharedMemory => MemoryHandle::Shared(SharedMemoryId::from_index(index)),
            _ => panic!("invalid memory ref: {:?}", addr),
        }
    }

    pub(crate) fn object_ref_for_memory_handle(&self, handle: MemoryHandle) -> ObjectRef {
        match handle {
            MemoryHandle::Local(id) => encode_object_ref(ObjectKind::LocalMemory, id.raw()),
            MemoryHandle::Shared(id) => encode_object_ref(ObjectKind::SharedMemory, id.raw()),
        }
    }

    pub(crate) fn local_memory(&self, id: LocalMemoryId) -> &LocalMemoryObject {
        &self.local_memories[id.index()]
    }

    pub(crate) fn local_memory_mut(&mut self, id: LocalMemoryId) -> &mut LocalMemoryObject {
        &mut self.local_memories[id.index()]
    }

    pub(crate) fn get_memory(&mut self, addr: ObjectRef) -> &mut super::Memory {
        match self.memory_handle(addr) {
            MemoryHandle::Local(id) => self.local_memory_mut(id).memory_mut(),
            MemoryHandle::Shared(_) => panic!("shared memory requires shared memory APIs"),
        }
    }

    pub(crate) fn with_memory_by_addr<T>(
        &mut self,
        addr: ObjectRef,
        f: impl FnOnce(&mut super::Memory) -> T,
    ) -> T {
        match self.memory_handle(addr) {
            MemoryHandle::Local(id) => f(self.local_memory_mut(id).memory_mut()),
            MemoryHandle::Shared(id) => self.shared_memory(id).with_memory(f),
        }
    }

    pub(crate) fn shared_memory(&self, id: SharedMemoryId) -> &SharedMemoryObject {
        self.shared_memories[id.index()].as_ref()
    }

    pub(crate) fn clone_shared_memory(&self, id: SharedMemoryId) -> Arc<SharedMemoryObject> {
        self.shared_memories[id.index()].clone()
    }
}

#[derive(Debug, Clone)]
pub struct InstanceHandle {
    pub(crate) store_identity: Weak<()>,
    pub(crate) instance: InstanceId,
    pub(crate) instance_id: u32,
    pub(crate) object_ref: ObjectRef,
}

impl InstanceHandle {
    pub(crate) fn new(store: &Store, instance: InstanceId, instance_id: u32) -> Self {
        Self {
            store_identity: store.identity_weak(),
            instance,
            instance_id,
            object_ref: encode_object_ref(ObjectKind::Instance, instance.raw()),
        }
    }

    pub(crate) fn matches_store(&self, store: &Store) -> bool {
        store.matches_identity(&self.store_identity)
    }

    pub(crate) fn instance_id(&self) -> u32 {
        self.instance_id
    }

    pub(crate) fn object_ref_for_store(&self, store: &Store) -> Option<ObjectRef> {
        self.matches_store(store).then_some(self.object_ref)
    }
}

#[derive(Clone)]
pub struct StoreReentryToken {
    store_identity: Weak<()>,
}

#[derive(Debug)]
pub(crate) enum StoreExecutionError {
    ReentrantCallDenied(&'static str),
}

impl fmt::Display for StoreExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReentrantCallDenied(api) => write!(
                f,
                "{api} is unsupported while the same store execution is already active on this thread"
            ),
        }
    }
}

pub(crate) struct StoreRuntimeGuard<'a> {
    guard: MutexGuard<'a, StoreInner>,
    identity: *const (),
}

impl Deref for StoreRuntimeGuard<'_> {
    type Target = StoreInner;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl DerefMut for StoreRuntimeGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl Drop for StoreRuntimeGuard<'_> {
    fn drop(&mut self) {
        ACTIVE_STORE_RUNTIME.with(|active| {
            let mut active = active.borrow_mut();
            let index = active
                .iter()
                .rposition(|(identity, _)| *identity == self.identity)
                .expect("store runtime stack must stay balanced");
            let (identity, _) = active.remove(index);
            debug_assert_eq!(identity, self.identity);
        });
    }
}

pub struct Store {
    runtime: Arc<Mutex<StoreInner>>,
    identity: Arc<()>,
    segments: Mutex<StoreSegments>,
    next_instance_id: AtomicU32,
    pub state: StoreState,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        Self::new_with_state(StoreState::default())
    }

    pub fn new_with_state(state: StoreState) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(StoreInner::new())),
            identity: Arc::new(()),
            segments: Mutex::new(StoreSegments::default()),
            next_instance_id: AtomicU32::new(1),
            state,
        }
    }

    pub(crate) fn lock_runtime_unchecked(&self) -> StoreRuntimeGuard<'_> {
        let mut guard = self.runtime.lock();
        let identity_ptr = Arc::as_ptr(&self.identity);
        let runtime_ptr = (&mut *guard) as *mut StoreInner;
        ACTIVE_STORE_RUNTIME.with(|active| active.borrow_mut().push((identity_ptr, runtime_ptr)));
        StoreRuntimeGuard {
            guard,
            identity: identity_ptr,
        }
    }

    pub(crate) fn lock_runtime(
        &self,
        api_name: &'static str,
    ) -> Result<StoreRuntimeGuard<'_>, StoreExecutionError> {
        if self.has_active_runtime_on_current_thread() {
            return Err(StoreExecutionError::ReentrantCallDenied(api_name));
        }
        Ok(self.lock_runtime_unchecked())
    }

    pub(crate) fn with_runtime<T>(
        &self,
        api_name: &'static str,
        f: impl FnOnce(&mut StoreInner) -> T,
    ) -> Result<T, StoreExecutionError> {
        let mut runtime = self.lock_runtime(api_name)?;
        Ok(f(&mut runtime))
    }

    pub(crate) fn with_active_runtime<T>(&self, f: impl FnOnce(&mut StoreInner) -> T) -> Option<T> {
        let identity_ptr = Arc::as_ptr(&self.identity);
        ACTIVE_STORE_RUNTIME.with(|active| {
            let active = active.borrow();
            let (_, runtime_ptr) = active
                .iter()
                .rev()
                .find(|(identity, _)| *identity == identity_ptr)?;
            // SAFETY: only used for explicit nested sync reentry on the current thread.
            Some(unsafe { f(&mut **runtime_ptr) })
        })
    }

    pub(crate) fn current_reentry_token(&self) -> Option<StoreReentryToken> {
        self.has_active_runtime_on_current_thread()
            .then(|| StoreReentryToken {
                store_identity: self.identity_weak(),
            })
    }

    pub(crate) fn with_reentry_token<T>(token: &StoreReentryToken, f: impl FnOnce() -> T) -> T {
        let identity = token
            .store_identity
            .upgrade()
            .map(|identity| Arc::as_ptr(&identity))
            .unwrap_or(std::ptr::null());
        ACTIVE_STORE_REENTRY.with(|stack| stack.borrow_mut().push(identity));
        struct ReentryGuard;
        impl Drop for ReentryGuard {
            fn drop(&mut self) {
                ACTIVE_STORE_REENTRY.with(|stack| {
                    stack
                        .borrow_mut()
                        .pop()
                        .expect("store reentry stack must stay balanced");
                });
            }
        }
        let _guard = ReentryGuard;
        f()
    }

    pub(crate) fn can_reenter_current_thread(&self) -> bool {
        let identity_ptr = Arc::as_ptr(&self.identity);
        ACTIVE_STORE_REENTRY.with(|stack| stack.borrow().iter().rev().any(|it| *it == identity_ptr))
    }

    pub(crate) fn identity_weak(&self) -> Weak<()> {
        Arc::downgrade(&self.identity)
    }

    pub(crate) fn lock_gc(&self) -> StoreRuntimeGuard<'_> {
        self.lock_runtime("lock_gc")
            .expect("lock_gc is unsupported while the same store execution is already active on this thread")
    }

    pub(crate) fn with_gc<T>(&self, f: impl FnOnce(&mut StoreInner) -> T) -> T {
        let mut runtime = self.lock_gc();
        f(&mut runtime)
    }

    pub(crate) fn has_active_gc_on_current_thread(&self) -> bool {
        self.has_active_runtime_on_current_thread()
    }

    pub(crate) fn lock_segments(&self) -> MutexGuard<'_, StoreSegments> {
        self.segments.lock()
    }

    pub(crate) fn new_instance_id(&self) -> u32 {
        self.next_instance_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn matches_identity(&self, identity: &Weak<()>) -> bool {
        identity
            .upgrade()
            .is_some_and(|identity| Arc::ptr_eq(&self.identity, &identity))
    }

    pub(crate) fn has_active_runtime_on_current_thread(&self) -> bool {
        let identity_ptr = Arc::as_ptr(&self.identity);
        ACTIVE_STORE_RUNTIME.with(|active| {
            active
                .borrow()
                .iter()
                .rev()
                .any(|(identity, _)| *identity == identity_ptr)
        })
    }
}

#[derive(Default, Clone, Copy)]
pub struct StoreState(usize);

impl StoreState {
    pub const fn empty() -> Self {
        StoreState(0)
    }

    pub fn from_static<T>(data: &'static T) -> Self
    where
        T: Sync,
    {
        unsafe { Self::from_ptr(data as *const T) }
    }

    /// # Safety
    ///
    /// `data` must remain valid for the entire time the store may expose this state,
    /// and the pointed-to value must be safe to share across threads.
    pub unsafe fn from_ptr<T>(data: *const T) -> Self
    where
        T: Sync,
    {
        StoreState(data.cast::<()>() as usize)
    }

    #[inline]
    /// # Safety
    ///
    /// The stored pointer must either be null or point to a live value of type `T`
    /// for the duration of the returned reference.
    pub unsafe fn get<T>(&self) -> Option<&T> {
        let ptr = self.0 as *const T;
        ptr.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LocalMemoryObject, MemoryHandle, SharedMemoryId, SharedMemoryObject, Stack, Store,
        StoreInner, StoreState, VMResult,
    };
    use crate::common::PAGE_SIZE;

    fn local_id(handle: MemoryHandle) -> super::LocalMemoryId {
        match handle {
            MemoryHandle::Local(id) => id,
            MemoryHandle::Shared(_) => panic!("expected local memory handle"),
        }
    }

    fn shared_id(handle: MemoryHandle) -> SharedMemoryId {
        match handle {
            MemoryHandle::Shared(id) => id,
            MemoryHandle::Local(_) => panic!("expected shared memory handle"),
        }
    }

    #[test]
    fn test_state() {
        static DATA: [i32; 3] = [1, 2, 3];
        let state = StoreState::from_static(&DATA);
        let value = unsafe { state.get::<[i32; 3]>() }.unwrap();
        assert_eq!(value, &DATA);
    }

    #[test]
    fn runtime_guard_rejects_same_thread_reentry() {
        let store = Store::new();
        let _guard = store.lock_runtime("lock_runtime").unwrap();
        let err = match store.lock_runtime("lock_runtime") {
            Ok(_) => panic!("lock_runtime should fail closed during same-thread reentry"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "lock_runtime is unsupported while the same store execution is already active on this thread"
        );
    }

    #[test]
    fn store_memory_dispatch_matches_handle_kind_and_cross_copy_paths() {
        let mut store = StoreInner::new();
        let local = local_id(store.alloc_local_memory(LocalMemoryObject::new(1, 3).unwrap()));
        let shared = shared_id(store.alloc_shared_memory(SharedMemoryObject::new(1, 3).unwrap()));
        let local_dst = local_id(store.alloc_local_memory(LocalMemoryObject::new(1, 3).unwrap()));
        let shared_dst =
            shared_id(store.alloc_shared_memory(SharedMemoryObject::new(1, 3).unwrap()));

        store
            .write_bytes(MemoryHandle::Local(local), 0, &[1, 2, 3, 4])
            .unwrap();
        store
            .copy_memory(MemoryHandle::Local(local), 8, 0, 4)
            .unwrap();
        store
            .fill_memory(MemoryHandle::Local(local), 12, 4, 0xaa)
            .unwrap();

        store
            .write_bytes(MemoryHandle::Shared(shared), 0, &[9, 8, 7, 6])
            .unwrap();
        store
            .copy_memory(MemoryHandle::Shared(shared), 8, 0, 4)
            .unwrap();
        store
            .fill_memory(MemoryHandle::Shared(shared), 12, 4, 0xbb)
            .unwrap();

        store
            .copy_memory_local_to_shared(shared, local, 16, 8, 4)
            .unwrap();
        store
            .copy_memory_shared_to_local(local_dst, shared, 4, 16, 4)
            .unwrap();
        store
            .copy_memory_shared_to_shared(shared_dst, shared, 0, 12, 4)
            .unwrap();

        let mut stack = Stack::new(32);
        store
            .push_memory_to_stack::<4>(MemoryHandle::Local(local), &mut stack, 0)
            .unwrap();
        assert_eq!(stack.pop_u8_array::<4>(), [1, 2, 3, 4]);
        store
            .push_memory_to_stack::<4>(MemoryHandle::Shared(shared), &mut stack, 0)
            .unwrap();
        assert_eq!(stack.pop_u8_array::<4>(), [9, 8, 7, 6]);

        assert_eq!(store.grow_memory(MemoryHandle::Local(local), 1).unwrap(), 1);
        assert_eq!(
            store.grow_memory(MemoryHandle::Shared(shared), 1).unwrap(),
            1
        );

        assert_eq!(
            store
                .local_memory(local)
                .memory()
                .read_u8_array::<16>(0)
                .unwrap(),
            [1, 2, 3, 4, 0, 0, 0, 0, 1, 2, 3, 4, 0xaa, 0xaa, 0xaa, 0xaa]
        );
        assert_eq!(
            store
                .shared_memory(shared)
                .with_memory(|memory| memory.read_u8_array::<20>(0).unwrap()),
            [9, 8, 7, 6, 0, 0, 0, 0, 9, 8, 7, 6, 0xbb, 0xbb, 0xbb, 0xbb, 1, 2, 3, 4,]
        );
        assert_eq!(
            store
                .local_memory(local_dst)
                .memory()
                .read_u8_array::<8>(0)
                .unwrap(),
            [0, 0, 0, 0, 1, 2, 3, 4]
        );
        assert_eq!(
            store
                .shared_memory(shared_dst)
                .with_memory(|memory| memory.read_u8_array::<4>(0).unwrap()),
            [0xbb, 0xbb, 0xbb, 0xbb]
        );
        assert_eq!(
            store
                .local_memory(local)
                .memory()
                .read_u8_array::<8>(PAGE_SIZE)
                .unwrap(),
            [0; 8]
        );
        assert_eq!(
            store
                .shared_memory(shared)
                .with_memory(|memory| memory.read_u8_array::<8>(PAGE_SIZE).unwrap()),
            [0; 8]
        );
    }

    #[test]
    fn store_atomic_cmpxchg_dispatch_preserves_old_value_and_alignment_errors() {
        let mut store = StoreInner::new();
        let local = local_id(store.alloc_local_memory(LocalMemoryObject::new(1, 1).unwrap()));
        let shared = shared_id(store.alloc_shared_memory(SharedMemoryObject::new(1, 1).unwrap()));

        store
            .atomic_store_u32(MemoryHandle::Local(local), 4, 0x1122_3344)
            .unwrap();
        store
            .atomic_store_u32(MemoryHandle::Shared(shared), 8, 0x5566_7788)
            .unwrap();

        let local_old = store
            .atomic_cmpxchg_u32(MemoryHandle::Local(local), 4, 0x1122_3344, 0xaabb_ccdd)
            .unwrap();
        assert_eq!(local_old, 0x1122_3344);
        assert_eq!(
            store
                .atomic_load_u32(MemoryHandle::Local(local), 4)
                .unwrap(),
            0xaabb_ccdd
        );

        let shared_old = store
            .atomic_cmpxchg_u32(MemoryHandle::Shared(shared), 8, 0xdead_beef, 0xfeed_face)
            .unwrap();
        assert_eq!(shared_old, 0x5566_7788);
        assert_eq!(
            store
                .atomic_load_u32(MemoryHandle::Shared(shared), 8)
                .unwrap(),
            0x5566_7788
        );

        assert!(matches!(
            store.atomic_cmpxchg_u32(MemoryHandle::Local(local), 2, 0, 1),
            VMResult::UnalignedAtomic
        ));
        assert!(matches!(
            store.atomic_cmpxchg_u32(MemoryHandle::Shared(shared), 2, 0, 1),
            VMResult::UnalignedAtomic
        ));
    }
}
