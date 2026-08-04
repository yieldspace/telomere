use crate::ir::types::ExportDecl;
use crate::ir::types::TypeId;

// Retained conservatively; no current decoder path materializes instance declarations.
#[allow(dead_code)]
#[allow(clippy::large_enum_variant)]
pub enum InstanceDecl {
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
