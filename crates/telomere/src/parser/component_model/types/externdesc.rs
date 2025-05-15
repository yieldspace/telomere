use crate::binary::BinaryReader;
use crate::component_model::types::{ComponentType, Type};
use crate::component_model::{ExternDesc, ResourceId};
use crate::parser::component_model::types::valtype::parse_valtype;
use crate::parser::component_model::{
    parse_instance_local_idx, parse_type_local_idx, ComponentParseError,
};
use crate::parser::component_model::{ParseContext, ParseResult};
use crate::parser::core::parse_u32;

pub fn parse_externdesc(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ExternDesc> {
    let desc = match ctx.reader.read_exact_one()? {
        0x03 => match ctx.reader.read_exact_one()? {
            0x00 => {
                let idx = parse_type_local_idx(ctx)?;
                let tid = ctx.validator.scope().type_indexes.get(idx)?;

                ExternDesc::Eq(tid)
            }
            0x01 => ExternDesc::Sub,
            _ => todo!(),
        },
        0x04 => {
            let idx = parse_type_local_idx(ctx)?;
            let id = ctx.validator.scope().type_indexes.get(idx)?;
            if !ctx.validator.get_type(id)?.is_component() {
                return Err(ComponentParseError::InvalidType(
                    "expected component type".to_string(),
                ));
            }
            ExternDesc::Component(id)
        }
        0x05 => {
            let idx = parse_type_local_idx(ctx)?;
            let id = ctx.validator.scope().type_indexes.get(idx)?;
            if !ctx.validator.get_type(id)?.is_instance() {
                return Err(ComponentParseError::InvalidType(
                    "expected instance type".to_string(),
                ));
            }
            ExternDesc::Instance(id)
        }
        x => todo!("{}", x),
    };
    Ok(desc)
}
