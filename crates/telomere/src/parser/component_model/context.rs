use crate::parser::component_model::validator::ParseState;
use crate::parser::component_model::Validator;
use binary_reader::BinaryReader;

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
