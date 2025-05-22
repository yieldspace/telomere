use crate::binary::BinaryReader;
use crate::component_model::types::CoreType;
use crate::parser::component_model::{ParseContext, SizedResult};

pub fn parse_core_type(_ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CoreType> {
    todo!()
}
