use crate::binary::BinaryReader;
use crate::component_model::id::{CoreFuncId, CoreInstanceIdx, CoreMemoryId, CoreTableId};
use crate::parser::component::ComponentParseError;
use crate::parser::component::ParseContext;
use crate::parser::core::parse_u32;

type Result<R> = std::result::Result<R, ComponentParseError>;

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
    let (len, id) = parse_u32(ctx.reader)?;
    if let Some(_) = ctx.builder.get_core_instance(id as usize) {
        Ok((len, CoreInstanceIdx(id as usize)))
    } else {
        Err(ComponentParseError::InvalidIdx(
            "Core instance".to_string(),
            id,
        ))
    }
}
