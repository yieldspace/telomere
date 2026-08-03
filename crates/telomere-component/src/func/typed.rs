use super::*;

/// A dynamically typed handle to one root component-function export.
///
/// Obtain a handle from [crate::ComponentInstance::get_func] when the function
/// signature is discovered at runtime, or call [Self::typed] to validate a
/// Rust representation once.
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

    /// Calls this function with dynamic Component Model arguments.
    pub async fn call(
        &self,
        store: &Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call(store, &self.name, args).await
    }

    /// Validates this function against P and R and returns a typed wrapper.
    ///
    /// The conversion constraints are checked before the first invocation, so a
    /// mismatched WIT signature fails as a link error instead of silently
    /// coercing values at call time.
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

/// A [ComponentFunc] whose parameters and result have been checked once.
///
/// Calls convert P and R through the canonical ABI using the hidden
/// [ComponentParams] and [ComponentReturn] implementation contracts.
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
    /// Calls the function with typed parameters and lifts its typed result.
    pub async fn call(&self, store: &Store, params: P) -> Result<R, ComponentError> {
        let results = self
            .func
            .call(store, &params.into_component_args()?)
            .await?;
        R::from_component_results(results)
    }
}
