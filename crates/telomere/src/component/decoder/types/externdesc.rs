use crate::binary::BinaryReader;
use crate::component::decoder::parse_core_type_local_idx;
use crate::component::decoder::types::valtype::parse_valtype;
use crate::component::decoder::{
    parse_type_local_idx, ComponentParseError, ParseContext, ParseResult,
};
use crate::component::ir::types::CoreType;
use crate::component::ir::ExternDesc;

pub fn parse_externdesc(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ExternDesc> {
    let desc = match ctx.reader.read_exact_one()? {
        0x00 => {
            let core_sort = ctx.reader.read_exact_one()?;
            if core_sort != 0x11 {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "invalid extern descriptor: 0x00 0x{core_sort:02x}"
                )));
            }
            let idx = parse_core_type_local_idx(ctx)?;
            let ty = ctx.validator.scope().core_types.get(idx)?.clone();
            let CoreType::Module(module_ty) = ty else {
                return Err(ComponentParseError::InvalidType(
                    "expected core module type".to_owned(),
                ));
            };
            ExternDesc::Module(module_ty)
        }
        0x01 => {
            let idx = parse_type_local_idx(ctx)?;
            let id = ctx.validator.scope().type_indexes.get(idx)?;
            if !ctx.validator.get_type(id)?.is_func() {
                Err(ComponentParseError::InvalidType(
                    "expected func type".to_string(),
                ))?
            }
            ExternDesc::Func(id)
        }
        0x02 => ExternDesc::Value(parse_valtype(ctx)?),
        0x03 => match ctx.reader.read_exact_one()? {
            0x00 => {
                let idx = parse_type_local_idx(ctx)?;
                let tid = ctx.validator.scope().type_indexes.get(idx)?;

                ExternDesc::Eq(tid)
            }
            0x01 => ExternDesc::Sub,
            x => {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "invalid type bound descriptor: {x}"
                )));
            }
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
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid extern descriptor: {x}"
            )));
        }
    };
    Ok(desc)
}
