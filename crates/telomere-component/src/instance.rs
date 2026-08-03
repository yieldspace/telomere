use crate::runtime::RuntimeInstance;
use crate::support::Store;
use crate::{ComponentError, ComponentProgram, ComponentValue};
use std::rc::Rc;

/// A view onto an instance's exports at a nested component path.
///
/// Obtain one with [`ComponentInstance::exports`] or
/// [`ComponentInstance::get_instance`], then descend with [`Self::get_instance`]
/// before calling an export at that path.
#[derive(Clone)]
pub struct ComponentExports {
    pub(crate) runtime: RuntimeInstance,
    pub(crate) path: Vec<String>,
}

impl ComponentExports {
    pub(crate) fn new(runtime: RuntimeInstance, path: Vec<String>) -> Self {
        Self { runtime, path }
    }

    /// Returns a view rooted at the named nested instance.
    ///
    /// The name is resolved when an export is called, which lets callers build
    /// a path without eagerly instantiating another component.
    pub fn get_instance(&self, name: impl Into<String>) -> Self {
        let mut path = self.path.clone();
        path.push(name.into());
        Self {
            runtime: self.runtime.clone(),
            path,
        }
    }

    /// Calls a function exported from this view's component-instance path.
    ///
    /// `args` use dynamic [`ComponentValue`] representations; use
    /// [`ComponentInstance::get_typed_func`] for checked Rust conversions at a
    /// root export.
    pub async fn call(
        &self,
        store: &Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call_path(store, &self.path, name, args).await
    }
}

/// An instantiated component and the program metadata that describes it.
///
/// Instances are created by [`crate::ComponentEngine::instantiate`]. They may
/// call root functions dynamically, obtain checked function wrappers, or expose
/// nested instances through [`Self::exports`].
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

    /// Calls a root function export with dynamically represented arguments.
    ///
    /// The function name and values are validated against the component's
    /// canonical ABI while the future executes.
    pub async fn call(
        &self,
        store: &Store,
        name: &str,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call(store, name, args).await
    }

    /// Returns a root export view for traversing nested component instances.
    pub fn exports(&self) -> ComponentExports {
        ComponentExports::new(self.runtime.clone(), Vec::new())
    }

    /// Returns a view of a named nested component instance.
    pub fn get_instance(&self, name: impl Into<String>) -> ComponentExports {
        self.exports().get_instance(name)
    }
}
