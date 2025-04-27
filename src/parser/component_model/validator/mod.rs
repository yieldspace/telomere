mod component;
mod store;
mod idx;
mod parent;
mod child;

use crate::component_model::{Binding, ComponentBinding, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport, ComponentType, CoreFuncIdx, CoreFunction, CoreFunctionBinding, CoreGlobalBinding, CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceBinding, CoreInstanceIdx, CoreMemoryBinding, CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleBinding, CoreModuleIdx, CoreTableBinding, CoreTableIdx, CoreTableRef, CoreType, CoreTypeBinding, CoreTypeIdx, FlattenComponent, FuncIdx, Idx, InlineComponent, Instance, InstanceBinding, InstanceIdx, Resolvable, Resolver, Type, TypeBinding, TypeIdx};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::error::ComponentParseError;
use either::Either;
use std::collections::HashMap;
pub use store::LocalStore;
pub use parent::*;
pub use component::ComponentValidator;
pub use crate::parser::component_model::validator::idx::IdxValidator;

pub trait DefaultValidator: Validator 
    + Resolver<CoreType, Error=ComponentParseError>
    + Resolver<CoreModule, Error=ComponentParseError>
    + Resolver<CoreInstance, Error=ComponentParseError>
    + Resolver<CoreFunction, Error=ComponentParseError>
    + Resolver<Type, Error=ComponentParseError>
    + Resolver<Instance, Error=ComponentParseError>
    + Resolver<InlineComponent, Error=ComponentParseError>
    + Resolver<ComponentFunction, Error=ComponentParseError>
    + IdxValidator<CoreTypeIdx, CoreType>
    + IdxValidator<CoreModuleIdx, CoreModule>
    + IdxValidator<CoreInstanceIdx, CoreInstance>
    + IdxValidator<CoreFuncIdx, CoreFunction>
    + IdxValidator<CoreMemoryIdx, CoreMemoryRef>
    + IdxValidator<CoreTableIdx, CoreTableRef>
    + IdxValidator<CoreGlobalIdx, CoreGlobalRef>
    + IdxValidator<ComponentIdx, InlineComponent>
    + IdxValidator<FuncIdx, ComponentFunction>
    + IdxValidator<TypeIdx, Type>
    + IdxValidator<InstanceIdx, Instance>
{}

pub trait Validator: private::Sealed {
    fn get_flatten_component(&self) -> &FlattenComponent where Self: Sized;
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent where Self: Sized;
    fn get_parent(&self) -> Option<&impl Parent> where Self: Sized;
    fn get_local_store(&self) -> &LocalStore where Self: Sized;
    fn get_local_store_mut(&mut self) -> &mut LocalStore where Self: Sized;

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn validate_value_idx(&self, local: usize) -> Result<ValueIdx, ComponentParseError> where Self: Sized {
        self.get_local_store()
            .values
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "value".to_string()))
    }

    fn add_core_module(
        &mut self,
        module: CoreModuleBinding,
    ) -> Result<CoreModuleIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().core_modules.len();
        let idx = CoreModuleIdx::new(global_idx);
        self.get_flatten_component_mut().core_modules.push(module);
        self.get_local_store_mut().core_modules.push(idx);
        Ok(idx)
    }

    fn add_core_instance(
        &mut self,
        instance: CoreInstanceBinding,
    ) -> Result<CoreInstanceIdx, ComponentParseError> where Self: Sized {
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
    ) -> Result<CoreFuncIdx, ComponentParseError> where Self: Sized{
        let global_idx = self.get_flatten_component().core_functions.len();
        let idx = CoreFuncIdx::new(global_idx);
        self.get_flatten_component_mut().core_functions.push(func);
        self.get_local_store_mut().core_funcs.push(idx);
        Ok(idx)
    }

    fn add_core_type(&mut self, ty: CoreTypeBinding) -> Result<CoreTypeIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().core_types.len();
        let idx = CoreTypeIdx::new(global_idx);
        self.get_flatten_component_mut().core_types.push(ty);
        self.get_local_store_mut().core_types.push(idx);
        Ok(idx)
    }

    fn add_core_memory(
        &mut self,
        memory: CoreMemoryBinding,
    ) -> Result<CoreMemoryIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().core_memories.len();
        let idx = CoreMemoryIdx::new(global_idx);
        self.get_flatten_component_mut().core_memories.push(memory);
        self.get_local_store_mut().core_memories.push(idx);
        Ok(idx)
    }

    fn add_core_table(
        &mut self,
        table: CoreTableBinding,
    ) -> Result<CoreTableIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().core_tables.len();
        let idx = CoreTableIdx::new(global_idx);
        self.get_flatten_component_mut().core_tables.push(table);
        self.get_local_store_mut().core_tables.push(idx);
        Ok(idx)
    }

    fn add_core_global(
        &mut self,
        global: CoreGlobalBinding,
    ) -> Result<CoreGlobalIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().core_globals.len();
        let idx = CoreGlobalIdx::new(global_idx);
        self.get_flatten_component_mut().core_globals.push(global);
        self.get_local_store_mut().core_globals.push(idx);
        Ok(idx)
    }

    fn add_component(
        &mut self,
        component: ComponentBinding,
    ) -> Result<ComponentIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().components.len();
        let idx = ComponentIdx::new(global_idx);
        self.get_flatten_component_mut().components.push(component);
        self.get_local_store_mut().components.push(idx);
        Ok(idx)
    }

    fn add_instance(
        &mut self,
        instance: InstanceBinding,
    ) -> Result<InstanceIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().instances.len();
        let idx = InstanceIdx::new(global_idx);
        self.get_flatten_component_mut().instances.push(instance);
        self.get_local_store_mut().instances.push(idx);
        Ok(idx)
    }

    fn add_func(
        &mut self,
        func: Binding<ComponentFunction>,
    ) -> Result<FuncIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().functions.len();
        let idx = FuncIdx::new(global_idx);
        self.get_flatten_component_mut().functions.push(func);
        self.get_local_store_mut().functions.push(idx);
        Ok(idx)
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> where Self: Sized {
        let global_idx = self.get_flatten_component().types.len();
        let idx = TypeIdx::new(global_idx);
        self.get_flatten_component_mut().types.push(ty);
        self.get_local_store_mut().types.push(idx);
        Ok(idx)
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn add_value(&mut self, value: Binding<ValueBound>) -> Result<ValueIdx, ComponentParseError> where Self: Sized {
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
    ) -> Result<(), ComponentParseError> where Self: Sized {
        self.get_local_store_mut().imports.insert(name, import);
        Ok(())
    }

    fn add_export(
        &mut self,
        name: String,
        export: ComponentExport,
    ) -> Result<(), ComponentParseError> where Self: Sized {
        self.get_local_store_mut().exports.insert(name, export);
        Ok(())
    }
}

mod private {
    use crate::parser::component_model::types::TypeValidator;
    use crate::parser::component_model::{ComponentValidator, DefaultValidator, Validator};
    use crate::parser::component_model::validator::parent::Parent;

    pub trait Sealed {}

    // 同じ型に実装
    impl<P: Parent> Sealed for ComponentValidator<'_, P> {}
    impl<P: Parent> Sealed for TypeValidator<P> {}
}
