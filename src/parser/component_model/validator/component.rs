use std::collections::HashMap;
use either::Either;
use crate::component_model::{Binding, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport, ComponentType, CoreFuncIdx, CoreFunction, CoreFunctionBinding, CoreGlobalBinding, CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceBinding, CoreInstanceIdx, CoreMemoryBinding, CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleBinding, CoreModuleIdx, CoreTableBinding, CoreTableIdx, CoreTableRef, CoreType, CoreTypeBinding, CoreTypeIdx, FlattenComponent, FuncIdx, Idx, InlineComponent, Instance, InstanceBinding, InstanceIdx, Resolvable, Resolver, Type, TypeBinding, TypeIdx};
use crate::parser::component_model::{ComponentParseError, Validator};
use crate::parser::component_model::validator::DefaultValidator;
use crate::parser::component_model::validator::idx::IdxValidator;
use crate::parser::component_model::validator::parent::Parent;
use crate::parser::component_model::validator::store::LocalStore;

pub enum ComponentValidator<'a, P: Parent> {
    TopLevel(&'a mut FlattenComponent, LocalStore),
    Child(P, LocalStore),
}

impl<'a, P: Parent> ComponentValidator<'a, P> {
    pub fn new(component: &'a mut FlattenComponent) -> Self {
        Self::TopLevel(component, LocalStore::default())
    }

    pub fn new_child(parent: P) -> Self {
        Self::Child(parent, LocalStore::default())
    }
}

impl<P: Parent> Validator for ComponentValidator<'_, P> {
    fn get_flatten_component(&self) -> &FlattenComponent {
        match self {
            ComponentValidator::TopLevel(component, _) => component,
            ComponentValidator::Child(parent, _) => parent.get().unwrap().get_flatten_component()
        }
    }

    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent
    where
        Self: Sized
    {
        match self {
            ComponentValidator::TopLevel(component, _) => component,
            ComponentValidator::Child(parent, _) => parent.get_mut().unwrap().get_flatten_component_mut()
        }
    }

    fn get_parent(&self) -> Option<&P>
    where
        Self: Sized
    {
        match self {
            ComponentValidator::TopLevel(_, _) => None,
            ComponentValidator::Child(parent, _) => Some(parent)
        }
    }

    fn get_local_store(&self) -> &LocalStore {
        match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        }
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        }
    }
}


impl<P: Parent> Resolver<Type> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<Instance> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<InlineComponent> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<CoreType> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<CoreInstance> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<CoreMemoryRef> for ComponentValidator<'_, P> {
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreMemoryRef, Self::Error>
    where
        I: Idx + Resolvable<CoreMemoryRef>
    {
        match self
            .get_flatten_component()
            .core_memories
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "core memory".to_string())
            })? {
            CoreMemoryBinding::Real(value) => Ok(value),
            CoreMemoryBinding::Alias(idx) => self.resolve(&CoreMemoryIdx::new(*idx)),
            CoreMemoryBinding::Reference(value, _) => Ok(value),
        }
    }
}

impl<P: Parent> Resolver<CoreTableRef> for ComponentValidator<'_, P> {
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreTableRef, Self::Error>
    where
        I: Idx + Resolvable<CoreTableRef>
    {
        match self
            .get_flatten_component()
            .core_tables
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "core table".to_string())
            })? {
            CoreTableBinding::Real(value) => Ok(value),
            CoreTableBinding::Alias(idx) => self.resolve(&CoreTableIdx::new(*idx)),
            CoreTableBinding::Reference(value, _) => Ok(value),
        }
    }
}

impl<P: Parent> Resolver<CoreFunction> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<CoreGlobalRef> for ComponentValidator<'_, P> {
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&CoreGlobalRef, Self::Error>
    where
        I: Idx + Resolvable<CoreGlobalRef>
    {
        match self
            .get_flatten_component()
            .core_globals
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "core global".to_string())
            })? {
            CoreGlobalBinding::Real(value) => Ok(value),
            CoreGlobalBinding::Alias(idx) => self.resolve(&CoreGlobalIdx::new(*idx)),
            CoreGlobalBinding::Reference(value, _) => Ok(value),
        }
    }
}

impl<P: Parent> Resolver<CoreModule> for ComponentValidator<'_, P> {
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

impl<P: Parent> Resolver<ComponentFunction> for ComponentValidator<'_, P> {
    type Error = ComponentParseError;

    fn resolve<I>(&self, idx: &I) -> Result<&ComponentFunction, Self::Error>
    where
        I: Idx + Resolvable<ComponentFunction>,
    {
        match self
            .get_flatten_component()
            .functions
            .get(idx.global())
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(idx.global(), "component function".to_string())
            })? {
            Binding::Real(value) => Ok(value),
            Binding::Alias(idx) => self.resolve(&FuncIdx::new(*idx)),
            Binding::Reference(value, _) => Ok(value),
        }
    }
}

impl<P: Parent> IdxValidator<CoreTypeIdx, CoreType> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreTypeIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_types
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core type".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreTypeIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<CoreModuleIdx, CoreModule> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreModuleIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_modules
            .get(local_idx as usize)
            .cloned()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core module".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreModuleIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<CoreInstanceIdx, CoreInstance> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreInstanceIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_instances
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core instance".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreInstanceIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<CoreFuncIdx, CoreFunction> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreFuncIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_funcs
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core function".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreFuncIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<CoreMemoryIdx, CoreMemoryRef> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreMemoryIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_memories
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core memory".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreMemoryIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<CoreTableIdx, CoreTableRef> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreTableIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_tables
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core table".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreTableIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<CoreGlobalIdx, CoreGlobalRef> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<CoreGlobalIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .core_globals
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "core global".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreGlobalIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<ComponentIdx, InlineComponent> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<ComponentIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .components
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "component".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<ComponentIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<FuncIdx, ComponentFunction> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<FuncIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .functions
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "component function".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<FuncIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<TypeIdx, Type> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<TypeIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .types
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "type".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<TypeIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> IdxValidator<InstanceIdx, Instance> for ComponentValidator<'_, P> {
    fn validate_idx(&self, local_idx: u32) -> Result<InstanceIdx, ComponentParseError> {
        let store = match self {
            ComponentValidator::TopLevel(_, store) => store,
            ComponentValidator::Child(_, store) => store,
        };
        store
            .instances
            .get(local_idx as usize)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "instance".to_string()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<InstanceIdx, ComponentParseError> {
        if ct == 0 {
            self.validate_idx(idx)
        } else {
            self.get_parent().unwrap().get().unwrap().validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: Parent> DefaultValidator for ComponentValidator<'_, P> {}
