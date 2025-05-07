use crate::component_model::types::export_decl::ExportDecl;
use crate::component_model::types::{Type, TypeId};

pub enum InstanceDecl {
    // CoreType(),
    Type(TypeId),
    Instance(TypeId),
    Export(ExportDecl),
}
