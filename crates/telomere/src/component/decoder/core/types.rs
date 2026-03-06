use crate::binary::BinaryReader;
use crate::component::decoder::{ParseContext, SizedResult};
use crate::component::ir::types::CoreType;

pub fn parse_core_type(_ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CoreType> {
    todo!()
}
