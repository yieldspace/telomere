use crate::component::runtime::RuntimeInstance;
use crate::component::{ComponentError, ComponentProgram, ComponentValue};
use crate::Store;

#[derive(Clone)]
pub struct ComponentInstance {
    runtime: RuntimeInstance,
    pub(crate) _program: ComponentProgram,
}

impl ComponentInstance {
    pub(crate) fn new(program: ComponentProgram, runtime: RuntimeInstance) -> Self {
        Self {
            runtime,
            _program: program,
        }
    }

    pub async fn call(
        &self,
        store: &mut Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call(store, name, args).await
    }
}
