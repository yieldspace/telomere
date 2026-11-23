use crate::func::{ComponentParams, ComponentReturn};
use crate::runtime::name_match;
use crate::support::common::InstanceHandle;
use crate::support::Store;
use crate::{ComponentError, ComponentFuture, ComponentValue};
use std::collections::HashMap;
use std::future::ready;
use std::sync::Arc;

/// The stored form of every `*_async` host registration. The future it
/// produces is subject to the first-poll contract on [`ComponentFuture`]:
/// the runtime polls it once, synchronously, and never drives it further.
pub(crate) type AsyncHostFn = Arc<
    dyn for<'a> Fn(
            &'a Store,
            &'a [ComponentValue],
        ) -> ComponentFuture<'a, Result<Vec<ComponentValue>, ComponentError>>
        + 'static,
>;

/// Releases one host-provided resource handle.
///
/// Registered for an imported instance's resource export with
/// [ComponentLinker::register_import_resource_drop]; the runtime invokes it
/// when a guest executes `canon resource.drop` on a live handle of that
/// resource type, so the host can release the state behind the handle.
pub(crate) type ResourceDropFn = Arc<
    dyn for<'a> Fn(&'a Store, u32) -> ComponentFuture<'a, Result<(), ComponentError>> + 'static,
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

/// Resolves a requested name against the names a linker map was
/// registered under.
///
/// Exact names win. On a miss, a registration on the same semver
/// compatible track is accepted (see [`name_match`]), mirroring the
/// rules export lookup uses: a component importing `wasi:io/error@0.2.3`
/// is satisfied by a provider registered as `wasi:io/error@0.2.6`, and
/// vice versa. When several registrations share the track, the maximal
/// version wins.
fn resolve_registered<V: Clone>(map: &HashMap<String, V>, name: &str) -> Option<V> {
    if let Some(value) = map.get(name) {
        return Some(value.clone());
    }
    let matched = name_match::compatible_name(name, map.keys().map(String::as_str))?;
    map.get(matched).cloned()
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
    /// declaration order.
    ///
    /// # First-poll contract
    ///
    /// Despite the name, this registration is **not** driven by an executor:
    /// the component runtime is synchronous, so when the guest calls the
    /// function, the returned future is polled exactly once with a no-op
    /// waker. A future that returns `Poll::Pending` on that poll fails the
    /// call and is never polled again. Prefer the synchronous counterpart
    /// unless the future is guaranteed ready on its first poll; see
    /// [`ComponentFuture`] for the full contract.
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
    ///
    /// # First-poll contract
    ///
    /// Despite the name, this registration is **not** driven by an executor:
    /// the component runtime is synchronous, so when the guest calls the
    /// function, the returned future is polled exactly once with a no-op
    /// waker. A future that returns `Poll::Pending` on that poll fails the
    /// call and is never polled again. Prefer the synchronous counterpart
    /// unless the future is guaranteed ready on its first poll; see
    /// [`ComponentFuture`] for the full contract.
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
        resolve_registered(&self.exports, name)
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
    import_resource_drops: HashMap<(String, String), ResourceDropFn>,
    import_values: HashMap<String, ComponentValue>,
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
    ///
    /// # First-poll contract
    ///
    /// Despite the name, this registration is **not** driven by an executor:
    /// the component runtime is synchronous, so when the guest calls the
    /// function, the returned future is polled exactly once with a no-op
    /// waker. A future that returns `Poll::Pending` on that poll fails the
    /// call and is never polled again. Prefer the synchronous counterpart
    /// unless the future is guaranteed ready on its first poll; see
    /// [`ComponentFuture`] for the full contract.
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
    ///
    /// # First-poll contract
    ///
    /// Despite the name, this registration is **not** driven by an executor:
    /// the component runtime is synchronous, so when the guest calls the
    /// function, the returned future is polled exactly once with a no-op
    /// waker. A future that returns `Poll::Pending` on that poll fails the
    /// call and is never polled again. Prefer the synchronous counterpart
    /// unless the future is guaranteed ready on its first poll; see
    /// [`ComponentFuture`] for the full contract.
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
    ///
    /// # First-poll contract
    ///
    /// Despite the name, this registration is **not** driven by an executor:
    /// the component runtime is synchronous, so when the guest calls the
    /// function, the returned future is polled exactly once with a no-op
    /// waker. A future that returns `Poll::Pending` on that poll fails the
    /// call and is never polled again. Prefer the synchronous counterpart
    /// unless the future is guaranteed ready on its first poll; see
    /// [`ComponentFuture`] for the full contract.
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
    /// it with the component import name. Lookups accept the registered
    /// name exactly or on the same semver compatible track, mirroring
    /// export lookup (see the module-level resolution rules).
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
        resolve_registered(&self.imports, name)
    }

    /// Registers a host-provided value for a component's value import.
    ///
    /// The value is checked against the import's declared type when the
    /// component is instantiated.
    pub fn register_import_value(&mut self, name: impl Into<String>, value: ComponentValue) {
        self.import_values.insert(name.into(), value);
    }

    pub(crate) fn resolve_import_value(&self, name: &str) -> Option<ComponentValue> {
        resolve_registered(&self.import_values, name)
    }

    /// Names a value import registered on an incompatible version track
    /// (see [`crate::runtime::name_match`]).
    pub(crate) fn incompatible_import_value(&self, name: &str) -> Option<String> {
        name_match::incompatible_name(name, self.import_values.keys().map(String::as_str))
            .map(str::to_owned)
    }

    pub(crate) fn resolve_import_instance(&self, name: &str) -> Option<ComponentLinkerInstance> {
        resolve_registered(&self.import_instances, name)
    }

    /// Names a root import registered on an incompatible version track
    /// for the same interface as `name`, for error messages that name
    /// both what was expected and what was found.
    pub(crate) fn incompatible_import(&self, name: &str) -> Option<String> {
        name_match::incompatible_name(name, self.imports.keys().map(String::as_str))
            .map(str::to_owned)
    }

    /// Names an import instance registered on an incompatible version
    /// track for the same interface as `name`, for error messages that
    /// name both what was expected and what was found.
    pub(crate) fn incompatible_import_instance(&self, name: &str) -> Option<String> {
        name_match::incompatible_name(name, self.import_instances.keys().map(String::as_str))
            .map(str::to_owned)
    }

    /// Registers an asynchronous destructor for a resource type exported by
    /// an imported instance.
    ///
    /// When a guest drops a handle of the resource named `resource` from the
    /// instance imported as `instance` (`canon resource.drop`), the runtime
    /// invokes `func` with the handle so the host can release the state
    /// behind it. Dropping a handle that is not live traps in the guest, as
    /// the canonical ABI requires, and never reaches `func`. Without a
    /// registration, dropping a host-provided resource of that type traps.
    ///
    /// # First-poll contract
    ///
    /// Despite the name, this registration is **not** driven by an executor:
    /// the component runtime is synchronous, so when the guest calls the
    /// function, the returned future is polled exactly once with a no-op
    /// waker. A future that returns `Poll::Pending` on that poll fails the
    /// call and is never polled again. Prefer the synchronous counterpart
    /// unless the future is guaranteed ready on its first poll; see
    /// [`ComponentFuture`] for the full contract.
    pub fn register_import_resource_drop_async(
        &mut self,
        instance: impl Into<String>,
        resource: impl Into<String>,
        func: impl for<'a> Fn(&'a Store, u32) -> ComponentFuture<'a, Result<(), ComponentError>>
            + 'static,
    ) {
        self.import_resource_drops.insert(
            (instance.into(), resource.into()),
            Arc::new(func) as ResourceDropFn,
        );
    }

    /// Registers an immediate-result destructor for a resource type exported
    /// by an imported instance.
    ///
    /// This is the synchronous counterpart to
    /// [Self::register_import_resource_drop_async].
    pub fn register_import_resource_drop(
        &mut self,
        instance: impl Into<String>,
        resource: impl Into<String>,
        func: impl Fn(&Store, u32) -> Result<(), ComponentError> + 'static,
    ) {
        self.register_import_resource_drop_async(instance, resource, move |store, handle| {
            Box::pin(ready(func(store, handle)))
        });
    }

    pub(crate) fn resolve_import_resource_drop(
        &self,
        instance: &str,
        resource: &str,
    ) -> Option<ResourceDropFn> {
        if let Some(drop_fn) = self
            .import_resource_drops
            .get(&(instance.to_owned(), resource.to_owned()))
        {
            return Some(drop_fn.clone());
        }
        // The resource name must match exactly, but the instance name
        // follows the same semver-track rules as import resolution: a
        // guest importing `wasi:io/error@0.2.0` uses the destructor the
        // host registered for `wasi:io/error@0.2.6`.
        let matched = name_match::compatible_name(
            instance,
            self.import_resource_drops
                .keys()
                .filter(|(_, registered_resource)| registered_resource == resource)
                .map(|(registered_instance, _)| registered_instance.as_str()),
        )?;
        self.import_resource_drops
            .get(&(matched.to_owned(), resource.to_owned()))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::block_on;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn instance_with_export(export: &str) -> ComponentLinkerInstance {
        let mut instance = ComponentLinkerInstance::new();
        instance.register_func(export, |_store, _args| Ok(vec![]));
        instance
    }

    fn call(binding: LinkerBinding) -> Vec<ComponentValue> {
        let LinkerBinding::Host(func) = binding else {
            panic!("expected a host binding");
        };
        let store = Store::new();
        block_on(func(&store, &[])).expect("host function should succeed")
    }

    #[test]
    fn import_instance_resolves_semver_compatible_version() {
        // The guest imports 0.2.6, the host registered 0.2.0 (#258).
        let mut linker = ComponentLinker::new();
        linker.register_import_instance("test:pkg/iface@0.2.0", instance_with_export("from-0.2.0"));
        let resolved = linker
            .resolve_import_instance("test:pkg/iface@0.2.6")
            .expect("compatible version should resolve");
        assert!(resolved.resolve_export("from-0.2.0").is_some());

        // And the reverse direction resolves too.
        let mut linker = ComponentLinker::new();
        linker.register_import_instance("test:pkg/iface@0.2.6", instance_with_export("from-0.2.6"));
        let resolved = linker
            .resolve_import_instance("test:pkg/iface@0.2.0")
            .expect("compatible version should resolve");
        assert!(resolved.resolve_export("from-0.2.6").is_some());
    }

    #[test]
    fn import_instance_exact_registration_wins_over_maximal() {
        let mut linker = ComponentLinker::new();
        linker.register_import_instance("test:pkg/iface@0.2.0", instance_with_export("from-0.2.0"));
        linker.register_import_instance("test:pkg/iface@0.2.6", instance_with_export("from-0.2.6"));
        // An exact registration beats the maximal version on the track.
        let resolved = linker
            .resolve_import_instance("test:pkg/iface@0.2.0")
            .expect("exact registration should resolve");
        assert!(resolved.resolve_export("from-0.2.0").is_some());
        assert!(resolved.resolve_export("from-0.2.6").is_none());
        // Without an exact registration, the maximal version wins.
        let resolved = linker
            .resolve_import_instance("test:pkg/iface@0.2.3")
            .expect("track lookup should resolve");
        assert!(resolved.resolve_export("from-0.2.6").is_some());
    }

    #[test]
    fn import_instance_incompatible_version_is_rejected_and_named() {
        let mut linker = ComponentLinker::new();
        linker.register_import_instance("test:pkg/iface@0.3.0", instance_with_export("from-0.3.0"));
        assert!(linker
            .resolve_import_instance("test:pkg/iface@0.2.6")
            .is_none());
        assert_eq!(
            linker.incompatible_import_instance("test:pkg/iface@0.2.6"),
            Some("test:pkg/iface@0.3.0".to_owned())
        );
        // A wholly absent interface has nothing to report.
        assert!(linker
            .resolve_import_instance("test:pkg/other@0.2.6")
            .is_none());
        assert_eq!(
            linker.incompatible_import_instance("test:pkg/other@0.2.6"),
            None
        );
    }

    #[test]
    fn root_import_resolves_semver_compatible_version() {
        let mut linker = ComponentLinker::new();
        linker.register_import("test:pkg/func@0.2.0", |_store, _args| {
            Ok(vec![ComponentValue::S32(20)])
        });
        linker.register_import("test:pkg/func@0.2.6", |_store, _args| {
            Ok(vec![ComponentValue::S32(26)])
        });
        // Exact wins.
        let binding = linker
            .resolve_import("test:pkg/func@0.2.0")
            .expect("exact registration should resolve");
        assert_eq!(call(binding), vec![ComponentValue::S32(20)]);
        // Otherwise the maximal version on the track wins.
        let binding = linker
            .resolve_import("test:pkg/func@0.2.3")
            .expect("track lookup should resolve");
        assert_eq!(call(binding), vec![ComponentValue::S32(26)]);
        // Incompatible tracks do not resolve but are named.
        assert!(linker.resolve_import("test:pkg/func@0.3.0").is_none());
        assert_eq!(
            linker.incompatible_import("test:pkg/func@0.3.0"),
            Some("test:pkg/func@0.2.6".to_owned())
        );
    }

    #[test]
    fn resource_drop_resolves_compatible_instance_name() {
        let fired = Rc::new(RefCell::new(Vec::new()));
        let mut linker = ComponentLinker::new();
        for version in ["0.2.0", "0.2.6"] {
            let fired = Rc::clone(&fired);
            linker.register_import_resource_drop(
                format!("wasi:io/error@{version}"),
                "error",
                move |_store, handle| {
                    fired.borrow_mut().push((version, handle));
                    Ok(())
                },
            );
        }
        let store = Store::new();
        // Exact registration wins.
        let drop_fn = linker
            .resolve_import_resource_drop("wasi:io/error@0.2.0", "error")
            .expect("exact registration should resolve");
        block_on(drop_fn(&store, 1)).expect("drop should succeed");
        // A version on the same track uses the maximal registration.
        let drop_fn = linker
            .resolve_import_resource_drop("wasi:io/error@0.2.3", "error")
            .expect("compatible instance should resolve");
        block_on(drop_fn(&store, 2)).expect("drop should succeed");
        assert_eq!(*fired.borrow(), vec![("0.2.0", 1), ("0.2.6", 2)]);
        // The resource name itself must match exactly, and an
        // incompatible instance track does not resolve.
        assert!(linker
            .resolve_import_resource_drop("wasi:io/error@0.2.3", "other")
            .is_none());
        assert!(linker
            .resolve_import_resource_drop("wasi:io/error@0.3.0", "error")
            .is_none());
    }
}
