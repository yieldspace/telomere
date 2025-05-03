use crate::binary::BinaryReader;
use crate::component_model::{AliasType, ExternDesc, Sort};
use crate::parser::component_model::{
    parse_export_name, parse_instance_idx, parse_sort, ComponentParseError,
    ParseContext, SizedResult,
};
use crate::parser::core::parse_u32;

pub fn parse_alias_type<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<AliasType> {
    let start_count = ctx.reader.read_count();
    let (_, sort) = parse_sort(ctx)?;
    let alias = match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_instance_idx(ctx)?;
            let instance = ctx.validator.get_instance_type(idx)?;
            let (_, name) = parse_export_name(ctx)?;
            let ty = instance.get_export_type(&name)?;
            match ty {
                ExternDesc::Type(ty) => AliasType::Type(ty.clone()),
                ExternDesc::Instance(ty) => AliasType::Instance(ty.clone()),
                _ => {
                    return Err(ComponentParseError::InvalidSignature(format!(
                        "Invalid alias type for instance decl: {sort:?}"
                    )));
                }
            }
        }
        0x02 => {
            let (_, ct) = parse_u32(ctx.reader)?;
            let (_, idx) = parse_u32(ctx.reader)?;
            let outer_validator = ctx.validator.get_outer(ct);
            match sort {
                Sort::Type => {
                    let idx = outer_validator.validate_type_idx(idx)?;
                    let ty = outer_validator.get_type(idx)?;
                    AliasType::Type(ty)
                }
                Sort::Instance => {
                    let idx = outer_validator.validate_instance_idx(idx)?;
                    let ty = outer_validator.get_instance_type(idx)?;
                    AliasType::Instance(ty)
                }
                _ => {
                    return Err(ComponentParseError::InvalidSignature(format!(
                        "Invalid alias type for instance decl: {sort:?}"
                    )));
                }
            }
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid alias type for instance decl: {sort:?}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start_count, alias))
}
