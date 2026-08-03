use super::*;

/// An owned handle for a resource defined by component type T.
///
/// Owning a handle transfers responsibility for the resource to the receiving
/// component boundary. Generated bindings use a marker type for T to prevent
/// unrelated resources from being mixed accidentally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Own<T> {
    handle: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Own<T> {
    /// Wraps a raw canonical-ABI resource handle.
    ///
    /// Callers normally receive this from generated bindings rather than
    /// constructing handles manually.
    pub fn new(handle: u32) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }

    /// Returns the raw resource handle for use by a host implementation.
    pub fn handle(self) -> u32 {
        self.handle
    }
}

/// A borrowed handle for a resource defined by component type T.
///
/// Unlike [Own], this wrapper represents a non-owning reference whose valid
/// lifetime is governed by the component call that supplied it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Borrow<T> {
    handle: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Borrow<T> {
    /// Wraps a raw canonical-ABI borrowed-resource handle.
    pub fn new(handle: u32) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }

    /// Returns the raw borrowed-resource handle.
    pub fn handle(self) -> u32 {
        self.handle
    }
}

/// Converts a Rust value into its dynamic Component Model representation.
///
/// Implement this trait for host types used as function parameters. Generated
/// bindings implement it for their public WIT-derived types.
pub trait LowerComponent: Sized {
    /// Lowers this value to a value accepted by a dynamic component call.
    fn lower_component(self) -> Result<ComponentValue, ComponentError>;

    /// Checks that this Rust representation matches a component value type.
    #[doc(hidden)]
    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError>;
}

/// Converts a dynamic Component Model value into a Rust value.
///
/// Implement this trait for host return types and use generated bindings when
/// the component interface is known ahead of time.
pub trait LiftComponent: Sized {
    /// Lifts one dynamic component value into this Rust type.
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError>;

    /// Checks that this Rust representation matches a component value type.
    #[doc(hidden)]
    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError>;
}

/// Internal conversion contract for typed component-function parameters.
#[doc(hidden)]
pub trait ComponentParams: Sized {
    /// Builds the parameter tuple from dynamic arguments.
    fn from_component_args(args: &[ComponentValue]) -> Result<Self, ComponentError>;
    /// Lowers the parameter tuple into dynamic arguments.
    fn into_component_args(self) -> Result<Vec<ComponentValue>, ComponentError>;
    /// Verifies this tuple against a function parameter list.
    fn matches_params(params: &[ValType], program: &ComponentProgram)
        -> Result<(), ComponentError>;
}

/// Internal conversion contract for typed component-function results.
#[doc(hidden)]
pub trait ComponentReturn: Sized {
    /// Builds this return value from dynamic component results.
    fn from_component_results(results: Vec<ComponentValue>) -> Result<Self, ComponentError>;
    /// Lowers this return value into dynamic component results.
    fn into_component_results(self) -> Result<Vec<ComponentValue>, ComponentError>;
    /// Verifies this return type against a function result declaration.
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
