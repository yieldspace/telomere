use crate::func::{ComponentParams, ComponentReturn};
use crate::support::common::InstanceHandle;
use crate::support::Store;
use crate::{ComponentError, ComponentFuture, ComponentValue};
use semver::Version;
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

#[derive(Default, Clone)]
pub struct ComponentLinkerInstance {
    exports: HashMap<String, LinkerBinding>,
}

impl ComponentLinkerInstance {
    pub fn new() -> Self {
        Self::default()
    }

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

    pub fn register_func(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_func_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

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

#[derive(Default, Clone)]
pub struct ComponentLinker {
    imports: VersionedNameMap<LinkerBinding>,
    exports: HashMap<String, LinkerBinding>,
    import_instances: VersionedNameMap<ComponentLinkerInstance>,
}

impl ComponentLinker {
    pub fn new() -> Self {
        Self::default()
    }

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

    pub fn register_import(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_import_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

    pub fn register_export(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_export_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

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

    pub fn register_import_instance(
        &mut self,
        name: impl Into<String>,
        instance: ComponentLinkerInstance,
    ) {
        self.import_instances.insert(name.into(), instance);
    }

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

#[derive(Clone)]
struct VersionedNameMap<T> {
    exact: HashMap<String, T>,
    compatible: HashMap<String, CompatibleName>,
}

#[derive(Clone)]
struct CompatibleName {
    name: String,
    version: Version,
}

impl<T> VersionedNameMap<T> {
    fn insert(&mut self, name: String, value: T) {
        if let Some((key, version)) = compatible_lookup_key(&name) {
            let should_update = self
                .compatible
                .get(&key)
                .map(|entry| {
                    version > entry.version || (version == entry.version && name == entry.name)
                })
                .unwrap_or(true);
            if should_update {
                self.compatible.insert(
                    key,
                    CompatibleName {
                        name: name.clone(),
                        version,
                    },
                );
            }
        }
        self.exact.insert(name, value);
    }

    fn get(&self, name: &str) -> Option<&T> {
        self.exact.get(name).or_else(|| {
            let (key, _) = compatible_lookup_key(name)?;
            let entry = self.compatible.get(&key)?;
            self.exact.get(&entry.name)
        })
    }
}

impl<T> Default for VersionedNameMap<T> {
    fn default() -> Self {
        Self {
            exact: HashMap::new(),
            compatible: HashMap::new(),
        }
    }
}

fn compatible_lookup_key(name: &str) -> Option<(String, Version)> {
    let (base, version_text) = name.rsplit_once('@')?;
    if !base.contains(':') || !base.contains('/') || base.contains("://") {
        return None;
    }
    let version = Version::parse(version_text).ok()?;
    if !version.pre.is_empty() {
        return None;
    }
    let key = if version.major > 0 {
        format!("{base}@{}", version.major)
    } else if version.minor > 0 {
        format!("{base}@0.{}", version.minor)
    } else {
        return None;
    };
    Some((key, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_binding(value: u32) -> LinkerBinding {
        LinkerBinding::Host(Arc::new(move |_, _| {
            Box::pin(async move { Ok(vec![ComponentValue::U32(value)]) })
        }))
    }

    fn resolved_host_value(binding: LinkerBinding) -> u32 {
        match binding {
            LinkerBinding::Host(func) => {
                let store = Store::new();
                futures::executor::block_on(func(&store, &[])).expect("host call should succeed")[0]
                    .as_u32()
                    .expect("host should return u32")
            }
            LinkerBinding::Core(_) => panic!("expected host binding"),
        }
    }

    #[test]
    fn import_resolution_prefers_exact_match_before_semver_fallback() {
        let mut linker = ComponentLinker::new();
        linker.register_import_async("wasi:cli/environment@0.2.6", |_, _| {
            Box::pin(async { Ok(vec![ComponentValue::U32(6)]) })
        });
        linker.register_import_async("wasi:cli/environment@0.2.0", |_, _| {
            Box::pin(async { Ok(vec![ComponentValue::U32(0)]) })
        });

        let resolved = linker
            .resolve_import("wasi:cli/environment@0.2.0")
            .expect("exact import should resolve");
        assert_eq!(resolved_host_value(resolved), 0);
    }

    #[test]
    fn import_resolution_uses_semver_compatible_component_package_names() {
        let mut map = VersionedNameMap::default();
        map.insert("wasi:cli/environment@0.2.6".to_owned(), host_binding(26));
        map.insert("wasi:cli/environment@0.2.1".to_owned(), host_binding(21));
        map.insert("wasi:cli/environment@1.4.0".to_owned(), host_binding(140));

        assert_eq!(
            resolved_host_value(
                map.get("wasi:cli/environment@0.2.0")
                    .expect("0.2 track should resolve")
                    .clone()
            ),
            26
        );
        assert_eq!(
            resolved_host_value(
                map.get("wasi:cli/environment@0.2.9")
                    .expect("0.2 track should resolve")
                    .clone()
            ),
            26
        );
        assert_eq!(
            resolved_host_value(
                map.get("wasi:cli/environment@1.0.0")
                    .expect("1 track should resolve")
                    .clone()
            ),
            140
        );
    }

    #[test]
    fn import_resolution_does_not_cross_major_minor_or_prerelease_tracks() {
        let mut map = VersionedNameMap::default();
        map.insert("wasi:cli/environment@0.2.6".to_owned(), host_binding(26));
        map.insert(
            "wasi:cli/environment@0.3.0-rc-2026-03-15".to_owned(),
            host_binding(30),
        );

        assert!(map.get("wasi:cli/environment@0.3.0").is_none());
        assert!(map
            .get("wasi:cli/environment@0.3.0-rc-2026-03-14")
            .is_none());
        assert!(map.get("wasi:cli/environment@0.0.1").is_none());
    }

    #[test]
    fn import_instance_resolution_uses_semver_compatible_names() {
        let mut linker = ComponentLinker::new();
        let mut instance = ComponentLinkerInstance::new();
        instance.register_func("value", |_, _| Ok(vec![ComponentValue::U32(9)]));
        linker.register_import_instance("wasi:cli/environment@0.2.6", instance);

        let resolved = linker
            .resolve_import_instance("wasi:cli/environment@0.2.0")
            .expect("compatible instance should resolve");
        assert!(resolved.resolve_export("value").is_some());
    }
}
