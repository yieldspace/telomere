use crate::component_model::types::{ExportDecl, InstanceDecl, Type};
use crate::component_model::{Alias, CoreType};

pub struct TypeSort {
    types: Vec<Type>,
    core_types: Vec<CoreType>,
    aliases: Vec<Alias>,
    exports: Vec<ExportDecl>,
}

impl TypeSort {
    pub fn new() -> Self {
        Self {
            types: vec![],
            core_types: vec![],
            aliases: vec![],
            exports: vec![],
        }
    }

    pub fn add_type(&mut self, ty: Type) {
        self.types.push(ty);
    }

    pub fn get_types(&self) -> &[Type] {
        &self.types
    }

    pub fn add_instance_decl(&mut self, decl: InstanceDecl) {
        match decl {
            InstanceDecl::CoreType(coretype) => {
                self.core_types.push(coretype);
            }
            InstanceDecl::Type(ty) => {
                self.types.push(ty);
            }
            InstanceDecl::Alias(alias) => {
                self.aliases.push(alias);
            }
            InstanceDecl::ExportDecl(decl) => {
                self.exports.push(decl);
            }
        }
    }
}
