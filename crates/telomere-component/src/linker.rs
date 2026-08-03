use crate::func::{ComponentParams, ComponentReturn};
use crate::support::common::InstanceHandle;
use crate::support::Store;
use crate::{ComponentError, ComponentFuture, ComponentValue};
use std::collections::HashMap;
use std::future::ready;
use std::sync::Arc;

pub(crate) type AsyncHostFn = Arc<
    dyn for<'a> Fn(
            &'a Store,
            &'a [ComponentValue],
        ) -> ComponentFuture<'a, Result<Vec<ComponentValue>, ComponentError>>
        + 'static,
>;

#[derive(Clone)]
pub(crate) struct CoreExportBinding {
    pub instance: InstanceHandle,
    pub export_name: String,
}

#[derive(Clone)]
pub(crate) enum LinkerBinding {
    Host(AsyncHostFn),
    Core(CoreExportBinding),
}

/// A collection of functions that satisfies a component instance import.
///
/// Register functions on this value, then install it under an import-instance
/// name with [ComponentLinker::register_import_instance].
#[derive(Default, Clone)]
pub struct ComponentLinkerInstance {
    exports: HashMap<String, LinkerBinding>,
}

impl ComponentLinkerInstance {
    /// Creates an empty instance binding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an asynchronous host function in this instance.
    ///
    /// The closure receives dynamic component values and must return results in
    /// declaration order. Its future is polled on the component runtime's local
    /// execution context.
    pub fn register_func_async(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(
                &'a Store,
                &'a [ComponentValue],
            )
                -> ComponentFuture<'a, Result<Vec<ComponentValue>, ComponentError>>
            + 'static,
    ) {
        self.exports
            .insert(name.into(), LinkerBinding::Host(Arc::new(func)));
    }

    /// Registers a synchronous host function in this instance.
    ///
    /// This is a convenience wrapper around [Self::register_func_async] for
    /// hosts that can return their result immediately.
    pub fn register_func(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_func_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

    /// Registers an asynchronous host function with checked Rust parameter types.
    ///
    /// P and R are converted through the Component Model canonical ABI before
    /// and after invoking the host function.
    pub fn register_func_typed_async<P, R>(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(&'a Store, P) -> ComponentFuture<'a, Result<R, ComponentError>> + 'static,
    ) where
        P: ComponentParams + 'static,
        R: ComponentReturn + 'static,
    {
        let func = Arc::new(func);
        self.register_func_async(name, move |store, args| {
            match P::from_component_args(args) {
                Ok(params) => {
                    let func = Arc::clone(&func);
                    Box::pin(async move {
                        let result = (func)(store, params).await?;
                        result.into_component_results()
                    })
                }
                Err(error) => Box::pin(ready(Err(error))),
            }
        });
    }

    /// Registers a synchronous typed host function in this instance.
    ///
    /// This is the immediate-result counterpart to
    /// [Self::register_func_typed_async].
    pub fn register_func_typed<P, R>(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, P) -> Result<R, ComponentError> + 'static,
    ) where
        P: ComponentParams + 'static,
        R: ComponentReturn + 'static,
    {
        self.register_func_typed_async(name, move |store, params| {
            Box::pin(ready(func(store, params)))
        });
    }

    pub(crate) fn resolve_export(&self, name: &str) -> Option<LinkerBinding> {
        self.exports.get(name).cloned()
    }
}

/// Registrations used to resolve a component's imported and host-exported functions.
///
/// A linker is mutable while it is being configured and can then be reused by
/// calls to [crate::ComponentEngine::instantiate].
#[derive(Default, Clone)]
pub struct ComponentLinker {
    imports: HashMap<String, LinkerBinding>,
    exports: HashMap<String, LinkerBinding>,
    import_instances: HashMap<String, ComponentLinkerInstance>,
}

impl ComponentLinker {
    /// Creates an empty linker with no host imports or exports.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an asynchronous root import function under a name.
    ///
    /// Use this for imports declared directly by a component rather than inside
    /// an imported instance.
    pub fn register_import_async(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(
                &'a Store,
                &'a [ComponentValue],
            )
                -> ComponentFuture<'a, Result<Vec<ComponentValue>, ComponentError>>
            + 'static,
    ) {
        self.imports
            .insert(name.into(), LinkerBinding::Host(Arc::new(func)));
    }

    /// Registers an asynchronous function that can satisfy a component export.
    ///
    /// Export registrations are useful when a component re-exports a host
    /// capability as part of a composed instance.
    pub fn register_export_async(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(
                &'a Store,
                &'a [ComponentValue],
            )
                -> ComponentFuture<'a, Result<Vec<ComponentValue>, ComponentError>>
            + 'static,
    ) {
        self.exports
            .insert(name.into(), LinkerBinding::Host(Arc::new(func)));
    }

    /// Registers an immediate-result root import function.
    ///
    /// This is the synchronous counterpart to [Self::register_import_async].
    pub fn register_import(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_import_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

    /// Registers an immediate-result host export function.
    ///
    /// This is the synchronous counterpart to [Self::register_export_async].
    pub fn register_export(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_export_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

    /// Registers an asynchronous root import with canonical-ABI type checking.
    ///
    /// P and R describe the Rust representation used by the host function;
    /// incompatible component signatures fail during linking or invocation.
    pub fn register_import_typed_async<P, R>(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(&'a Store, P) -> ComponentFuture<'a, Result<R, ComponentError>> + 'static,
    ) where
        P: ComponentParams + 'static,
        R: ComponentReturn + 'static,
    {
        let func = Arc::new(func);
        self.register_import_async(name, move |store, args| {
            match P::from_component_args(args) {
                Ok(params) => {
                    let func = Arc::clone(&func);
                    Box::pin(async move {
                        let result = (func)(store, params).await?;
                        result.into_component_results()
                    })
                }
                Err(error) => Box::pin(ready(Err(error))),
            }
        });
    }

    /// Registers an immediate-result typed root import.
    ///
    /// This is the synchronous counterpart to [Self::register_import_typed_async].
    pub fn register_import_typed<P, R>(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, P) -> Result<R, ComponentError> + 'static,
    ) where
        P: ComponentParams + 'static,
        R: ComponentReturn + 'static,
    {
        self.register_import_typed_async(name, move |store, params| {
            Box::pin(ready(func(store, params)))
        });
    }

    /// Maps a component import to an export of an already-instantiated core module.
    ///
    /// The instance is a core-runtime handle and the export name identifies the
    /// core function to lower through the canonical ABI.
    pub fn register_import_core(
        &mut self,
        name: impl Into<String>,
        instance: InstanceHandle,
        export_name: impl Into<String>,
    ) {
        self.imports.insert(
            name.into(),
            LinkerBinding::Core(CoreExportBinding {
                instance,
                export_name: export_name.into(),
            }),
        );
    }

    /// Registers a named imported component instance.
    ///
    /// Populate the instance with [ComponentLinkerInstance] before associating
    /// it with the component import name.
    pub fn register_import_instance(
        &mut self,
        name: impl Into<String>,
        instance: ComponentLinkerInstance,
    ) {
        self.import_instances.insert(name.into(), instance);
    }

    /// Maps a host-provided component export to a core-module function.
    ///
    /// This is the export-side counterpart to [Self::register_import_core].
    pub fn register_export_core(
        &mut self,
        name: impl Into<String>,
        instance: InstanceHandle,
        export_name: impl Into<String>,
    ) {
        self.exports.insert(
            name.into(),
            LinkerBinding::Core(CoreExportBinding {
                instance,
                export_name: export_name.into(),
            }),
        );
    }

    pub(crate) fn resolve_export(&self, name: &str) -> Option<LinkerBinding> {
        self.exports.get(name).cloned()
    }

    pub(crate) fn resolve_import(&self, name: &str) -> Option<LinkerBinding> {
        self.imports.get(name).cloned()
    }

    pub(crate) fn resolve_import_instance(&self, name: &str) -> Option<ComponentLinkerInstance> {
        self.import_instances.get(name).cloned()
    }
}
