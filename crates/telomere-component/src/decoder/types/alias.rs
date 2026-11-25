use crate::decoder::name::parse_export_name;
use crate::decoder::sort::parse_sort;
use crate::decoder::{parse_instance_local_idx, ComponentParseError, ParseContext, ParseResult};
use crate::ir::types::{CoreSortType, InstanceExportType, SortType};
use crate::ir::LocalIdx;
use crate::support::binary::BinaryReader;
use crate::support::parser::core::parse_u32;

pub fn parse_alias_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let sort = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => {
            // Inside a component or instance type, an alias export
            // may only refer to types or instances (a subset of the
            // sorts allowed for alias definitions).
            if !matches!(sort, SortType::Type | SortType::Instance) {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "aliases in a component or instance type may only refer to types or instances: found {sort:?}"
                )));
            }
            let idx = parse_instance_local_idx(ctx)?;
            let name = parse_export_name(ctx)?;
            let instance_type_id = ctx.validator.scope().instance_indexes.get(idx)?;
            let instance_type = ctx.validator.get_instance_type(instance_type_id)?;
            let export = instance_type.get_export(&name.original)?.clone();
            match (sort, export) {
                (SortType::Component, InstanceExportType::Component(id)) => {
                    let id = ctx.validator.freshen_import_type_id(id)?;
                    ctx.validator.scope_mut().component_indexes.add(id);
                }
                (SortType::Func, InstanceExportType::Func(id)) => {
                    ctx.validator.scope_mut().func_indexes.add(id);
                }
                (SortType::Type, InstanceExportType::Type(id)) => {
                    ctx.validator.scope_mut().type_indexes.add(id);
                }
                (SortType::Instance, InstanceExportType::Instance(id)) => {
                    let id = ctx.validator.freshen_import_type_id(id)?;
                    ctx.validator.scope_mut().instance_indexes.add(id);
                }
                (SortType::Core(CoreSortType::Module), InstanceExportType::CoreModule(ty)) => {
                    ctx.validator.scope_mut().core_modules.add(ty);
                }
                (SortType::Value, InstanceExportType::Value(ty)) => {
                    ctx.validator.scope_mut().value_indexes.add(ty);
                }
                (expected, found) => {
                    return Err(ComponentParseError::InvalidSignature(format!(
                        "alias type mismatch: expected {expected:?}, found {found:?}"
                    )));
                }
            }
        }
        0x02 => {
            // Inside a component or instance type, an alias outer
            // may only refer to types or core types.
            if !matches!(sort, SortType::Type | SortType::Core(CoreSortType::Type)) {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "aliases in a component or instance type may only refer to types or instances: found {sort:?}"
                )));
            }
            let (_, count) = parse_u32(ctx.reader)?;
            let (_, index) = parse_u32(ctx.reader)?;
            match sort {
                SortType::Component => {
                    let type_id = {
                        ctx.validator
                            .outer_scope(count)?
                            .component_indexes
                            .get(LocalIdx::new(index))?
                    };
                    ctx.validator.scope_mut().component_indexes.add(type_id);
                }
                SortType::Func => {
                    let type_id = {
                        ctx.validator
                            .outer_scope(count)?
                            .func_indexes
                            .get(LocalIdx::new(index))?
                    };
                    ctx.validator.scope_mut().func_indexes.add(type_id);
                }
                SortType::Type => {
                    let type_id = {
                        ctx.validator
                            .outer_scope(count)?
                            .type_indexes
                            .get(LocalIdx::new(index))?
                    };
                    ctx.validator.validate_type_decl_alias_resources(type_id)?;
                    ctx.validator.scope_mut().type_indexes.add(type_id);
                }
                SortType::Instance => {
                    let type_id = {
                        ctx.validator
                            .outer_scope(count)?
                            .instance_indexes
                            .get(LocalIdx::new(index))?
                    };
                    ctx.validator.scope_mut().instance_indexes.add(type_id);
                }
                SortType::Core(CoreSortType::Type) => {
                    let ty = {
                        ctx.validator
                            .outer_scope(count)?
                            .core_types
                            .get(LocalIdx::new(index))?
                            .clone()
                    };
                    ctx.validator.scope_mut().core_types.add(ty);
                }
                SortType::Core(CoreSortType::Module) => {
                    let ty = {
                        ctx.validator
                            .outer_scope(count)?
                            .core_modules
                            .get(LocalIdx::new(index))?
                            .clone()
                    };
                    ctx.validator.scope_mut().core_modules.add(ty);
                }
                SortType::Value => {
                    let ty = {
                        ctx.validator
                            .outer_scope(count)?
                            .value_indexes
                            .get(LocalIdx::new(index))?
                            .clone()
                    };
                    ctx.validator.scope_mut().value_indexes.add(ty);
                }
                SortType::Core(kind) => {
                    return Err(ComponentParseError::Unsupported(format!(
                        "unsupported outer alias type for core sort: {kind:?}"
                    )));
                }
            }
        }
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid alias type for instance decl: {x} ({sort:?})"
            )));
        }
    }
    Ok(())
}
