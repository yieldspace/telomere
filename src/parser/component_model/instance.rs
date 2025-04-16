use crate::binary::BinaryReader;
use crate::component_model::{
    CoreSort, CoreSortWithIdx, Idx, InlineExport, Instance, InstanceIdx, Instantiate,
    InstantiateArg, SortWithIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::{
    parse_core_func_idx, parse_core_memory_idx, parse_core_sort, parse_core_type_idx,
};
use crate::parser::component_model::idx::{
    parse_component_idx, parse_func_idx, parse_instance_idx, parse_type_idx,
};
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::SizedResult;
use crate::parser::core::{parse_name, parse_vec};
use crate::runtime::component_model::instantiate::{
    instantiate_inline_instance, instantiate_instance_end, instantiate_instance_start,
    InstantiateInstr, InstantiateOperand,
};

pub fn parse_instance(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<InstanceIdx> {
    let start_count = ctx.reader.read_count();

    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, component_idx) = parse_component_idx(ctx)?;
            let (_, args) = parse_vec(ctx, |v| v.reader, parse_instantiate_arg)?;
            let idx = ctx
                .validator
                .add_instance(Instance::Instantiate(Instantiate {
                    component_idx,
                    args,
                }))?;
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
                .add_instance(Instance::InlineExport(exports))?;
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

fn parse_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<InstantiateArg> {
    let start_count = ctx.reader.read_count();

    let (_, name) = parse_name(ctx.reader)?;
    let (_, sort) = parse_sort_with_idx(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        InstantiateArg { name, sort },
    ))
}

fn parse_inlineexport(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<InlineExport> {
    let start_count = ctx.reader.read_count();

    let (_, name) = parse_name(ctx.reader)?;
    let (_, sort) = parse_sort_with_idx(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        InlineExport { name, sort },
    ))
}

pub fn parse_sort_with_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<SortWithIdx> {
    let start_count = ctx.reader.read_count();

    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, sort) = parse_core_sort(ctx)?;
            match sort {
                CoreSort::Func => {
                    let (_, func_idx) = parse_core_func_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Func(func_idx)),
                    ))
                }
                CoreSort::Table => todo!(),
                CoreSort::Memory => {
                    let (_, memory_idx) = parse_core_memory_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Memory(memory_idx)),
                    ))
                }
                CoreSort::Global => todo!(),
                CoreSort::Type => {
                    let (_, type_idx) = parse_core_type_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Type(type_idx)),
                    ))
                }
                CoreSort::Module => todo!(),
                CoreSort::Instance => todo!(),
            }
        }
        0x01 => {
            let (_, idx) = parse_func_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Func(idx),
            ))
        }
        #[cfg(feature = "component-value")]
        0x02 => todo!(),
        0x03 => {
            let (_, idx) = parse_type_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Type(idx),
            ))
        }
        0x04 => {
            let (_, idx) = parse_component_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Component(idx),
            ))
        }
        0x05 => {
            let (_, idx) = parse_instance_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Instance(idx),
            ))
        }
        _ => unreachable!(),
    }
}
