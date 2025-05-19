use crate::binary::BinaryReader;
use crate::component_model::types::{ComponentType, FuncType, InstanceType, SortType, Type};
use crate::component_model::{PlaceholderId, Relation};
use crate::parser::component_model::name::parse_export_name;
use crate::parser::component_model::sort::parse_sort;
use crate::parser::component_model::{parse_instance_local_idx, ParseContext, ParseResult};

pub fn parse_alias(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let sort = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => parse_export_alias(ctx, sort)?,
        0x01 => parse_core_export(ctx, sort)?,
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
    }
    Ok(())
}

fn parse_core_export(ctx: &mut ParseContext<impl BinaryReader>, sort: SortType) -> ParseResult<()> {
    Ok(())
}

fn parse_outer_export(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: SortType,
) -> ParseResult<()> {
    Ok(())
}
