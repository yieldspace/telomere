use crate::binary::BinaryReader;
use crate::component_model::{
    CoreInstance, CoreInstanceImport, CoreInstanceInlineExport, CoreInstanceInlineExportType,
    CoreInstanceType, CoreSort, GlobalIdx, Instance, InstanceType,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::id::{parse_core_instance_idx, parse_core_module_idx};
use crate::parser::component_model::core::sort::parse_core_sort;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::{parse_vec_range, ParseResult, SizedResult};
use crate::parser::core::{parse_name, parse_u32, parse_vec};
use std::collections::HashMap;
use tracing::trace;

pub fn parse_core_instance(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(CoreInstance, CoreInstanceType)> {
    trace!("parse core instance");
    let start = ctx.reader.read_count();
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_core_module_idx(ctx)?;
            let global_idx = ctx.validator.get_global_core_module(idx)?;
            let module_type = ctx.validator.get_core_module_type(idx)?;
            let mut imports = HashMap::new();
            for _ in parse_vec_range(ctx)? {
                let (_, (name, _ty, idx)) = parse_core_instantiate_arg(ctx)?;
                // todo check module_type's import

                imports.insert(name, idx);
            }
            // let imports =
            //     HashMap::from_iter(parse_vec(ctx, |c| c.reader, parse_core_instantiate_arg)?.1);
            // let idx = ctx
            //     .validator
            //     .state
            //     .add_core_instance(Binding::Real(CoreInstance::Real {
            //         module_idx: idx,
            //         imports,
            //     }))?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_core_instance,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         core_instance_idx: idx.global(),
            //     },
            // });
            let inst = CoreInstance::Real {
                module_idx: global_idx,
                imports,
            };
            let ty = CoreInstanceType::from(module_type);
            Ok((ctx.reader.read_count() - start, (inst, ty)))
        }
        0x01 => {
            let mut exports = HashMap::new();
            let mut export_types = HashMap::new();
            for _ in parse_vec_range(ctx)? {
                let (name, ty, export) = parse_core_instance_inline_export(ctx)?;
                exports.insert(name.clone(), export);
                export_types.insert(name, ty.try_into()?);
            }
            // let idx = ctx
            //     .validator
            //     .state
            //     .add_core_instance(Binding::Real(CoreInstance::Alias { exports }))?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_core_instance,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         core_instance_idx: idx.global(),
            //     },
            // });
            let inst = CoreInstance::Alias { exports };
            let ty = CoreInstanceType::new(export_types);
            Ok((ctx.reader.read_count() - start, (inst, ty)))
        }
        _ => unreachable!(),
    }
}

pub fn parse_core_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(String, InstanceType, GlobalIdx<Instance>)> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_name(ctx.reader)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x12], "instantiate arg")?;
    let instance_idx = parse_core_instance_idx(ctx)?;
    let ty = ctx.validator.get_instance_type(instance_idx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        (name, ty, ctx.validator.get_global_instance(instance_idx)?),
    ))
}

pub fn parse_core_instance_inline_export(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<(
    String,
    CoreInstanceInlineExportType,
    CoreInstanceInlineExport,
)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    let (sort_len, sort) = parse_core_sort(ctx)?;
    let (idx_len, idx) = parse_u32(ctx.reader)?;
    match sort {
        CoreSort::Func => {
            let idx = ctx.validator.validate_core_func_idx(idx)?;
            let ty = ctx.validator.get_core_func_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Func(ctx.validator.get_global_core_func(idx)?),
            ))
        }
        CoreSort::Table => {
            let idx = ctx.validator.validate_core_table_idx(idx)?;
            let ty = ctx.validator.get_core_table_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Table(ctx.validator.get_global_core_table(idx)?),
            ))
        }
        CoreSort::Memory => {
            let idx = ctx.validator.validate_core_memory_idx(idx)?;
            let ty = ctx.validator.get_core_memory_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Memory(ctx.validator.get_global_core_memory(idx)?),
            ))
        }
        CoreSort::Global => {
            let idx = ctx.validator.validate_core_global_idx(idx)?;
            let ty = ctx.validator.get_core_global_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Global(ctx.validator.get_global_core_global(idx)?),
            ))
        }
        CoreSort::Type => {
            let idx = ctx.validator.validate_core_type_idx(idx)?;
            let ty = ctx.validator.get_core_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Type(ctx.validator.get_core_type(idx)?),
            ))
        }
        CoreSort::Module => {
            let idx = ctx.validator.validate_core_module_idx(idx)?;
            let ty = ctx.validator.get_core_module_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Module(ctx.validator.get_global_core_module(idx)?),
            ))
        }
        CoreSort::Instance => {
            let idx = ctx.validator.validate_core_instance_idx(idx)?;
            let ty = ctx.validator.get_core_instance_type(idx)?;
            Ok((
                name,
                ty.into(),
                CoreInstanceInlineExport::Instance(ctx.validator.get_global_core_instance(idx)?),
            ))
        }
    }
}
