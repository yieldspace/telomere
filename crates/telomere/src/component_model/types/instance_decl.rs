use crate::component_model::types::ExportDecl;
use crate::component_model::types::TypeId;

pub enum InstanceDecl {
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
