use super::*;

impl ComponentInstance {
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
