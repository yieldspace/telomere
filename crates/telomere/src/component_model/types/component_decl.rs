use crate::component_model::types::{ExportDecl, ImportDecl, TypeId};

pub enum ComponentDecl {
    Import(ImportDecl),
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
