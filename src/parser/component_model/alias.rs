use crate::binary::BinaryReader;
use crate::component_model::{AliasIdx, Binding, ComponentExportSlot, CoreExportSlot, Idx, Sort};
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::{
    parse_core_instance_idx, parse_instance_idx, parse_sort, ParseContext, SizedResult,
};
use crate::parser::core::{parse_name, parse_u32};

pub fn parse_alias(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<AliasIdx> {
    let start_count = ctx.reader.read_count();

    let (_, sort) = parse_sort(ctx)?;
    let idx: AliasIdx = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, instance_idx) = parse_instance_idx(ctx)?;
            let instance = ctx.validator.get_instance(&instance_idx);
            let (_, name) = parse_name(ctx.reader)?;
            let export = instance.get_export(ctx, instance_idx, name, sort)?;
            match export {
                ComponentExportSlot::CoreModule(slot) => {
                    AliasIdx::CoreModule(ctx.validator.add_core_module(slot.into())?)
                }
                ComponentExportSlot::Func(slot) => {
                    AliasIdx::Func(ctx.validator.add_func(slot.into())?)
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ComponentExportSlot::Value => todo!(),
                ComponentExportSlot::Type(slot) => {
                    AliasIdx::Type(ctx.validator.add_type(slot.into())?)
                }
                ComponentExportSlot::Component(slot) => {
                    AliasIdx::Component(ctx.validator.add_component(slot.into())?)
                }
                ComponentExportSlot::Instance(slot) => {
                    AliasIdx::Instance(ctx.validator.add_instance(slot.into())?)
                }
            }
        }
        0x01 => {
            let (_, core_inst_idx) = parse_core_instance_idx(ctx)?;
            let (_, name) = parse_name(ctx.reader)?;
            let core_inst = ctx.validator.get_core_instance(&core_inst_idx);
            match sort {
                Sort::Core(cs) => {
                    let export =
                        core_inst.get_export(ctx.validator, core_inst_idx, cs, name.clone())?;
                    match export {
                        CoreExportSlot::Func(slot, _) => {
                            AliasIdx::CoreFunc(ctx.validator.add_core_func(slot.into())?)
                        }
                        CoreExportSlot::Table(slot, _) => {
                            AliasIdx::CoreTable(ctx.validator.add_core_table(slot.into())?)
                        }
                        CoreExportSlot::Memory(slot, _) => {
                            AliasIdx::CoreMemory(ctx.validator.add_core_memory(slot.into())?)
                        }
                        CoreExportSlot::Global(slot, _) => {
                            AliasIdx::CoreGlobal(ctx.validator.add_core_global(slot.into())?)
                        }
                        CoreExportSlot::Type(slot, _) => {
                            AliasIdx::CoreType(ctx.validator.add_core_type(slot.into())?)
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        0x02 => {
            let (_, ct) = parse_u32(ctx.reader)?;
            let (_, idx) = parse_u32(ctx.reader)?;
            let target_validator = get_outer(ctx.validator, ct);
            // question: module?
            match sort {
                Sort::Type => {
                    let type_idx = target_validator.validate_type_idx(idx as usize)?;
                    AliasIdx::Type(ctx.validator.add_type(Binding::Alias(type_idx.global()))?)
                }
                Sort::Component => {
                    let component_idx = target_validator.validate_component_idx(idx as usize)?;
                    AliasIdx::Component(
                        ctx.validator
                            .add_component(Binding::Alias(component_idx.global()))?,
                    )
                }
                _ => unreachable!("invalid"),
            }
        }
        _ => unreachable!("invalid"),
    };
    Ok((ctx.reader.read_count() - start_count, idx))
}

fn get_outer(validator: &dyn Validator, ct: u32) -> &dyn Validator {
    if ct == 0 {
        validator
    } else {
        get_outer(validator.get_parent().unwrap(), ct - 1)
    }
}
