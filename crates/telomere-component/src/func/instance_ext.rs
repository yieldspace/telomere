use super::*;

impl ComponentInstance {
    /// Looks up a root function export for dynamic invocation.
    ///
    /// Returns [ComponentError::ExportNotFound] when the name is absent or does
    /// not name a function.
    pub fn get_func(&self, name: &str) -> Result<ComponentFunc, ComponentError> {
        let type_id = self
            ._program
            .get_root_func_type_id(name)
            .ok_or_else(|| ComponentError::ExportNotFound(name.to_owned()))?;
        Ok(ComponentFunc::new(
            self.runtime.clone(),
            Rc::clone(&self._program),
            name,
            type_id,
        ))
    }

    /// Looks up and type-checks a root function export against P and R.
    ///
    /// Prefer this to dynamic [ComponentInstance::call] when the interface is
    /// stable and Rust conversion errors should be reported before invocation.
    pub fn get_typed_func<P, R>(
        &self,
        name: &str,
    ) -> Result<TypedComponentFunc<P, R>, ComponentError>
    where
        P: ComponentParams,
        R: ComponentReturn,
    {
        self.get_func(name)?.typed()
    }
}
