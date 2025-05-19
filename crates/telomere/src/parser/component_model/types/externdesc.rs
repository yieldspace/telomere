use crate::binary::BinaryReader;
use crate::component_model::types::Type;
use crate::component_model::{ExternDesc, ResourceId};
use crate::parser::component_model::parse_type_local_idx;
use crate::parser::component_model::types::valtype::parse_valtype;
use crate::parser::component_model::{ParseContext, ParseResult};
use crate::parser::core::parse_u32;

pub fn parse_externdesc(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ExternDesc> {
    let desc = match ctx.reader.read_exact_one()? {
        0x03 => match ctx.reader.read_exact_one()? {
            0x00 => {
                let idx = parse_type_local_idx(ctx)?;
                let tid = ctx.validator.scope().types.get(idx)?;

                ExternDesc::Eq(tid)
            }
            0x01 => ExternDesc::Sub,
            _ => todo!(),
        },
        0x04 => {
            let idx = parse_type_local_idx(ctx)?;
            let id = ctx.validator.scope().types.get(idx)?;
            ExternDesc::Component(id)
        }
        _ => todo!(),
    };
    Ok(desc)
}
