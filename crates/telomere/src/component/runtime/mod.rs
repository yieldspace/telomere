use crate::component::instance::ResolvedCallable;
use crate::component::linker::LinkerBinding;
use crate::component::{ComponentError, ComponentInstance, ComponentLinker, ComponentProgram};
use crate::Store;
use std::collections::HashMap;

pub async fn instantiate(
    program: ComponentProgram,
    _store: &mut Store,
    linker: &ComponentLinker,
) -> Result<ComponentInstance, ComponentError> {
    let mut exports: HashMap<String, ResolvedCallable> = HashMap::new();

    for name in &program.callable_exports {
        let binding = linker
            .resolve_export(name)
            .or_else(|| linker.resolve_import(name))
            .or_else(|| {
                if program.callable_imports.len() == 1 {
                    linker.resolve_import(&program.callable_imports[0])
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                ComponentError::Link(format!("callable export '{}' is unresolved", name))
            })?;

        let resolved = match binding {
            LinkerBinding::Host(f) => ResolvedCallable::Host(f),
            LinkerBinding::Core(binding) => ResolvedCallable::from(binding),
        };
        exports.insert(name.clone(), resolved);
    }

    Ok(ComponentInstance::new(program, exports))
}
