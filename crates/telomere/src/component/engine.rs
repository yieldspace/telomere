use crate::component::ir::{ComponentExport, ComponentImport};
use crate::component::runtime;
use crate::component::validate::{ParseState, Validator};
use crate::component::{
    ComponentError, ComponentInstance, ComponentLinker, ComponentOp, ComponentProgram,
    ComponentTypeInfo,
};
use crate::{IoReadBinaryReader, Store};

#[derive(Default, Debug, Clone, Copy)]
pub struct ComponentEngine;

impl ComponentEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn compile(&self, bytes: &[u8]) -> Result<ComponentProgram, ComponentError> {
        let mut reader = IoReadBinaryReader::from(bytes);
        let state_arena = typed_arena::Arena::new();
        let mut state = ParseState::new(&state_arena);
        let validator_arena = typed_arena::Arena::new();
        let mut validator = Validator::new(&validator_arena);

        crate::component::decoder::parse_component(&mut reader, &mut state, &mut validator)?;

        let scope = state.scope();
        let root = scope.make_component();

        let mut imports = Vec::with_capacity(scope.imports.len());
        let mut callable_imports = Vec::new();
        for (name, import) in &scope.imports {
            imports.push(name.clone());
            if matches!(import, ComponentImport::Func) {
                callable_imports.push(name.clone());
            }
        }

        let mut exports = Vec::with_capacity(scope.exports.len());
        let mut callable_exports = Vec::new();
        let mut ops = Vec::with_capacity(scope.exports.len());

        for (name, export) in &scope.exports {
            exports.push(name.clone());
            match export {
                ComponentExport::Func(_) => {
                    callable_exports.push(name.clone());
                    ops.push(ComponentOp::CanonLift { func_idx: 0 });
                }
                _ => {
                    ops.push(ComponentOp::Export { name: name.clone() });
                }
            }
        }

        let types = Vec::<ComponentTypeInfo>::new();

        Ok(ComponentProgram {
            types,
            imports,
            callable_imports,
            exports,
            callable_exports,
            ops,
            bytes: bytes.to_vec(),
            root,
            type_map: validator.snapshot_types(),
            component_store: state.component_store.snapshot(),
            instance_store: state.instance_store.snapshot(),
            func_store: state.func_store.snapshot(),
            core_module_store: state.core_module_store.snapshot(),
            core_type_store: state.core_type_store.snapshot(),
            core_instance_store: state.core_instance_store.snapshot(),
            core_func_store: state.core_func_store.snapshot(),
            core_memory_store: state.core_memory_store.snapshot(),
            core_global_store: state.core_global_store.snapshot(),
            core_table_store: state.core_table_store.snapshot(),
        })
    }

    pub async fn instantiate(
        &self,
        program: &ComponentProgram,
        store: &mut Store,
        linker: &ComponentLinker,
    ) -> Result<ComponentInstance, ComponentError> {
        runtime::instantiate(program.clone(), store, linker).await
    }
}
