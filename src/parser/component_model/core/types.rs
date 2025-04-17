use crate::binary::BinaryReader;
use crate::component_model::CoreTypeIdx;
use crate::parser::component_model::{ParseContext, SizedResult, Validator};

pub(crate) fn parse_core_type(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreTypeIdx> {
    todo!()
}
