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
    AsyncHostFunctionDefinition, AsyncHostFuture, AsyncNativeModule, ExecuteContext,
    FuncType as CoreFuncType, HostFunctionDefinition, Instr, NativeModule, ReturnSlot, VMResult,
    ValType as CoreValType, WasmValue,
};
use crate::support::runtime::{instantiate_native_async_module, instantiate_native_module};
use crate::support::{aliasing, Module, Registry, ResultValue, Store, VMResult as CoreVMResult};
use crate::{
    ComponentError, ComponentFuture, ComponentInstance, ComponentLinker, ComponentProgram,
    ComponentValue,
};
use futures::executor::block_on;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

thread_local! {
    static HOST_BINDINGS: RefCell<HashMap<(u32, u32), Rc<HostBinding>>> = RefCell::new(HashMap::new());
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
    caches: Rc<RuntimeCaches>,
}

#[derive(Default)]
struct RuntimeCaches {
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
    error_contexts: RefCell<HashMap<u32, String>>,
    waitable_sets: RefCell<HashMap<u32, WaitableSet>>,
    stream_future_handles: RefCell<HashMap<u32, StreamFutureHandle>>,
    stream_payloads: RefCell<HashMap<u32, VecDeque<Option<ComponentValue>>>>,
    future_payloads: RefCell<HashMap<u32, Vec<ComponentValue>>>,
    waitable_events: RefCell<HashMap<u32, WaitableEvent>>,
    pending_stream_reads: RefCell<HashMap<u32, PendingStreamRead>>,
    pending_future_reads: RefCell<HashMap<u32, PendingFutureRead>>,
    task_returns: RefCell<Vec<Option<Vec<ComponentValue>>>>,
}

#[derive(Clone)]
struct ResourceRecord {
    rep: i32,
    destructor: Option<RuntimeCoreFunc>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamFutureKind {
    Stream,
    Future,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamFutureEnd {
    Readable,
    Writable,
}

#[derive(Clone, Copy)]
struct StreamFutureHandle {
    type_id: TypeId,
    kind: StreamFutureKind,
    end: StreamFutureEnd,
    peer: u32,
    waitable_set: Option<u32>,
}

#[derive(Clone)]
struct PendingStreamRead {
    type_id: TypeId,
    payload: Option<ValType>,
    options: RuntimeCanonicalOptions,
    program: Rc<ComponentProgram>,
    ptr: u32,
    count: u32,
}

#[derive(Clone)]
struct PendingFutureRead {
    type_id: TypeId,
    payload: Option<ValType>,
    options: RuntimeCanonicalOptions,
    program: Rc<ComponentProgram>,
    ptr: u32,
}

#[derive(Clone, Copy)]
struct WaitableEvent {
    code: WaitableEventCode,
    index: u32,
    payload: u32,
}

#[derive(Clone, Copy)]
enum WaitableEventCode {
    None = 0,
    StreamRead = 2,
    FutureRead = 4,
}

#[derive(Default)]
struct WaitableSet {
    members: Vec<u32>,
}

#[derive(Clone)]
struct CoreExportRef {
    instance: InstanceHandle,
    export_name: String,
}

#[derive(Clone)]
struct RuntimeCanonicalOptions {
    string_encoding: Option<CanonicalStringEncoding>,
    memory: Option<CoreExportRef>,
    realloc: Option<RuntimeCoreFunc>,
    post_return: Option<RuntimeCoreFunc>,
    callback: Option<RuntimeCoreFunc>,
    async_: bool,
    shared: Rc<SharedState>,
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
        options: Box<RuntimeCanonicalOptions>,
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
    ErrorContextNew {
        options: RuntimeCanonicalOptions,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    ErrorContextDebugMessage {
        options: RuntimeCanonicalOptions,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    ErrorContextDrop {
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    TaskCancel {
        signature: CoreFuncType,
    },
    SubtaskCancel {
        signature: CoreFuncType,
    },
    SubtaskDrop {
        signature: CoreFuncType,
    },
    WaitableSetNew {
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    WaitableSetWait {
        memory: CoreExportRef,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    WaitableSetPoll {
        memory: CoreExportRef,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    WaitableSetDrop {
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    WaitableJoin {
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    TaskReturn {
        result_func_type: FuncType,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    StreamFutureNew {
        type_id: TypeId,
        kind: StreamFutureKind,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    StreamRead {
        type_id: TypeId,
        payload: Option<ValType>,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    StreamWrite {
        type_id: TypeId,
        payload: Option<ValType>,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    StreamFutureCancel {
        type_id: TypeId,
        kind: StreamFutureKind,
        end: StreamFutureEnd,
        async_: bool,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    StreamFutureDrop {
        type_id: TypeId,
        kind: StreamFutureKind,
        end: StreamFutureEnd,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    FutureRead {
        type_id: TypeId,
        payload: Option<ValType>,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
    FutureWrite {
        type_id: TypeId,
        payload: Option<ValType>,
        options: RuntimeCanonicalOptions,
        program: Rc<ComponentProgram>,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
    },
}

mod canonical;
mod env;
mod host;

pub use env::instantiate;
