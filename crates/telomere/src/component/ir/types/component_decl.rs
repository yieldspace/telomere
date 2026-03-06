use crate::component::ir::types::{ExportDecl, ImportDecl, TypeId};

pub enum ComponentDecl {
    Import(ImportDecl),
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
