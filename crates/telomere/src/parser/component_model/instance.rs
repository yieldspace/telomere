use crate::binary::BinaryReader;
use crate::component_model::{
    ExportName, GlobalIdx, Instance, InstantiateArg, Relation, SortWithIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::idx::parse_component_idx;
use crate::parser::component_model::{parse_export_name, SizedResult};
use crate::parser::component_model::{parse_sort_with_idx, ComponentParseError};
use crate::parser::core::{parse_name, parse_vec};
use std::collections::HashMap;
use tracing::trace;

pub fn parse_instance(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<GlobalIdx<Instance>> {
    trace!("parse instance");
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
            trace!("parsed instance for {:?}", component);
            if args.len() != component.imports.len() {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid number of args: {}",
                    args.len()
                )));
            }
            let mut imports = HashMap::new();
            for (import_name, ty) in component.imports.iter() {
                trace!("parse_instance imports for {}", import_name);
                // todo: check ty and arg type
                let arg = args.get(import_name).expect("export not found");
                imports.insert(import_name.clone(), arg.clone().try_into()?);
            }
            let value = Instance {
                component_idx: Some(ctx.validator.get_global_component(component_idx)?),
                imports,
                exports: component.exports,
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
                imports: Default::default(),
                exports: {
                    let mut exs = HashMap::new();
                    for (name, sort) in exports {
                        exs.insert(name, sort.try_into()?);
                    }
                    exs
                },
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
    trace!("parse_instantiate_arg name: {name}");
    Ok((
        ctx.reader.read_count() - start_count,
        InstantiateArg { name, sort },
    ))
}

fn parse_inlineexport(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(ExportName, SortWithIdx)> {
    let start_count = ctx.reader.read_count();

    let (_, name) = parse_export_name(ctx)?;
    let (_, sort) = parse_sort_with_idx(ctx)?;
    Ok((ctx.reader.read_count() - start_count, (name, sort)))
}
