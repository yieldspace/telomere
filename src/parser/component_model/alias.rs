use crate::binary::BinaryReader;
use crate::component_model::{AliasIdx, Binding, ComponentFunction, ComponentIdx, CoreExportSlot, CoreExportType, CoreFuncRef, CoreFunction, CoreGlobalRef, CoreInstance, CoreInstanceExportType, CoreMemoryRef, CoreModule, CoreModuleBinding, CoreModuleIdx, CoreModuleReference, CoreSort, CoreSortWithIdx, CoreTableRef, FuncReference, Idx, InlineComponent, InlineComponentReference, Instance, InstanceExportType, InstanceIdx, InstanceReference, Resolvable, Sort, SortWithIdx, TypeIdx};
use crate::parser::component_model::validator::{DefaultValidatorState, Validator};
use crate::parser::component_model::{parse_core_instance_idx, parse_instance_idx, parse_instance_idx_resolved, parse_sort, ComponentParseError, ParseContext, ParseResult, SizedResult};
use crate::parser::core::{parse_name, parse_u32};

pub fn parse_alias(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> SizedResult<AliasIdx> {
    let start_count = ctx.reader.read_count();

    let (_, sort) = parse_sort(ctx)?;
    let idx: AliasIdx = match ctx.reader.read_exact_one()? {
        0x00 => {
            parse_export_alias(ctx, sort)?
        }
        0x01 => {
            parse_core_export(ctx, sort)?
        }
        0x02 => {
            parse_outer_export(ctx, sort)?
        }
        _ => unreachable!("invalid"),
    };
    Ok((ctx.reader.read_count() - start_count, idx))
}

fn parse_export_alias(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>, sort: Sort) -> ParseResult<AliasIdx> {
    let instance_idx = parse_instance_idx(ctx)?;
    let instance = ctx.validator.resolve_idx::<Instance>(&instance_idx)?;
    let (_, name) = parse_name(ctx.reader)?;
    let export = instance.ty.get_export_type(&name)?.clone();
    if export != sort {
        return Err(ComponentParseError::InvalidSignature(format!(
            "Invalid export type: expected {:?}, found {:?}",
            export, sort
        )));
    }
    let idx = if let Some(s) = instance.get_export(&name)? {
        match s {
            SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
                let idx = ctx.validator.state.add_core_module(
                    Binding::Alias(idx.global())
                )?;
                AliasIdx::CoreModule(idx)
            }
            SortWithIdx::Func(idx) => {
                let idx = ctx.validator.state.add_func(
                    Binding::Alias(idx.global())
                )?;
                AliasIdx::Func(idx)
            }
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            SortWithIdx::Value(_) => {}
            SortWithIdx::Type(idx) => {
                let idx = ctx.validator.state.add_type(
                    Binding::Alias(idx.global())
                )?;
                AliasIdx::Type(idx)
            }
            SortWithIdx::Component(idx) => {
                let idx = ctx.validator.state.add_component(
                    Binding::Alias(idx.global())
                )?;
                AliasIdx::Component(idx)
            }
            SortWithIdx::Instance(idx) => {
                let idx = ctx.validator.state.add_instance(
                    Binding::Alias(idx.global())
                )?;
                AliasIdx::Instance(idx)
            }
            _ => unreachable!()
        }
    } else {
        match export {
            InstanceExportType::CoreModule(ty) => {
                let idx = ctx.validator.state.add_core_module(
                    Binding::Reference(CoreModule::new(None, ty), CoreModuleReference::Alias(instance_idx, name))
                )?;
                AliasIdx::CoreModule(idx)
            }
            InstanceExportType::Func(ty) => {
                let idx = ctx.validator.state.add_func(
                    Binding::Reference(ComponentFunction::new(None, ty), FuncReference::Alias(instance_idx, name))
                )?;
                AliasIdx::Func(idx)
            }
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            InstanceExportType::Value(_) => {}
            InstanceExportType::Type(ty) => {
                let idx = ctx.validator.state.add_type(
                    Binding::Real(ty)
                )?;
                AliasIdx::Type(idx)
            }
            InstanceExportType::Component(ty) => {
                let idx = ctx.validator.state.add_component(
                    Binding::Reference(
                        InlineComponent::new(None, ty),
                        InlineComponentReference::Alias(instance_idx, name),
                    )
                )?;
                AliasIdx::Component(idx)
            }
            InstanceExportType::Instance(ty) => {
                let idx = ctx.validator.state.add_instance(
                    Binding::Reference(
                        Instance::new(None, ty),
                        InstanceReference::Alias(instance_idx, name),
                    )
                )?;
                AliasIdx::Instance(idx)
            }
        }
    };
    Ok(idx)
}

fn parse_core_export(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>, sort: Sort) -> ParseResult<AliasIdx> {
    let core_inst_idx = parse_core_instance_idx(ctx)?;
    let core_instance = ctx.validator.resolve_idx(&core_inst_idx)?;
    let (_, name) = parse_name(ctx.reader)?;
    let Sort::Core(coresort) = sort else {
        return Err(ComponentParseError::InvalidSort(sort, "Core".to_string()));
    };
    let export = core_instance.ty.get_export_type(&coresort, &name)?;
    match export {
        CoreInstanceExportType::Memory(name) => {
            let idx = ctx.validator.state.add_core_memory(
                Binding::Real(CoreMemoryRef(core_inst_idx, name))
            )?;
            Ok(AliasIdx::CoreMemory(idx))
        }
        CoreInstanceExportType::Table(name) => {
            let idx = ctx.validator.state.add_core_table(
                Binding::Real(CoreTableRef(core_inst_idx, name))
            )?;
            Ok(AliasIdx::CoreTable(idx))
        }
        CoreInstanceExportType::Func(name) => {
            let idx = ctx.validator.state.add_core_func(
                Binding::Real(CoreFunction::Export(CoreFuncRef(core_inst_idx, name)))
            )?;
            Ok(AliasIdx::CoreFunc(idx))
        }
        CoreInstanceExportType::Global(name) => {
            let idx = ctx.validator.state.add_core_global(
                Binding::Real(CoreGlobalRef(core_inst_idx, name))
            )?;
            Ok(AliasIdx::CoreGlobal(idx))
        }
    }
}

fn parse_outer_export(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>, sort: Sort) -> ParseResult<AliasIdx> {
    let (_, ct) = parse_u32(ctx.reader)?;
    let (_, idx) = parse_u32(ctx.reader)?;
    match sort {
        Sort::Core(CoreSort::Module) => {
            let idx = ctx.validator.validate_outer_idx::<CoreModuleIdx>(ct, idx)?;
            Ok(AliasIdx::CoreModule(
                ctx.validator.state.add_core_module(
                    CoreModuleBinding::Alias(idx.global())
                )?
            ))
        }
        Sort::Type => {
            let idx = ctx.validator.validate_outer_idx::<TypeIdx>(ct, idx)?;
            let ty = ctx.validator.resolve_idx(&idx)?;
            if ty.is_resource_type() {
                return Err(ComponentParseError::InvalidSignature(
                    "Outer alias type cannot be a resource type".to_string(),
                ));
            }
            let idx = ctx.validator.state.add_type(
                Binding::Alias(idx.global())
            )?;
            Ok(AliasIdx::Type(idx))
        }
        Sort::Component => {
            let idx = ctx.validator.validate_outer_idx::<ComponentIdx>(ct, idx)?;
            let idx = ctx.validator.state.add_component(
                Binding::Alias(idx.global())
            )?;
            Ok(AliasIdx::Component(idx))
        }
        Sort::Instance => {
            let idx = ctx.validator.validate_outer_idx::<InstanceIdx>(ct, idx)?;
            let idx = ctx.validator.state.add_instance(
                Binding::Alias(idx.global())
            )?;
            Ok(AliasIdx::Instance(idx))
        }
        _ => {
            Err(ComponentParseError::InvalidSort(
                sort,
                "Core Module, Type, Component or Instance".to_string(),
            ))
        }
    }
}
