use crate::binary::BinaryReader;
use crate::component_model::types::{ComponentType, InstanceExportType, InstanceType, SortType};
use crate::component_model::LocalIdx;
use crate::parser::component_model::name::parse_export_name;
use crate::parser::component_model::sort::parse_sort;
use crate::parser::component_model::{
    parse_instance_local_idx, ComponentParseError, ParseContext, ParseResult,
};
use crate::parser::core::parse_u32;

pub fn parse_alias_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let start_count = ctx.reader.read_count();
    let sort = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_instance_local_idx(ctx)?;
            let instance_type_id = ctx.validator.scope().instances.get(idx)?;
            let instance_type: InstanceType = ctx
                .validator
                .scope_mut()
                .get_type(instance_type_id)?
                .clone()
                .try_into()
                .map_err(ComponentParseError::TypeMismatch)?;
            let name = parse_export_name(ctx)?;
            let (pid, ty) =
                instance_type
                    .get_export(&name)
                    .ok_or(ComponentParseError::ExportNotFound(
                        name.original.to_string(),
                    ))?;
            match (sort, ty) {
                (SortType::Instance, InstanceExportType::Instance(id)) => {
                    ctx.validator.scope_mut().instances.register(*id);
                }
                (SortType::Type, InstanceExportType::Type(id)) => {
                    ctx.validator.scope_mut().types.register(*id);
                }
                (SortType::Type, InstanceExportType::Sub(id)) => {
                    ctx.validator.scope_mut().types.register(*id);
                }
                _ => panic!(),
            }
        }
        0x02 => {
            let (_, ct) = parse_u32(ctx.reader)?;
            let (_, idx) = parse_u32(ctx.reader)?;
            let outer_scope = ctx.validator.outer_scope(ct);
            // match sort {
            //     SortType::Type => {
            //         let idx = outer_scope.types.get(LocalIdx::new(idx))?;
            //         let ty = outer_validator.get_type(idx)?;
            //         AliasType::Type(ty)
            //     }
            //     SortType::Instance => {
            //         let idx = outer_validator.validate_instance_idx(idx)?;
            //         let ty = outer_validator.get_instance_type(idx)?;
            //         AliasType::Instance(ty)
            //     }
            //     _ => {
            //         return Err(ComponentParseError::InvalidSignature(format!(
            //             "Invalid alias type for instance decl: {sort:?}"
            //         )));
            //     }
            // }
            todo!()
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid alias type for instance decl: {sort:?}"
            )));
        }
    };
    Ok(())
}
