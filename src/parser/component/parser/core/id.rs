use crate::binary::BinaryReader;
use crate::component_model::id::{CoreFuncId, CoreInstanceIdx, CoreMemoryId, CoreTableId};
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::component::ParseContext;

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_core_memory_id(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, CoreMemoryId)> {
    todo!()
}

pub fn parse_core_func_id(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, CoreFuncId)> {
    todo!()
}

pub fn parse_core_table_id(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, CoreTableId)> {
    todo!()
}

pub fn parse_core_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, CoreInstanceIdx)> {
    todo!()
}
