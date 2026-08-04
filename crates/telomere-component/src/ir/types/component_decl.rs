use crate::ir::types::{ExportDecl, ImportDecl, TypeId};

// Retained conservatively; no current decoder path materializes component declarations.
#[allow(dead_code)]
#[allow(clippy::large_enum_variant)]
pub enum ComponentDecl {
    Import(ImportDecl),
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
