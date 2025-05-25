mod component_decl;
mod core;
mod defval;
mod export_decl;
mod func;
mod import_decl;
mod instance_decl;
mod primitive;
mod sort;
mod val;

use crate::component_model::{ResourceId, TypeId};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
pub use component_decl::*;
pub use core::*;
pub use defval::*;
pub use export_decl::*;
pub use func::*;
pub use import_decl::*;
pub use instance_decl::*;
pub use primitive::*;
pub use sort::{CoreSortType, SortType};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
pub use val::*;

#[derive(Clone, Debug)]
pub enum Type {
    DefVal(DefValType),
    Generic(Generic),
    Func(FuncType),
    Resource(ResourceId),
    Component(ComponentType),
    Instance(InstanceType),
}

impl Type {
    pub fn is_generic(&self) -> bool {
        matches!(self, Type::Generic(_))
    }
    pub fn is_resource(&self) -> bool {
        matches!(self, Type::Resource(_))
    }
    pub fn is_component(&self) -> bool {
        matches!(self, Type::Component(_))
    }
    pub fn is_instance(&self) -> bool {
        matches!(self, Type::Instance(_))
    }
    pub fn is_func(&self) -> bool {
        matches!(self, Type::Func(_))
    }
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        match (self, parent) {
            (Type::DefVal(a), Type::DefVal(b)) => {
                a.assert_subtype_of(b, validator)?;
                Ok(())
            }
            (Type::Generic(_), _) => {
                todo!()
            }
            (_, Type::Generic(_)) => {
                todo!()
            }
            (Type::Func(a), Type::Func(b)) => a.assert_subtype_of(b, validator),
            (Type::Resource(a), Type::Resource(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(ComponentParseError::TypeMismatch(
                        "resource id mismatch".to_owned(),
                    ))
                }
            }
            (Type::Component(a), Type::Component(b)) => a.assert_subtype_of(b, validator),
            _ => Err(ComponentParseError::TypeMismatch(
                "resource kind mismatch".to_owned(),
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Generic {
    pub id: usize,
    pub bound: GenericBound,
}

impl Generic {
    pub fn new(bound: GenericBound) -> Self {
        static GENERIC_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: GENERIC_ID.fetch_add(1, Ordering::Relaxed),
            bound,
        }
    }
}

#[derive(Clone, Debug)]
pub enum GenericBound {
    Eq(TypeId),
    Sub,
}
impl GenericBound {
    pub fn assert_subtype_of(
        &self,
        parent: &GenericBound,
        validator: &Validator,
    ) -> ParseResult<()> {
        match (self, parent) {
            (GenericBound::Eq(a), GenericBound::Eq(b)) => a.assert_subtype_of(*b, validator)?,
            (GenericBound::Eq(type_id), GenericBound::Sub) => {
                if !validator.get_type(*type_id)?.is_resource() {
                    Err(ComponentParseError::TypeMismatch(
                        "expected any resource".to_owned(),
                    ))?
                }
            }
            (GenericBound::Sub, GenericBound::Eq(_type_id)) => {
                // FIMXE: sould retrive type_id and validate it?
                Err(ComponentParseError::TypeMismatch(
                    "sub resource cannot assign to except sub resource".to_owned(),
                ))?
            }
            (GenericBound::Sub, GenericBound::Sub) => {
                // ok
            }
        };
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct ComponentType {
    pub imports: HashMap<String, Generic>,
    pub exports: HashMap<String, ComponentExportType>,
}
impl ComponentType {
    pub fn assert_subtype_of(
        &self,
        parent: &ComponentType,
        validator: &Validator,
    ) -> ParseResult<()> {
        if self.imports.len() > parent.imports.len() {
            Err(ComponentParseError::TypeMismatch(
                "import count mismatch".to_owned(),
            ))?
        }
        for (child_entry_name, child_ty) in &self.imports {
            let parent_ty = parent.imports.get(child_entry_name).ok_or_else(|| {
                ComponentParseError::TypeMismatch("import name mismatch".to_owned())
            })?;
            child_ty
                .bound
                .assert_subtype_of(&parent_ty.bound, validator)?
        }
        if parent.exports.len() > self.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "export count mismatch".to_owned(),
            ))?
        }
        for (parent_entry_name, expected_ty) in &parent.exports {
            let actual_ty = self.exports.get(parent_entry_name).ok_or_else(|| {
                ComponentParseError::TypeMismatch("import name mismatch".to_owned())
            })?;
            expected_ty
                .cv_type(validator)?
                .assert_subtype_of(&actual_ty.cv_type(validator)?, validator)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum ComponentExportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Resource(ResourceId),
    NewResource(TypeId),
}

impl ComponentExportType {
    pub fn cv_type<'a>(&self, validator: &'a Validator<'a>) -> ParseResult<Type> {
        match self {
            ComponentExportType::Component(type_id) => Ok(Type::Component(
                validator.get_component_type(*type_id)?.clone(),
            )),
            ComponentExportType::Instance(type_id) => Ok(Type::Instance(
                validator.get_instance_type(*type_id)?.clone(),
            )),
            ComponentExportType::Type(type_id) => validator.get_type(*type_id).cloned(),
            ComponentExportType::Resource(resource_id) => Ok(Type::Resource(*resource_id)),
            ComponentExportType::NewResource(_) => ParseResult::Err(
                ComponentParseError::TypeMismatch("NewResource cannot be used here".to_owned()),
            ),
        }
    }
}
#[derive(Clone, Debug)]
pub struct InstanceType {
    pub exports: HashMap<String, InstanceExportType>,
}

impl InstanceType {
    pub fn get_export(&self, name: &String) -> ParseResult<&InstanceExportType> {
        self.exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

#[derive(Clone, Debug)]
pub enum InstanceExportType {
    CoreModule(CoreModuleType),
    Func(TypeId),
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    SubType,
}
