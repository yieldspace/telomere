use crate::ir::types::{Case, DefValType, FuncType, LabelValType, PrimValType, Type, ValType};
use crate::ir::{
    CanonicalOptions, CanonicalStringEncoding, Component, ComponentExport, CoreFunc, CoreInstance,
    CoreInstanceInlineExport, CoreMemory, CoreRelation, CoreTable, DefinedInstance, Func,
    GlobalIdx, Instance, InstanceImport, Relation, ResourceId, TypeId, Value,
};
use crate::linker::{ComponentLinkerInstance, LinkerBinding, ResourceDropFn};
use crate::support::common::InstanceHandle;
use crate::support::common::{
    FuncType as CoreFuncType, VMResult, ValType as CoreValType, WasmValue,
};
use crate::support::{aliasing, Module, Registry, ResultValue, Store, VMResult as CoreVMResult};
use crate::support::{instantiate_native_module, HostContext, HostFnDefinition, HostModule};
use crate::{ComponentError, ComponentInstance, ComponentLinker, ComponentProgram, ComponentValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

thread_local! {
    static HOST_BINDINGS: RefCell<HashMap<(u32, u32), Rc<HostBinding>>> = RefCell::new(HashMap::new());
}

/// Drives `future` to completion on the current thread.
///
/// This deliberately does not use `futures::executor::block_on`: the
/// `futures` executor takes a thread-local enter guard, so an internal
/// `block_on` panicked with `EnterError` whenever an embedder drove the
/// public API with `futures::executor::block_on` itself (#167). The poll
/// loop below enters no executor at all -- it only parks the current
/// thread while the future is pending -- so it composes with any outer
/// executor (futures, tokio, or a hand-written one) and with itself.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
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
    Value(ComponentValue),
}

#[derive(Clone)]
enum RuntimeExport {
    Component(RuntimeComponentDef),
    Instance(Rc<RuntimeComponentInstance>),
    Func(Rc<ResolvedCallable>),
    CoreModule(Box<Module>),
    Value(ComponentValue),
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
    handles: Rc<RefCell<HandleTable>>,
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
    values: RefCell<HashMap<GlobalIdx<Value>, ComponentValue>>,
}

#[derive(Default)]
struct SharedState {
    generic_resources: RefCell<HashMap<TypeId, ResourceId>>,
    /// Destructors the host registered for imported resource types, keyed by
    /// the resource type's identity in the guest program. Populated once at
    /// instantiation from the root component's imports.
    host_resource_drops: RefCell<HashMap<ResourceId, ResourceDropFn>>,
    /// Live handles of host-provided resources that have entered guest code,
    /// per resource type. A `canon resource.drop` removes the handle here
    /// before invoking the registered destructor; a handle that is absent
    /// (never seen or already dropped) traps, per the canonical ABI.
    host_resources: RefCell<HashMap<ResourceId, std::collections::HashSet<u32>>>,
    /// Opened component-bundle AoT artifacts (#201), keyed by core
    /// module index, for the instantiation that owns this state.
    /// `None` for ordinary (non-bundle) instantiation; defined core
    /// instances created from a keyed module get their artifact
    /// installed right after creation.
    #[cfg(feature = "aot-bundle")]
    aot_artifacts: Option<std::rc::Rc<crate::bundle::BundleArtifactSet>>,
}

#[derive(Clone)]
struct ResourceRecord {
    rep: i32,
    destructor: Option<RuntimeCoreFunc>,
}

/// One component instance's resource handle table (#317).
///
/// The canonical ABI gives every component instance a single handle
/// table shared by all of its resource types: indices allocate
/// sequentially from 1, and an index freed by `resource.drop` is reused
/// by the next allocation before any fresh index is handed out (LIFO).
/// Each entry remembers its resource type, so a handle of one type
/// cannot be used through another type's canon ops.
struct HandleTable {
    entries: HashMap<u32, HandleEntry>,
    free_list: Vec<u32>,
    next_index: u32,
}

impl Default for HandleTable {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            free_list: Vec::new(),
            next_index: 1,
        }
    }
}

struct HandleEntry {
    resource: ResourceId,
    record: ResourceRecord,
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
        options: Box<RuntimeCanonicalOptions>,
        program: Rc<ComponentProgram>,
        shared: Rc<SharedState>,
        handles: Rc<RefCell<HandleTable>>,
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
        shared: Rc<SharedState>,
        handles: Rc<RefCell<HandleTable>>,
    },
    ResourceNew {
        resource: ResourceId,
        destructor: Option<RuntimeCoreFunc>,
        signature: CoreFuncType,
        handles: Rc<RefCell<HandleTable>>,
    },
    ResourceDrop {
        resource: ResourceId,
        signature: CoreFuncType,
        shared: Rc<SharedState>,
        handles: Rc<RefCell<HandleTable>>,
    },
    ResourceRep {
        resource: ResourceId,
        signature: CoreFuncType,
        handles: Rc<RefCell<HandleTable>>,
    },
}

mod canonical;
mod env;
#[cfg(feature = "fuzzing")]
mod fuzz_api;
mod host;
pub(crate) mod name_match;

pub use env::instantiate;
#[cfg(feature = "aot-bundle")]
pub use env::instantiate_with_artifacts;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub use fuzz_api::{
    fuzz_canonical_lift_args, fuzz_canonical_lift_args_from_bytes, FuzzCanonicalLiftInput,
    FuzzStringEncoding, MAX_FUZZ_MEMORY_IMAGE_BYTES,
};
