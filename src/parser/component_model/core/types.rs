use crate::binary::BinaryReader;
use crate::component_model::CoreType;
use crate::parser::component_model::validator::{DefaultValidatorState, ValidatorStateImpl};
use crate::parser::component_model::{ParseContext, SizedResult, Validator};

pub(crate) fn parse_core_type(
    _ctx: &mut ParseContext<impl BinaryReader, impl ValidatorStateImpl>,
) -> SizedResult<CoreType> {
    todo!()
}
