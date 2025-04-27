use crate::component_model::{Idx, Resolvable, Resolver};
use crate::parser::component_model::{ComponentParseError, Validator};

pub trait IdxValidator<I, T>
where
    I: Resolvable<T>,
    Self: Validator + Resolver<T, Error = ComponentParseError> + Sized,
    T: Clone
{
    fn validate_idx(&self, local_idx: u32) -> Result<I, ComponentParseError>;
    fn validate_idx_resolved(&self, local_idx: u32) -> Result<T, ComponentParseError> {
        let idx = self.validate_idx(local_idx)?;
        idx.resolve(self).cloned()
    }
    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<I, ComponentParseError>;
    fn validate_outer_idx_resolved(&self, ct: u32, idx: u32) -> Result<T, ComponentParseError> {
        let idx = self.validate_outer_idx(ct, idx)?;
        idx.resolve(self).cloned()
    }
}
