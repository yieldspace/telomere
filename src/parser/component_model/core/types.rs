use crate::binary::BinaryReader;
use crate::component_model::CoreType;
use crate::parser::component_model::{ParseContext, SizedResult};

pub(crate) fn parse_core_type(_ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CoreType> {
    todo!()
}
