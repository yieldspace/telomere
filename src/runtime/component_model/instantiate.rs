use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use crate::component_model::{Component};
use crate::{Module};
use crate::component_model::types::ExternDesc;

pub struct ComponentInstance {
    instances: Vec<ComponentInstance>,
    exports: HashMap<String, ComponentExport>,
    imports: HashMap<String, ComponentImport>
}

pub enum ComponentExport {
    Reference,
    CoreModule,
    Value,
    Type,
    Component,
    Instance,
}

pub enum ComponentImport {
    CoreModule,
    Value,
    Type,
    Component,
    Instance,
}

pub struct CoreModuleInstance {
    module: Module,
}

pub struct ComponentFunctionInstance {

}

pub struct ComponentRegistry {
    components: HashMap<String, ComponentInstance>,
    core_modules: HashMap<String, CoreModuleInstance>,
    component_functions: HashMap<String, ComponentFunctionInstance>,
    children: HashMap<String, ComponentRegistry>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            core_modules: HashMap::new(),
            component_functions: HashMap::new(),
            children: HashMap::new(),
        }
    }

    pub fn create_child<IntoString>(&mut self, name: IntoString) -> &mut ComponentRegistry where IntoString: Into<String> {
        let child = ComponentRegistry::new();
        let name = name.into();
        self.children.insert(name.clone(), child);
        self.children.get_mut(&name).unwrap()
    }
}

pub struct ImportStore {

}

pub fn instantiate(component: Component, registry: &ComponentRegistry) -> ComponentInstance {
    let Component {
        modules,
        imports,
    } = component;

    // importを処理する
    for (name, ty) in imports {
        match ty {
            ExternDesc::Core(_) => {
                // core moduleを処理する
                if let Some(core_module) = registry.core_modules.get(&name) {
                    // core moduleを処理する
                } else {
                    panic!()
                    // core moduleが見つからない場合の処理
                }
            }
            ExternDesc::Func(_) => {
                let func = registry.component_functions.get(&name).unwrap();
                // funcを処理する
            }
            #[cfg(feature = "import_export")]
            ExternDesc::Value(_) => {
                // valueを処理する
            }
            ExternDesc::Type(_) => {
                todo!()
                // typeを処理する
            }
            ExternDesc::Component(_) => {
                let component = registry.components.get(&name).unwrap();
                // componentを処理する
            }
            ExternDesc::Instance(_) => {

                // instanceを処理する
            }
        }
    }
    // exportを処理する
    // moduleをinstantiateする
    // child componentを処理する
    ComponentInstance {
        instances: vec![],

        // core_module_instances: vec![],
        exports: Default::default(),
        imports: Default::default(),
    }
}
