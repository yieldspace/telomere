use crate::component_model::{Binding, ComponentBinding, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport, CoreFuncIdx, CoreFunction, CoreFunctionBinding, CoreGlobalBinding, CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceBinding, CoreInstanceIdx, CoreMemoryBinding, CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleBinding, CoreModuleIdx, CoreTableBinding, CoreTableIdx, CoreTableRef, CoreType, CoreTypeBinding, CoreTypeIdx, FlattenComponent, FuncIdx, FunctionBinding, Idx, InlineComponent, Instance, InstanceBinding, InstanceIdx, Resolvable, Resolver, Type, TypeBinding, TypeIdx};
use crate::parser::component_model::validator::{IdxValidator, LocalStore};
use crate::parser::component_model::{ComponentParseError, TypeSuperValidator};

pub trait DefaultValidatorState:
    ValidatorStateImpl
    + Resolver<CoreType, Error = ComponentParseError>
    + Resolver<CoreModule, Error = ComponentParseError>
    + Resolver<CoreInstance, Error = ComponentParseError>
    + Resolver<CoreFunction, Error = ComponentParseError>
    + Resolver<CoreMemoryRef, Error = ComponentParseError>
    + Resolver<CoreTableRef, Error = ComponentParseError>
    + Resolver<CoreGlobalRef, Error = ComponentParseError>
    + Resolver<Type, Error = ComponentParseError>
    + Resolver<Instance, Error = ComponentParseError>
    + Resolver<InlineComponent, Error = ComponentParseError>
    + Resolver<ComponentFunction, Error = ComponentParseError>
    + IdxValidator<CoreModuleIdx, Resolved = CoreModule>
    + IdxValidator<CoreTypeIdx, Resolved = CoreType>
    + IdxValidator<CoreInstanceIdx, Resolved = CoreInstance>
    + IdxValidator<CoreFuncIdx, Resolved = CoreFunction>
    + IdxValidator<CoreMemoryIdx, Resolved = CoreMemoryRef>
    + IdxValidator<CoreTableIdx, Resolved = CoreTableRef>
    + IdxValidator<CoreGlobalIdx, Resolved = CoreGlobalRef>
    + IdxValidator<TypeIdx, Resolved = Type>
    + IdxValidator<ComponentIdx, Resolved = InlineComponent>
    + IdxValidator<InstanceIdx, Resolved = Instance>
    + IdxValidator<FuncIdx, Resolved = ComponentFunction>
    + TypeSuperValidator
{
}

pub trait ValidatorStateImpl {
    fn component(&self) -> &FlattenComponent;
    fn component_mut(&mut self) -> &mut FlattenComponent;
    fn add_core_module(
        &mut self,
        module: CoreModuleBinding,
    ) -> Result<CoreModuleIdx, ComponentParseError>
    where
        Self:
            Resolver<CoreModule, Error = ComponentParseError> + IdxValidator<CoreModuleIdx> + Sized {
        unimplemented!("add_core_module is not implemented");
    }
    fn add_core_instance(
        &mut self,
        instance: CoreInstanceBinding,
    ) -> Result<CoreInstanceIdx, ComponentParseError>
    where
        Self: Resolver<CoreInstance, Error = ComponentParseError>
            + IdxValidator<CoreInstanceIdx>
            + Sized {
        unimplemented!("add_core_instance is not implemented");
    }
    fn add_core_func(
        &mut self,
        func: CoreFunctionBinding,
    ) -> Result<CoreFuncIdx, ComponentParseError>
    where
        Self:
            Resolver<CoreFunction, Error = ComponentParseError> + IdxValidator<CoreFuncIdx> + Sized {
        unimplemented!("add_core_func is not implemented");
    }
    fn add_core_memory(
        &mut self,
        memory: CoreMemoryBinding,
    ) -> Result<CoreMemoryIdx, ComponentParseError>
    where
        Self: Resolver<CoreMemoryRef, Error = ComponentParseError>
            + IdxValidator<CoreMemoryIdx>
            + Sized {
        unimplemented!("add_core_memory is not implemented");
    }
    fn add_core_table(
        &mut self,
        table: CoreTableBinding,
    ) -> Result<CoreTableIdx, ComponentParseError>
    where
        Self: Resolver<CoreTableRef, Error = ComponentParseError>
            + IdxValidator<CoreTableIdx>
            + Sized {
        unimplemented!("add_core_table is not implemented");
    }
    fn add_core_global(
        &mut self,
        global: CoreGlobalBinding,
    ) -> Result<CoreGlobalIdx, ComponentParseError>
    where
        Self: Resolver<CoreGlobalRef, Error = ComponentParseError>
            + IdxValidator<CoreGlobalIdx>
            + Sized {
        unimplemented!("add_core_global is not implemented");
    }
    fn add_core_type(&mut self, ty: CoreTypeBinding) -> Result<CoreTypeIdx, ComponentParseError>
    where
        Self: Resolver<CoreType, Error = ComponentParseError> + IdxValidator<CoreTypeIdx> + Sized {
        unimplemented!("add_core_type is not implemented");
    }
    fn add_component(
        &mut self,
        component: ComponentBinding,
    ) -> Result<ComponentIdx, ComponentParseError>
    where
        Self: Resolver<InlineComponent, Error = ComponentParseError>
            + IdxValidator<ComponentIdx>
            + Sized {
        unimplemented!("add_component is not implemented");
    }
    fn add_instance(
        &mut self,
        instance: InstanceBinding,
    ) -> Result<InstanceIdx, ComponentParseError>
    where
        Self: Resolver<Instance, Error = ComponentParseError>
            + IdxValidator<InstanceIdx, Resolved = Instance>
            + Sized {
        unimplemented!("add_instance is not implemented");
    }
    fn add_func(&mut self, func: FunctionBinding) -> Result<FuncIdx, ComponentParseError>
    where
        Self: Resolver<ComponentFunction, Error = ComponentParseError>
            + IdxValidator<FuncIdx>
            + Sized {
        unimplemented!("add_func is not implemented");
    }
    fn add_type(&mut self, ty: TypeBinding) -> Result<TypeIdx, ComponentParseError>
    where
        Self: Resolver<Type, Error = ComponentParseError> + IdxValidator<TypeIdx> + Sized {
        unimplemented!("add_type is not implemented");
    }
    fn add_import(&mut self, name: String, import: ComponentImport) -> Result<(), ComponentParseError> where Self: Sized {
        unimplemented!("add_import is not implemented");
    }
    fn add_export(&mut self, name: String, export: ComponentExport) -> Result<(), ComponentParseError> where Self: Sized {
        unimplemented!("add_export is not implemented");
    }
}

pub enum ValidatorState<'a> {
    TopLevel {
        component: &'a mut FlattenComponent,
        store: LocalStore,
    },
    HasParent {
        store: LocalStore,
        parent: &'a mut Box<dyn DefaultValidatorState>,
    },
}

impl DefaultValidatorState for ValidatorState<'_> {}

impl<'a> ValidatorState<'a> {
    pub fn new(component: &'a mut FlattenComponent) -> Self {
        ValidatorState::TopLevel {
            component,
            store: LocalStore::default(),
        }
    }

    pub fn new_child(parent: &'a mut dyn DefaultValidatorState) -> Self {
        ValidatorState::HasParent {
            store: LocalStore::default(),
            parent,
        }
    }

    fn validate_idx_from<I: Idx>(
        &self,
        source: &Vec<I>,
        local: u32,
    ) -> Result<I, ComponentParseError> {
        Ok(source
            .get(local as usize).cloned().unwrap())
            // .ok_or_else(|| ComponentParseError::InvalidIdx(local as usize, "store".to_string()))
            // .cloned()
    }
}

impl ValidatorStateImpl for ValidatorState<'_> {
    fn component(&self) -> &FlattenComponent {
        match self {
            ValidatorState::TopLevel { component, .. } => component,
            ValidatorState::HasParent { parent, .. } => parent.component(),
        }
    }

    fn component_mut(&mut self) -> &mut FlattenComponent {
        match self {
            ValidatorState::TopLevel { component, .. } => component,
            ValidatorState::HasParent { parent, .. } => parent.component_mut(),
        }
    }

    fn add_core_module(
        &mut self,
        module: CoreModuleBinding,
    ) -> Result<CoreModuleIdx, ComponentParseError>
    where
        Self:
            Resolver<CoreModule, Error = ComponentParseError> + IdxValidator<CoreModuleIdx> + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreModuleIdx::new(component.core_modules.len());
        component.core_modules.push(module);
        store.core_modules.push(global_idx);
        Ok(global_idx)
    }

    fn add_core_instance(
        &mut self,
        instance: CoreInstanceBinding,
    ) -> Result<CoreInstanceIdx, ComponentParseError>
    where
        Self: Resolver<CoreInstance, Error = ComponentParseError>
            + IdxValidator<CoreInstanceIdx>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreInstanceIdx::new(component.core_instances.len());
        component.core_instances.push(instance);
        store.core_instances.push(global_idx);
        Ok(global_idx)
    }

    fn add_core_func(
        &mut self,
        func: CoreFunctionBinding,
    ) -> Result<CoreFuncIdx, ComponentParseError>
    where
        Self:
            Resolver<CoreFunction, Error = ComponentParseError> + IdxValidator<CoreFuncIdx> + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreFuncIdx::new(component.core_functions.len());
        component.core_functions.push(func);
        store.core_funcs.push(global_idx);
        Ok(global_idx)
    }

    fn add_core_memory(
        &mut self,
        memory: CoreMemoryBinding,
    ) -> Result<CoreMemoryIdx, ComponentParseError>
    where
        Self: Resolver<CoreMemoryRef, Error = ComponentParseError>
            + IdxValidator<CoreMemoryIdx>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreMemoryIdx::new(component.core_memories.len());
        component.core_memories.push(memory);
        store.core_memories.push(global_idx);
        Ok(global_idx)
    }

    fn add_core_table(
        &mut self,
        table: CoreTableBinding,
    ) -> Result<CoreTableIdx, ComponentParseError>
    where
        Self: Resolver<CoreTableRef, Error = ComponentParseError>
            + IdxValidator<CoreTableIdx>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreTableIdx::new(component.core_tables.len());
        component.core_tables.push(table);
        store.core_tables.push(global_idx);
        Ok(global_idx)
    }

    fn add_core_global(
        &mut self,
        global: CoreGlobalBinding,
    ) -> Result<CoreGlobalIdx, ComponentParseError>
    where
        Self: Resolver<CoreGlobalRef, Error = ComponentParseError>
            + IdxValidator<CoreGlobalIdx>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreGlobalIdx::new(component.core_globals.len());
        component.core_globals.push(global);
        store.core_globals.push(global_idx);
        Ok(global_idx)
    }

    fn add_core_type(&mut self, ty: CoreTypeBinding) -> Result<CoreTypeIdx, ComponentParseError>
    where
        Self: Resolver<CoreType, Error = ComponentParseError> + IdxValidator<CoreTypeIdx> + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = CoreTypeIdx::new(component.core_types.len());
        component.core_types.push(ty);
        store.core_types.push(global_idx);
        Ok(global_idx)
    }

    fn add_component(
        &mut self,
        value: ComponentBinding,
    ) -> Result<ComponentIdx, ComponentParseError>
    where
        Self: Resolver<InlineComponent, Error = ComponentParseError>
            + IdxValidator<ComponentIdx>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = ComponentIdx::new(component.components.len());
        component.components.push(value);
        store.components.push(global_idx);
        Ok(global_idx)
    }

    fn add_instance(
        &mut self,
        instance: InstanceBinding,
    ) -> Result<InstanceIdx, ComponentParseError>
    where
        Self: Resolver<Instance, Error = ComponentParseError>
            + IdxValidator<InstanceIdx, Resolved = Instance>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = InstanceIdx::new(component.instances.len());
        component.instances.push(instance);
        store.instances.push(global_idx);
        Ok(global_idx)
    }

    fn add_func(&mut self, func: FunctionBinding) -> Result<FuncIdx, ComponentParseError>
    where
        Self: Resolver<ComponentFunction, Error = ComponentParseError>
            + IdxValidator<FuncIdx>
            + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = FuncIdx::new(component.functions.len());
        component.functions.push(func);
        store.functions.push(global_idx);
        Ok(global_idx)
    }

    fn add_type(&mut self, ty: TypeBinding) -> Result<TypeIdx, ComponentParseError>
    where
        Self: Resolver<Type, Error = ComponentParseError> + IdxValidator<TypeIdx> + Sized,
    {
        let (component, store) = match self {
            ValidatorState::TopLevel { component, store } => (component as &mut FlattenComponent, store),
            ValidatorState::HasParent { parent, store } => (parent.component_mut(), store)
        };
        let global_idx = TypeIdx::new(component.types.len());
        component.types.push(ty);
        store.types.push(global_idx);
        Ok(global_idx)
    }

    fn add_import(&mut self, name: String, import: ComponentImport) -> Result<(), ComponentParseError>
    where
        Self: Sized,
    {
        let (store) = match self {
            ValidatorState::TopLevel { component, store } => store,
            ValidatorState::HasParent { parent, store } => store,
        };
        store.imports.insert(name.clone(), import);
        Ok(())
    }

    fn add_export(&mut self, name: String, export: ComponentExport) -> Result<(), ComponentParseError>
    where
        Self: Sized,
    {
        let (store) = match self {
            ValidatorState::TopLevel { store, .. } => store,
            ValidatorState::HasParent { store, .. } => store,
        };
        store.exports.insert(name.clone(), export);
        Ok(())
    }
}

macro_rules! impl_resolver {
    ($name:ident, $field:ident) => {
        impl Resolver<$name> for ValidatorState<'_> {
            type Error = ComponentParseError;

            fn resolve<I>(&self, idx: &I) -> Result<&$name, Self::Error>
            where
                I: Idx + Resolvable<$name>,
            {
                match self.component().$field
                    .get(idx.global())
                    .ok_or_else(|| ComponentParseError::InvalidIdx(idx.global(), "store from state".to_string()))?
                {
                    Binding::Real(real) => Ok(real),
                    Binding::Alias(idx) => self.resolve(&I::new(*idx)),
                    Binding::Reference(real, _) => Ok(real),
                }
            }
        }
    };
}

impl_resolver!(CoreModule, core_modules);
impl_resolver!(CoreTableRef, core_tables);
impl_resolver!(CoreMemoryRef, core_memories);
impl_resolver!(CoreGlobalRef, core_globals);
impl_resolver!(CoreInstance, core_instances);
impl_resolver!(CoreFunction, core_functions);
impl_resolver!(CoreType, core_types);
impl_resolver!(Type, types);
impl_resolver!(ComponentFunction, functions);
impl_resolver!(InlineComponent, components);
impl_resolver!(Instance, instances);

macro_rules! impl_idx_validator {
    ($idx:ident, $field:ident, $target:ident) => {
        impl IdxValidator<$idx> for ValidatorState<'_> {
            type Resolved = $target;
            fn validate_local_idx(&self, local_idx: u32) -> Result<$idx, ComponentParseError> {
                match self {
                    ValidatorState::TopLevel { store, .. } => {
                        self.validate_idx_from(&store.$field, local_idx)
                    }
                    ValidatorState::HasParent { store, .. } => {
                        self.validate_idx_from(&store.$field, local_idx)
                    }
                }
            }

            fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<$idx, ComponentParseError> {
                if ct == 0 {
                    self.validate_local_idx(idx)
                } else {
                    match self {
                        ValidatorState::TopLevel { .. } => panic!(),
                        ValidatorState::HasParent { parent, .. } => {
                            parent.validate_outer_idx(ct - 1, idx)
                        }
                    }
                }
            }
        }
    };
}

impl_idx_validator!(CoreModuleIdx, core_modules, CoreModule);
impl_idx_validator!(CoreTypeIdx, core_types, CoreType);
impl_idx_validator!(CoreTableIdx, core_tables, CoreTableRef);
impl_idx_validator!(CoreMemoryIdx, core_memories, CoreMemoryRef);
impl_idx_validator!(CoreGlobalIdx, core_globals, CoreGlobalRef);
impl_idx_validator!(CoreInstanceIdx, core_instances, CoreInstance);
impl_idx_validator!(CoreFuncIdx, core_funcs, CoreFunction);
impl_idx_validator!(TypeIdx, types, Type);
impl_idx_validator!(ComponentIdx, components, InlineComponent);
impl_idx_validator!(FuncIdx, functions, ComponentFunction);
impl_idx_validator!(InstanceIdx, instances, Instance);
