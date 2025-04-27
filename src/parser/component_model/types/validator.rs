use crate::component_model::{Binding, ComponentFunction, ComponentIdx, ComponentImportType, ComponentType, CoreFuncIdx, CoreFunction, CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceIdx, CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleIdx, CoreModuleType, CoreTableIdx, CoreTableRef, CoreType, CoreTypeIdx, ExternDesc, FlattenComponent, FuncIdx, FuncType, Idx, InlineComponent, Instance, InstanceIdx, InstanceType, Resolvable, Resolver, Type, TypeIdx};
use crate::parser::component_model::validator::{IdxValidator, LocalStore, Parent};
use crate::parser::component_model::{ComponentParseError, DefaultValidator, Validator};
use std::collections::HashMap;

/// A type validator that can be used to validate types in a component model.
///
/// 型のパースをする際に，実際にglobal idxを付与してvalidateをすると無駄な情報をinstantiate時まで持つ必要があるため，
/// type validatorを使って型レベルでvalidateを行えるようにした．
pub struct TypeValidator<P> where P: Parent {
    parent: P,
    type_map: TypeMap,
}

impl<P> TypeValidator<P> where P: Parent {
    pub fn new(parent: P) -> Self {
        Self { parent, type_map: TypeMap::default()}
    }
}

impl<P> Validator for TypeValidator<P> where P: Parent {
    fn get_flatten_component(&self) -> &FlattenComponent {
        unreachable!("Flatten Component can't use in type validating");
    }

    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        unreachable!("Flatten Component can't use in type validating");
    }

    fn get_parent(&self) -> Option<&impl Parent>
    where
        Self: Sized
    {
        Some(&self.parent)
    }

    fn get_local_store(&self) -> &LocalStore {
        unreachable!("Local Store can't use in type validating");
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        unreachable!("Local Store can't use in type validating");
    }
}

impl<P> TypeValidator<P> where P: Parent {
    pub fn validate_core_module_type(&self, local: u32) -> Result<CoreModuleType, ComponentParseError> {
        match self.type_map.core_module_types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "core type".to_string(),
            )),
        }
    }

    pub fn validate_type(&self, local: u32) -> Result<Type, ComponentParseError> {
        match self.type_map.types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "type".to_string(),
            )),
        }
    }

    pub fn validate_instance(&self, local: u32) -> Result<InstanceType, ComponentParseError> {
        match self.type_map.instance_types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "instance type".to_string(),
            )),
        }
    }

    pub fn validate_component(&self, local: u32) -> Result<ComponentType, ComponentParseError> {
        match self.type_map.component_types.get(local as usize) {
            Some(ty) => Ok(ty.clone()),
            None => Err(ComponentParseError::InvalidIdx(
                local as usize,
                "component type".to_string(),
            )),
        }
    }

    pub fn add_core_module_type(&mut self, ty: CoreModuleType) {
        self.type_map.core_module_types.push(ty);
    }

    pub fn add_type(&mut self, ty: Type) {
        self.type_map.types.push(ty);
    }

    pub fn add_instance_type(&mut self, ty: InstanceType) {
        self.type_map.instance_types.push(ty);
    }

    pub fn add_component_type(&mut self, ty: ComponentType) {
        self.type_map.component_types.push(ty);
    }
    
    pub fn add_func_type(&mut self, ty: FuncType) {
        self.type_map.func_types.push(ty);
    }

    pub fn add_export(&mut self, name: String, ty: ExternDesc) {
        self.type_map.exports.insert(name, ty);
    }
}

macro_rules! unimplemented_resolver {
    ($name:ident) => {
        impl<P> Resolver<$name> for TypeValidator<P>
        where
            P: Parent,
        {
            type Error = ComponentParseError;
        
            fn resolve<I>(&self, _idx: &I) -> Result<&$name, Self::Error>
            where
                I: Idx + Resolvable<$name>
            {
                unimplemented!("unimplemented resolver for {} in type validator", stringify!($name));
            }
        }
    };
}
macro_rules! unimplemented_validator {
    ($name:ident, $target:ident) => {
        impl<P> IdxValidator<$name, $target> for TypeValidator<P>
        where
            P: Parent,
        {
            fn validate_idx(&self, _local_idx: u32) -> Result<$name, ComponentParseError> {
                unimplemented!("unimplemented resolver for {} in type validator", stringify!($name));
            }
        
            fn validate_outer_idx(&self, _ct: u32, _idx: u32) -> Result<$name, ComponentParseError> {
                unimplemented!("unimplemented resolver for {} in type validator", stringify!($name));
            }
        }
    };
}

impl<P> Resolver<Instance> for TypeValidator<P> where P: Parent {
    type Error = ComponentParseError;

    fn resolve<I>(&self, _idx: &I) -> Result<&Instance, Self::Error>
    where
        I: Idx + Resolvable<Instance>
    {
        unimplemented!("idx resolver can't be used in type validator");
    }
}

impl<P> IdxValidator<InstanceIdx, Instance> for TypeValidator<P> where P: Parent {
    fn validate_idx(&self, _local_idx: u32) -> Result<InstanceIdx, ComponentParseError> {
        unreachable!("idx validator can't be used in type validator");
    }

    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Instance, ComponentParseError> {
        match self.type_map.instance_types.get(local_idx as usize) {
            Some(ty) => Ok(Instance::new(None, ty.clone())),
            None => Err(ComponentParseError::InvalidIdx(
                local_idx as usize,
                "instance type".to_string(),
            )),
        }
    }

    fn validate_outer_idx(&self, _ct: u32, _idx: u32) -> Result<InstanceIdx, ComponentParseError> {
        unreachable!("idx validator can't be used in type validator");
    }

    fn validate_outer_idx_resolved(&self, ct: u32, idx: u32) -> Result<Instance, ComponentParseError> {
        if ct == 0 {
            self.validate_idx_resolved(idx)
        } else {
            self.parent.get().unwrap().validate_outer_idx_resolved(ct - 1, idx)
        }
    }
}

unimplemented_resolver!(CoreType);
unimplemented_resolver!(CoreModule);
unimplemented_resolver!(CoreInstance);
unimplemented_resolver!(CoreFunction);
unimplemented_resolver!(CoreTableRef);
unimplemented_resolver!(CoreMemoryRef);
unimplemented_resolver!(CoreGlobalRef);
unimplemented_resolver!(Type);
unimplemented_resolver!(InlineComponent);
unimplemented_resolver!(ComponentFunction);
unimplemented_validator!(CoreTypeIdx, CoreType);
unimplemented_validator!(CoreModuleIdx, CoreModule);
unimplemented_validator!(CoreInstanceIdx, CoreInstance);
unimplemented_validator!(CoreFuncIdx, CoreFunction);
unimplemented_validator!(CoreTableIdx, CoreTableRef);
unimplemented_validator!(CoreMemoryIdx, CoreMemoryRef);
unimplemented_validator!(CoreGlobalIdx, CoreGlobalRef);
unimplemented_validator!(ComponentIdx, InlineComponent);
unimplemented_validator!(FuncIdx, ComponentFunction);
unimplemented_validator!(TypeIdx, Type);


impl<P> DefaultValidator for TypeValidator<P> where P: Parent {}

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
