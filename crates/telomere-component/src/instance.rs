use crate::runtime::RuntimeInstance;
use crate::support::Store;
use crate::{ComponentError, ComponentProgram, ComponentValue};
use std::rc::Rc;

#[derive(Clone)]
pub struct ComponentExports {
    pub(crate) runtime: RuntimeInstance,
    pub(crate) path: Vec<String>,
}

impl ComponentExports {
    pub(crate) fn new(runtime: RuntimeInstance, path: Vec<String>) -> Self {
        Self { runtime, path }
    }

    pub fn get_instance(&self, name: impl Into<String>) -> Self {
        let mut path = self.path.clone();
        path.push(name.into());
        Self {
            runtime: self.runtime.clone(),
            path,
        }
    }

    pub async fn call(
        &self,
        store: &Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call_path(store, &self.path, name, args).await
    }
}

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
        store: &Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call(store, name, args).await
    }

    pub fn exports(&self) -> ComponentExports {
        ComponentExports::new(self.runtime.clone(), Vec::new())
    }

    pub fn get_instance(&self, name: impl Into<String>) -> ComponentExports {
        self.exports().get_instance(name)
    }
}
