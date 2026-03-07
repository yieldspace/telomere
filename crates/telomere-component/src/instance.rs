use crate::runtime::RuntimeInstance;
use crate::support::Store;
use crate::{ComponentError, ComponentProgram, ComponentValue};
use std::rc::Rc;

#[derive(Clone)]
pub struct ComponentInstance {
    pub(crate) runtime: RuntimeInstance,
    pub(crate) _program: Rc<ComponentProgram>,
}

impl ComponentInstance {
    pub(crate) fn new(program: ComponentProgram, runtime: RuntimeInstance) -> Self {
        Self {
            runtime,
            _program: Rc::new(program),
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
