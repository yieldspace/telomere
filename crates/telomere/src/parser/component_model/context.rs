use crate::binary::BinaryReader;
use crate::parser::component_model::validator::{ParseState, ScopeGuard};
use crate::parser::component_model::Validator;
use std::cell::RefMut;
use std::fmt::{Debug, Formatter};

pub struct ParseContext<'a, 'b, 'c, R>
where
    R: BinaryReader,
{
    pub reader: &'a mut R,
    pub state: &'a mut ParseState<'b>,
    pub validator: &'a mut Validator<'c>,
}

impl<'a, 'b, 'c, R> ParseContext<'a, 'b, 'c, R>
where
    R: BinaryReader,
{
    pub fn new(
        reader: &'a mut R,
        state: &'a mut ParseState<'b>,
        validator: &'a mut Validator<'c>,
    ) -> Self {
        Self {
            reader,
            state,
            validator,
        }
    }
}
