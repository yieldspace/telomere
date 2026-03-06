use crate::component::ir::types::ExportDecl;
use crate::component::ir::types::TypeId;

pub enum InstanceDecl {
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
