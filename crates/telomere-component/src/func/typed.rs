use super::*;

#[derive(Clone)]
pub struct ComponentFunc {
    runtime: RuntimeInstance,
    program: Rc<ComponentProgram>,
    type_id: crate::ir::TypeId,
    name: String,
}

impl ComponentFunc {
    pub(crate) fn new(
        runtime: RuntimeInstance,
        program: Rc<ComponentProgram>,
        name: impl Into<String>,
        type_id: crate::ir::TypeId,
    ) -> Self {
        Self {
            runtime,
            program,
            type_id,
            name: name.into(),
        }
    }

    pub async fn call(
        &self,
        store: &Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call(store, &self.name, args).await
    }

    pub fn typed<P, R>(&self) -> Result<TypedComponentFunc<P, R>, ComponentError>
    where
        P: ComponentParams,
        R: ComponentReturn,
    {
        let func_type = match self.program.get_type(self.type_id) {
            Some(Type::Func(func_type)) => func_type,
            _ => {
                return Err(ComponentError::Link(format!(
                    "component export '{}' is not a function",
                    self.name
                )))
            }
        };
        P::matches_params(&func_type.params, &self.program)?;
        R::matches_result(func_type.result.as_ref(), &self.program)?;
        Ok(TypedComponentFunc {
            func: self.clone(),
            _marker: PhantomData,
        })
    }
}

#[derive(Clone)]
pub struct TypedComponentFunc<P, R> {
    func: ComponentFunc,
    _marker: PhantomData<fn(P) -> R>,
}

impl<P, R> TypedComponentFunc<P, R>
where
    P: ComponentParams,
    R: ComponentReturn,
{
    pub async fn call(&self, store: &Store, params: P) -> Result<R, ComponentError> {
        let results = self
            .func
            .call(store, &params.into_component_args()?)
            .await?;
        R::from_component_results(results)
    }
}
