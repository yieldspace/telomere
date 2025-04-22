use crate::binary::BinaryReader;
use crate::component_model::{AliasIdx, Binding, ComponentExportSlot, ComponentExportValue, ComponentFunction, CoreExportSlot, CoreModule, CoreModuleReference, CoreModuleType, CoreSortWithIdx, ExternDesc, Idx, InlineComponent, Instance, Sort, SortWithIdx, Type, TypeBound};
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::{parse_core_instance_idx, parse_instance_idx, parse_sort, ComponentParseError, ParseContext, SizedResult};
use crate::parser::core::{parse_name, parse_u32};

pub fn parse_alias(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<AliasIdx> {
    let start_count = ctx.reader.read_count();

    let (_, sort) = parse_sort(ctx)?;
    let idx: AliasIdx = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, instance_idx) = parse_instance_idx(ctx)?;
            let instance = ctx.validator.get_instance(&instance_idx);
            let (_, name) = parse_name(ctx.reader)?;
            let export = instance.get_export(&name)?;
            if export.is_none() {
                let export_type = instance.get_export_type(&name)?;
                match export_type.desc {
                    ExternDesc::Core(module_idx) => {
                        let ty: CoreModuleType = ctx.validator.get_core_type(&module_idx).clone().try_into()?;
                        AliasIdx::CoreModule(
                            ctx.validator.add_core_module(Binding::Real(
                                CoreModule::new(None, ty, Some(CoreModuleReference::Instance(instance_idx, name.clone()))),
                            ))?
                        )
                    }
                    ExternDesc::Func(idx) => {
                        let func = ctx.validator.get_type(&idx);
                        let func = func.clone().try_into()?;
                        AliasIdx::Func(ctx.validator.add_func(Binding::Real(ComponentFunction::new(None, func)))?)
                    }
                    #[cfg(feature = "component-gated-feature-value-imports-exports")]
                    ExternDesc::Value(_) => todo!(),
                    ExternDesc::Type(bound) => match bound {
                        TypeBound::Eq(idx) => {
                            AliasIdx::Type(ctx.validator.add_type(Binding::Alias(idx.global()))?)
                        }
                        TypeBound::Sub => {
                            AliasIdx::Type(ctx.validator.add_type(Binding::Real(Type::UniqueResource))?)
                        }
                    }
                    ExternDesc::Component(idx) => {
                        let component = ctx.validator.get_type(&idx);
                        let component = component.clone().try_into()?;
                        AliasIdx::Component(ctx.validator.add_component(Binding::Real(
                            InlineComponent::new(None, component)
                        ))?)
                    }
                    ExternDesc::Instance(idx) => {
                        let instance = ctx.validator.get_type(&idx);
                        let instance = instance.clone().try_into()?;
                        AliasIdx::Instance(ctx.validator.add_instance(Binding::Real(
                            Instance::new(None, instance)
                        ))?)
                    }
                }
            } else {
                let export = export.unwrap();
                match export.value {
                    None => {
                        export.ty
                    }
                    Some(value) => {
                        match value.sort {
                            SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
                                AliasIdx::CoreModule(ctx.validator.add_core_module(Binding::Alias(*idx))?)
                            }
                            SortWithIdx::Func(idx) => {
                                AliasIdx::Func(ctx.validator.add_func(Binding::Alias(*idx))?)
                            }
                            #[cfg(feature = "component-gated-feature-value-imports-exports")]
                            SortWithIdx::Value(idx) => {
                                AliasIdx::Value(ctx.validator.add_value(Binding::Alias(*idx))?)
                            }
                            SortWithIdx::Type(idx) => {
                                AliasIdx::Type(ctx.validator.add_type(Binding::Alias(*idx))?)
                            }
                            SortWithIdx::Component(idx) => {
                                AliasIdx::Component(ctx.validator.add_component(Binding::Alias(*idx))?)
                            }
                            SortWithIdx::Instance(idx) => {
                                AliasIdx::Instance(ctx.validator.add_instance(Binding::Alias(*idx))?)
                            }
                            _ => {
                                panic!()
                            }
                        }                        
                    }
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
