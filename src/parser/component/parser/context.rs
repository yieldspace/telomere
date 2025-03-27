use crate::component_model::{Alias, Component, CoreType, Instance};
use crate::Module;
use crate::parser::component::parser::ComponentModelParserError;

pub struct ParseContext<'a> {
    parent: Option<&'a ParseContext<'a>>,
    core_modules: Vec<Module>,
    children: Vec<Component>,
    instances: Vec<Instance>,
    core_types: Vec<CoreType>,
    aliases: Vec<Alias>,
}

impl<'a> ParseContext<'a> {
    pub fn new() -> Self {
        ParseContext {
            parent: None,
            core_modules: Vec::new(),
            children: Vec::new(),
            instances: Vec::new(),
            core_types: Vec::new(),
            aliases: Vec::new(),
        }
    }

    pub fn create_child(&'a self) -> Self {
        ParseContext {
            parent: Some(self),
            core_modules: Vec::new(),
            children: Vec::new(),
            instances: Vec::new(),
            core_types: Vec::new(),
            aliases: Vec::new(),
        }
    }

    pub fn push_core_module(&mut self, module: Module) {
        self.core_modules.push(module);
    }

    pub fn push_child(&mut self, child: Component) {
        self.children.push(child);
    }

    pub fn push_instance(&mut self, instance: Instance) {
        self.instances.push(instance);
    }

    pub fn push_core_type(&mut self, core_type: CoreType) {
        self.core_types.push(core_type);
    }

    pub fn push_alias(&mut self, alias: Alias) {
        self.aliases.push(alias);
    }
}
