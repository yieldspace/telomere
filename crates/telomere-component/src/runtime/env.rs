use super::canonical::{program_func_type, runtime_resource_id};
use super::host::{
    linker_binding_to_callable, materialize_inline_core_instance, vm_result_to_component_error,
};
use super::*;

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
    let root = Rc::new(RuntimeComponentInstance::new_component(
        program.root.clone(),
        env,
    ));

    materialize_defined_core_instances(root.env.as_ref(), store)?;

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

fn materialize_defined_core_instances(
    env: &RuntimeEnv,
    store: &mut Store,
) -> Result<(), ComponentError> {
    let mut pending = env
        .program
        .core_instance_store
        .keys()
        .copied()
        .collect::<Vec<_>>();
    pending.sort_by_key(|idx| AnyGlobalIdx::from(*idx).raw());

    for idx in pending {
        env.resolve_core_instance(idx, store)?;
    }

    Ok(())
}

impl RuntimeInstance {
    pub(crate) async fn call(
        &self,
        store: &mut Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_path(store, &[], name, args).await
    }

    pub(crate) async fn call_path(
        &self,
        store: &mut Store,
        path: &[String],
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_path_sync(store, path, name, args)
    }

    fn call_path_sync(
        &self,
        store: &mut Store,
        path: &[String],
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        let namespace = self.resolve_namespace(store, path)?;
        match namespace.resolve_export(name, store)? {
            RuntimeExport::Func(callable) => callable.call_sync(store, args),
            _ => Err(ComponentError::ExportNotFound(name.to_owned())),
        }
    }

    fn resolve_namespace(
        &self,
        store: &mut Store,
        path: &[String],
    ) -> Result<Rc<RuntimeComponentInstance>, ComponentError> {
        let mut current = self.root.clone();
        for segment in path {
            current = match current.resolve_export(segment, store)? {
                RuntimeExport::Instance(instance) => instance,
                _ => return Err(ComponentError::ExportNotFound(segment.clone())),
            };
        }
        Ok(current)
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
                Rc::new(RuntimeComponentInstance::new_component(
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
        idx: GlobalIdx<crate::ir::CoreModule>,
        store: &mut Store,
    ) -> Result<Module, ComponentError> {
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
                match block_on(crate::support::instantiate(module, store, &registry)) {
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

    pub(super) fn resolve_core_func(
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
                let resource = runtime_resource_id(&self.program, &self.shared, *type_id)?;
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
                let resource = runtime_resource_id(&self.program, &self.shared, *type_id)?;
                RuntimeCoreFunc::Host(Rc::new(HostBinding::ResourceDrop {
                    resource,
                    signature: CoreFuncType::new(vec![CoreValType::I32], vec![]),
                    shared: self.shared.clone(),
                }))
            }
            Some(CoreRelation::Defined(CoreFunc::CanonResourceRep { type_id })) => {
                let resource = runtime_resource_id(&self.program, &self.shared, *type_id)?;
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

    pub(super) fn resolve_core_memory(
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

    pub(super) fn resolve_core_table(
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
            post_return: match options.post_return {
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
                .linker
                .resolve_import_instance(name)
                .map(|instance| {
                    Rc::new(RuntimeComponentInstance::new_linker(
                        instance,
                        Rc::new(self.clone_shallow()),
                    ))
                })
                .or_else(|| {
                    self.parent
                        .as_ref()
                        .and_then(|parent| parent.lookup_import_instance(name).ok())
                })
                .ok_or_else(|| {
                    ComponentError::Unsupported(format!(
                        "instance import '{}' is not supported by linker",
                        name
                    ))
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

    fn lookup_import_core_module(&self, name: &str) -> Result<Module, ComponentError> {
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
    fn new_component(component: Component, env: Rc<RuntimeEnv>) -> Self {
        Self {
            source: RuntimeComponentSource::Component(component),
            env,
            exports: RefCell::new(HashMap::new()),
        }
    }

    fn new_linker(instance: ComponentLinkerInstance, env: Rc<RuntimeEnv>) -> Self {
        Self {
            source: RuntimeComponentSource::LinkerInstance(instance),
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
        let export = match &self.source {
            RuntimeComponentSource::Component(component) => match component.exports.get(name) {
                Some(ComponentExport::Component(idx)) => {
                    RuntimeExport::Component(self.env.resolve_component(*idx, store)?)
                }
                Some(ComponentExport::Instance(idx)) => {
                    RuntimeExport::Instance(self.env.resolve_instance(*idx, store)?)
                }
                Some(ComponentExport::Func { idx, .. }) => {
                    if self.env.parent.is_none() {
                        if let Some(Relation::Import(_)) = self.env.program.func_store.get(idx) {
                            if let Some(binding) = self.env.linker.resolve_export(name) {
                                let export = RuntimeExport::Func(Rc::new(
                                    linker_binding_to_callable(binding),
                                ));
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
            },
            RuntimeComponentSource::LinkerInstance(instance) => {
                let Some(binding) = instance.resolve_export(name) else {
                    return Err(ComponentError::ExportNotFound(name.to_owned()));
                };
                RuntimeExport::Func(Rc::new(linker_binding_to_callable(binding)))
            }
        };
        self.exports
            .borrow_mut()
            .insert(name.to_owned(), export.clone());
        Ok(export)
    }
}
