use super::canonical::{
    func_types_equal_modulo_variant_form, program_func_type, register_host_argument_handles,
    runtime_resource_id,
};
use super::host::{
    linker_binding_to_callable, materialize_inline_core_instance, vm_result_to_component_error,
};
use super::*;

pub async fn instantiate(
    program: ComponentProgram,
    store: &Store,
    linker: &ComponentLinker,
) -> Result<ComponentInstance, ComponentError> {
    let runtime = instantiate_sync(program.clone(), store, linker, SharedState::default())?;
    Ok(ComponentInstance::new(program, runtime))
}

/// Bundle instantiation (#201): like [`instantiate`], but the shared
/// state carries the bundle's opened artifacts, so each defined core
/// instance created from a substituted module gets its artifact
/// installed into the store's JIT cache (see
/// [`crate::bundle::ComponentBundleArtifacts`]).
#[cfg(feature = "aot-bundle")]
pub async fn instantiate_with_artifacts(
    program: ComponentProgram,
    artifacts: &crate::bundle::ComponentBundleArtifacts,
    store: &Store,
    linker: &ComponentLinker,
) -> Result<ComponentInstance, ComponentError> {
    let shared = SharedState {
        aot_artifacts: Some(artifacts.set()),
        ..SharedState::default()
    };
    let runtime = instantiate_sync(program.clone(), store, linker, shared)?;
    Ok(ComponentInstance::new(program, runtime))
}

fn instantiate_sync(
    program: ComponentProgram,
    store: &Store,
    linker: &ComponentLinker,
    shared: SharedState,
) -> Result<RuntimeInstance, ComponentError> {
    let program = Rc::new(program);
    let shared = Rc::new(shared);
    register_host_resource_drops(&program, linker, &shared);
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

    // Host-provided values bypass the decoder, so check them against the
    // declared import types here: this is the canonical-ABI-side soundness
    // gate for the value index space.
    for (name, import) in &program.root.imports {
        if let crate::ir::ComponentImport::Value(ty) = import {
            if let Some(value) = linker.resolve_import_value(name) {
                value_matches_type(&program, &value, ty).map_err(|_| {
                    ComponentError::Link(format!(
                        "value import '{name}' does not match its declared type"
                    ))
                })?;
            }
        }
    }

    // Every declared import must be satisfied at instantiation, used or
    // not (#313). Resolution used to be fully lazy, so an import nothing
    // referenced was never looked up and an unsatisfiable component
    // instantiated successfully.
    check_imports_satisfied(root.env.as_ref(), &program.root)?;

    materialize_component_instances(&root, store)?;

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

/// Verifies that every import declared by `component` resolves in `env`,
/// whether or not the component ever references it.
///
/// The component model requires all imports to be satisfied at
/// instantiation time. Resolution is otherwise lazy (an import is looked
/// up when an alias / lower / export first references it), so without
/// this check an import nothing references is never looked up and a
/// component with an unsatisfiable import instantiates successfully
/// (#313). The lookups are the same ones a use would perform, so a
/// satisfied import costs one extra lookup and an unsatisfied one fails
/// with the same error it would have failed with at first use.
fn check_imports_satisfied(env: &RuntimeEnv, component: &Component) -> Result<(), ComponentError> {
    use crate::ir::ComponentImport;

    for (name, import) in &component.imports {
        match import {
            ComponentImport::Func(_) => {
                if env.lookup_import_func(name).is_err() {
                    // A func import the component re-exports can instead
                    // be satisfied by a host export the embedder
                    // registered under the export's name: that is the
                    // resolution `resolve_export` performs for such
                    // exports (#271), and it exists only at the root.
                    let satisfied_via_host_export = env.parent.is_none()
                        && component.exports.iter().any(|(export_name, export)| {
                            let ComponentExport::Func { idx, .. } = export else {
                                return false;
                            };
                            matches!(
                                env.program.func_store.get(idx),
                                Some(Relation::Import(import_name)) if import_name == name
                            ) && env.linker.resolve_export(export_name).is_some()
                        });
                    if !satisfied_via_host_export {
                        env.lookup_import_func(name)?;
                    }
                }
            }
            ComponentImport::Instance(_) => {
                env.lookup_import_instance(name)?;
            }
            ComponentImport::Value(_) => {
                env.lookup_import_value(name)?;
            }
            ComponentImport::Component => {
                env.lookup_import_component(name)?;
            }
            ComponentImport::Module => {
                env.lookup_import_core_module(name)?;
            }
            // The decoder never constructs resource imports, and resource
            // imports have no linker-side resolution path to check against.
            ComponentImport::Resource => {}
        }
    }
    Ok(())
}

/// Maps each resource type exported by a root-level imported instance to
/// the destructor the host registered for it, keyed by `ResourceId`.
///
/// The decoder retains the declared type of every instance import, so the
/// resource exports of the declared instance type identify exactly which
/// `ResourceId`s a guest-side `canon resource.drop` can name. Types from
/// instances the host did not register a destructor for stay unmapped and
/// keep trapping on drop, as before.
fn register_host_resource_drops(
    program: &ComponentProgram,
    linker: &ComponentLinker,
    shared: &SharedState,
) {
    use crate::ir::types::InstanceExportType;
    use crate::ir::ComponentImport;

    for (name, import) in &program.root.imports {
        let ComponentImport::Instance(type_id) = import else {
            continue;
        };
        let Some(Type::Instance(instance_type)) = program.get_type(*type_id) else {
            continue;
        };
        for (export_name, export) in &instance_type.exports {
            let InstanceExportType::Type(export_type_id) = export else {
                continue;
            };
            let Ok(resource) = runtime_resource_id(program, shared, *export_type_id) else {
                continue;
            };
            if let Some(drop_fn) = linker.resolve_import_resource_drop(name, export_name) {
                shared
                    .host_resource_drops
                    .borrow_mut()
                    .insert(resource, drop_fn);
            }
        }
    }
}

/// Materialise everything the component of `instance` defines, in the
/// environment of `instance` itself: its core instances, and the component
/// instances it defines, in definition order across both kinds.
///
/// Core instances must be created with the environment that carries the
/// component instance's instantiation arguments: resolving them from the
/// root environment (whose import map is empty) loses every `(with ...)`
/// argument, so a nested component whose core instances depend on supplied
/// funcs, instances, or core modules failed to instantiate (#316).
///
/// The defined instances are resolved eagerly for the same reason
/// instantiation itself is eager in the component model: creating an
/// instance must materialise the core instances it defines — running any
/// core start functions — even when none of its exports are ever
/// referenced. Leaving them to on-demand resolution in `resolve_instance`
/// would silently skip those start functions (and any instantiation
/// errors) for explicitly instantiated but otherwise unused instances.
/// The order matters, not just eagerness: a core start function observes
/// the effects of every instance created before it, so materialising in
/// any order other than definition order (for example, all core
/// instances before all component instances) is observable to the guest
/// (#333). Resolving a defined instance recurses through
/// `resolve_instance`, which calls this function for the new instance,
/// so the whole defined subtree is materialised. This is called for the
/// root instance from `instantiate_sync` and for every other instance
/// from `RuntimeEnv::resolve_instance`, the single place where component
/// instances are created.
fn materialize_component_instances(
    instance: &RuntimeComponentInstance,
    store: &Store,
) -> Result<(), ComponentError> {
    if let RuntimeComponentSource::Component(component) = &instance.source {
        for def in &component.defined_instances {
            match def {
                DefinedInstance::Core(idx) => {
                    instance.env.resolve_core_instance(*idx, store)?;
                }
                DefinedInstance::Component(idx) => {
                    instance.env.resolve_instance(*idx, store)?;
                }
            }
        }
    }
    Ok(())
}

impl RuntimeInstance {
    pub(crate) async fn call(
        &self,
        store: &Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_path(store, &[], name, args).await
    }

    pub(crate) async fn call_path(
        &self,
        store: &Store,
        path: &[String],
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_path_sync(store, path, name, args)
    }

    fn call_path_sync(
        &self,
        store: &Store,
        path: &[String],
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        let namespace = self.resolve_namespace(store, path)?;
        match namespace.resolve_export(name, store)? {
            RuntimeExport::Func(callable) => {
                // Handles the embedder supplies directly as arguments
                // live in the host namespace; register them before the
                // call so the boundary validation can tell them from
                // guest-invented indices. Borrow registrations end with
                // this call.
                let (shared, borrows) = match &*callable {
                    ResolvedCallable::Lifted {
                        func_type,
                        program,
                        shared,
                        ..
                    } => (
                        Some(shared.clone()),
                        register_host_argument_handles(func_type, args, program, shared),
                    ),
                    _ => (None, Vec::new()),
                };
                let result = callable.call_sync(store, args);
                if let Some(shared) = &shared {
                    for handle in borrows {
                        shared.release_host_borrow(handle);
                    }
                }
                result
            }
            _ => Err(ComponentError::ExportNotFound(name.to_owned())),
        }
    }

    pub(crate) fn value_path(
        &self,
        store: &Store,
        path: &[String],
        name: &str,
    ) -> Result<ComponentValue, ComponentError> {
        let namespace = self.resolve_namespace(store, path)?;
        match namespace.resolve_export(name, store)? {
            RuntimeExport::Value(value) => Ok(value),
            _ => Err(ComponentError::ExportNotFound(name.to_owned())),
        }
    }

    fn resolve_namespace(
        &self,
        store: &Store,
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
            handles: Rc::new(RefCell::new(HandleTable::default())),
            caches: Rc::new(RuntimeCaches::default()),
        }
    }

    fn resolve_component(
        &self,
        idx: GlobalIdx<Component>,
        store: &Store,
    ) -> Result<RuntimeComponentDef, ComponentError> {
        if let Some(component) = self.caches.components.borrow().get(&idx).cloned() {
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
        self.caches
            .components
            .borrow_mut()
            .insert(idx, component.clone());
        Ok(component)
    }

    fn resolve_instance(
        &self,
        idx: GlobalIdx<Instance>,
        store: &Store,
    ) -> Result<Rc<RuntimeComponentInstance>, ComponentError> {
        if let Some(instance) = self.caches.instances.borrow().get(&idx).cloned() {
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
                        InstanceImport::Value(idx) => {
                            RuntimeImport::Value(self.resolve_value(*idx, store)?)
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
                let instance = Rc::new(RuntimeComponentInstance::new_component(
                    component.component.clone(),
                    env,
                ));
                // Creating the instance is its instantiation: every import
                // the component declares must be satisfied here, whether
                // or not the component ever references it (#313).
                check_imports_satisfied(instance.env.as_ref(), &component.component)?;
                materialize_component_instances(&instance, store)?;
                instance
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
        self.caches
            .instances
            .borrow_mut()
            .insert(idx, instance.clone());
        Ok(instance)
    }

    fn resolve_func(
        &self,
        idx: GlobalIdx<Func>,
        store: &Store,
    ) -> Result<Rc<ResolvedCallable>, ComponentError> {
        if let Some(func) = self.caches.funcs.borrow().get(&idx).cloned() {
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
                    options: Box::new(options),
                    program: self.program.clone(),
                    shared: self.shared.clone(),
                    handles: self.handles.clone(),
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
        self.caches.funcs.borrow_mut().insert(idx, func.clone());
        Ok(func)
    }

    fn resolve_value(
        &self,
        idx: GlobalIdx<Value>,
        store: &Store,
    ) -> Result<ComponentValue, ComponentError> {
        if let Some(value) = self.caches.values.borrow().get(&idx).cloned() {
            return Ok(value);
        }
        let value = match self.program.value_store.get(&idx) {
            Some(Relation::Defined(def)) => def.value.clone(),
            Some(Relation::Import(name)) => self.lookup_import_value(name)?,
            Some(Relation::FromExport(instance_idx, name)) => match self
                .resolve_instance(*instance_idx, store)?
                .resolve_export(name, store)?
            {
                RuntimeExport::Value(value) => value,
                _ => {
                    return Err(ComponentError::Link(format!(
                        "value export '{}' is not a value",
                        name
                    )))
                }
            },
            None => return Err(ComponentError::Link("value relation missing".to_owned())),
        };
        self.caches.values.borrow_mut().insert(idx, value.clone());
        Ok(value)
    }

    fn resolve_core_module(
        &self,
        idx: GlobalIdx<crate::ir::CoreModule>,
        store: &Store,
    ) -> Result<Module, ComponentError> {
        if let Some(module) = self.caches.core_modules.borrow().get(&idx).cloned() {
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
        self.caches
            .core_modules
            .borrow_mut()
            .insert(idx, module.clone());
        Ok(module)
    }

    /// The component definition behind an instance index, following
    /// instance aliases (`FromExport` chains) through the program's
    /// static relations. Returns `None` for imported instances and for
    /// chains that leave the program (linker-provided instances have
    /// no static definition). Mirrors the traversal
    /// [`RuntimeEnv::resolve_instance`] performs at runtime, but yields
    /// the definition instead of a live instance.
    #[cfg(feature = "aot-bundle")]
    fn component_of_instance(&self, idx: GlobalIdx<Instance>) -> Option<crate::ir::Component> {
        match self.program.instance_store.get(&idx)? {
            Relation::Defined(instance) => {
                let component_idx = instance.component_idx?;
                match self.program.component_store.get(&component_idx)? {
                    Relation::Defined(component) => Some(component.clone()),
                    _ => None,
                }
            }
            Relation::FromExport(parent_idx, name) => {
                let parent = self.component_of_instance(*parent_idx)?;
                let key = resolve_component_export_key(&parent, name).ok()?;
                match parent.exports.get(&key)? {
                    ComponentExport::Instance(inner) => self.component_of_instance(*inner),
                    _ => None,
                }
            }
            Relation::Import(_) => None,
        }
    }

    /// The core module index a bundle artifact is keyed to for `idx`:
    /// the index of the module's *definition*. A core instance may be
    /// instantiated from an alias slot (`alias export` of a nested
    /// component's exported module), whose index differs from the
    /// definition the decoder registered the substituted module (and
    /// its artifact) under; resolving through the alias chain
    /// preserves the artifact's identity so aliased modules are
    /// installed too. Indices that do not trace back to a definition
    /// in this program (imports, unsupported alias forms) are returned
    /// unchanged, so their lookup simply misses, as before.
    #[cfg(feature = "aot-bundle")]
    fn core_module_definition_idx(
        &self,
        idx: GlobalIdx<crate::ir::CoreModule>,
    ) -> GlobalIdx<crate::ir::CoreModule> {
        // Alias chains follow strictly earlier definitions in a
        // well-formed program, so this walk terminates; the visited
        // set is a defensive bound against a malformed relation graph.
        let mut visited = std::collections::HashSet::new();
        let mut current = idx;
        while visited.insert(current) {
            match self.program.core_module_store.get(&current) {
                Some(CoreRelation::FromExport(instance_idx, name)) => {
                    let Some(component) = self.component_of_instance(*instance_idx) else {
                        return idx;
                    };
                    let Ok(key) = resolve_component_export_key(&component, name) else {
                        return idx;
                    };
                    match component.exports.get(&key) {
                        Some(ComponentExport::Module(inner)) => current = *inner,
                        _ => return idx,
                    }
                }
                _ => return current,
            }
        }
        idx
    }

    /// Installs the bundle artifact keyed to `module_idx` (if any)
    /// into a freshly created defined core instance (#201). Ordinary
    /// instantiation carries no artifact set and skips this entirely.
    /// `module_idx` may name an alias of the substituted module; the
    /// artifact is keyed by the definition index, so the alias is
    /// resolved first (see `core_module_definition_idx`).
    #[cfg(feature = "aot-bundle")]
    fn install_bundle_artifact(
        &self,
        store: &Store,
        module_idx: GlobalIdx<crate::ir::CoreModule>,
        instance: &InstanceHandle,
    ) -> Result<(), ComponentError> {
        if let Some(set) = &self.shared.aot_artifacts {
            let definition_idx = self.core_module_definition_idx(module_idx);
            set.install_for(store, definition_idx, instance)?;
        }
        Ok(())
    }

    fn resolve_core_instance(
        &self,
        idx: GlobalIdx<CoreInstance>,
        store: &Store,
    ) -> Result<InstanceHandle, ComponentError> {
        if let Some(instance) = self.caches.core_instances.borrow().get(&idx).cloned() {
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
                    CoreVMResult::Success(instance) => {
                        #[cfg(feature = "aot-bundle")]
                        self.install_bundle_artifact(store, *module_idx, &instance)?;
                        instance
                    }
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
        self.caches
            .core_instances
            .borrow_mut()
            .insert(idx, instance.clone());
        Ok(instance)
    }

    pub(super) fn resolve_core_func(
        &self,
        idx: GlobalIdx<CoreFunc>,
        store: &Store,
    ) -> Result<RuntimeCoreFunc, ComponentError> {
        if let Some(func) = self.caches.core_funcs.borrow().get(&idx).cloned() {
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
                    shared: self.shared.clone(),
                    handles: self.handles.clone(),
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
                    handles: self.handles.clone(),
                }))
            }
            Some(CoreRelation::Defined(CoreFunc::CanonResourceDrop { type_id })) => {
                let resource = runtime_resource_id(&self.program, &self.shared, *type_id)?;
                RuntimeCoreFunc::Host(Rc::new(HostBinding::ResourceDrop {
                    resource,
                    signature: CoreFuncType::new(vec![CoreValType::I32], vec![]),
                    shared: self.shared.clone(),
                    handles: self.handles.clone(),
                }))
            }
            Some(CoreRelation::Defined(CoreFunc::CanonResourceRep { type_id })) => {
                let resource = runtime_resource_id(&self.program, &self.shared, *type_id)?;
                RuntimeCoreFunc::Host(Rc::new(HostBinding::ResourceRep {
                    resource,
                    signature: CoreFuncType::new(vec![CoreValType::I32], vec![CoreValType::I32]),
                    handles: self.handles.clone(),
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
        self.caches
            .core_funcs
            .borrow_mut()
            .insert(idx, func.clone());
        Ok(func)
    }

    pub(super) fn resolve_core_memory(
        &self,
        idx: GlobalIdx<CoreMemory>,
        store: &Store,
    ) -> Result<CoreExportRef, ComponentError> {
        if let Some(memory) = self.caches.core_memories.borrow().get(&idx).cloned() {
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
        self.caches
            .core_memories
            .borrow_mut()
            .insert(idx, memory.clone());
        Ok(memory)
    }

    pub(super) fn resolve_core_table(
        &self,
        idx: GlobalIdx<CoreTable>,
        store: &Store,
    ) -> Result<CoreExportRef, ComponentError> {
        if let Some(table) = self.caches.core_tables.borrow().get(&idx).cloned() {
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
        self.caches
            .core_tables
            .borrow_mut()
            .insert(idx, table.clone());
        Ok(table)
    }

    pub(super) fn resolve_core_global(
        &self,
        idx: GlobalIdx<CoreGlobal>,
        store: &Store,
    ) -> Result<CoreExportRef, ComponentError> {
        if let Some(global) = self.caches.core_globals.borrow().get(&idx).cloned() {
            return Ok(global);
        }
        let global = match self.program.core_global_store.get(&idx) {
            Some(CoreRelation::FromCoreExport(instance_idx, name)) => CoreExportRef {
                instance: self.resolve_core_instance(*instance_idx, store)?,
                export_name: name.clone(),
            },
            _ => {
                return Err(ComponentError::Unsupported(
                    "runtime only supports core globals from core exports".to_owned(),
                ))
            }
        };
        self.caches
            .core_globals
            .borrow_mut()
            .insert(idx, global.clone());
        Ok(global)
    }

    fn resolve_runtime_options(
        &self,
        options: &CanonicalOptions,
        store: &Store,
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
            // A component definition's env names the enclosing instance's
            // scope, so it shares that instance's handle table; each new
            // instance env built by `RuntimeEnv::new` starts a fresh one.
            handles: self.handles.clone(),
            // Cached component defs own cloned envs, so sharing this Rc can
            // make a cache retain itself through RuntimeComponentDef.
            caches: Rc::new(self.clone_caches_detached()),
        }
    }

    fn clone_caches_detached(&self) -> RuntimeCaches {
        let caches = self.caches.as_ref();
        RuntimeCaches {
            components: RefCell::new(caches.components.borrow().clone()),
            instances: RefCell::new(caches.instances.borrow().clone()),
            funcs: RefCell::new(caches.funcs.borrow().clone()),
            core_modules: RefCell::new(caches.core_modules.borrow().clone()),
            core_instances: RefCell::new(caches.core_instances.borrow().clone()),
            core_funcs: RefCell::new(caches.core_funcs.borrow().clone()),
            core_memories: RefCell::new(caches.core_memories.borrow().clone()),
            core_tables: RefCell::new(caches.core_tables.borrow().clone()),
            core_globals: RefCell::new(caches.core_globals.borrow().clone()),
            values: RefCell::new(caches.values.borrow().clone()),
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
            None => {
                if let Some(instance) = self.linker.resolve_import_instance(name) {
                    return Ok(Rc::new(RuntimeComponentInstance::new_linker(
                        instance,
                        Rc::new(self.clone_shallow()),
                    )));
                }
                if let Some(parent) = &self.parent {
                    if let Ok(instance) = parent.lookup_import_instance(name) {
                        return Ok(instance);
                    }
                }
                if let Some(found) = self.linker.incompatible_import_instance(name) {
                    return Err(ComponentError::Link(format!(
                        "instance import '{name}' is unresolved (found incompatible version {found})"
                    )));
                }
                Err(ComponentError::Unsupported(format!(
                    "instance import '{}' is not supported by linker",
                    name
                )))
            }
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
                        Err(match self.linker.incompatible_import(name) {
                            Some(found) => ComponentError::Link(format!(
                                "function import '{name}' is unresolved (found incompatible version {found})"
                            )),
                            None => ComponentError::Link(format!(
                                "function import '{}' is unresolved",
                                name
                            )),
                        })
                    })
            }
        }
    }

    fn lookup_import_value(&self, name: &str) -> Result<ComponentValue, ComponentError> {
        match self.imports.get(name) {
            Some(RuntimeImport::Value(value)) => Ok(value.clone()),
            Some(_) => Err(ComponentError::Link(format!(
                "import '{}' is not a value",
                name
            ))),
            None => {
                if let Some(value) = self.linker.resolve_import_value(name) {
                    return Ok(value);
                }
                if let Some(parent) = &self.parent {
                    if let Ok(value) = parent.lookup_import_value(name) {
                        return Ok(value);
                    }
                }
                if let Some(found) = self.linker.incompatible_import_value(name) {
                    return Err(ComponentError::Link(format!(
                        "value import '{name}' is unresolved (found incompatible version {found})"
                    )));
                }
                Err(ComponentError::Link(format!(
                    "value import '{}' is unresolved",
                    name
                )))
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

/// Resolves the export key to use for a lookup against a component's own
/// exports.
///
/// Exact names win. On a miss, a semver-compatible export on the same
/// version track is accepted (see [`name_match`]), so a component exporting
/// `wasi:cli/run@0.2.0` satisfies a lookup for `wasi:cli/run@0.2.6`. When the
/// component exports the same interface only at an incompatible version, the
/// error names both the requested name and the version that was found.
fn resolve_component_export_key(
    component: &Component,
    name: &str,
) -> Result<String, ComponentError> {
    if component.exports.contains_key(name) {
        return Ok(name.to_owned());
    }
    let available = component.exports.keys().map(String::as_str);
    if let Some(matched) = name_match::compatible_name(name, available.clone()) {
        return Ok(matched.to_owned());
    }
    if let Some(found) = name_match::incompatible_name(name, available) {
        return Err(ComponentError::ExportNotFound(format!(
            "{name} (found incompatible version {found})"
        )));
    }
    Err(ComponentError::ExportNotFound(name.to_owned()))
}

/// Present a resolved function under the type its export declares.
///
/// An export may ascribe a type to the function it exports; validation
/// has already checked the function's own type is a subtype of the
/// ascribed one, and the ascribed type is the function's public type —
/// the one `get_func` / `get_typed_func` validate bindings against. The
/// callable, however, is built from the function's definition and
/// converts values at the boundary using the *definition's* type. When
/// the two types differ only in the declared constructors of
/// variant-shaped types (a definition returning a declared `variant`
/// exported as returning the `result` it desugars to, say), they are
/// the same type with the same canonical layout, but the definition's
/// constructors would choose the wrong dynamic representations at this
/// boundary: a typed binding accepted against the export type would
/// then fail when invoked. Rebuild the callable under the export type
/// in exactly that case.
///
/// Anything further apart — an ascription that changes the canonical
/// layout, or a callable whose types belong to another program — keeps
/// the definition's type, as before.
fn present_callable_under_export_type(
    program: &Rc<ComponentProgram>,
    callable: Rc<ResolvedCallable>,
    export_type_id: TypeId,
) -> Rc<ResolvedCallable> {
    let ResolvedCallable::Lifted {
        core,
        func_type,
        options,
        program: callable_program,
        shared,
        handles,
    } = &*callable
    else {
        return callable;
    };
    if !Rc::ptr_eq(program, callable_program) {
        return callable;
    }
    let Ok(export_type) = program_func_type(program, export_type_id) else {
        return callable;
    };
    if !func_types_equal_modulo_variant_form(program, func_type, &export_type) {
        return callable;
    }
    Rc::new(ResolvedCallable::Lifted {
        core: core.clone(),
        func_type: export_type,
        options: options.clone(),
        program: callable_program.clone(),
        shared: shared.clone(),
        handles: handles.clone(),
    })
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

    fn resolve_export(&self, name: &str, store: &Store) -> Result<RuntimeExport, ComponentError> {
        if let Some(export) = self.exports.borrow().get(name).cloned() {
            return Ok(export);
        }
        let export = match &self.source {
            RuntimeComponentSource::Component(component) => {
                // Retain the resolved (version-matched) export key: a
                // re-exported import is registered with the linker under
                // the export's own name, so the host-export lookup below
                // must use this key rather than the requested name (#271).
                let key = resolve_component_export_key(component, name)?;
                match component.exports.get(&key) {
                    Some(ComponentExport::Component(idx)) => {
                        RuntimeExport::Component(self.env.resolve_component(*idx, store)?)
                    }
                    Some(ComponentExport::Instance(idx)) => {
                        RuntimeExport::Instance(self.env.resolve_instance(*idx, store)?)
                    }
                    Some(ComponentExport::Func { idx, type_id }) => {
                        if self.env.parent.is_none() {
                            if let Some(Relation::Import(_)) = self.env.program.func_store.get(idx)
                            {
                                if let Some(binding) = self.env.linker.resolve_export(&key) {
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
                        let callable = self.env.resolve_func(*idx, store)?;
                        RuntimeExport::Func(present_callable_under_export_type(
                            &self.env.program,
                            callable,
                            *type_id,
                        ))
                    }
                    Some(ComponentExport::Module(idx)) => RuntimeExport::CoreModule(Box::new(
                        self.env.resolve_core_module(*idx, store)?,
                    )),
                    Some(ComponentExport::Value { idx, .. }) => {
                        RuntimeExport::Value(self.env.resolve_value(*idx, store)?)
                    }
                    Some(ComponentExport::Resource(_)) | None => {
                        return Err(ComponentError::ExportNotFound(name.to_owned()))
                    }
                }
            }
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

/// Structural check that a host-provided value conforms to a declared value
/// type. Defined values are produced by the decoder and need no check; this
/// guards the one place unvalidated values enter the runtime (linker-supplied
/// value imports). Recursion follows the value's own structure, so it
/// terminates on any finite value even for recursive types.
///
/// Alongside the canonical forms, the dedicated `Enum` / `Option` / `Result`
/// representations are accepted for the variant shapes they desugar to, since
/// those are what canonical-ABI lifting produces for such types.
fn value_matches_type(
    program: &ComponentProgram,
    value: &ComponentValue,
    ty: &ValType,
) -> Result<(), ComponentError> {
    fn mismatch() -> ComponentError {
        ComponentError::Link("value does not match the declared type".to_owned())
    }
    match ty {
        ValType::Primitive(prim) => value_matches_prim(value, prim),
        // An `(eq)` type -- an alias, or the face of a plain type
        // export -- denotes its underlying type; judge the value
        // against the denoted type.
        ValType::Type(id) => match program.resolve_eq_type(*id) {
            Some(Type::DefVal(def)) => {
                let def = def.clone();
                value_matches_defval(program, value, &def)
            }
            Some(Type::Resource(_)) => match value {
                ComponentValue::Own(_) | ComponentValue::Borrow(_) => Ok(()),
                _ => Err(mismatch()),
            },
            _ => Err(mismatch()),
        },
    }
}

fn value_matches_prim(value: &ComponentValue, prim: &PrimValType) -> Result<(), ComponentError> {
    let ok = matches!(
        (value, prim),
        (ComponentValue::Bool(_), PrimValType::Bool)
            | (ComponentValue::U8(_), PrimValType::U8)
            | (ComponentValue::S8(_), PrimValType::S8)
            | (ComponentValue::U16(_), PrimValType::U16)
            | (ComponentValue::S16(_), PrimValType::S16)
            | (ComponentValue::U32(_), PrimValType::U32)
            | (ComponentValue::S32(_), PrimValType::S32)
            | (ComponentValue::U64(_), PrimValType::U64)
            | (ComponentValue::S64(_), PrimValType::S64)
            | (ComponentValue::F32(_), PrimValType::F32)
            | (ComponentValue::F64(_), PrimValType::F64)
            | (ComponentValue::Char(_), PrimValType::Char)
            | (ComponentValue::String(_), PrimValType::String)
    );
    if ok {
        Ok(())
    } else {
        Err(ComponentError::Link(
            "value does not match the declared type".to_owned(),
        ))
    }
}

fn value_matches_defval(
    program: &ComponentProgram,
    value: &ComponentValue,
    def: &DefValType,
) -> Result<(), ComponentError> {
    fn mismatch() -> ComponentError {
        ComponentError::Link("value does not match the declared type".to_owned())
    }
    match def {
        DefValType::Primitive(prim) => value_matches_prim(value, prim),
        DefValType::Record(fields) => match value {
            ComponentValue::Record(entries) => {
                if entries.len() != fields.len() {
                    return Err(mismatch());
                }
                for (field, (label, entry)) in fields.iter().zip(entries.iter()) {
                    if field.label.flat() != *label {
                        return Err(mismatch());
                    }
                    value_matches_type(program, entry, &field.ty)?;
                }
                Ok(())
            }
            // A tuple is a record with numeric labels "0", "1", ... in order.
            ComponentValue::Tuple(entries) => {
                if entries.len() != fields.len() {
                    return Err(mismatch());
                }
                for (index, (field, entry)) in fields.iter().zip(entries.iter()).enumerate() {
                    if field.label.flat() != index.to_string() {
                        return Err(mismatch());
                    }
                    value_matches_type(program, entry, &field.ty)?;
                }
                Ok(())
            }
            _ => Err(mismatch()),
        },
        DefValType::Variant(cases) => match value {
            ComponentValue::Variant {
                case,
                value: payload,
            } => {
                let case = cases
                    .iter()
                    .find(|c| c.label.flat() == *case)
                    .ok_or_else(mismatch)?;
                match (&case.ty, payload) {
                    (Some(ty), Some(payload)) => value_matches_type(program, payload, ty),
                    (None, None) => Ok(()),
                    _ => Err(mismatch()),
                }
            }
            ComponentValue::Enum(case) => {
                let case = cases
                    .iter()
                    .find(|c| c.label.flat() == *case)
                    .ok_or_else(mismatch)?;
                if case.ty.is_none() {
                    Ok(())
                } else {
                    Err(mismatch())
                }
            }
            ComponentValue::Option(payload) => {
                // `option t` desugars to the variant `none | some(t)`.
                let is_option_shape = cases.len() == 2
                    && cases[0].label.flat() == "none"
                    && cases[0].ty.is_none()
                    && cases[1].label.flat() == "some";
                if !is_option_shape {
                    return Err(mismatch());
                }
                match (payload, &cases[1].ty) {
                    (None, _) => Ok(()),
                    (Some(payload), Some(ty)) => value_matches_type(program, payload, ty),
                    _ => Err(mismatch()),
                }
            }
            ComponentValue::Result { ok, err } => {
                // `result t e` desugars to the variant `ok(t) | err(e)`.
                let is_result_shape = cases.len() == 2
                    && cases[0].label.flat() == "ok"
                    && cases[1].label.flat() == "err";
                if !is_result_shape {
                    return Err(mismatch());
                }
                match (ok, err) {
                    (Some(payload), None) => match &cases[0].ty {
                        Some(ty) => value_matches_type(program, payload, ty),
                        None => Err(mismatch()),
                    },
                    (None, Some(payload)) => match &cases[1].ty {
                        Some(ty) => value_matches_type(program, payload, ty),
                        None => Err(mismatch()),
                    },
                    (None, None) => {
                        if cases[0].ty.is_none() && cases[1].ty.is_none() {
                            Ok(())
                        } else {
                            Err(mismatch())
                        }
                    }
                    _ => Err(mismatch()),
                }
            }
            _ => Err(mismatch()),
        },
        DefValType::Flags(labels) => match value {
            ComponentValue::Flags(selected) => {
                if selected
                    .iter()
                    .all(|label| labels.iter().any(|l| l.flat() == *label))
                {
                    Ok(())
                } else {
                    Err(mismatch())
                }
            }
            _ => Err(mismatch()),
        },
        DefValType::List(elem, len) => match value {
            ComponentValue::List(items) => {
                if let Some(len) = len {
                    if items.len() != *len {
                        return Err(mismatch());
                    }
                }
                for item in items {
                    value_matches_type(program, item, elem)?;
                }
                Ok(())
            }
            _ => Err(mismatch()),
        },
        DefValType::Own(_) => match value {
            ComponentValue::Own(_) => Ok(()),
            _ => Err(mismatch()),
        },
        DefValType::Borrow(_) => match value {
            ComponentValue::Borrow(_) => Ok(()),
            _ => Err(mismatch()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_component() -> Component {
        Component {
            imports: HashMap::new(),
            exports: HashMap::new(),
            defined_instances: Vec::new(),
        }
    }

    fn program_with_defined_component(component_idx: GlobalIdx<Component>) -> ComponentProgram {
        ComponentProgram {
            type_infos: Vec::new(),
            imports: Vec::new(),
            callable_imports: Vec::new(),
            exports: Vec::new(),
            callable_exports: Vec::new(),
            ops: Vec::new(),
            bytes: Vec::new(),
            root: empty_component(),
            types: Vec::new().into_boxed_slice(),
            component_store: HashMap::from([(component_idx, Relation::Defined(empty_component()))]),
            instance_store: HashMap::new(),
            func_store: HashMap::new(),
            core_module_store: HashMap::new(),
            core_type_store: HashMap::new(),
            core_instance_store: HashMap::new(),
            core_func_store: HashMap::new(),
            core_memory_store: HashMap::new(),
            core_global_store: HashMap::new(),
            core_table_store: HashMap::new(),
            value_store: HashMap::new(),
        }
    }

    #[test]
    fn resolve_component_cache_does_not_retain_root_cache() {
        let component_idx = GlobalIdx::new();
        let program = Rc::new(program_with_defined_component(component_idx));
        let env = Rc::new(RuntimeEnv::new(
            program,
            ComponentLinker::new(),
            None,
            HashMap::new(),
            Rc::new(SharedState::default()),
        ));
        let store = Store::new();

        let component = env
            .resolve_component(component_idx, &store)
            .expect("defined component should resolve");

        assert!(!Rc::ptr_eq(&env.caches, &component.env.caches));
        assert_eq!(Rc::strong_count(&env.caches), 1);
        assert_eq!(env.caches.components.borrow().len(), 1);

        let root_caches = Rc::downgrade(&env.caches);
        drop(env);

        assert!(root_caches.upgrade().is_none());
    }
}
