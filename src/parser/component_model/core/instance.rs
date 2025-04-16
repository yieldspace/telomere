use crate::binary::BinaryReader;
use crate::component_model::{
    CoreInstance, CoreInstanceIdx, CoreInstanceImport, CoreInstanceInlineExport, CoreSort, Idx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::id::{parse_core_instance_idx, parse_core_module_idx};
use crate::parser::component_model::core::sort::parse_core_sort;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::SizedResult;
use crate::parser::core::{parse_name, parse_u32, parse_vec};
use crate::runtime::component_model::instantiate::{
    instantiate_core_module, InstantiateInstr, InstantiateOperand,
};
use std::collections::HashMap;

pub fn parse_core_instance(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreInstanceIdx> {
    let start = ctx.reader.read_count();
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, idx) = parse_core_module_idx(ctx)?;
            let imports = HashMap::from_iter(
                parse_vec(ctx, |c| c.reader, parse_core_instantiate_arg)?
                    .1
                    .into_iter(),
            );
            let idx = ctx.validator.add_core_instance(CoreInstance::Real {
                module_idx: idx,
                imports,
            })?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_core_module,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    core_instance_idx: idx.global(),
                },
            });
            Ok((ctx.reader.read_count() - start, idx))
        }
        0x01 => {
            let exports = HashMap::<String, CoreInstanceInlineExport>::from_iter(
                parse_vec(ctx, |c| c.reader, parse_core_instance_inline_export)?
                    .1
                    .into_iter(),
            );
            let idx = ctx
                .validator
                .add_core_instance(CoreInstance::Alias { exports })?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_core_module,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    core_instance_idx: idx.global(),
                },
            });
            Ok((ctx.reader.read_count() - start, idx))
        }
        _ => todo!(),
    }
}

pub fn parse_core_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<(String, CoreInstanceImport)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x12], "instantiate arg")?;
    let (idx_len, instance_idx) = parse_core_instance_idx(ctx)?;
    Ok((
        name_len + 1 + idx_len,
        (name, CoreInstanceImport::Instance(instance_idx)),
    ))
}

pub fn parse_core_instance_inline_export(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
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
                    ctx.validator.validate_core_function_idx(idx as usize)?,
                ),
            ),
        )),
        _ => todo!(),
    }
}
