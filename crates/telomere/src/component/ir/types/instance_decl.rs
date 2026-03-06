use crate::component::ir::types::ExportDecl;
use crate::component::ir::types::TypeId;

#[allow(clippy::large_enum_variant)]
pub enum InstanceDecl {
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
