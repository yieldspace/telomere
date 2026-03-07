mod component_decl;
mod core;
mod defval;
mod export_decl;
mod func;
mod generics_replace;
mod import_decl;
mod instance_decl;
mod primitive;
mod sort;
mod val;
use crate::component::decoder::{ComponentParseError, ParseResult, Validator};
use crate::component::ir::{ResourceId, TypeId};
pub use component_decl::*;
pub use core::*;
pub use defval::*;
pub use export_decl::*;
pub use func::*;
pub use generics_replace::GenericsReplaceDSL;
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
            (
                Type::Generic(Generic {
                    id: _,
                    bound: GenericBound::Eq(a),
                }),
                b,
            ) => validator.get_type(*a)?.assert_subtype_of(b, validator),
            (
                Type::Generic(Generic {
                    id: _,
                    bound: GenericBound::Sub,
                }),
                Type::Generic(Generic {
                    id: _,
                    bound: parent_bound,
                }),
            ) => GenericBound::Sub.assert_subtype_of(parent_bound, validator),
            (
                Type::Generic(Generic {
                    id: _,
                    bound: GenericBound::Sub,
                }),
                _,
            ) => Err(ComponentParseError::TypeMismatch(
                "generic resource cannot assign to concrete type".to_owned(),
            )),
            (a, Type::Generic(parent)) => match &parent.bound {
                GenericBound::Eq(b) => a.assert_subtype_of(validator.get_type(*b)?, validator),
                GenericBound::Sub => {
                    if a.is_resource() {
                        Ok(())
                    } else {
                        Err(ComponentParseError::TypeMismatch(
                            "expected any resource".to_owned(),
                        ))
                    }
                }
            },
            (Type::Func(a), Type::Func(b)) => a.assert_subtype_of(b, validator),
            (Type::Resource(a), Type::Resource(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(ComponentParseError::TypeMismatch(
                        "resource types are not the same".to_owned(),
                    ))
                }
            }
            (Type::Component(a), Type::Component(b)) => a.assert_subtype_of(b, validator),
            (Type::Instance(a), Type::Instance(b)) => a.assert_subtype_of(b, validator),
            (a, b) => {
                tracing::trace!("resource kind mismatch {a:?} {b:?}");
                Err(ComponentParseError::TypeMismatch(
                    "resource kind mismatch".to_owned(),
                ))
            }
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

#[derive(Clone, Debug)]
pub enum ComponentImportType {
    Type { type_id: TypeId, generic: Generic },
    CoreModule(CoreModuleType),
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
    pub import_order: Vec<String>,
    pub imports: HashMap<String, ComponentImportType>,
    pub exports: HashMap<String, ComponentExportType>,
    pub generics_replacing_program: Vec<GenericsReplaceDSL>,
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
            match (child_ty, parent_ty) {
                (
                    ComponentImportType::Type {
                        generic: child,
                        type_id: _,
                    },
                    ComponentImportType::Type {
                        generic: parent,
                        type_id: _,
                    },
                ) => child.bound.assert_subtype_of(&parent.bound, validator)?,
                (
                    ComponentImportType::CoreModule(child),
                    ComponentImportType::CoreModule(parent),
                ) if child == parent => {}
                _ => Err(ComponentParseError::TypeMismatch(
                    "import kind mismatch".to_owned(),
                ))?,
            }
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
            expected_ty.assert_subtype_of(actual_ty, validator)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum ComponentExportType {
    CoreModule(CoreModuleType),
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Func(TypeId),
    //Resource(ResourceId),
    //NewResource(TypeId),
}

impl ComponentExportType {
    pub fn assert_subtype_of<'a>(
        &self,
        parent: &Self,
        validator: &'a Validator<'a>,
    ) -> ParseResult<()> {
        use ComponentExportType::*;
        match (self, parent) {
            (CoreModule(a), CoreModule(b)) => {
                if a != b {
                    Err(ComponentParseError::TypeMismatch(
                        "core module export mismatch".to_owned(),
                    ))?
                }
            }
            (Component(a), Component(b)) => {
                validator.assert_component_type_ids_subtype_of(*a, *b)?;
            }
            (Instance(a), Instance(b)) => {
                validator.assert_instance_type_ids_subtype_of(*a, *b)?;
            }
            (Type(a), Type(b)) => {
                validator.assert_type_ids_subtype_of(*a, *b)?;
            }
            (Func(a), Func(b)) => validator
                .get_func_type(*a)?
                .assert_subtype_of(validator.get_func_type(*b)?, validator)?,
            _ => Err(ComponentParseError::TypeMismatch(
                "export kind mismatch".to_owned(),
            ))?,
        };
        Ok(())
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
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        if self.exports.len() < parent.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "instance export count".to_owned(),
            ))?
        }
        for (name, ty) in &parent.exports {
            self.get_export(name)?.assert_subtype_of(ty, validator)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum InstanceExportType {
    CoreModule(CoreModuleType),
    Func(TypeId),
    Component(TypeId),
    Instance(TypeId),
    //Resource(ResourceId),
    Type(TypeId),
    //SubResource(TypeId),
}
impl InstanceExportType {
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        use InstanceExportType::*;
        match (self, parent) {
            (CoreModule(a), CoreModule(b)) => {
                if a != b {
                    Err(ComponentParseError::TypeMismatch(
                        "core module export mismatch".to_owned(),
                    ))?
                }
                Ok(())
            }
            (Func(a), Func(b)) => validator
                .get_func_type(*a)?
                .assert_subtype_of(validator.get_func_type(*b)?, validator),
            (Component(a), Component(b)) => validator.assert_component_type_ids_subtype_of(*a, *b),
            (Instance(a), Instance(b)) => validator.assert_instance_type_ids_subtype_of(*a, *b),
            (Type(a), Type(b)) => validator.assert_type_ids_subtype_of(*a, *b),
            _ => Err(ComponentParseError::TypeMismatch(
                "export kind mismatch".to_owned(),
            ))?,
        }
    }
}
