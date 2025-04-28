use crate::component_model::{Idx, Resolvable, Resolver};
use crate::parser::component_model::{ComponentParseError, Validator};

pub trait IdxValidator<I>
where
    I: Idx,
    Self: Sized,
{
    type Resolved: Clone;
    fn validate_local_idx(&self, local_idx: u32) -> Result<I, ComponentParseError>;
    fn validate_idx_resolved(&self, local_idx: u32) -> Result<Self::Resolved, ComponentParseError>
    where
        I: Resolvable<Self::Resolved>,
        Self: Resolver<Self::Resolved, Error = ComponentParseError>,
    {
        let idx = self.validate_local_idx(local_idx)?;
        idx.resolve(self).cloned()
    }
    fn validate_outer_idx(&self, ct: u32, idx: u32) -> Result<I, ComponentParseError>;
    fn validate_outer_idx_resolved(
        &self,
        ct: u32,
        idx: u32,
    ) -> Result<Self::Resolved, ComponentParseError>
    where
        I: Resolvable<Self::Resolved>,
        Self: Resolver<Self::Resolved, Error = ComponentParseError>,
    {
        let idx = self.validate_outer_idx(ct, idx)?;
        idx.resolve(self).cloned()
    }
}
