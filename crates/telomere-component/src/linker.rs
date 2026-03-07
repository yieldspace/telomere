use crate::func::{ComponentParams, ComponentReturn};
use crate::support::common::InstanceHandle;
use crate::support::Store;
use crate::{ComponentError, ComponentFuture, ComponentValue};
use std::collections::HashMap;
use std::future::ready;
use std::sync::Arc;

pub(crate) type AsyncHostFn = Arc<
    dyn for<'a> Fn(
            &'a mut Store,
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
pub struct ComponentLinker {
    imports: HashMap<String, LinkerBinding>,
    exports: HashMap<String, LinkerBinding>,
}

impl ComponentLinker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_import_async(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(
                &'a mut Store,
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
                &'a mut Store,
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
        func: impl Fn(&mut Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_import_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

    pub fn register_export(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(&mut Store, &[ComponentValue]) -> Result<Vec<ComponentValue>, ComponentError>
            + 'static,
    ) {
        self.register_export_async(name, move |store, args| Box::pin(ready(func(store, args))));
    }

    pub fn register_import_typed_async<P, R>(
        &mut self,
        name: impl Into<String>,
        func: impl for<'a> Fn(&'a mut Store, P) -> ComponentFuture<'a, Result<R, ComponentError>>
            + 'static,
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
        func: impl Fn(&mut Store, P) -> Result<R, ComponentError> + 'static,
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
}
