use crate::binary::BinaryReader;
use crate::component_model::{
    Binding, Idx, InlineExport, Instance, InstanceIdx, Instantiate, InstantiateArg,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::idx::parse_component_idx;
use crate::parser::component_model::parse_sort_with_idx;
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::SizedResult;
use crate::parser::core::{parse_name, parse_vec};
use crate::runtime::component_model::instantiate::{
    instantiate_inline_instance, instantiate_instance_end, instantiate_instance_start,
    InstantiateInstr, InstantiateOperand,
};

pub fn parse_instance(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<InstanceIdx> {
    let start_count = ctx.reader.read_count();

    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, component_idx) = parse_component_idx(ctx)?;
            let (_, args) = parse_vec(ctx, |v| v.reader, parse_instantiate_arg)?;
            let idx = ctx
                .validator
                .add_instance(Binding::Real(Instance::Instantiate(Instantiate {
                    component_idx,
                    args,
                })))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_instance_start,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    instance_idx: idx.global(),
                },
            });
            let instrs = ctx.validator.get_component(&component_idx).instrs.clone();
            ctx.extend_instr(instrs.into_iter());
            ctx.push_instr(InstantiateInstr {
                op: instantiate_instance_end,
            });
            Ok((ctx.reader.read_count() - start_count, idx))
        }
        0x01 => {
            let (_, exports) = parse_vec(ctx, |v| v.reader, parse_inlineexport)?;
            let idx = ctx
                .validator
                .add_instance(Binding::Real(Instance::InlineExport(exports)))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_inline_instance,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    instance_idx: idx.global(),
                },
            });
            Ok((ctx.reader.read_count() - start_count, idx))
        }
        _ => unreachable!(),
    }
}

fn parse_instantiate_arg(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<InstantiateArg> {
    let start_count = ctx.reader.read_count();

    let (_, name) = parse_name(ctx.reader)?;
    let (_, sort) = parse_sort_with_idx(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        InstantiateArg { name, sort },
    ))
}

fn parse_inlineexport(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<InlineExport> {
    let start_count = ctx.reader.read_count();

    let (_, name) = parse_name(ctx.reader)?;
    let (_, sort) = parse_sort_with_idx(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        InlineExport { name, sort },
    ))
}
