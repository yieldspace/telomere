use crate::binary::BinaryReader;
use crate::component_model::{
    CoreInstance, CoreInstanceImport, CoreInstanceInlineExport, CoreInstanceType, CoreSort,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::id::{parse_core_instance_idx, parse_core_module_idx};
use crate::parser::component_model::core::sort::parse_core_sort;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::SizedResult;
use crate::parser::core::{parse_name, parse_u32, parse_vec};
use std::collections::HashMap;

pub fn parse_core_instance(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(CoreInstance, CoreInstanceType)> {
    let start = ctx.reader.read_count();
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_core_module_idx(ctx)?;
            let global_idx = ctx.validator.get_global_core_module(idx)?;
            let module_type = ctx.validator.get_core_module_type(idx)?;
            let imports =
                HashMap::from_iter(parse_vec(ctx, |c| c.reader, parse_core_instantiate_arg)?.1);
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
            let ty = CoreInstanceType::from((&inst, Some(&module_type)));
            Ok((ctx.reader.read_count() - start, (inst, ty)))
        }
        0x01 => {
            let exports = HashMap::<String, CoreInstanceInlineExport>::from_iter(
                parse_vec(ctx, |c| c.reader, parse_core_instance_inline_export)?.1,
            );
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
            let ty = CoreInstanceType::from((&inst, None));
            Ok((ctx.reader.read_count() - start, (inst, ty)))
        }
        _ => unreachable!(),
    }
}

pub fn parse_core_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(String, CoreInstanceImport)> {
    let start_count = ctx.reader.read_count();
    let (name_len, name) = parse_name(ctx.reader)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x12], "instantiate arg")?;
    let instance_idx = parse_core_instance_idx(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        (
            name,
            CoreInstanceImport::Instance(ctx.validator.get_global_core_instance(instance_idx)?),
        ),
    ))
}

pub fn parse_core_instance_inline_export(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(String, CoreInstanceInlineExport)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    let (sort_len, sort) = parse_core_sort(ctx)?;
    let (idx_len, idx) = parse_u32(ctx.reader)?;
    match sort {
        CoreSort::Func => Ok((
            name_len + sort_len + idx_len,
            (
                name,
                CoreInstanceInlineExport::Func(
                    ctx.validator
                        .get_global_core_func(ctx.validator.validate_core_func_idx(idx).unwrap())?,
                ),
            ),
        )),
        CoreSort::Table => {
            Ok((
                name_len + sort_len + idx_len,
                (
                    name,
                    CoreInstanceInlineExport::Table(ctx.validator.get_global_core_table(
                        ctx.validator.validate_core_table_idx(idx).unwrap(),
                    )?),
                ),
            ))
        }
        CoreSort::Memory => Ok((
            name_len + sort_len + idx_len,
            (
                name,
                CoreInstanceInlineExport::Memory(ctx.validator.get_global_core_memory(
                    ctx.validator.validate_core_memory_idx(idx).unwrap(),
                )?),
            ),
        )),
        CoreSort::Global => Ok((
            name_len + sort_len + idx_len,
            (
                name,
                CoreInstanceInlineExport::Global(
                    ctx.validator
                        .get_global_core_global(ctx.validator.validate_core_global_idx(idx)?)?,
                ),
            ),
        )),
        CoreSort::Type => Ok((
            name_len + sort_len + idx_len,
            (
                name,
                CoreInstanceInlineExport::Type(
                    ctx.validator
                        .get_core_type(ctx.validator.validate_core_type_idx(idx)?)?,
                ),
            ),
        )),
        CoreSort::Module => Ok((
            name_len + sort_len + idx_len,
            (
                name,
                CoreInstanceInlineExport::Module(
                    ctx.validator
                        .get_global_core_module(ctx.validator.validate_core_module_idx(idx)?)?,
                ),
            ),
        )),
        CoreSort::Instance => Ok((
            name_len + sort_len + idx_len,
            (
                name,
                CoreInstanceInlineExport::Instance(
                    ctx.validator
                        .get_global_core_instance(ctx.validator.validate_core_instance_idx(idx)?)?,
                ),
            ),
        )),
    }
}
