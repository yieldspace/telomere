use crate::binary::BinaryReader;
use crate::parser::component_model::validator::{ScopeGuard, ValidatorState};
use crate::parser::component_model::Validator;
use std::cell::RefMut;
use std::fmt::{Debug, Formatter};

pub struct ParseContext<'a, 'b, R>
where
    R: BinaryReader,
{
    pub reader: &'a mut R,
    pub state: &'a mut ValidatorState,
    pub validator: &'a mut Validator<'b>,
}

impl<R: BinaryReader> Debug for ParseContext<'_, '_, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ParseContext<{:x}>", self.reader.read_count())
    }
}

impl<'a, 'b, R> ParseContext<'a, 'b, R>
where
    R: BinaryReader,
{
    pub fn new(
        reader: &'a mut R,
        state: &'a mut ValidatorState,
        validator: &'a mut Validator<'b>,
    ) -> Self {
        Self {
            reader,
            state,
            validator,
        }
    }
}
