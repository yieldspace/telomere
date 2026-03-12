use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Own<T> {
    handle: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Own<T> {
    pub fn new(handle: u32) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }

    pub fn handle(self) -> u32 {
        self.handle
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Borrow<T> {
    handle: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Borrow<T> {
    pub fn new(handle: u32) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }

    pub fn handle(self) -> u32 {
        self.handle
    }
}

pub trait LowerComponent: Sized {
    fn lower_component(self) -> Result<ComponentValue, ComponentError>;

    #[doc(hidden)]
    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError>;
}

pub trait LiftComponent: Sized {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError>;

    #[doc(hidden)]
    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError>;
}

#[doc(hidden)]
pub trait ComponentParams: Sized {
    fn from_component_args(args: &[ComponentValue]) -> Result<Self, ComponentError>;
    fn into_component_args(self) -> Result<Vec<ComponentValue>, ComponentError>;
    fn matches_params(params: &[ValType], program: &ComponentProgram)
        -> Result<(), ComponentError>;
}

#[doc(hidden)]
pub trait ComponentReturn: Sized {
    fn from_component_results(results: Vec<ComponentValue>) -> Result<Self, ComponentError>;
    fn into_component_results(self) -> Result<Vec<ComponentValue>, ComponentError>;
    fn matches_result(
        result: Option<&ValType>,
        program: &ComponentProgram,
    ) -> Result<(), ComponentError>;
}

pub(crate) trait ResultPayload: Sized {
    fn lower_result_payload(self) -> Result<Option<Box<ComponentValue>>, ComponentError>;

    fn lift_result_payload(value: Option<Box<ComponentValue>>) -> Result<Self, ComponentError>;

    fn matches_result_payload(
        ty: Option<&ValType>,
        program: &ComponentProgram,
    ) -> Result<(), ComponentError>;
}
