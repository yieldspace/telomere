use crate::ir::types::{Case, DefValType, FuncType, LabelValType, PrimValType, Type, ValType};
use crate::ir::AnyGlobalIdx;
use crate::ir::{
    CanonicalOptions, CanonicalStringEncoding, Component, ComponentExport, CoreFunc, CoreInstance,
    CoreInstanceInlineExport, CoreMemory, CoreRelation, CoreTable, Func, GlobalIdx, Instance,
    InstanceImport, Relation, ResourceId, TypeId,
};
use crate::linker::{ComponentLinkerInstance, LinkerBinding};
use crate::support::common::InstanceHandle;
use crate::support::common::{
    ExecuteContext, FuncType as CoreFuncType, HostFunctionDefinition, Instr, NativeModule,
    VMResult, ValType as CoreValType, WasmValue,
};
use crate::support::runtime::instantiate_native_module;
use crate::support::{
    aliasing, run_module_function, Module, Registry, ResultValue, Store, VMResult as CoreVMResult,
};
use crate::{ComponentError, ComponentInstance, ComponentLinker, ComponentProgram, ComponentValue};
use futures::executor::block_on;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

thread_local! {
    static HOST_BINDINGS: RefCell<HashMap<(u32, u32), Rc<HostBinding>>> = RefCell::new(HashMap::new());
    static ACTIVE_COMPONENT_HOST_GC: Cell<*mut crate::support::common::gc::MemoryPool> = const { Cell::new(std::ptr::null_mut()) };
}

const MAX_FLAT_PARAMS: usize = 16;
const MAX_FLAT_RESULTS: usize = 1;

#[derive(Clone)]
pub(crate) struct RuntimeInstance {
    root: Rc<RuntimeComponentInstance>,
}

#[derive(Clone)]
enum RuntimeImport {
    Component(RuntimeComponentDef),
    Instance(Rc<RuntimeComponentInstance>),
    Func(Rc<ResolvedCallable>),
    CoreModule(Box<Module>),
}

#[derive(Clone)]
enum RuntimeExport {
    Component(RuntimeComponentDef),
    Instance(Rc<RuntimeComponentInstance>),
    Func(Rc<ResolvedCallable>),
    CoreModule(Box<Module>),
}

#[derive(Clone)]
struct RuntimeComponentDef {
    component: Component,
    env: Rc<RuntimeEnv>,
}

struct RuntimeComponentInstance {
    source: RuntimeComponentSource,
    env: Rc<RuntimeEnv>,
    exports: RefCell<HashMap<String, RuntimeExport>>,
}

#[derive(Clone)]
enum RuntimeComponentSource {
    Component(Component),
    LinkerInstance(ComponentLinkerInstance),
}

struct RuntimeEnv {
    program: Rc<ComponentProgram>,
    linker: ComponentLinker,
    parent: Option<Rc<RuntimeEnv>>,
    imports: HashMap<String, RuntimeImport>,
    shared: Rc<SharedState>,
    components: RefCell<HashMap<GlobalIdx<Component>, RuntimeComponentDef>>,
    instances: RefCell<HashMap<GlobalIdx<Instance>, Rc<RuntimeComponentInstance>>>,
    funcs: RefCell<HashMap<GlobalIdx<Func>, Rc<ResolvedCallable>>>,
    core_modules: RefCell<HashMap<GlobalIdx<crate::ir::CoreModule>, Module>>,
    core_instances: RefCell<HashMap<GlobalIdx<CoreInstance>, InstanceHandle>>,
    core_funcs: RefCell<HashMap<GlobalIdx<CoreFunc>, RuntimeCoreFunc>>,
    core_memories: RefCell<HashMap<GlobalIdx<CoreMemory>, CoreExportRef>>,
    core_tables: RefCell<HashMap<GlobalIdx<CoreTable>, CoreExportRef>>,
}

#[derive(Default)]
struct SharedState {
    next_resource_handle: Cell<u32>,
    resources: RefCell<HashMap<ResourceId, HashMap<u32, ResourceRecord>>>,
    generic_resources: RefCell<HashMap<TypeId, ResourceId>>,
}

#[derive(Clone)]
struct ResourceRecord {
    rep: i32,
    destructor: Option<RuntimeCoreFunc>,
}

#[derive(Clone)]
struct CoreExportRef {
    instance: InstanceHandle,
    export_name: String,
}

#[derive(Clone, Default)]
struct RuntimeCanonicalOptions {
    string_encoding: Option<CanonicalStringEncoding>,
    memory: Option<CoreExportRef>,
    realloc: Option<RuntimeCoreFunc>,
    post_return: Option<RuntimeCoreFunc>,
}

#[derive(Clone)]
enum RuntimeCoreFunc {
    Export {
        instance: InstanceHandle,
        export_name: String,
    },
    Host(Rc<HostBinding>),
}

#[derive(Clone)]
enum ResolvedCallable {
    Host(crate::linker::AsyncHostFn),
    Core(CoreExportRef),
    Lifted {
        core: RuntimeCoreFunc,
        func_type: FuncType,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
    },
}

#[derive(Clone)]
enum HostBinding {
    Lower {
        callable: Rc<ResolvedCallable>,
        func_type: FuncType,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
        signature: CoreFuncType,
    },
    ResourceNew {
        resource: ResourceId,
        destructor: Option<RuntimeCoreFunc>,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    ResourceDrop {
        resource: ResourceId,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    ResourceRep {
        resource: ResourceId,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
}

mod canonical;
mod env;
mod host;

pub use env::instantiate;
