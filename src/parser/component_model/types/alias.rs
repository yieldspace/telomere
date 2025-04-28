use crate::binary::BinaryReader;
use crate::component_model::{
    AliasType, CoreTypeIdx, Instance, InstanceExportType, InstanceIdx, Sort,
    SortWithIdx, Type, TypeIdx,
};
use crate::parser::component_model::{parse_instance_idx, parse_instance_idx_resolved, parse_sort, ComponentParseError, ParseContext, SizedResult, Validator};
use crate::parser::core::{parse_name, parse_u32};

pub fn parse_alias_type<
    R: BinaryReader,
>(
    ctx: &mut ParseContext<R>,
) -> SizedResult<AliasType> {
    let start_count = ctx.reader.read_count();
    let (_, sort) = parse_sort(ctx)?;
    let alias = match ctx.reader.read_exact_one()? {
        0x00 => {
            let instance = parse_instance_idx_resolved(ctx)?;
            let (_, name) = parse_name(ctx.reader)?;
            let ty = instance.get_export_type(&name)?;
            match ty {
                InstanceExportType::Type(ty) => AliasType::Type(ty),
                InstanceExportType::Instance(ty) => AliasType::Instance(ty),
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
            if sort != Sort::Type {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid alias sort for core instance decl: {sort:?}"
                )));
            }
            let ty = ctx.validator.validate_outer_idx_resolved::<Type, TypeIdx>(ct, idx)?;
            AliasType::Type(ty)
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid alias type for instance decl: {sort:?}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start_count, alias))
}
