use crate::component_model::types::{
    CoreModuleExportType, CoreSortType, InstanceExportType, SortType,
};
use crate::component_model::{CoreRelation, LocalIdx, Relation};
use crate::parser::component_model::name::parse_export_name;
use crate::parser::component_model::sort::parse_sort;
use crate::parser::component_model::{
    parse_core_instance_local_idx, parse_instance_local_idx, ComponentParseError, ParseContext,
    ParseResult,
};
use crate::runtime::component_model::instantiate::{AliasOp, InstantiateOp};
use binary_reader::BinaryReader;
use telomere_wasm::parser::core::{parse_name, parse_u32};

pub fn parse_alias(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let sort = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => parse_export_alias(ctx, sort)?,
        0x01 => {
            if let SortType::Core(cs) = sort {
                parse_core_export(ctx, cs)?;
            } else {
                return Err(ComponentParseError::InvalidSortType(
                    SortType::Core(CoreSortType::Module),
                    sort,
                ));
            }
        }
        0x02 => parse_outer_export(ctx, sort)?,
        _ => unreachable!("invalid"),
    };
    Ok(())
}

fn parse_export_alias(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: SortType,
) -> ParseResult<()> {
    let instance_lidx = parse_instance_local_idx(ctx)?;
    let instance_gidx = ctx.state.scope().instances.get(instance_lidx)?;
    let instance_type_id = ctx.validator.scope().instance_indexes.get(instance_lidx)?;
    let name = parse_export_name(ctx)?;
    let instance_type = ctx.validator.get_instance_type(instance_type_id)?;
    let export = instance_type.get_export(&name.original)?.clone();
    match sort {
        SortType::Component => {
            let InstanceExportType::Component(id) = export else {
                return Err(ComponentParseError::InvalidSignature(
                    "alias type is mismatch".into(),
                ));
            };
            let gidx = ctx
                .state
                .component_store
                .register(Relation::FromExport(instance_gidx, name.original.clone()));
            ctx.state.scope_mut().components.register(gidx);
            ctx.validator.scope_mut().component_indexes.add(id);
        }
        SortType::Func => {
            let InstanceExportType::Func(id) = export else {
                return Err(ComponentParseError::InvalidSignature(
                    "alias type is mismatch".into(),
                ));
            };
            let gidx = ctx
                .state
                .func_store
                .register(Relation::FromExport(instance_gidx, name.original.clone()));
            ctx.state.scope_mut().funcs.register(gidx);
            ctx.validator.scope_mut().func_indexes.add(id);
        }
        SortType::Type => {
            let InstanceExportType::Type(id) = export else {
                return Err(ComponentParseError::InvalidSignature(
                    "alias type is mismatch".into(),
                ));
            };
            ctx.validator.scope_mut().type_indexes.add(id);
        }
        SortType::Instance => {
            let InstanceExportType::Instance(id) = export else {
                return Err(ComponentParseError::InvalidSignature(
                    "alias type is mismatch".into(),
                ));
            };
            let gidx = ctx
                .state
                .instance_store
                .register(Relation::FromExport(instance_gidx, name.original.clone()));
            ctx.state.scope_mut().instances.register(gidx);
            ctx.state
                .scope_mut()
                .push_op(InstantiateOp::Alias(AliasOp::Instance(gidx)));
            ctx.validator.scope_mut().instance_indexes.add(id);
        }
        SortType::Core(CoreSortType::Module) => {
            let gidx = ctx
                .state
                .core_module_store
                .register(CoreRelation::FromExport(
                    instance_gidx,
                    name.original.clone(),
                ));
            ctx.state.scope_mut().core_modules.register(gidx);
            if let InstanceExportType::CoreModule(ty) = export {
                ctx.validator.scope_mut().core_modules.add(ty);
            } else {
                return Err(ComponentParseError::InvalidSignature(
                    "alais type is mismatch".into(),
                )); // todo: rewrite
            }
        }
        _ => panic!("invalid sort type"),
    }
    Ok(())
}

fn parse_core_export(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: CoreSortType,
) -> ParseResult<()> {
    let core_instance_lidx = parse_core_instance_local_idx(ctx)?;
    let core_instance_gidx = ctx.state.scope().core_instances.get(core_instance_lidx)?;
    let core_instance_type = ctx
        .validator
        .scope()
        .core_instances
        .get(core_instance_lidx)?;
    let (_, name) = parse_name(ctx.reader)?;
    let target_export_type = core_instance_type
        .exports
        .get(&name)
        .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))?
        .clone();
    match sort {
        CoreSortType::Func => {
            let gidx = ctx
                .state
                .core_func_store
                .register(CoreRelation::FromCoreExport(core_instance_gidx, name));
            ctx.state.scope_mut().core_funcs.register(gidx);
            if let CoreModuleExportType::Func(ty) = target_export_type {
                ctx.validator.scope_mut().core_funcs.add(ty);
            } else {
                return Err(ComponentParseError::InvalidSortType(
                    SortType::Core(CoreSortType::Func),
                    SortType::Core(target_export_type.into()),
                ));
            }
        }
        CoreSortType::Table => {
            let gidx = ctx
                .state
                .core_table_store
                .register(CoreRelation::FromCoreExport(core_instance_gidx, name));
            ctx.state.scope_mut().core_tables.register(gidx);
            if let CoreModuleExportType::Table(ty) = target_export_type {
                ctx.validator.scope_mut().core_tables.add(ty);
            } else {
                return Err(ComponentParseError::InvalidSortType(
                    SortType::Core(CoreSortType::Table),
                    SortType::Core(target_export_type.into()),
                ));
            }
        }
        CoreSortType::Memory => {
            let gidx = ctx
                .state
                .core_memory_store
                .register(CoreRelation::FromCoreExport(core_instance_gidx, name));
            ctx.state.scope_mut().core_memories.register(gidx);
            if let CoreModuleExportType::Memory(ty) = target_export_type {
                ctx.validator.scope_mut().core_memories.add(ty);
            } else {
                return Err(ComponentParseError::InvalidSortType(
                    SortType::Core(CoreSortType::Memory),
                    SortType::Core(target_export_type.into()),
                ));
            }
        }
        CoreSortType::Global => {
            let gidx = ctx
                .state
                .core_global_store
                .register(CoreRelation::FromCoreExport(core_instance_gidx, name));
            ctx.state.scope_mut().core_globals.register(gidx);
            if let CoreModuleExportType::Global(ty) = target_export_type {
                ctx.validator.scope_mut().core_globals.add(ty);
            } else {
                return Err(ComponentParseError::InvalidSortType(
                    SortType::Core(CoreSortType::Global),
                    SortType::Core(target_export_type.into()),
                ));
            }
        }
        CoreSortType::Type => {
            unimplemented!("core type export proposal");
        }
        CoreSortType::Module => {
            let gidx = ctx
                .state
                .core_module_store
                .register(CoreRelation::FromCoreExport(core_instance_gidx, name));
            ctx.state.scope_mut().core_modules.register(gidx);
            unimplemented!("core module export proposal")
        }
        CoreSortType::Instance => {
            let gidx = ctx
                .state
                .core_instance_store
                .register(CoreRelation::FromCoreExport(core_instance_gidx, name));
            ctx.state.scope_mut().core_instances.register(gidx);
            unimplemented!("core instance export proposal")
        }
    }
    Ok(())
}

fn parse_outer_export(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: SortType,
) -> ParseResult<()> {
    let (_, ct) = parse_u32(ctx.reader)?;
    let (_, idx) = parse_u32(ctx.reader)?;
    match sort {
        SortType::Core(CoreSortType::Module) => {
            let gidx = ctx
                .state
                .outer_scope(ct)?
                .core_modules
                .get(LocalIdx::new(idx))?;
            ctx.state.scope_mut().core_modules.register(gidx);
            let ty = ctx
                .validator
                .outer_scope(ct)
                .core_modules
                .get(LocalIdx::new(idx))?
                .clone();
            ctx.validator.scope_mut().core_modules.add(ty);
        }
        SortType::Component => {
            let gidx = ctx
                .state
                .outer_scope(ct)?
                .components
                .get(LocalIdx::new(idx))?;
            ctx.state.scope_mut().components.register(gidx);
        }
        SortType::Func => {
            let gidx = ctx.state.outer_scope(ct)?.funcs.get(LocalIdx::new(idx))?;
            ctx.state.scope_mut().funcs.register(gidx);
        }
        SortType::Type => {
            todo!() // todo(type) add type
        }
        SortType::Instance => {
            let gidx = ctx
                .state
                .outer_scope(ct)?
                .instances
                .get(LocalIdx::new(idx))?;
            ctx.state.scope_mut().instances.register(gidx);
        }
        _ => panic!("invalid sort type"),
    }
    Ok(())
}
