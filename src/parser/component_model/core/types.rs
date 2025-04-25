use crate::binary::BinaryReader;
use crate::component_model::CoreType;
use crate::parser::component_model::{ParseContext, SizedResult, Validator};

pub(crate) fn parse_core_type(
    _ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreType> {
    todo!()
}
