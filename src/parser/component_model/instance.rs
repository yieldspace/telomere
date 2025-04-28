use crate::binary::BinaryReader;
use crate::component_model::{
    GlobalIdx, Idx, InlineExport, Instance, InstantiateArg, Relation,
    SortWithIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::idx::parse_component_idx;
use crate::parser::component_model::SizedResult;
use crate::parser::component_model::{parse_sort_with_idx, ComponentParseError};
use crate::parser::core::{parse_name, parse_vec};
use std::collections::HashMap;

pub fn parse_instance(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<GlobalIdx<Instance>> {
    let start_count = ctx.reader.read_count();

    match ctx.reader.read_exact_one()? {
        0x00 => {
            let component_idx = parse_component_idx(ctx)?;
            let (_, args) = parse_vec(ctx, |v| v.reader, parse_instantiate_arg)?;
            let args = args
                .into_iter()
                .map(|InstantiateArg { name, sort }| (name, sort))
                .collect::<HashMap<String, SortWithIdx>>();
            let component = ctx.validator.get_component_type(component_idx)?;
            if args.len() != component.imports.len() {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid number of args: {}",
                    args.len()
                )));
            }
            let mut exports = HashMap::new();
            for (export_name, ty) in component.exports.iter() {
                // todo: check ty and arg type
                let arg = args.get(export_name).expect("export not found");
                exports.insert(export_name.clone(), arg.clone());
            }
            let value = Instance {
                component_idx: Some(ctx.validator.get_global_component(component_idx)?),
                args,
                exports,
            };
            let ty = value.as_type();
            let idx = ctx.validator.add_instance_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_instance(global_idx, Relation::Defined(value));
            ctx.validator.register_global_instance(idx, global_idx)?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_instance_start,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         instance_idx: idx.global(),
            //     },
            // });
            // todo: instantiateしている間にやる
            // let instrs = ctx.validator.get_component(&component_idx).instrs.clone();
            // ctx.extend_instr(instrs.into_iter());
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_instance_end,
            // });
            Ok((ctx.reader.read_count() - start_count, global_idx))
        }
        0x01 => {
            let (_, exports) = parse_vec(ctx, |v| v.reader, parse_inlineexport)?;
            let value = Instance {
                component_idx: None,
                args: Default::default(),
                exports: exports
                    .into_iter()
                    .map(|InlineExport { name, sort }| (name, sort))
                    .collect(),
            };
            let ty = value.as_type();
            let idx = ctx.validator.add_instance_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_instance(global_idx, Relation::Defined(value));
            ctx.validator.register_global_instance(idx, global_idx)?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_inline_instance,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         instance_idx: idx.global(),
            //     },
            // });
            Ok((ctx.reader.read_count() - start_count, global_idx))
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
