use crate::binary::BinaryReader;
use crate::component_model::types::{CoreInstanceType, CoreSortType, InstanceType};
use crate::component_model::{
    CoreInstance, CoreInstanceInlineExport, CoreRelation, CoreSort, GlobalIdx, Instance, LocalIdx,
    Relation,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::{
    parse_core_instance_local_idx, parse_core_module_local_idx, parse_core_sort_type,
    parse_vec_range, ParseResult, SizedResult,
};
use crate::parser::core::{parse_name, parse_u32};
use std::collections::HashMap;
use tracing::trace;

pub fn parse_core_instance(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse core instance");
    let inst = match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_core_module_local_idx(ctx)?;
            let module_gidx = ctx.state.scope().core_modules.get(idx)?;
            let mut imports = HashMap::new();
            for _ in parse_vec_range(ctx)? {
                let (_, (name, _ty, idx)) = parse_core_instantiate_arg(ctx)?;
                // todo(type) check module_type's import

                imports.insert(name, idx);
            }
            CoreInstance::Defined {
                module_idx: module_gidx,
                imports,
            }
        }
        0x01 => {
            let mut exports = HashMap::new();
            for _ in parse_vec_range(ctx)? {
                let (name, export) = parse_core_instance_inline_export(ctx)?;
                exports.insert(name.clone(), export);
            }
            CoreInstance::InlineExport { exports }
        }
        _ => unreachable!(),
    };
    let instance_gidx = ctx
        .state
        .core_instance_store
        .register(CoreRelation::Defined(inst));
    ctx.state.scope_mut().core_instances.register(instance_gidx);
    // todo(type) register type
    Ok(())
}

pub fn parse_core_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(String, CoreInstanceType, GlobalIdx<CoreInstance>)> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_name(ctx.reader)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x12], "instantiate arg")?;
    let instance_idx = parse_core_instance_local_idx(ctx)?;
    let ty = CoreInstanceType {
        exports: Default::default(),
    }; // todo(type) get type
    Ok((
        ctx.reader.read_count() - start_count,
        (
            name,
            ty,
            ctx.state.scope().core_instances.get(instance_idx)?,
        ),
    ))
}

pub fn parse_core_instance_inline_export(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<(String, CoreInstanceInlineExport)> {
    let (_, name) = parse_name(ctx.reader)?;
    let sort = parse_core_sort_type(ctx)?;
    let (_, idx) = parse_u32(ctx.reader)?;
    match sort {
        CoreSortType::Func => {
            let idx = ctx.state.scope().core_funcs.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Func(idx)))
        }
        CoreSortType::Table => {
            let idx = ctx.state.scope().core_tables.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Table(idx)))
        }
        CoreSortType::Memory => {
            let idx = ctx.state.scope().core_memories.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Memory(idx)))
        }
        CoreSortType::Global => {
            let idx = ctx.state.scope().core_globals.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Global(idx)))
        }
        CoreSortType::Type => {
            let idx = ctx.state.scope().core_types.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Type(idx)))
        }
        CoreSortType::Module => {
            let idx = ctx.state.scope().core_modules.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Module(idx)))
        }
        CoreSortType::Instance => {
            let idx = ctx.state.scope().core_instances.get(LocalIdx::new(idx))?;
            Ok((name, CoreInstanceInlineExport::Instance(idx)))
        }
    }
}
