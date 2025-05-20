use crate::binary::BinaryReader;
use crate::component_model::types::{
    ComponentType, CoreSortType, FuncType, InstanceType, SortType, Type,
};
use crate::component_model::{CoreRelation, LocalIdx, PlaceholderId, Relation};
use crate::parser::component_model::name::parse_export_name;
use crate::parser::component_model::sort::parse_sort;
use crate::parser::component_model::{
    parse_core_instance_local_idx, parse_instance_local_idx, ComponentParseError, ParseContext,
    ParseResult,
};
use crate::parser::core::{parse_name, parse_u32};

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
    let name = parse_export_name(ctx)?;
    match sort {
        SortType::Component => {
            let ty = ctx.validator.new_type(Type::Component(ComponentType {
                imports: Default::default(),
                exports: Default::default(),
            })); // todo(type) get type from instance export
            let gidx = ctx.state.component_store.register(Relation::FromExport(
                instance_gidx,
                PlaceholderId::new(&name),
            ));
            ctx.state.scope_mut().components.register(gidx);
            ctx.validator.scope_mut().component_indexes.add(ty);
        }
        SortType::Func => {
            let ty = ctx.validator.new_type(Type::Func(FuncType {
                params: vec![],
                result: None,
            })); // todo(type) get type from instance export
            let gidx = ctx.state.func_store.register(Relation::FromExport(
                instance_gidx,
                PlaceholderId::new(&name),
            ));
            ctx.state.scope_mut().funcs.register(gidx);
            ctx.validator.scope_mut().func_indexes.add(ty);
        }
        SortType::Type => {
            let ty = todo!(); // todo(type) get type from instance export
            ctx.validator.scope_mut().type_indexes.add(ty);
        }
        SortType::Instance => {
            let ty = ctx.validator.new_type(Type::Instance(InstanceType {
                exports: Default::default(),
            })); // todo(type) get type from instance export
            let gidx = ctx.state.instance_store.register(Relation::FromExport(
                instance_gidx,
                PlaceholderId::new(&name),
            ));
            ctx.state.scope_mut().instances.register(gidx);
            ctx.validator.scope_mut().instance_indexes.add(ty);
        }
        SortType::Core(CoreSortType::Module) => {
            let gidx = ctx
                .state
                .core_module_store
                .register(CoreRelation::FromExport(
                    instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_modules.register(gidx);
            // todo(type) add type
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
    let (_, name) = parse_name(ctx.reader)?;
    match sort {
        CoreSortType::Func => {
            let gidx = ctx
                .state
                .core_func_store
                .register(CoreRelation::FromCoreExport(
                    core_instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_funcs.register(gidx);
        }
        CoreSortType::Table => {
            let gidx = ctx
                .state
                .core_table_store
                .register(CoreRelation::FromCoreExport(
                    core_instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_tables.register(gidx);
        }
        CoreSortType::Memory => {
            let gidx = ctx
                .state
                .core_memory_store
                .register(CoreRelation::FromCoreExport(
                    core_instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_memories.register(gidx);
        }
        CoreSortType::Global => {
            let gidx = ctx
                .state
                .core_global_store
                .register(CoreRelation::FromCoreExport(
                    core_instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_globals.register(gidx);
        }
        CoreSortType::Type => {
            todo!(); // todo(type) add type
        }
        CoreSortType::Module => {
            let gidx = ctx
                .state
                .core_module_store
                .register(CoreRelation::FromCoreExport(
                    core_instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_modules.register(gidx);
        }
        CoreSortType::Instance => {
            let gidx = ctx
                .state
                .core_instance_store
                .register(CoreRelation::FromCoreExport(
                    core_instance_gidx,
                    PlaceholderId::new(&name),
                ));
            ctx.state.scope_mut().core_instances.register(gidx);
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
