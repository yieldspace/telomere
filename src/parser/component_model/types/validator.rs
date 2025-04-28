use crate::component_model::{
    Binding, ComponentBinding, ComponentFunction, ComponentIdx, ComponentImportType, ComponentType,
    CoreFuncIdx, CoreFunction, CoreFunctionBinding, CoreGlobalBinding, CoreGlobalIdx,
    CoreGlobalRef, CoreInstance, CoreInstanceBinding, CoreInstanceIdx, CoreMemoryBinding,
    CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleBinding, CoreModuleIdx, CoreModuleType,
    CoreTableBinding, CoreTableIdx, CoreTableRef, CoreType, CoreTypeBinding, CoreTypeIdx,
    ExternDesc, FlattenComponent, FuncIdx, FuncType, FunctionBinding, Idx, InlineComponent,
    Instance, InstanceBinding, InstanceIdx, InstanceType, Resolvable, Resolver, Type, TypeBinding,
    TypeIdx,
};
use crate::parser::component_model::validator::{IdxValidator, LocalStore, ValidatorStateImpl};
use crate::parser::component_model::{ComponentParseError, Validator, ValidatorState};
use std::collections::HashMap;

pub trait TypeSuperValidator: ValidatorStateImpl + IdxValidator<CoreModuleIdx, Resolved = CoreModule>
+ IdxValidator<FuncIdx, Resolved = ComponentFunction> + IdxValidator<TypeIdx, Resolved = Type> + IdxValidator<InstanceIdx, Resolved = Instance>
+ IdxValidator<ComponentIdx, Resolved = InlineComponent> + IdxValidator<CoreTypeIdx, Resolved = CoreType> + Resolver<CoreModule, Error=ComponentParseError>
+Resolver<ComponentFunction, Error=ComponentParseError> + Resolver<Type, Error=ComponentParseError>
{}

impl TypeSuperValidator for ValidatorState<'_> {}
impl<P: TypeSuperValidator> TypeSuperValidator for TypeValidatorState<'_, P> {}

/// A type validator that can be used to validate types in a component model.
///
/// 型のパースをする際に，実際にglobal idxを付与してvalidateをすると無駄な情報をinstantiate時まで持つ必要があるため，
/// type validatorを使って型レベルでvalidateを行えるようにした．
pub struct TypeValidatorState<'a, P: TypeSuperValidator + 'a>
{
    parent: &'a mut P,
    type_map: TypeMap,
}

impl<'a, P: TypeSuperValidator> TypeValidatorState<'a, P>
{
    pub fn new(parent: &'a mut P) -> Self {
        Self {
            parent,
            type_map: TypeMap::default(),
        }
    }
}

impl<P: TypeSuperValidator> TypeValidatorState<'_, P>
{
    fn validate_core_module_type(&self, local: u32) -> Result<CoreModuleType, ComponentParseError> {
        match self.type_map.core_module_types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "core type".to_string(),
            )),
        }
    }

    fn validate_type(&self, local: u32) -> Result<Type, ComponentParseError> {
        match self.type_map.types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "type".to_string(),
            )),
        }
    }

    fn validate_instance(&self, local: u32) -> Result<InstanceType, ComponentParseError> {
        match self.type_map.instance_types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "instance type".to_string(),
            )),
        }
    }

    fn validate_component(&self, local: u32) -> Result<ComponentType, ComponentParseError> {
        match self.type_map.component_types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "component type".to_string(),
            )),
        }
    }
}

impl<P: TypeSuperValidator> ValidatorStateImpl for TypeValidatorState<'_, P>
{
    fn component(&self) -> &FlattenComponent {
        self.parent.component()
    }

    fn component_mut(&mut self) -> &mut FlattenComponent {
        self.parent.component_mut()
    }

    fn add_component(
        &mut self,
        component: ComponentBinding,
    ) -> Result<ComponentIdx, ComponentParseError>
    where
        Self: Resolver<InlineComponent, Error = ComponentParseError>
            + IdxValidator<ComponentIdx>
            + Sized,
    {
        match component {
            ComponentBinding::Real(component) => {
                self.type_map.component_types.push(component.ty);
            }
            ComponentBinding::Alias(idx) => {
                let component = self
                    .type_map
                    .component_types
                    .get(idx)
                    .ok_or_else(|| {
                        ComponentParseError::InvalidIdx(idx, "component type".to_string())
                    })?;
                self.type_map.component_types.push(component.clone());
            }
            ComponentBinding::Reference(component, _) => {
                self.type_map.component_types.push(component.ty);
            }
        }
        Ok(ComponentIdx::new(usize::MAX))
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
        match instance {
            InstanceBinding::Real(instance) => {
                self.type_map.instance_types.push(instance.ty);
            }
            InstanceBinding::Alias(idx) => {
                let instance = self
                    .type_map
                    .instance_types
                    .get(idx)
                    .ok_or_else(|| {
                        ComponentParseError::InvalidIdx(idx, "instance type".to_string())
                    })?;
                self.type_map.instance_types.push(instance.clone());
            }
            InstanceBinding::Reference(instance, _) => {
                self.type_map.instance_types.push(instance.ty);
            }
        }
        Ok(InstanceIdx::new(usize::MAX))
    }

    fn add_func(&mut self, func: FunctionBinding) -> Result<FuncIdx, ComponentParseError>
    where
        Self: Resolver<ComponentFunction, Error = ComponentParseError>
            + IdxValidator<FuncIdx>
            + Sized,
    {
        match func {
            FunctionBinding::Real(func) => {
                self.type_map.func_types.push(func.ty);
            }
            FunctionBinding::Alias(idx) => {
                let func = self
                    .type_map
                    .func_types
                    .get(idx)
                    .ok_or_else(|| {
                        ComponentParseError::InvalidIdx(idx, "function type".to_string())
                    })?;
                self.type_map.func_types.push(func.clone());
            }
            FunctionBinding::Reference(func, _) => {
                self.type_map.func_types.push(func.ty);
            }
        }
        Ok(FuncIdx::new(usize::MAX))
    }

    fn add_type(&mut self, ty: TypeBinding) -> Result<TypeIdx, ComponentParseError>
    where
        Self: Resolver<Type, Error = ComponentParseError> + IdxValidator<TypeIdx> + Sized,
    {
        match ty {
            TypeBinding::Real(ty) => {
                self.type_map.types.push(ty);
            }
            TypeBinding::Alias(idx) => {
                let ty = self.type_map.types.get(idx).ok_or_else(|| {
                    ComponentParseError::InvalidIdx(idx, "type".to_string())
                })?;
                self.type_map.types.push(ty.clone());
            }
            TypeBinding::Reference(ty, _) => {
                self.type_map.types.push(ty.clone());
            }
        }
        Ok(TypeIdx::new(usize::MAX))
    }
}

impl<P: TypeSuperValidator> IdxValidator<CoreModuleIdx> for TypeValidatorState<'_, P>
{
    type Resolved = CoreModule;

    fn validate_local_idx(&self, _local_idx: u32) -> Result<CoreModuleIdx, ComponentParseError> {
        unimplemented!("Cannot validate local idx in type validator")
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<ComponentFunction>,
        Self: Resolver<ComponentFunction, Error = ComponentParseError> + Sized,
        ComponentFunction: Clone,
    {
        self.type_map
            .core_module_types
            .get(local_idx as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(local_idx as usize, "core module".to_string())
            })
            .map(|x| CoreModule::new(None, x.clone()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreModuleIdx, ComponentParseError> {
        if ct == 0 {
            unimplemented!("Cannot validate local idx in type validator")
        } else {
            self.parent.validate_outer_idx(ct - 1, idx)
        }
    }

    fn validate_outer_idx_resolved(&self, ct: u32, idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        CoreModuleIdx: Resolvable<Self::Resolved>,
        Self: Resolver<Self::Resolved, Error=ComponentParseError> + Sized,
    {
        if ct == 0 {
            <TypeValidatorState<'_, P> as IdxValidator<CoreModuleIdx>>::validate_idx_resolved(self, idx)
        } else {
            <P as IdxValidator<CoreModuleIdx>>::validate_outer_idx_resolved(self.parent, ct - 1, idx)
        }
    }
}

impl<P: TypeSuperValidator> IdxValidator<FuncIdx> for TypeValidatorState<'_, P>
{
    type Resolved = ComponentFunction;
    fn validate_local_idx(&self, _local_idx: u32) -> Result<FuncIdx, ComponentParseError> {
        unimplemented!("Cannot validate local idx in type validator")
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<ComponentFunction>,
        Self: Resolver<ComponentFunction, Error = ComponentParseError> + Sized,
        ComponentFunction: Clone,
    {
        self.type_map
            .func_types
            .get(local_idx as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(local_idx as usize, "function type".to_string())
            })
            .map(|x| ComponentFunction::new(None, x.clone()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<FuncIdx, ComponentParseError> {
        if ct == 0 {
            unimplemented!("Cannot validate local idx in type validator")
        } else {
            self.parent.validate_outer_idx(ct - 1, idx)
        }
    }

    fn validate_outer_idx_resolved(&self, ct: u32, idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<Self::Resolved>,
        Self: Resolver<Self::Resolved, Error=ComponentParseError> + Sized,
    {
        if ct == 0 {
            <TypeValidatorState<'_, P> as IdxValidator<FuncIdx>>::validate_idx_resolved(self, idx)
        } else {
            <P as IdxValidator<FuncIdx>>::validate_outer_idx_resolved(self.parent, ct - 1, idx)
        }
    }
}

impl<P: TypeSuperValidator> IdxValidator<TypeIdx> for TypeValidatorState<'_, P>
{
    type Resolved = Type;

    fn validate_local_idx(&self, _local_idx: u32) -> Result<TypeIdx, ComponentParseError> {
        unimplemented!("Cannot validate local idx in type validator")
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<ComponentFunction>,
        Self: Resolver<ComponentFunction, Error = ComponentParseError> + Sized,
        ComponentFunction: Clone,
    {
        self.type_map
            .types
            .get(local_idx as usize)
            .ok_or_else(|| ComponentParseError::InvalidIdx(local_idx as usize, "type".to_string()))
            .cloned()
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<TypeIdx, ComponentParseError> {
        if ct == 0 {
            unimplemented!("Cannot validate local idx in type validator")
        } else {
            self.parent.validate_outer_idx(ct - 1, idx)
        }
    }

    fn validate_outer_idx_resolved(&self, ct: u32, idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        TypeIdx: Resolvable<Self::Resolved>,
        Self: Resolver<Self::Resolved, Error=ComponentParseError> + Sized,
    {
        if ct == 0 {
            <TypeValidatorState<'_, P> as IdxValidator<TypeIdx>>::validate_idx_resolved(self, idx)
        } else {
            <P as IdxValidator<TypeIdx>>::validate_outer_idx_resolved(self.parent, ct - 1, idx)
        }
    }
}

impl<P: TypeSuperValidator> IdxValidator<InstanceIdx> for TypeValidatorState<'_, P>
{
    type Resolved = Instance;

    fn validate_local_idx(&self, _local_idx: u32) -> Result<InstanceIdx, ComponentParseError> {
        unimplemented!("Cannot validate local idx in type validator")
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<ComponentFunction>,
        Self: Resolver<ComponentFunction, Error = ComponentParseError> + Sized,
        ComponentFunction: Clone,
    {
        self.type_map
            .instance_types
            .get(local_idx as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(local_idx as usize, "instance type".to_string())
            })
            .map(|x| Instance::new(None, x.clone()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<InstanceIdx, ComponentParseError> {
        if ct == 0 {
            unimplemented!("Cannot validate local idx in type validator")
        } else {
            self.parent.validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: TypeSuperValidator> IdxValidator<ComponentIdx> for TypeValidatorState<'_, P>
{
    type Resolved = InlineComponent;

    fn validate_local_idx(&self, _local_idx: u32) -> Result<ComponentIdx, ComponentParseError> {
        unimplemented!("Cannot validate local idx in type validator")
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<ComponentFunction>,
        Self: Resolver<ComponentFunction, Error = ComponentParseError> + Sized,
        ComponentFunction: Clone,
    {
        self.type_map
            .component_types
            .get(local_idx as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(local_idx as usize, "component type".to_string())
            })
            .map(|x| InlineComponent::new(None, x.clone()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<ComponentIdx, ComponentParseError> {
        if ct == 0 {
            unimplemented!("Cannot validate local idx in type validator")
        } else {
            self.parent.validate_outer_idx(ct - 1, idx)
        }
    }
}

impl<P: TypeSuperValidator> IdxValidator<CoreTypeIdx> for TypeValidatorState<'_, P>
{
    type Resolved = CoreType;

    fn validate_local_idx(&self, _local_idx: u32) -> Result<CoreTypeIdx, ComponentParseError> {
        unimplemented!("Cannot validate local idx in type validator")
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        FuncIdx: Resolvable<ComponentFunction>,
        Self: Resolver<ComponentFunction, Error = ComponentParseError> + Sized,
        ComponentFunction: Clone,
    {
        self.type_map
            .core_module_types
            .get(local_idx as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidIdx(local_idx as usize, "core type".to_string())
            })
            .map(|x| CoreType::ModuleType(x.clone()))
    }

    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<CoreTypeIdx, ComponentParseError> {
        if ct == 0 {
            unimplemented!("Cannot validate local idx in type validator")
        } else {
            self.parent.validate_outer_idx(ct - 1, idx)
        }
    }
}

macro_rules! unimplemented_resolver {
    ($ty:ident, $message:expr) => {
        impl<P: TypeSuperValidator> Resolver<$ty> for TypeValidatorState<'_, P>
        {
            type Error = ComponentParseError;

            fn resolve<I>(&self, _idx: &I) -> Result<&$ty, Self::Error>
            where
                I: Idx + Resolvable<$ty>,
                Self: Sized,
            {
                unimplemented!($message)
            }
        }
    };
}

unimplemented_resolver!(
    CoreModule,
    "Cannot resolve core module from idx in type validator"
);
unimplemented_resolver!(
    CoreType,
    "Cannot resolve core type from idx in type validator"
);
unimplemented_resolver!(Type, "Cannot resolve type from idx in type validator");
unimplemented_resolver!(
    InlineComponent,
    "Cannot resolve component from idx in type validator"
);
unimplemented_resolver!(
    ComponentFunction,
    "Cannot resolve component function from idx in type validator"
);
unimplemented_resolver!(
    Instance,
    "Cannot resolve instance from idx in type validator"
);

#[derive(Default)]
pub struct TypeMap {
    pub core_module_types: Vec<CoreModuleType>,
    pub types: Vec<Type>,
    pub func_types: Vec<FuncType>,
    pub instance_types: Vec<InstanceType>,
    pub component_types: Vec<ComponentType>,
    pub imports: HashMap<String, ExternDesc>,
    pub exports: HashMap<String, ExternDesc>,
}
