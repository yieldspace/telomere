use crate::component::ir::types::{ExportDecl, ImportDecl, TypeId};

#[allow(clippy::large_enum_variant)]
pub enum ComponentDecl {
    Import(ImportDecl),
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
