use crate::common::{
    ExecuteContext, ExportDesc, FuncType as CoreFuncType, HostFunctionDefinition, Instr,
    NativeModule, VMResult, ValType as CoreValType, WasmValue,
};
use crate::component::ir::types::{DefValType, FuncType, PrimValType, Type, ValType};
use crate::component::ir::{
    CanonicalOptions, CanonicalStringEncoding, Component, ComponentExport, CoreFunc, CoreInstance,
    CoreInstanceInlineExport, CoreMemory, CoreRelation, CoreTable, Func, GlobalIdx, Instance,
    InstanceImport, Relation, ResourceId, TypeId,
};
use crate::component::linker::LinkerBinding;
use crate::component::{
    ComponentError, ComponentInstance, ComponentLinker, ComponentProgram, ComponentValue,
};
use crate::{
    aliasing, common::InstanceHandle, run_module_function, runtime::instantiate_native_module,
    Registry, ResultValue, Store, VMResult as CoreVMResult,
};
use futures::executor::block_on;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static HOST_BINDINGS: RefCell<HashMap<(u32, u32), Rc<HostBinding>>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub(crate) struct RuntimeInstance {
    root: Rc<RuntimeComponentInstance>,
}

#[derive(Clone)]
enum RuntimeImport {
    Component(RuntimeComponentDef),
    Instance(Rc<RuntimeComponentInstance>),
    Func(Rc<ResolvedCallable>),
    CoreModule(Box<crate::Module>),
}

#[derive(Clone)]
enum RuntimeExport {
    Component(RuntimeComponentDef),
    Instance(Rc<RuntimeComponentInstance>),
    Func(Rc<ResolvedCallable>),
    CoreModule(Box<crate::Module>),
}

#[derive(Clone)]
struct RuntimeComponentDef {
    component: Component,
    env: Rc<RuntimeEnv>,
}

struct RuntimeComponentInstance {
    component: Component,
    env: Rc<RuntimeEnv>,
    exports: RefCell<HashMap<String, RuntimeExport>>,
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
    core_modules: RefCell<HashMap<GlobalIdx<crate::component::ir::CoreModule>, crate::Module>>,
    core_instances: RefCell<HashMap<GlobalIdx<CoreInstance>, InstanceHandle>>,
    core_funcs: RefCell<HashMap<GlobalIdx<CoreFunc>, RuntimeCoreFunc>>,
    core_memories: RefCell<HashMap<GlobalIdx<CoreMemory>, CoreExportRef>>,
    core_tables: RefCell<HashMap<GlobalIdx<CoreTable>, CoreExportRef>>,
}

#[derive(Default)]
struct SharedState {
    next_resource_handle: Cell<u32>,
    resources: RefCell<HashMap<ResourceId, HashMap<u32, ResourceRecord>>>,
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
    _post_return: Option<RuntimeCoreFunc>,
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
    Host(crate::component::linker::AsyncHostFn),
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

pub async fn instantiate(
    program: ComponentProgram,
    store: &mut Store,
    linker: &ComponentLinker,
) -> Result<ComponentInstance, ComponentError> {
    let runtime = instantiate_sync(program.clone(), store, linker)?;
    Ok(ComponentInstance::new(program, runtime))
}

fn instantiate_sync(
    program: ComponentProgram,
    store: &mut Store,
    linker: &ComponentLinker,
) -> Result<RuntimeInstance, ComponentError> {
    let program = Rc::new(program);
    let shared = Rc::new(SharedState::default());
    let env = Rc::new(RuntimeEnv::new(
        program.clone(),
        linker.clone(),
        None,
        HashMap::new(),
        shared,
    ));
    let root = Rc::new(RuntimeComponentInstance::new(program.root.clone(), env));

    for name in &program.callable_exports {
        let export = root.resolve_export(name, store)?;
        if !matches!(export, RuntimeExport::Func(_)) {
            return Err(ComponentError::Link(format!(
                "callable export '{}' is unresolved",
                name
            )));
        }
    }

    Ok(RuntimeInstance { root })
}

impl RuntimeInstance {
    pub(crate) async fn call(
        &self,
        store: &mut Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_sync(store, name, args)
    }

    fn call_sync(
        &self,
        store: &mut Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        match self.root.resolve_export(name, store)? {
            RuntimeExport::Func(callable) => callable.call_sync(store, args),
            _ => Err(ComponentError::ExportNotFound(name.to_owned())),
        }
    }
}

impl RuntimeEnv {
    fn new(
        program: Rc<ComponentProgram>,
        linker: ComponentLinker,
        parent: Option<Rc<RuntimeEnv>>,
        imports: HashMap<String, RuntimeImport>,
        shared: Rc<SharedState>,
    ) -> Self {
        Self {
            program,
            linker,
            parent,
            imports,
            shared,
            components: RefCell::new(HashMap::new()),
            instances: RefCell::new(HashMap::new()),
            funcs: RefCell::new(HashMap::new()),
            core_modules: RefCell::new(HashMap::new()),
            core_instances: RefCell::new(HashMap::new()),
            core_funcs: RefCell::new(HashMap::new()),
            core_memories: RefCell::new(HashMap::new()),
            core_tables: RefCell::new(HashMap::new()),
        }
    }

    fn resolve_component(
        &self,
        idx: GlobalIdx<Component>,
        store: &mut Store,
    ) -> Result<RuntimeComponentDef, ComponentError> {
        if let Some(component) = self.components.borrow().get(&idx).cloned() {
            return Ok(component);
        }
        let component = match self.program.component_store.get(&idx) {
            Some(Relation::Defined(component)) => RuntimeComponentDef {
                component: component.clone(),
                env: Rc::new(self.clone_shallow()),
            },
            Some(Relation::Import(name)) => self.lookup_import_component(name)?,
            Some(Relation::FromExport(instance_idx, name)) => match self
                .resolve_instance(*instance_idx, store)?
                .resolve_export(name, store)?
            {
                RuntimeExport::Component(component) => component,
                _ => {
                    return Err(ComponentError::Link(format!(
                        "component export '{}' is not a component",
                        name
                    )))
                }
            },
            None => {
                return Err(ComponentError::Link(
                    "component relation missing".to_owned(),
                ))
            }
        };
        self.components.borrow_mut().insert(idx, component.clone());
        Ok(component)
    }

    fn resolve_instance(
        &self,
        idx: GlobalIdx<Instance>,
        store: &mut Store,
    ) -> Result<Rc<RuntimeComponentInstance>, ComponentError> {
        if let Some(instance) = self.instances.borrow().get(&idx).cloned() {
            return Ok(instance);
        }
        let instance = match self.program.instance_store.get(&idx) {
            Some(Relation::Defined(instance)) => {
                let component_idx = instance.component_idx.ok_or_else(|| {
                    ComponentError::Link("instance does not reference a component".to_owned())
                })?;
                let component = self.resolve_component(component_idx, store)?;
                let mut imports = HashMap::new();
                for (name, import) in &instance.imports {
                    let value = match import {
                        InstanceImport::CoreModule(idx) => RuntimeImport::CoreModule(Box::new(
                            self.resolve_core_module(*idx, store)?,
                        )),
                        InstanceImport::Func(idx) => {
                            RuntimeImport::Func(self.resolve_func(*idx, store)?)
                        }
                        InstanceImport::Component(idx) => {
                            RuntimeImport::Component(self.resolve_component(*idx, store)?)
                        }
                        InstanceImport::Instance(idx) => {
                            RuntimeImport::Instance(self.resolve_instance(*idx, store)?)
                        }
                    };
                    imports.insert(name.clone(), value);
                }
                let env = Rc::new(RuntimeEnv::new(
                    self.program.clone(),
                    self.linker.clone(),
                    Some(component.env.clone()),
                    imports,
                    self.shared.clone(),
                ));
                Rc::new(RuntimeComponentInstance::new(
                    component.component.clone(),
                    env,
                ))
            }
            Some(Relation::Import(name)) => self.lookup_import_instance(name)?,
            Some(Relation::FromExport(instance_idx, name)) => match self
                .resolve_instance(*instance_idx, store)?
                .resolve_export(name, store)?
            {
                RuntimeExport::Instance(instance) => instance,
                _ => {
                    return Err(ComponentError::Link(format!(
                        "instance export '{}' is not an instance",
                        name
                    )))
                }
            },
            None => return Err(ComponentError::Link("instance relation missing".to_owned())),
        };
        self.instances.borrow_mut().insert(idx, instance.clone());
        Ok(instance)
    }

    fn resolve_func(
        &self,
        idx: GlobalIdx<Func>,
        store: &mut Store,
    ) -> Result<Rc<ResolvedCallable>, ComponentError> {
        if let Some(func) = self.funcs.borrow().get(&idx).cloned() {
            return Ok(func);
        }
        let func = match self.program.func_store.get(&idx) {
            Some(Relation::Defined(Func::CanonLift {
                core_func,
                type_id,
                options,
            })) => {
                let core = self.resolve_core_func(*core_func, store)?;
                let func_type = program_func_type(&self.program, *type_id)?;
                let options = self.resolve_runtime_options(options, store)?;
                Rc::new(ResolvedCallable::Lifted {
                    core,
                    func_type,
                    options,
                    program: self.program.clone(),
                })
            }
            Some(Relation::Import(name)) => self.lookup_import_func(name)?,
            Some(Relation::FromExport(instance_idx, name)) => match self
                .resolve_instance(*instance_idx, store)?
                .resolve_export(name, store)?
            {
                RuntimeExport::Func(func) => func,
                _ => {
                    return Err(ComponentError::Link(format!(
                        "func export '{}' is not a function",
                        name
                    )))
                }
            },
            None => return Err(ComponentError::Link("function relation missing".to_owned())),
        };
        self.funcs.borrow_mut().insert(idx, func.clone());
        Ok(func)
    }

    fn resolve_core_module(
        &self,
        idx: GlobalIdx<crate::component::ir::CoreModule>,
        store: &mut Store,
    ) -> Result<crate::Module, ComponentError> {
        if let Some(module) = self.core_modules.borrow().get(&idx).cloned() {
            return Ok(module);
        }
        let module = match self.program.core_module_store.get(&idx) {
            Some(CoreRelation::Defined(module)) => module.module.clone(),
            Some(CoreRelation::ImportModule(name)) => self.lookup_import_core_module(name)?,
            Some(CoreRelation::FromExport(instance_idx, name)) => match self
                .resolve_instance(*instance_idx, store)?
                .resolve_export(name, store)?
            {
                RuntimeExport::CoreModule(module) => *module,
                _ => {
                    return Err(ComponentError::Link(format!(
                        "core module export '{}' is not a core module",
                        name
                    )))
                }
            },
            Some(CoreRelation::FromCoreExport(_, _)) => {
                return Err(ComponentError::Unsupported(
                    "core module aliases from core exports are not supported at runtime".to_owned(),
                ))
            }
            None => {
                return Err(ComponentError::Link(
                    "core module relation missing".to_owned(),
                ))
            }
        };
        self.core_modules.borrow_mut().insert(idx, module.clone());
        Ok(module)
    }

    fn resolve_core_instance(
        &self,
        idx: GlobalIdx<CoreInstance>,
        store: &mut Store,
    ) -> Result<InstanceHandle, ComponentError> {
        if let Some(instance) = self.core_instances.borrow().get(&idx).cloned() {
            return Ok(instance);
        }
        let instance = match self.program.core_instance_store.get(&idx) {
            Some(CoreRelation::Defined(CoreInstance::Defined {
                module_idx,
                imports,
            })) => {
                let module = self.resolve_core_module(*module_idx, store)?;
                let mut registry = Registry::new();
                for (name, instance_idx) in imports {
                    registry.register(
                        name.clone(),
                        self.resolve_core_instance(*instance_idx, store)?,
                    );
                }
                match block_on(crate::instantiate(module, store, &registry)) {
                    CoreVMResult::Success(instance) => instance,
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "core module instantiation",
                        ))
                    }
                }
            }
            Some(CoreRelation::Defined(CoreInstance::InlineExport { exports })) => {
                materialize_inline_core_instance(self, exports, store)?
            }
            Some(CoreRelation::FromExport(_, _)) => return Err(ComponentError::Unsupported(
                "core instances re-exported from component instances are not supported at runtime"
                    .to_owned(),
            )),
            Some(CoreRelation::FromCoreExport(_, _)) => {
                return Err(ComponentError::Unsupported(
                    "core instance alias from core export is not supported at runtime".to_owned(),
                ))
            }
            Some(CoreRelation::ImportModule(_)) => {
                return Err(ComponentError::Unsupported(
                    "core instance imports are not supported".to_owned(),
                ))
            }
            None => {
                return Err(ComponentError::Link(
                    "core instance relation missing".to_owned(),
                ))
            }
        };
        self.core_instances
            .borrow_mut()
            .insert(idx, instance.clone());
        Ok(instance)
    }

    fn resolve_core_func(
        &self,
        idx: GlobalIdx<CoreFunc>,
        store: &mut Store,
    ) -> Result<RuntimeCoreFunc, ComponentError> {
        if let Some(func) = self.core_funcs.borrow().get(&idx).cloned() {
            return Ok(func);
        }
        let func = match self.program.core_func_store.get(&idx) {
            Some(CoreRelation::Defined(CoreFunc::CanonLower {
                func,
                type_id,
                options,
                signature,
            })) => {
                let callable = self.resolve_func(*func, store)?;
                let func_type = program_func_type(&self.program, *type_id)?;
                let options = self.resolve_runtime_options(options, store)?;
                RuntimeCoreFunc::Host(Rc::new(HostBinding::Lower {
                    callable,
                    func_type,
                    options,
                    program: self.program.clone(),
                    signature: signature.clone(),
                }))
            }
            Some(CoreRelation::Defined(CoreFunc::CanonResourceNew { type_id })) => {
                let resource = program_resource_id(&self.program, *type_id)?;
                let destructor = match resource.dtor() {
                    Some(dtor) => Some(self.resolve_core_func(dtor.into(), store)?),
                    None => None,
                };
                RuntimeCoreFunc::Host(Rc::new(HostBinding::ResourceNew {
                    resource,
                    destructor,
                    signature: CoreFuncType::new(vec![CoreValType::I32], vec![CoreValType::I32]),
                    shared: self.shared.clone(),
                }))
            }
            Some(CoreRelation::Defined(CoreFunc::CanonResourceDrop { type_id })) => {
                let resource = program_resource_id(&self.program, *type_id)?;
                RuntimeCoreFunc::Host(Rc::new(HostBinding::ResourceDrop {
                    resource,
                    signature: CoreFuncType::new(vec![CoreValType::I32], vec![]),
                    shared: self.shared.clone(),
                }))
            }
            Some(CoreRelation::Defined(CoreFunc::CanonResourceRep { type_id })) => {
                let resource = program_resource_id(&self.program, *type_id)?;
                RuntimeCoreFunc::Host(Rc::new(HostBinding::ResourceRep {
                    resource,
                    signature: CoreFuncType::new(vec![CoreValType::I32], vec![CoreValType::I32]),
                    shared: self.shared.clone(),
                }))
            }
            Some(CoreRelation::FromCoreExport(instance_idx, name)) => {
                let instance = self.resolve_core_instance(*instance_idx, store)?;
                RuntimeCoreFunc::Export {
                    instance,
                    export_name: name.clone(),
                }
            }
            Some(CoreRelation::ImportModule(_)) => {
                return Err(ComponentError::Unsupported(
                    "core function imports are not supported".to_owned(),
                ))
            }
            Some(CoreRelation::FromExport(_, _)) => {
                return Err(ComponentError::Unsupported(
                    "core function aliases from component exports are not supported at runtime"
                        .to_owned(),
                ))
            }
            None => {
                return Err(ComponentError::Link(
                    "core function relation missing".to_owned(),
                ))
            }
        };
        self.core_funcs.borrow_mut().insert(idx, func.clone());
        Ok(func)
    }

    fn resolve_core_memory(
        &self,
        idx: GlobalIdx<CoreMemory>,
        store: &mut Store,
    ) -> Result<CoreExportRef, ComponentError> {
        if let Some(memory) = self.core_memories.borrow().get(&idx).cloned() {
            return Ok(memory);
        }
        let memory = match self.program.core_memory_store.get(&idx) {
            Some(CoreRelation::FromCoreExport(instance_idx, name)) => CoreExportRef {
                instance: self.resolve_core_instance(*instance_idx, store)?,
                export_name: name.clone(),
            },
            _ => {
                return Err(ComponentError::Unsupported(
                    "runtime only supports core memories from core exports".to_owned(),
                ))
            }
        };
        self.core_memories.borrow_mut().insert(idx, memory.clone());
        Ok(memory)
    }

    fn resolve_core_table(
        &self,
        idx: GlobalIdx<CoreTable>,
        store: &mut Store,
    ) -> Result<CoreExportRef, ComponentError> {
        if let Some(table) = self.core_tables.borrow().get(&idx).cloned() {
            return Ok(table);
        }
        let table = match self.program.core_table_store.get(&idx) {
            Some(CoreRelation::FromCoreExport(instance_idx, name)) => CoreExportRef {
                instance: self.resolve_core_instance(*instance_idx, store)?,
                export_name: name.clone(),
            },
            _ => {
                return Err(ComponentError::Unsupported(
                    "runtime only supports core tables from core exports".to_owned(),
                ))
            }
        };
        self.core_tables.borrow_mut().insert(idx, table.clone());
        Ok(table)
    }

    fn resolve_runtime_options(
        &self,
        options: &CanonicalOptions,
        store: &mut Store,
    ) -> Result<RuntimeCanonicalOptions, ComponentError> {
        Ok(RuntimeCanonicalOptions {
            string_encoding: options.string_encoding,
            memory: match options.memory {
                Some(memory) => Some(self.resolve_core_memory(memory, store)?),
                None => None,
            },
            realloc: match options.realloc {
                Some(realloc) => Some(self.resolve_core_func(realloc, store)?),
                None => None,
            },
            _post_return: match options.post_return {
                Some(post_return) => Some(self.resolve_core_func(post_return, store)?),
                None => None,
            },
        })
    }

    fn clone_shallow(&self) -> Self {
        Self {
            program: self.program.clone(),
            linker: self.linker.clone(),
            parent: self.parent.clone(),
            imports: self.imports.clone(),
            shared: self.shared.clone(),
            components: RefCell::new(self.components.borrow().clone()),
            instances: RefCell::new(self.instances.borrow().clone()),
            funcs: RefCell::new(self.funcs.borrow().clone()),
            core_modules: RefCell::new(self.core_modules.borrow().clone()),
            core_instances: RefCell::new(self.core_instances.borrow().clone()),
            core_funcs: RefCell::new(self.core_funcs.borrow().clone()),
            core_memories: RefCell::new(self.core_memories.borrow().clone()),
            core_tables: RefCell::new(self.core_tables.borrow().clone()),
        }
    }

    fn lookup_import_component(&self, name: &str) -> Result<RuntimeComponentDef, ComponentError> {
        match self.imports.get(name) {
            Some(RuntimeImport::Component(component)) => Ok(component.clone()),
            Some(_) => Err(ComponentError::Link(format!(
                "import '{}' is not a component",
                name
            ))),
            None => self
                .parent
                .as_ref()
                .map(|parent| parent.lookup_import_component(name))
                .unwrap_or_else(|| {
                    Err(ComponentError::Unsupported(format!(
                        "component import '{}' is not supported by linker",
                        name
                    )))
                }),
        }
    }

    fn lookup_import_instance(
        &self,
        name: &str,
    ) -> Result<Rc<RuntimeComponentInstance>, ComponentError> {
        match self.imports.get(name) {
            Some(RuntimeImport::Instance(instance)) => Ok(instance.clone()),
            Some(_) => Err(ComponentError::Link(format!(
                "import '{}' is not an instance",
                name
            ))),
            None => self
                .parent
                .as_ref()
                .map(|parent| parent.lookup_import_instance(name))
                .unwrap_or_else(|| {
                    Err(ComponentError::Unsupported(format!(
                        "instance import '{}' is not supported by linker",
                        name
                    )))
                }),
        }
    }

    fn lookup_import_func(&self, name: &str) -> Result<Rc<ResolvedCallable>, ComponentError> {
        match self.imports.get(name) {
            Some(RuntimeImport::Func(func)) => Ok(func.clone()),
            Some(_) => Err(ComponentError::Link(format!(
                "import '{}' is not a function",
                name
            ))),
            None => {
                if let Some(binding) = self.linker.resolve_import(name) {
                    return Ok(Rc::new(linker_binding_to_callable(binding)));
                }
                self.parent
                    .as_ref()
                    .map(|parent| parent.lookup_import_func(name))
                    .unwrap_or_else(|| {
                        Err(ComponentError::Link(format!(
                            "function import '{}' is unresolved",
                            name
                        )))
                    })
            }
        }
    }

    fn lookup_import_core_module(&self, name: &str) -> Result<crate::Module, ComponentError> {
        match self.imports.get(name) {
            Some(RuntimeImport::CoreModule(module)) => Ok((**module).clone()),
            Some(_) => Err(ComponentError::Link(format!(
                "import '{}' is not a core module",
                name
            ))),
            None => self
                .parent
                .as_ref()
                .map(|parent| parent.lookup_import_core_module(name))
                .unwrap_or_else(|| {
                    Err(ComponentError::Unsupported(format!(
                        "core module import '{}' is not supported by linker",
                        name
                    )))
                }),
        }
    }
}

impl RuntimeComponentInstance {
    fn new(component: Component, env: Rc<RuntimeEnv>) -> Self {
        Self {
            component,
            env,
            exports: RefCell::new(HashMap::new()),
        }
    }

    fn resolve_export(
        &self,
        name: &str,
        store: &mut Store,
    ) -> Result<RuntimeExport, ComponentError> {
        if let Some(export) = self.exports.borrow().get(name).cloned() {
            return Ok(export);
        }
        let export = match self.component.exports.get(name) {
            Some(ComponentExport::Component(idx)) => {
                RuntimeExport::Component(self.env.resolve_component(*idx, store)?)
            }
            Some(ComponentExport::Instance(idx)) => {
                RuntimeExport::Instance(self.env.resolve_instance(*idx, store)?)
            }
            Some(ComponentExport::Func(idx)) => {
                if self.env.parent.is_none() {
                    if let Some(Relation::Import(_)) = self.env.program.func_store.get(idx) {
                        if let Some(binding) = self.env.linker.resolve_export(name) {
                            let export =
                                RuntimeExport::Func(Rc::new(linker_binding_to_callable(binding)));
                            self.exports
                                .borrow_mut()
                                .insert(name.to_owned(), export.clone());
                            return Ok(export);
                        }
                    }
                }
                RuntimeExport::Func(self.env.resolve_func(*idx, store)?)
            }
            Some(ComponentExport::Module(idx)) => {
                RuntimeExport::CoreModule(Box::new(self.env.resolve_core_module(*idx, store)?))
            }
            Some(ComponentExport::Resource(_)) | None => {
                return Err(ComponentError::ExportNotFound(name.to_owned()))
            }
        };
        self.exports
            .borrow_mut()
            .insert(name.to_owned(), export.clone());
        Ok(export)
    }
}

impl ResolvedCallable {
    fn call_sync(
        &self,
        store: &mut Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        match self {
            ResolvedCallable::Host(func) => block_on(func(store, args)),
            ResolvedCallable::Core(export) => {
                let core_args = args
                    .iter()
                    .map(component_value_to_direct_wasm)
                    .collect::<Result<Vec<_>, _>>()?;
                let results = call_core_export_sync(
                    &export.instance,
                    store,
                    &export.export_name,
                    &core_args,
                )?;
                results
                    .into_iter()
                    .map(direct_wasm_to_component_value)
                    .collect()
            }
            ResolvedCallable::Lifted {
                core,
                func_type,
                options,
                program,
            } => {
                let core_args = lower_component_args(func_type, args, options, program, store)?;
                let core_results = core.call_sync(store, &core_args)?;
                lift_component_results(func_type, &core_results, options, program, store)
            }
        }
    }
}

impl RuntimeCoreFunc {
    fn call_sync(
        &self,
        store: &mut Store,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, ComponentError> {
        match self {
            RuntimeCoreFunc::Export {
                instance,
                export_name,
                ..
            } => call_core_export_sync(instance, store, export_name, args),
            RuntimeCoreFunc::Host(binding) => binding.call_sync(store, args),
        }
    }
}

impl HostBinding {
    fn signature(&self) -> CoreFuncType {
        match self {
            HostBinding::Lower { signature, .. }
            | HostBinding::ResourceNew { signature, .. }
            | HostBinding::ResourceDrop { signature, .. }
            | HostBinding::ResourceRep { signature, .. } => signature.clone(),
        }
    }

    fn call_sync(
        &self,
        store: &mut Store,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, ComponentError> {
        match self {
            HostBinding::Lower {
                callable,
                func_type,
                options,
                program,
                ..
            } => {
                let component_args = lift_component_args(func_type, args, options, program, store)?;
                let results = callable.call_sync(store, &component_args)?;
                lower_component_results(func_type, &results, options, program, store)
            }
            HostBinding::ResourceNew {
                resource,
                destructor,
                shared,
                ..
            } => {
                let rep = match args.first() {
                    Some(WasmValue::I32(v)) => *v,
                    Some(other) => {
                        return Err(ComponentError::Runtime(format!(
                            "resource.new expects i32, got {other:?}"
                        )))
                    }
                    None => {
                        return Err(ComponentError::Runtime(
                            "resource.new missing rep argument".to_owned(),
                        ))
                    }
                };
                let handle = shared.alloc_resource(*resource, rep, destructor.clone());
                Ok(vec![WasmValue::I32(handle as i32)])
            }
            HostBinding::ResourceDrop {
                resource, shared, ..
            } => {
                let handle = match args.first() {
                    Some(WasmValue::I32(v)) => *v as u32,
                    Some(other) => {
                        return Err(ComponentError::Runtime(format!(
                            "resource.drop expects i32, got {other:?}"
                        )))
                    }
                    None => {
                        return Err(ComponentError::Runtime(
                            "resource.drop missing handle".to_owned(),
                        ))
                    }
                };
                let (rep, destructor) = shared.drop_resource(*resource, handle)?;
                if let Some(dtor) = destructor {
                    let _ = dtor.call_sync(store, &[WasmValue::I32(rep)]);
                }
                Ok(Vec::new())
            }
            HostBinding::ResourceRep {
                resource, shared, ..
            } => {
                let handle = match args.first() {
                    Some(WasmValue::I32(v)) => *v as u32,
                    Some(other) => {
                        return Err(ComponentError::Runtime(format!(
                            "resource.rep expects i32, got {other:?}"
                        )))
                    }
                    None => {
                        return Err(ComponentError::Runtime(
                            "resource.rep missing handle".to_owned(),
                        ))
                    }
                };
                Ok(vec![WasmValue::I32(
                    shared.resource_rep(*resource, handle)?,
                )])
            }
        }
    }
}

impl SharedState {
    fn alloc_resource(
        &self,
        resource: ResourceId,
        rep: i32,
        destructor: Option<RuntimeCoreFunc>,
    ) -> u32 {
        let handle = self.next_resource_handle.get() + 1;
        self.next_resource_handle.set(handle);
        self.resources
            .borrow_mut()
            .entry(resource)
            .or_default()
            .insert(handle, ResourceRecord { rep, destructor });
        handle
    }

    fn drop_resource(
        &self,
        resource: ResourceId,
        handle: u32,
    ) -> Result<(i32, Option<RuntimeCoreFunc>), ComponentError> {
        self.resources
            .borrow_mut()
            .get_mut(&resource)
            .and_then(|records| records.remove(&handle))
            .map(|record| (record.rep, record.destructor))
            .ok_or_else(|| ComponentError::Trap(format!("resource handle {handle} is invalid")))
    }

    fn resource_rep(&self, resource: ResourceId, handle: u32) -> Result<i32, ComponentError> {
        self.resources
            .borrow()
            .get(&resource)
            .and_then(|records| records.get(&handle))
            .map(|record| record.rep)
            .ok_or_else(|| ComponentError::Trap(format!("resource handle {handle} is invalid")))
    }
}

fn materialize_inline_core_instance(
    env: &RuntimeEnv,
    exports: &HashMap<String, CoreInstanceInlineExport>,
    store: &mut Store,
) -> Result<InstanceHandle, ComponentError> {
    let mut registry = Registry::new();
    let mut triplets = Vec::new();
    let mut host_functions = Vec::new();

    for (export_name, export) in exports {
        match export {
            CoreInstanceInlineExport::Func(idx) => match env.resolve_core_func(*idx, store)? {
                RuntimeCoreFunc::Export {
                    instance,
                    export_name: source_name,
                    ..
                } => {
                    let module_name = format!("core-func-{}-{}", export_name, triplets.len());
                    registry.register(module_name.clone(), instance);
                    triplets.push((module_name, source_name, export_name.clone()));
                }
                RuntimeCoreFunc::Host(binding) => {
                    host_functions.push((export_name.clone(), binding))
                }
            },
            CoreInstanceInlineExport::Memory(idx) => {
                let memory = env.resolve_core_memory(*idx, store)?;
                let module_name = format!("core-memory-{}-{}", export_name, triplets.len());
                registry.register(module_name.clone(), memory.instance);
                triplets.push((module_name, memory.export_name, export_name.clone()));
            }
            CoreInstanceInlineExport::Table(idx) => {
                let table = env.resolve_core_table(*idx, store)?;
                let module_name = format!("core-table-{}-{}", export_name, triplets.len());
                registry.register(module_name.clone(), table.instance);
                triplets.push((module_name, table.export_name, export_name.clone()));
            }
            CoreInstanceInlineExport::Global(_)
            | CoreInstanceInlineExport::Type(_)
            | CoreInstanceInlineExport::Instance(_)
            | CoreInstanceInlineExport::Module(_) => {
                return Err(ComponentError::Unsupported(
                    "runtime inline core instances only support func/memory/table exports"
                        .to_owned(),
                ));
            }
        }
    }

    if !host_functions.is_empty() {
        let native = NativeModule {
            functions: host_functions
                .iter()
                .map(|(name, binding)| HostFunctionDefinition {
                    name: Some(name.clone()),
                    signature: binding.signature(),
                    fp: component_host_trampoline,
                })
                .collect(),
        };
        let host_instance =
            match block_on(instantiate_native_module(native, store, &Registry::new())) {
                CoreVMResult::Success(instance) => instance,
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline instantiation",
                    ))
                }
            };
        register_host_bindings(&host_instance, &host_functions, store);
        let module_name = format!("host-inline-{}", triplets.len());
        registry.register(module_name.clone(), host_instance);
        for (name, _) in host_functions {
            triplets.push((module_name.clone(), name.clone(), name));
        }
    }

    match aliasing(&registry, &triplets, store) {
        CoreVMResult::Success(instance) => Ok(instance),
        other => Err(vm_result_to_component_error(other, "core aliasing")),
    }
}

fn register_host_bindings(
    instance: &InstanceHandle,
    bindings: &[(String, Rc<HostBinding>)],
    store: &mut Store,
) {
    let instance_id = instance_id(instance, store);
    HOST_BINDINGS.with(|host_bindings| {
        let mut host_bindings = host_bindings.borrow_mut();
        for (funcidx, (_, binding)) in bindings.iter().enumerate() {
            host_bindings.insert((instance_id, funcidx as u32), binding.clone());
        }
    });
}

fn component_host_trampoline(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let key = (ctx.instance_id(), ctx.func().funcidx);
    let binding = HOST_BINDINGS.with(|bindings| bindings.borrow().get(&key).cloned());
    let Some(binding) = binding else {
        return VMResult::Unlinkable;
    };
    let args = match read_core_args_from_locals(ctx, &binding.signature()) {
        Ok(args) => args,
        Err(_) => return VMResult::Unreachable,
    };
    let results = match binding.call_sync(ctx.store, &args) {
        Ok(results) => results,
        Err(_) => return VMResult::Unreachable,
    };
    if push_core_results(ctx, &results).is_err() {
        return VMResult::Unreachable;
    }
    let return_size = binding
        .signature()
        .1
        .iter()
        .map(|ty| ty.stack_size().u32())
        .sum::<u32>() as usize;
    let (prev_local_ref, return_addr) =
        ctx.stack.function_return(&ctx.local_reference, return_size);
    ctx.local_reference = prev_local_ref;
    VMResult::Success(return_addr)
}

fn read_core_args_from_locals(
    ctx: &mut ExecuteContext,
    signature: &CoreFuncType,
) -> Result<Vec<WasmValue>, ComponentError> {
    let mut offset = 0u32;
    let mut args = Vec::with_capacity(signature.0 .0.len());
    for ty in signature.0.iter() {
        match ty {
            CoreValType::I32 | CoreValType::F32 | CoreValType::FuncRef | CoreValType::ExternRef => {
                match ctx
                    .stack
                    .local_get(&ctx.local_reference(), offset as usize, 4)
                {
                    VMResult::Success(()) => {}
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "host trampoline local_get",
                        ))
                    }
                }
            }
            CoreValType::I64 | CoreValType::F64 => {
                match ctx
                    .stack
                    .local_get(&ctx.local_reference(), offset as usize, 8)
                {
                    VMResult::Success(()) => {}
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "host trampoline local_get",
                        ))
                    }
                }
            }
            CoreValType::V128 => {
                match ctx
                    .stack
                    .local_get(&ctx.local_reference(), offset as usize, 16)
                {
                    VMResult::Success(()) => {}
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "host trampoline local_get",
                        ))
                    }
                }
            }
        }
        let value = match ty {
            CoreValType::I32 => WasmValue::I32(ctx.stack.pop_i32()),
            CoreValType::I64 => WasmValue::I64(ctx.stack.pop_i64()),
            CoreValType::F32 => WasmValue::F32(ctx.stack.pop_f32()),
            CoreValType::F64 => WasmValue::F64(ctx.stack.pop_f64()),
            CoreValType::FuncRef => WasmValue::FuncRef(ctx.stack.pop_u32()),
            CoreValType::ExternRef => WasmValue::ExternRef(ctx.stack.pop_u32()),
            CoreValType::V128 => WasmValue::V128(ctx.stack.pop_u128()),
        };
        args.push(value);
        offset += ty.stack_size().u32();
    }
    Ok(args)
}

fn push_core_results(
    ctx: &mut ExecuteContext,
    results: &[WasmValue],
) -> Result<(), ComponentError> {
    for value in results {
        match value {
            WasmValue::I32(v) => match ctx.stack.push_i32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::I64(v) => match ctx.stack.push_i64(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::F32(v) => match ctx.stack.push_f32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::F64(v) => match ctx.stack.push_f64(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::FuncRef(v) => match ctx.stack.push_u32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::ExternRef(v) => match ctx.stack.push_u32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::V128(v) => match ctx.stack.push_u128(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
        }
    }
    Ok(())
}

fn linker_binding_to_callable(binding: LinkerBinding) -> ResolvedCallable {
    match binding {
        LinkerBinding::Host(func) => ResolvedCallable::Host(func),
        LinkerBinding::Core(binding) => ResolvedCallable::Core(CoreExportRef {
            instance: binding.instance,
            export_name: binding.export_name,
        }),
    }
}

fn instance_id(instance: &InstanceHandle, store: &mut Store) -> u32 {
    let gc = store.gc.borrow();
    unsafe { (*gc.get_instance_unchecked(instance.get_gc_ref_with_pool(&gc))).instance_id }
}

fn call_core_export_sync(
    instance: &InstanceHandle,
    store: &mut Store,
    export_name: &str,
    args: &[WasmValue],
) -> Result<Vec<WasmValue>, ComponentError> {
    let result = block_on(run_module_function(
        instance,
        store,
        export_name,
        &ResultValue::new(args.to_vec()),
    ));
    match result {
        CoreVMResult::Success(values) => Ok(values.iter().copied().collect()),
        other => Err(vm_result_to_component_error(other, export_name)),
    }
}

fn vm_result_to_component_error(result: CoreVMResult<impl Sized>, context: &str) -> ComponentError {
    match result {
        CoreVMResult::Success(_) => unreachable!(),
        CoreVMResult::Unreachable => {
            ComponentError::Trap(format!("{context} trapped: unreachable"))
        }
        CoreVMResult::StackOverflow => {
            ComponentError::Trap(format!("{context} trapped: stack overflow"))
        }
        CoreVMResult::MemoryIndexOutOfRange => {
            ComponentError::Trap(format!("{context} trapped: memory index out of range"))
        }
        CoreVMResult::TableIndexOutOfRange => {
            ComponentError::Trap(format!("{context} trapped: table index out of range"))
        }
        CoreVMResult::CallIndirectInvalidType => {
            ComponentError::Trap(format!("{context} trapped: call indirect invalid type"))
        }
        CoreVMResult::TableUninitialized => {
            ComponentError::Trap(format!("{context} trapped: table uninitialized"))
        }
        CoreVMResult::Unlinkable => ComponentError::Link(format!("{context} failed: unlinkable")),
        CoreVMResult::InvalidOperand => {
            ComponentError::Runtime(format!("{context} failed: invalid operand"))
        }
    }
}

fn direct_wasm_to_component_value(value: WasmValue) -> Result<ComponentValue, ComponentError> {
    Ok(match value {
        WasmValue::I32(v) => ComponentValue::I32(v),
        WasmValue::I64(v) => ComponentValue::I64(v),
        WasmValue::F32(v) => ComponentValue::F32(v),
        WasmValue::F64(v) => ComponentValue::F64(v),
        WasmValue::FuncRef(v) => ComponentValue::Own(v),
        WasmValue::ExternRef(v) => ComponentValue::Borrow(v),
        WasmValue::V128(_) => {
            return Err(ComponentError::Unsupported(
                "v128 values are not supported in component runtime".to_owned(),
            ))
        }
    })
}

fn component_value_to_direct_wasm(value: &ComponentValue) -> Result<WasmValue, ComponentError> {
    Ok(match value {
        ComponentValue::Bool(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U8(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::S8(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U16(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::S16(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U32(v) => WasmValue::I32(*v as i32),
        ComponentValue::S32(v) | ComponentValue::I32(v) => WasmValue::I32(*v),
        ComponentValue::U64(v) => WasmValue::I64(*v as i64),
        ComponentValue::S64(v) | ComponentValue::I64(v) => WasmValue::I64(*v),
        ComponentValue::F32(v) => WasmValue::F32(*v),
        ComponentValue::F64(v) => WasmValue::F64(*v),
        ComponentValue::Own(v) | ComponentValue::Borrow(v) => WasmValue::I32(*v as i32),
        other => {
            return Err(ComponentError::Unsupported(format!(
                "direct core invocation does not support {other:?}"
            )))
        }
    })
}

fn lower_component_args(
    func_type: &FuncType,
    args: &[ComponentValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    if func_type.params.len() != args.len() {
        return Err(ComponentError::InvalidArgument(format!(
            "expected {} component arguments, got {}",
            func_type.params.len(),
            args.len()
        )));
    }
    let mut lowered = Vec::new();
    for (value, ty) in args.iter().zip(func_type.params.iter()) {
        lower_value(value, ty, options, program, store, &mut lowered)?;
    }
    if lowered.len() > 16 {
        return Err(ComponentError::Unsupported(
            "indirect canonical parameter passing is not implemented yet".to_owned(),
        ));
    }
    Ok(lowered)
}

fn lift_component_args(
    func_type: &FuncType,
    args: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let mut cursor = CoreValueCursor::new(args);
    func_type
        .params
        .iter()
        .map(|ty| lift_value(ty, options, program, store, &mut cursor))
        .collect()
}

fn lift_component_results(
    func_type: &FuncType,
    results: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        return Ok(Vec::new());
    };
    let mut cursor = CoreValueCursor::new(results);
    let result = if value_flat_len(result_ty, program)? > 1 {
        let pointer = cursor.next_i32()? as u32;
        lift_indirect_result(result_ty, options, program, store, pointer)?
    } else {
        lift_value(result_ty, options, program, store, &mut cursor)?
    };
    Ok(vec![result])
}

fn lower_component_results(
    func_type: &FuncType,
    results: &[ComponentValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        if results.is_empty() {
            return Ok(Vec::new());
        }
        return Err(ComponentError::InvalidArgument(
            "function does not return a value".to_owned(),
        ));
    };
    let value = results
        .first()
        .ok_or_else(|| ComponentError::InvalidArgument("function result is missing".to_owned()))?;
    let flat_len = value_flat_len(result_ty, program)?;
    if flat_len > 1 {
        let memory = options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let return_ptr = options.realloc.as_ref().ok_or_else(|| {
            ComponentError::Runtime("canonical option `realloc` is required".to_owned())
        })?;
        let flat = lower_value_to_flat(value, result_ty, options, program, store)?;
        let area_size = flat.iter().map(wasm_value_size).sum::<usize>() as i32;
        let area_ptr = call_realloc(return_ptr, store, 0, 0, 4, area_size)? as u32;
        write_flat_values(store, &memory, area_ptr, &flat)?;
        Ok(vec![WasmValue::I32(area_ptr as i32)])
    } else {
        lower_value_to_flat(value, result_ty, options, program, store)
    }
}

fn lower_value(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
    out: &mut Vec<WasmValue>,
) -> Result<(), ComponentError> {
    out.extend(lower_value_to_flat(value, ty, options, program, store)?);
    Ok(())
}

fn lower_value_to_flat(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    match ty {
        ValType::Primitive(prim) => lower_primitive(value, prim, options, store, program),
        ValType::Type(type_id) => lower_defined_value(value, *type_id, options, program, store),
    }
}

fn lower_primitive(
    value: &ComponentValue,
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    store: &mut Store,
    program: &ComponentProgram,
) -> Result<Vec<WasmValue>, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => vec![WasmValue::I32(i32::from(expect_bool(value)?))],
        PrimValType::S8 => vec![WasmValue::I32(expect_i32(value)? as i8 as i32)],
        PrimValType::U8 => vec![WasmValue::I32(expect_u32(value)? as u8 as i32)],
        PrimValType::S16 => vec![WasmValue::I32(expect_i32(value)? as i16 as i32)],
        PrimValType::U16 => vec![WasmValue::I32(expect_u32(value)? as u16 as i32)],
        PrimValType::S32 => vec![WasmValue::I32(expect_i32(value)?)],
        PrimValType::U32 => vec![WasmValue::I32(expect_u32(value)? as i32)],
        PrimValType::S64 => vec![WasmValue::I64(expect_i64(value)?)],
        PrimValType::U64 => vec![WasmValue::I64(expect_u64(value)? as i64)],
        PrimValType::F32 => vec![WasmValue::F32(expect_f32(value)?)],
        PrimValType::F64 => vec![WasmValue::F64(expect_f64(value)?)],
        PrimValType::Char => vec![WasmValue::I32(expect_char(value)? as u32 as i32)],
        PrimValType::String => lower_string(value, options, program, store)?,
    })
}

fn lower_defined_value(
    value: &ComponentValue,
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    match program
        .type_map
        .get(&type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            lower_primitive(value, prim, options, store, program)
        }
        Type::DefVal(DefValType::Own(resource)) | Type::DefVal(DefValType::Borrow(resource)) => {
            let _ = program_resource_id(program, *resource)?;
            Ok(vec![WasmValue::I32(expect_handle(value)? as i32)])
        }
        Type::Resource(_resource) => Ok(vec![WasmValue::I32(expect_handle(value)? as i32)]),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn lift_value(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    match ty {
        ValType::Primitive(prim) => lift_primitive(prim, options, store, cursor),
        ValType::Type(type_id) => lift_defined_value(*type_id, options, program, store, cursor),
    }
}

fn lift_indirect_result(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &mut Store,
    pointer: u32,
) -> Result<ComponentValue, ComponentError> {
    match ty {
        ValType::Primitive(PrimValType::String) => {
            let memory = options.memory.clone().ok_or_else(|| {
                ComponentError::Runtime("canonical option `memory` is required".to_owned())
            })?;
            let (data_ptr, len) = read_string_pointer_pair(store, &memory, pointer)?;
            read_string_from_memory(store, &memory, data_ptr, len, options.string_encoding)
                .map(ComponentValue::String)
        }
        _ => Err(ComponentError::Unsupported(
            "indirect canonical results are not implemented yet".to_owned(),
        )),
    }
}

fn lift_defined_value(
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &mut Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    match program
        .type_map
        .get(&type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => lift_primitive(prim, options, store, cursor),
        Type::DefVal(DefValType::Own(resource)) => {
            let _ = program_resource_id(program, *resource)?;
            Ok(ComponentValue::Own(cursor.next_i32()? as u32))
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            let _ = program_resource_id(program, *resource)?;
            Ok(ComponentValue::Borrow(cursor.next_i32()? as u32))
        }
        Type::Resource(_resource) => Ok(ComponentValue::Own(cursor.next_i32()? as u32)),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn lift_primitive(
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    store: &mut Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => ComponentValue::Bool(cursor.next_i32()? != 0),
        PrimValType::S8 => ComponentValue::S8(cursor.next_i32()? as i8),
        PrimValType::U8 => ComponentValue::U8(cursor.next_i32()? as u8),
        PrimValType::S16 => ComponentValue::S16(cursor.next_i32()? as i16),
        PrimValType::U16 => ComponentValue::U16(cursor.next_i32()? as u16),
        PrimValType::S32 => ComponentValue::S32(cursor.next_i32()?),
        PrimValType::U32 => ComponentValue::U32(cursor.next_i32()? as u32),
        PrimValType::S64 => ComponentValue::S64(cursor.next_i64()?),
        PrimValType::U64 => ComponentValue::U64(cursor.next_i64()? as u64),
        PrimValType::F32 => ComponentValue::F32(cursor.next_f32()?),
        PrimValType::F64 => ComponentValue::F64(cursor.next_f64()?),
        PrimValType::Char => ComponentValue::Char(
            char::from_u32(cursor.next_i32()? as u32)
                .ok_or_else(|| ComponentError::Trap("invalid char scalar".to_owned()))?,
        ),
        PrimValType::String => {
            let memory = options.memory.as_ref().ok_or_else(|| {
                ComponentError::Runtime("canonical option `memory` is required".to_owned())
            })?;
            let ptr = cursor.next_i32()? as u32;
            let len = cursor.next_i32()? as u32;
            ComponentValue::String(read_string_from_memory(
                store,
                memory,
                ptr,
                len,
                options.string_encoding,
            )?)
        }
    })
}

fn lower_string(
    value: &ComponentValue,
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &mut Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let memory = options.memory.clone().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let realloc = options.realloc.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `realloc` is required".to_owned())
    })?;
    let string = value
        .as_str()
        .ok_or_else(|| ComponentError::InvalidArgument("expected string value".to_owned()))?;
    let encoding = options
        .string_encoding
        .unwrap_or(CanonicalStringEncoding::Utf8);
    let bytes = encode_string(string, encoding);
    let align = if matches!(
        encoding,
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16
    ) {
        2
    } else {
        1
    };
    let ptr = call_realloc(realloc, store, 0, 0, align, bytes.len() as i32)? as u32;
    write_memory(store, &memory, ptr, &bytes)?;
    let len = match encoding {
        CanonicalStringEncoding::Utf8 => bytes.len() as u32,
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            (bytes.len() / 2) as u32
        }
    };
    Ok(vec![WasmValue::I32(ptr as i32), WasmValue::I32(len as i32)])
}

fn encode_string(value: &str, encoding: CanonicalStringEncoding) -> Vec<u8> {
    match encoding {
        CanonicalStringEncoding::Utf8 => value.as_bytes().to_vec(),
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => value
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect(),
    }
}

fn read_string_from_memory(
    store: &mut Store,
    memory: &CoreExportRef,
    ptr: u32,
    len: u32,
    encoding: Option<CanonicalStringEncoding>,
) -> Result<String, ComponentError> {
    let encoding = encoding.unwrap_or(CanonicalStringEncoding::Utf8);
    let bytes = read_memory(
        store,
        memory,
        ptr,
        match encoding {
            CanonicalStringEncoding::Utf8 => len as usize,
            CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
                len as usize * 2
            }
        },
    )?;
    match encoding {
        CanonicalStringEncoding::Utf8 => {
            String::from_utf8(bytes).map_err(|error| ComponentError::Trap(error.to_string()))
        }
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            let mut units = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.chunks_exact(2) {
                units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            String::from_utf16(&units).map_err(|error| ComponentError::Trap(error.to_string()))
        }
    }
}

fn read_string_pointer_pair(
    store: &mut Store,
    memory: &CoreExportRef,
    pair_ptr: u32,
) -> Result<(u32, u32), ComponentError> {
    let bytes = read_memory(store, memory, pair_ptr, 8)?;
    Ok((
        u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
    ))
}

fn read_memory(
    store: &mut Store,
    memory: &CoreExportRef,
    ptr: u32,
    len: usize,
) -> Result<Vec<u8>, ComponentError> {
    let mut gc = store.gc.borrow_mut();
    let addr = memory_addr(memory, &mut gc)?;
    let memory = unsafe { gc.get_memory(addr) };
    let end = ptr
        .checked_add(len as u32)
        .ok_or_else(|| ComponentError::Trap("memory access overflow".to_owned()))?
        as usize;
    memory
        .get(ptr as usize..end)
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| ComponentError::Trap("memory access out of bounds".to_owned()))
}

fn write_memory(
    store: &mut Store,
    memory: &CoreExportRef,
    ptr: u32,
    bytes: &[u8],
) -> Result<(), ComponentError> {
    let mut gc = store.gc.borrow_mut();
    let addr = memory_addr(memory, &mut gc)?;
    let memory = unsafe { gc.get_memory(addr) };
    let end = ptr
        .checked_add(bytes.len() as u32)
        .ok_or_else(|| ComponentError::Trap("memory access overflow".to_owned()))?
        as usize;
    let slot = memory
        .get_mut(ptr as usize..end)
        .ok_or_else(|| ComponentError::Trap("memory access out of bounds".to_owned()))?;
    slot.copy_from_slice(bytes);
    Ok(())
}

fn write_flat_values(
    store: &mut Store,
    memory: &CoreExportRef,
    ptr: u32,
    values: &[WasmValue],
) -> Result<(), ComponentError> {
    let mut cursor = ptr;
    for value in values {
        match value {
            WasmValue::I32(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::I64(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 8;
            }
            WasmValue::F32(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::F64(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 8;
            }
            WasmValue::FuncRef(v) | WasmValue::ExternRef(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::V128(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 16;
            }
        }
    }
    Ok(())
}

fn memory_addr(
    memory: &CoreExportRef,
    gc: &mut crate::common::gc::MemoryPool,
) -> Result<crate::common::gc::GcRef, ComponentError> {
    let instance = unsafe { &*gc.get_instance_unchecked(memory.instance.get_gc_ref_with_pool(gc)) };
    let module = unsafe { gc.get_module(instance.module_addr) };
    let ExportDesc::Mem(idx) = module.exports.find(&memory.export_name).ok_or_else(|| {
        ComponentError::Link(format!("memory export '{}' is missing", memory.export_name))
    })?
    else {
        return Err(ComponentError::Link(format!(
            "export '{}' is not a memory",
            memory.export_name
        )));
    };
    instance
        .mems
        .as_slice(gc)
        .get(idx.0 as usize)
        .copied()
        .ok_or_else(|| ComponentError::Trap("memory index is out of bounds".to_owned()))
}

fn wasm_value_size(value: &WasmValue) -> usize {
    match value {
        WasmValue::I32(_) | WasmValue::F32(_) | WasmValue::FuncRef(_) | WasmValue::ExternRef(_) => {
            4
        }
        WasmValue::I64(_) | WasmValue::F64(_) => 8,
        WasmValue::V128(_) => 16,
    }
}

fn call_realloc(
    realloc: &RuntimeCoreFunc,
    store: &mut Store,
    old_ptr: i32,
    old_len: i32,
    align: i32,
    new_len: i32,
) -> Result<i32, ComponentError> {
    let result = realloc.call_sync(
        store,
        &[
            WasmValue::I32(old_ptr),
            WasmValue::I32(old_len),
            WasmValue::I32(align),
            WasmValue::I32(new_len),
        ],
    )?;
    match result.as_slice() {
        [WasmValue::I32(ptr)] => Ok(*ptr),
        _ => Err(ComponentError::Runtime(
            "realloc returned an unexpected result".to_owned(),
        )),
    }
}

fn program_func_type(
    program: &ComponentProgram,
    type_id: TypeId,
) -> Result<FuncType, ComponentError> {
    match program.type_map.get(&type_id) {
        Some(Type::Func(func_type)) => Ok(func_type.clone()),
        _ => Err(ComponentError::Runtime(
            "function type is missing".to_owned(),
        )),
    }
}

fn program_resource_id(
    program: &ComponentProgram,
    type_id: TypeId,
) -> Result<ResourceId, ComponentError> {
    match program.type_map.get(&type_id) {
        Some(Type::Resource(resource)) => Ok(*resource),
        Some(Type::DefVal(DefValType::Own(inner)))
        | Some(Type::DefVal(DefValType::Borrow(inner))) => program_resource_id(program, *inner),
        Some(Type::Generic(generic)) => match generic.bound {
            crate::component::ir::types::GenericBound::Eq(inner) => {
                program_resource_id(program, inner)
            }
            crate::component::ir::types::GenericBound::Sub => Err(ComponentError::Unsupported(
                "sub resource resolution is not implemented at runtime".to_owned(),
            )),
        },
        _ => Err(ComponentError::Runtime(
            "resource type is missing".to_owned(),
        )),
    }
}

fn value_flat_len(ty: &ValType, program: &ComponentProgram) -> Result<usize, ComponentError> {
    match ty {
        ValType::Primitive(PrimValType::String) => Ok(2),
        ValType::Primitive(_) => Ok(1),
        ValType::Type(type_id) => match program.type_map.get(type_id) {
            Some(Type::DefVal(DefValType::Primitive(PrimValType::String))) => Ok(2),
            Some(Type::DefVal(DefValType::Primitive(_))) => Ok(1),
            Some(Type::DefVal(DefValType::Own(_))) | Some(Type::DefVal(DefValType::Borrow(_))) => {
                Ok(1)
            }
            Some(Type::Resource(_)) => Ok(1),
            Some(Type::Generic(_)) => Ok(1),
            _ => Err(ComponentError::Unsupported(
                "flat length for this type is not implemented yet".to_owned(),
            )),
        },
    }
}

struct CoreValueCursor<'a> {
    values: &'a [WasmValue],
    offset: usize,
}

impl<'a> CoreValueCursor<'a> {
    fn new(values: &'a [WasmValue]) -> Self {
        Self { values, offset: 0 }
    }

    fn next(&mut self) -> Result<WasmValue, ComponentError> {
        let value = *self
            .values
            .get(self.offset)
            .ok_or_else(|| ComponentError::Trap("canonical ABI value underflow".to_owned()))?;
        self.offset += 1;
        Ok(value)
    }

    fn next_i32(&mut self) -> Result<i32, ComponentError> {
        match self.next()? {
            WasmValue::I32(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected i32, got {other:?}"))),
        }
    }

    fn next_i64(&mut self) -> Result<i64, ComponentError> {
        match self.next()? {
            WasmValue::I64(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected i64, got {other:?}"))),
        }
    }

    fn next_f32(&mut self) -> Result<f32, ComponentError> {
        match self.next()? {
            WasmValue::F32(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected f32, got {other:?}"))),
        }
    }

    fn next_f64(&mut self) -> Result<f64, ComponentError> {
        match self.next()? {
            WasmValue::F64(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected f64, got {other:?}"))),
        }
    }
}

fn expect_bool(value: &ComponentValue) -> Result<bool, ComponentError> {
    value
        .as_bool()
        .ok_or_else(|| ComponentError::InvalidArgument("expected bool".to_owned()))
}

fn expect_u32(value: &ComponentValue) -> Result<u32, ComponentError> {
    value
        .as_u32()
        .ok_or_else(|| ComponentError::InvalidArgument("expected u32".to_owned()))
}

fn expect_i32(value: &ComponentValue) -> Result<i32, ComponentError> {
    value
        .as_i32()
        .ok_or_else(|| ComponentError::InvalidArgument("expected i32".to_owned()))
}

fn expect_i64(value: &ComponentValue) -> Result<i64, ComponentError> {
    match value {
        ComponentValue::I64(v) | ComponentValue::S64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected i64".to_owned())),
    }
}

fn expect_u64(value: &ComponentValue) -> Result<u64, ComponentError> {
    match value {
        ComponentValue::U64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected u64".to_owned())),
    }
}

fn expect_f32(value: &ComponentValue) -> Result<f32, ComponentError> {
    match value {
        ComponentValue::F32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected f32".to_owned())),
    }
}

fn expect_f64(value: &ComponentValue) -> Result<f64, ComponentError> {
    match value {
        ComponentValue::F64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected f64".to_owned())),
    }
}

fn expect_char(value: &ComponentValue) -> Result<char, ComponentError> {
    match value {
        ComponentValue::Char(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected char".to_owned())),
    }
}

fn expect_handle(value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::Own(v) | ComponentValue::Borrow(v) | ComponentValue::U32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument(
            "expected resource handle".to_owned(),
        )),
    }
}
