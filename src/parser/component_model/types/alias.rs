use crate::binary::BinaryReader;
use crate::component_model::{AliasType, Resolvable, Sort, SortWithIdx};
use crate::parser::component_model::{parse_core_instance_idx, parse_instance_idx, parse_sort, ComponentParseError, ParseContext, SizedResult, Validator};
use crate::parser::component_model::validator::get_outer_validator;
use crate::parser::core::{parse_name, parse_u32};

pub fn parse_alias_type(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<AliasType> {
    let start_count = ctx.reader.read_count();
    let (_, sort) = parse_sort(ctx)?;
    let alias = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, instance_idx) = parse_instance_idx(ctx)?;
            let instance = instance_idx.resolve(ctx.validator)?;
            let (_, name) = parse_name(ctx.reader)?;
            if let Some(sort) = instance.get_export(&name)? {
                match sort {
                    SortWithIdx::Type(idx) => {
                        let ty = idx.resolve(ctx.validator)?;
                        AliasType::Type(ty.clone())
                    }
                    SortWithIdx::Instance(idx) => {
                        let inst = idx.resolve(ctx.validator)?;
                        AliasType::Instance(inst.ty.clone())
                    }
                    _ => panic!("Invalid sort type"),
                }
            } else {
                return Err(ComponentParseError::ExportNotFound(name.clone()));
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
            let target_validator = get_outer_validator(ctx.validator, ct);
            let type_idx = target_validator.validate_type_idx(idx as usize)?;
            let ty = type_idx.resolve(target_validator)?;
            AliasType::Type(ty.clone())
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid alias type for instance decl: {sort:?}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start_count, alias))
}

