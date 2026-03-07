use crate::ir::types::ExportDecl;
use crate::ir::types::TypeId;

#[allow(clippy::large_enum_variant)]
pub enum InstanceDecl {
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
