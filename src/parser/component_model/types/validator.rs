use std::collections::HashMap;
use crate::component_model::{ComponentType, CoreModuleType, CoreType, CoreTypeIdx, ExportDecl, ExternDesc, FlattenComponent, Idx, Instance, InstanceIdx, InstanceType, Resolvable, Resolver, Type, TypeIdx};
use crate::parser::component_model::{ComponentParseError, Validator};
use crate::parser::component_model::validator::LocalStore;

/// A type validator that can be used to validate types in a component model.
/// 
/// 型のパースをする際に，実際にglobal idxを付与してvalidateをすると無駄な情報をinstantiate時まで持つ必要があるため，
/// type validatorを使って型レベルでvalidateを行えるようにした．
pub struct TypeValidator<'a> {
    parent: &'a mut dyn Validator,
    type_map: TypeMap,
}

impl<'a> Validator for TypeValidator<'a> {
    fn get_parent(&self) -> Option<&dyn Validator> {
        Some(self.parent)
    }

    fn get_flatten_component(&self) -> &FlattenComponent {
        self.parent.get_flatten_component()
    }

    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        self.parent.get_flatten_component_mut()
    }

    fn get_local_store(&self) -> &LocalStore {
        unreachable!("Local Store can't use in type validating");
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        unreachable!("Local Store can't use in type validating");
    }
}

impl<'a> TypeValidator<'a> {
    pub fn validate_core_type(&self, local: u32) -> Result<CoreType, ComponentParseError> {
        match self.type_map.core_types.get(local as usize) {
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
    
    pub fn add_core_type(&mut self, ty: CoreType) {
        self.type_map.core_types.push(ty);
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
    
    pub fn add_export(&mut self, name: String, ty: ExternDesc) {
        self.type_map.exports.insert(name, ty);
    }
}


#[derive(Default)]
pub struct TypeMap {
    pub core_types: Vec<CoreType>,
    pub types: Vec<Type>,
    pub instance_types: Vec<InstanceType>,
    pub component_types: Vec<ComponentType>,
    pub exports: HashMap<String, ExternDesc>
}
