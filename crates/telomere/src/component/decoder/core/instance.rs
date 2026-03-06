use crate::binary::BinaryReader;
use crate::component::decoder::context::ParseContext;
use crate::component::decoder::error::ComponentParseError;
use crate::component::decoder::{
    parse_core_instance_local_idx, parse_core_module_local_idx, parse_core_sort, parse_vec_range,
    ParseResult, SizedResult,
};
use crate::component::ir::types::{CoreInstanceType, CoreModuleExportType, CoreSortType};
use crate::component::ir::{
    CoreInstance, CoreInstanceInlineExport, CoreRelation, GlobalIdx, LocalIdx,
};
use crate::parser::core::{parse_name, parse_u32};
use std::collections::HashMap;
use tracing::trace;

pub fn parse_core_instance(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse core instance");
    let (inst_ty, inst) = match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_core_module_local_idx(ctx)?;
            let module_gidx = ctx.state.scope().core_modules.get(idx)?;
            let module_ty = ctx.validator.scope().core_modules.get(idx)?.clone();
            let mut imports = HashMap::new();
            for _ in parse_vec_range(ctx)? {
                let (_, (name, _ty, idx)) = parse_core_instantiate_arg(ctx)?;
                // todo(type) maybe? check module_type's import

                imports.insert(name, idx);
            }
            (
                CoreInstanceType::from(module_ty),
                CoreInstance::Defined {
                    module_idx: module_gidx,
                    imports,
                },
            )
        }
        0x01 => {
            let mut exports = HashMap::new();
            let mut export_types = HashMap::new();
            for _ in parse_vec_range(ctx)? {
                let (name, ty, export) = parse_core_instance_inline_export(ctx)?;
                exports.insert(name.clone(), export);
                export_types.insert(name, ty);
            }
            (
                CoreInstanceType {
                    exports: export_types,
                },
                CoreInstance::InlineExport { exports },
            )
        }
        _ => unreachable!(),
    };
    let instance_gidx = ctx
        .state
        .core_instance_store
        .register(CoreRelation::Defined(inst));
    ctx.state.scope_mut().core_instances.register(instance_gidx);
    ctx.validator.scope_mut().core_instances.add(inst_ty);
    Ok(())
}

pub fn parse_core_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(String, CoreInstanceType, GlobalIdx<CoreInstance>)> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_name(ctx.reader)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x12], "instantiate arg")?;
    let instance_idx = parse_core_instance_local_idx(ctx)?;
    let ty = ctx
        .validator
        .scope()
        .core_instances
        .get(instance_idx)?
        .clone();
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
) -> ParseResult<(String, CoreModuleExportType, CoreInstanceInlineExport)> {
    let (_, name) = parse_name(ctx.reader)?;
    let sort = parse_core_sort(ctx)?;
    let (_, idx) = parse_u32(ctx.reader)?;
    match sort {
        CoreSortType::Func => {
            let ty = ctx
                .validator
                .scope()
                .core_funcs
                .get(LocalIdx::new(idx))?
                .clone();
            let idx = ctx.state.scope().core_funcs.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Func(idx)))
        }
        CoreSortType::Table => {
            let ty = *ctx.validator.scope().core_tables.get(LocalIdx::new(idx))?;
            let idx = ctx.state.scope().core_tables.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Table(idx)))
        }
        CoreSortType::Memory => {
            let ty = *ctx
                .validator
                .scope()
                .core_memories
                .get(LocalIdx::new(idx))?;
            let idx = ctx.state.scope().core_memories.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Memory(idx)))
        }
        CoreSortType::Global => {
            let ty = *ctx.validator.scope().core_globals.get(LocalIdx::new(idx))?;
            let idx = ctx.state.scope().core_globals.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Global(idx)))
        }
        CoreSortType::Type => {
            let ty = ctx
                .validator
                .scope()
                .core_types
                .get(LocalIdx::new(idx))?
                .clone();
            let idx = ctx.state.scope().core_types.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Type(idx)))
        }
        CoreSortType::Module => {
            let ty = ctx
                .validator
                .scope()
                .core_modules
                .get(LocalIdx::new(idx))?
                .clone();
            let idx = ctx.state.scope().core_modules.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Module(idx)))
        }
        CoreSortType::Instance => {
            let ty = ctx
                .validator
                .scope()
                .core_instances
                .get(LocalIdx::new(idx))?
                .clone();
            let idx = ctx.state.scope().core_instances.get(LocalIdx::new(idx))?;
            Ok((name, ty.into(), CoreInstanceInlineExport::Instance(idx)))
        }
    }
}
