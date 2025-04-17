use crate::binary::BinaryReader;
use crate::component_model::CoreTypeIdx;
use crate::parser::component_model::{ParseContext, SizedResult};

pub(crate) fn parse_core_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<CoreTypeIdx> {
    todo!()
}
