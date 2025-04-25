mod child;

use crate::component_model::{
    Binding, ComponentBinding, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport,
    ComponentType, CoreFuncIdx, CoreFunction, CoreFunctionBinding, CoreGlobalBinding,
    CoreGlobalIdx, CoreInstance, CoreInstanceBinding, CoreInstanceIdx,
    CoreMemoryBinding, CoreMemoryIdx, CoreModule, CoreModuleBinding, CoreModuleIdx,
    CoreTableBinding, CoreTableIdx, CoreType, CoreTypeBinding, CoreTypeIdx,
    FlattenComponent, FuncIdx, Idx, InlineComponent, Instance, InstanceBinding, InstanceIdx,
    Resolvable, Resolver, Type, TypeBinding, TypeIdx,
};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::error::ComponentParseError;
pub use child::ChildValidator;
use either::Either;
use std::collections::HashMap;

#[derive(Default)]
pub struct LocalStore {
    pub core_modules: Vec<CoreModuleIdx>,
    pub core_instances: Vec<CoreInstanceIdx>,
    pub core_funcs: Vec<CoreFuncIdx>,
    pub components: Vec<ComponentIdx>,
    pub instances: Vec<InstanceIdx>,
    pub core_memories: Vec<CoreMemoryIdx>,
    pub core_tables: Vec<CoreTableIdx>,
    pub core_globals: Vec<CoreGlobalIdx>,
    pub core_types: Vec<CoreTypeIdx>,
    pub functions: Vec<FuncIdx>,
    pub types: Vec<TypeIdx>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    pub values: Vec<ValueIdx>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,
}

impl LocalStore {
    pub fn make_component_type(&self) -> ComponentType {
        ComponentType {
            imports: Default::default(),
            exports: Default::default(),
            core_types: self.core_types.clone(),
            types: self.types.clone(),
            instances: self.instances.clone(),
        }
    }
}

pub trait Validator: private::Sealed {
    fn get_parent(&self) -> Option<&dyn Validator>;
    fn get_flatten_component(&self) -> &FlattenComponent;
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent;
    fn get_local_store(&self) -> &LocalStore;
    fn get_local_store_mut(&mut self) -> &mut LocalStore;

    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError> {
        self.get_local_store()
            .core_modules
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core module".to_string()))
    }

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        self.get_local_store()
            .core_instances
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core instance".to_string()))
    }

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError> {
        self.get_local_store()
            .core_funcs
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core function".to_string()))
    }
    fn validate_core_memory_idx(&self, local: usize) -> Result<CoreMemoryIdx, ComponentParseError> {
        self.get_local_store()
            .core_memories
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core memory".to_string()))
    }
    fn validate_core_table_idx(&self, local: usize) -> Result<CoreTableIdx, ComponentParseError> {
        self.get_local_store()
            .core_tables
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core table".to_string()))
    }
    fn validate_core_type_idx(&self, local: usize) -> Result<CoreTypeIdx, ComponentParseError> {
        self.get_local_store()
            .core_types
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core type".to_string()))
    }
    fn validate_core_global_idx(&self, local: usize) -> Result<CoreGlobalIdx, ComponentParseError> {
        self.get_local_store()
            .core_globals
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core global".to_string()))
    }
    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError> {
        self.get_local_store()
            .components
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "component".to_string()))
    }

    fn validate_function_idx(&self, local: usize) -> Result<FuncIdx, ComponentParseError> {
        self.get_local_store()
            .functions
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "function".to_string()))
    }

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError> {
        self.get_local_store()
            .types
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "type".to_string()))
    }

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError> {
        self.get_local_store()
            .instances
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "instance".to_string()))
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn validate_value_idx(&self, local: usize) -> Result<ValueIdx, ComponentParseError> {
        self.get_local_store()
            .values
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "value".to_string()))
    }

    fn add_core_module(
        &mut self,
        module: CoreModuleBinding,
    ) -> Result<CoreModuleIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_modules.len();
        let idx = CoreModuleIdx::new(global_idx);
        self.get_flatten_component_mut().core_modules.push(module);
        self.get_local_store_mut().core_modules.push(idx);
        Ok(idx)
    }

    fn add_core_instance(
        &mut self,
        instance: CoreInstanceBinding,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_instances.len();
        let idx = CoreInstanceIdx::new(global_idx);
        self.get_flatten_component_mut()
            .core_instances
            .push(instance);
        self.get_local_store_mut().core_instances.push(idx);
        Ok(idx)
    }

    fn add_core_func(
        &mut self,
        func: CoreFunctionBinding,
    ) -> Result<CoreFuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_functions.len();
        let idx = CoreFuncIdx::new(global_idx);
        self.get_flatten_component_mut().core_functions.push(func);
        self.get_local_store_mut().core_funcs.push(idx);
        Ok(idx)
    }

    fn add_core_type(&mut self, ty: CoreTypeBinding) -> Result<CoreTypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_types.len();
        let idx = CoreTypeIdx::new(global_idx);
        self.get_flatten_component_mut().core_types.push(ty);
        self.get_local_store_mut().core_types.push(idx);
        Ok(idx)
    }

    fn add_core_memory(
        &mut self,
        memory: CoreMemoryBinding,
    ) -> Result<CoreMemoryIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_memories.len();
        let idx = CoreMemoryIdx::new(global_idx);
        self.get_flatten_component_mut().core_memories.push(memory);
        self.get_local_store_mut().core_memories.push(idx);
        Ok(idx)
    }

    fn add_core_table(
        &mut self,
        table: CoreTableBinding,
    ) -> Result<CoreTableIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_tables.len();
        let idx = CoreTableIdx::new(global_idx);
        self.get_flatten_component_mut().core_tables.push(table);
        self.get_local_store_mut().core_tables.push(idx);
        Ok(idx)
    }

    fn add_core_global(
        &mut self,
        global: CoreGlobalBinding,
    ) -> Result<CoreGlobalIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_globals.len();
        let idx = CoreGlobalIdx::new(global_idx);
        self.get_flatten_component_mut().core_globals.push(global);
        self.get_local_store_mut().core_globals.push(idx);
        Ok(idx)
    }

    fn add_component(
        &mut self,
        component: ComponentBinding,
    ) -> Result<ComponentIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().components.len();
        let idx = ComponentIdx::new(global_idx);
        self.get_flatten_component_mut().components.push(component);
        self.get_local_store_mut().components.push(idx);
        Ok(idx)
    }

    fn add_instance(
        &mut self,
        instance: InstanceBinding,
    ) -> Result<InstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().instances.len();
        let idx = InstanceIdx::new(global_idx);
        self.get_flatten_component_mut().instances.push(instance);
        self.get_local_store_mut().instances.push(idx);
        Ok(idx)
    }

    fn add_func(
        &mut self,
        func: Binding<ComponentFunction>,
    ) -> Result<FuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().functions.len();
        let idx = FuncIdx::new(global_idx);
        self.get_flatten_component_mut().functions.push(func);
        self.get_local_store_mut().functions.push(idx);
        Ok(idx)
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().types.len();
        let idx = TypeIdx::new(global_idx);
        self.get_flatten_component_mut().types.push(ty);
        self.get_local_store_mut().types.push(idx);
        Ok(idx)
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn add_value(&mut self, value: Binding<ValueBound>) -> Result<ValueIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().values.len();
        let idx = ValueIdx::new(global_idx);
        self.get_flatten_component_mut().values.push(value);
        self.get_local_store_mut().values.push(idx);
        Ok(idx)
    }

    fn add_import(
        &mut self,
        name: String,
        import: ComponentImport,
    ) -> Result<(), ComponentParseError> {
        self.get_local_store_mut().imports.insert(name, import);
        Ok(())
    }

    fn add_export(
        &mut self,
        name: String,
        export: ComponentExport,
    ) -> Result<(), ComponentParseError> {
        self.get_local_store_mut().exports.insert(name, export);
        Ok(())
    }
}

impl<T> Resolver<Type> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&Type, Self::Error>
    where
        I: Idx + Resolvable<Type>,
    {
        match self
            .get_flatten_component()
            .types
            .get(idx.global())
            .ok_or_else(|| ComponentParseError::InvalidIdx(idx.global(), "type".to_string()))?
        {
            TypeBinding::Real(ty) => Ok(ty),
            TypeBinding::Alias(idx) => self.resolve(&TypeIdx::new(*idx)),
            TypeBinding::Reference(ty, _) => Ok(ty),
        }
    }
}

impl<T> Resolver<Instance> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&Instance, Self::Error>
    where
        I: Idx + Resolvable<Instance>,
    {
        match self
            .get_flatten_component()
            .instances
            .get(idx.global())
            .ok_or_else(|| ComponentParseError::InvalidIdx(idx.global(), "instance".to_string()))?
        {
            InstanceBinding::Real(inst) => Ok(inst),
            InstanceBinding::Alias(idx) => self.resolve(&InstanceIdx::new(*idx)),
            InstanceBinding::Reference(inst, _) => Ok(inst),
        }
    }
}

impl<T> Resolver<InlineComponent> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&InlineComponent, Self::Error>
    where
        I: Idx + Resolvable<InlineComponent>,
    {
        match self
            .get_flatten_component()
            .components
            .get(idx.global())
            .ok_or_else(|| ComponentParseError::InvalidIdx(idx.global(), "component".to_string()))?
        {
            Binding::Real(comp) => Ok(comp),
            Binding::Alias(idx) => self.resolve(&ComponentIdx::new(*idx)),
            Binding::Reference(comp, _) => Ok(comp),
        }
    }
}

impl<T> Resolver<CoreType> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreType, Self::Error>
    where
        I: Idx + Resolvable<CoreType>,
    {
        match self
            .get_flatten_component()
            .core_types
            .get(idx.global())
            .ok_or_else(|| ComponentParseError::InvalidIdx(idx.global(), "core type".to_string()))?
        {
            CoreTypeBinding::Real(value) => Ok(value),
            CoreTypeBinding::Alias(idx) => self.resolve(&CoreTypeIdx::new(*idx)),
            CoreTypeBinding::Reference(value, _) => Ok(value),
        }
    }
}

impl<T> Resolver<CoreInstance> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreInstance, Self::Error>
    where
        I: Idx + Resolvable<CoreInstance>,
    {
        match self
            .get_flatten_component()
            .core_instances
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "core instance".to_string())
            })? {
            CoreInstanceBinding::Real(value) => Ok(value),
            CoreInstanceBinding::Alias(idx) => self.resolve(&CoreInstanceIdx::new(*idx)),
            CoreInstanceBinding::Reference(value, _) => Ok(value),
        }
    }
}

impl<T> Resolver<CoreFunction> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreFunction, Self::Error>
    where
        I: Idx + Resolvable<CoreFunction>,
    {
        match self
            .get_flatten_component()
            .core_functions
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "core function".to_string())
            })? {
            CoreFunctionBinding::Real(value) => Ok(value),
            CoreFunctionBinding::Alias(idx) => self.resolve(&CoreFuncIdx::new(*idx)),
            CoreFunctionBinding::Reference(value, _) => Ok(value),
        }
    }
}

impl<T> Resolver<CoreModule> for T
where
    T: Validator + ?Sized,
{
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreModule, Self::Error>
    where
        I: Idx + Resolvable<CoreModule>,
    {
        match self
            .get_flatten_component()
            .core_modules
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "core module".to_string())
            })? {
            CoreModuleBinding::Real(value) => Ok(value),
            CoreModuleBinding::Alias(idx) => self.resolve(&CoreModuleIdx::new(*idx)),
            CoreModuleBinding::Reference(value, _) => Ok(value),
        }
    }
}

pub struct ComponentValidator<'a, 'b> {
    resource: Either<&'a mut FlattenComponent, &'b mut dyn Validator>,
    store: LocalStore,
}

impl<'a, 'b> ComponentValidator<'a, 'b> {
    pub fn new(component: &'a mut FlattenComponent) -> Self {
        Self {
            resource: Either::Left(component),
            store: LocalStore::default(),
        }
    }

    pub fn new_child(parent: &'b mut dyn Validator) -> Self {
        Self {
            resource: Either::Right(parent),
            store: LocalStore::default(),
        }
    }
}

impl Validator for ComponentValidator<'_, '_> {
    #[inline]
    fn get_parent(&self) -> Option<&dyn Validator> {
        None
    }

    #[inline]
    fn get_flatten_component(&self) -> &FlattenComponent {
        match self.resource {
            Either::Left(ref left) => left,
            Either::Right(ref right) => right.get_flatten_component(),
        }
    }

    #[inline]
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        match self.resource {
            Either::Left(ref mut left) => left,
            Either::Right(ref mut right) => right.get_flatten_component_mut(),
        }
    }

    fn get_local_store(&self) -> &LocalStore {
        &self.store
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        &mut self.store
    }
}

mod private {
    use crate::parser::component_model::types::TypeValidator;
    use crate::parser::component_model::{ChildValidator, ComponentValidator};

    pub trait Sealed {}

    // 同じ型に実装
    impl Sealed for ComponentValidator<'_, '_> {}
    impl Sealed for ChildValidator<'_> {}
    impl Sealed for TypeValidator<'_> {}
}

pub fn get_outer_validator(validator: &dyn Validator, ct: u32) -> &dyn Validator {
    if ct == 0 {
        validator
    } else {
        get_outer_validator(validator.get_parent().unwrap(), ct - 1)
    }
}
