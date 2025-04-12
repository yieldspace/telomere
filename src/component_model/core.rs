use std::collections::HashMap;
use crate::component_model::id::{CoreFuncId, CoreInstanceIdx, CoreModuleIdx, InstanceIdx};
use crate::Module;
use crate::runtime::component_model::CoreInstantiate;

#[derive(Debug)]
pub struct CoreInstance {
    pub module_idx: Option<CoreModuleIdx>,
    pub exports: HashMap<String, CoreInstanceExport>,
    pub imports: HashMap<String, CoreInstanceIdx>,
}

impl CoreInstance {
    pub fn instantiate(self, modules: &Vec<Module>, instances: &Vec<impl CoreInstantiate>) -> impl CoreInstantiate {
        let Self {
            module_idx,
            exports,
            imports,
        } = self;
        let instantiate = match module_idx {
            Some(idx) => {
                let module = modules[idx.0].clone();

            }
            None => {

            },
        };
        todo!()
    }

}


#[derive(Debug)]
pub enum CoreInstanceExport {
    FuncReference(CoreFuncId),
}

#[derive(Debug)]
pub struct CoreModule(pub Module);

impl CoreModule {
    pub fn instantiate() {

    }
}
