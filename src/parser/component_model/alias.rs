use crate::binary::BinaryReader;
use crate::component_model::{
    Binding, CoreBinding, CoreFuncRef, CoreFunction, CoreGlobalRef, CoreMemoryRef, CoreSort,
    CoreSortWithIdx, CoreTableRef, Idx, Sort, SortWithIdx,
};
use crate::parser::component_model::{
    parse_core_instance_idx, parse_instance_idx, parse_sort, ParseContext, SizedResult, Validator,
};
use crate::parser::core::{parse_name, parse_u32};

pub fn parse_alias(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();

    let (_, sort) = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, instance_idx) = parse_instance_idx(ctx)?;
            let instance = ctx.validator.get_instance(&instance_idx);
            let (_, name) = parse_name(ctx.reader)?;
            let sort = instance.get_export(ctx, name, sort)?;
            match sort {
                SortWithIdx::Core(cs) => match &cs {
                    CoreSortWithIdx::Func(idx) => {
                        ctx.validator.add_core_func(Binding::Alias(idx.global()))?;
                    }
                    CoreSortWithIdx::Table(idx) => {
                        ctx.validator.add_core_table(Binding::Alias(idx.global()))?;
                    }
                    CoreSortWithIdx::Memory(idx) => {
                        ctx.validator
                            .add_core_memory(Binding::Alias(idx.global()))?;
                    }
                    CoreSortWithIdx::Global(idx) => {
                        ctx.validator
                            .add_core_global(Binding::Alias(idx.global()))?;
                    }
                    CoreSortWithIdx::Type(idx) => {
                        ctx.validator.add_core_type(Binding::Alias(idx.global()))?;
                    }
                    CoreSortWithIdx::Module(idx) => {
                        ctx.validator
                            .add_core_module(Binding::Alias(idx.global()))?;
                    }
                    CoreSortWithIdx::Instance(idx) => {
                        ctx.validator
                            .add_core_instance(Binding::Alias(idx.global()))?;
                    }
                },
                SortWithIdx::Func(idx) => {
                    ctx.validator.add_func(Binding::Alias(idx.global()))?;
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                SortWithIdx::Value(idx) => {
                    todo!()
                }
                SortWithIdx::Type(idx) => {
                    ctx.validator.add_type(Binding::Alias(idx.global()))?;
                }
                SortWithIdx::Component(idx) => {
                    ctx.validator.add_component(Binding::Alias(idx.global()))?;
                }
                SortWithIdx::Instance(idx) => {
                    ctx.validator.add_instance(Binding::Alias(idx.global()))?;
                }
            }
        }
        0x01 => {
            let (_, core_inst_idx) = parse_core_instance_idx(ctx)?;
            let (_, name) = parse_name(ctx.reader)?;
            let core_inst = ctx.validator.get_core_instance(&core_inst_idx);
            match sort {
                Sort::Core(cs) => match cs {
                    CoreSort::Func => {
                        match core_inst.get_func(ctx, name.clone()) {
                            CoreBinding::Real((idx, ty)) => {
                                ctx.validator.add_core_func(Binding::Real(
                                    CoreFunction::Export(CoreFuncRef(core_inst_idx, idx, ty, name)),
                                ))?;
                            }
                            CoreBinding::Binding(binding) => {
                                ctx.validator.add_core_func(binding)?;
                            }
                        }
                    }
                    CoreSort::Table => match core_inst.get_table(ctx, name) {
                        CoreBinding::Real(idx) => {
                            ctx.validator
                                .add_core_table(Binding::Real(CoreTableRef(core_inst_idx, idx)))?;
                        }
                        CoreBinding::Binding(binding) => {
                            ctx.validator.add_core_table(binding)?;
                        }
                    },
                    CoreSort::Memory => match core_inst.get_memory(ctx, name) {
                        CoreBinding::Real(idx) => {
                            ctx.validator.add_core_memory(Binding::Real(CoreMemoryRef(
                                core_inst_idx,
                                idx,
                            )))?;
                        }
                        CoreBinding::Binding(binding) => {
                            ctx.validator.add_core_memory(binding)?;
                        }
                    },
                    CoreSort::Global => match core_inst.get_global(ctx, name) {
                        CoreBinding::Real(idx) => {
                            ctx.validator.add_core_global(Binding::Real(CoreGlobalRef(
                                core_inst_idx,
                                idx,
                            )))?;
                        }
                        CoreBinding::Binding(binding) => {
                            ctx.validator.add_core_global(binding)?;
                        }
                    },
                    CoreSort::Type => unreachable!("export type proposal"),
                    CoreSort::Module => unreachable!("module link proposal"),
                    CoreSort::Instance => match core_inst.get_instance(name) {
                        CoreBinding::Real(_) => unreachable!("not supported yet"),
                        CoreBinding::Binding(binding) => {
                            ctx.validator.add_core_instance(binding)?;
                        }
                    },
                },
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
                    ctx.validator.add_type(Binding::Alias(type_idx.global()))?;
                }
                Sort::Component => {
                    let component_idx = target_validator.validate_component_idx(idx as usize)?;
                    ctx.validator
                        .add_component(Binding::Alias(component_idx.global()))?;
                }
                _ => unreachable!("invalid"),
            }
        }
        _ => unreachable!("invalid"),
    }
    Ok((ctx.reader.read_count() - start_count, ()))
}

fn get_outer(validator: &dyn Validator, ct: u32) -> &dyn Validator {
    if ct == 0 {
        validator
    } else {
        get_outer(validator.get_parent().unwrap(), ct - 1)
    }
}
