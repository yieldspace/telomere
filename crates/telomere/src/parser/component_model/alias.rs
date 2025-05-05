use crate::binary::BinaryReader;
use crate::component_model::{
    AliasIdx, CoreGlobalRef, CoreMemoryRef, CoreModuleExportType, CoreSort, CoreTableRef,
    ExternDesc, GlobalIdx, Relation, Sort,
};
use crate::parser::component_model::{
    parse_core_instance_idx, parse_export_name, parse_instance_idx, parse_sort,
    ComponentParseError, ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::{parse_name, parse_u32};
use tracing::trace;

pub fn parse_alias(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<AliasIdx> {
    let start_count = ctx.reader.read_count();

    let (_, sort) = parse_sort(ctx)?;
    let idx: AliasIdx = match ctx.reader.read_exact_one()? {
        0x00 => parse_export_alias(ctx, sort)?,
        0x01 => parse_core_export(ctx, sort)?,
        0x02 => parse_outer_export(ctx, sort)?,
        _ => unreachable!("invalid"),
    };
    Ok((ctx.reader.read_count() - start_count, idx))
}

fn parse_export_alias(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: Sort,
) -> ParseResult<AliasIdx> {
    trace!("parse export alias");
    let instance_idx = parse_instance_idx(ctx)?;
    let instance_global_idx = ctx.validator.get_global_instance(instance_idx)?;
    let instance = ctx.validator.get_instance_type(instance_idx)?;
    trace!("instance: {instance:?}");
    let name = parse_export_name(ctx)?;
    let export = instance.get_export_type(&name)?.clone();
    if export != sort {
        return Err(ComponentParseError::InvalidSignature(format!(
            "Invalid export type: expected {:?}, found {:?}",
            export, sort
        )));
    }
    let idx = match export {
        ExternDesc::CoreModule(ty) => {
            let idx = ctx.validator.add_core_module_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_module(global_idx, Relation::FromExport(instance_global_idx, name));
            ctx.validator.register_global_core_module(idx, global_idx)?;
            AliasIdx::CoreModule
        }
        ExternDesc::Func(ty) => {
            let idx = ctx.validator.add_func_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_func(global_idx, Relation::FromExport(instance_global_idx, name));
            ctx.validator.register_global_func(idx, global_idx)?;
            AliasIdx::Func
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        ExternDesc::Value(_) => {}
        ExternDesc::Type(ty) => {
            ctx.validator.add_type(ty)?;
            AliasIdx::Type
        }
        ExternDesc::Component(ty) => {
            let idx = ctx.validator.add_component_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_component(global_idx, Relation::FromExport(instance_global_idx, name));
            ctx.validator.register_global_component(idx, global_idx)?;
            AliasIdx::Component
        }
        ExternDesc::Instance(ty) => {
            let idx = ctx.validator.add_instance_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_instance(global_idx, Relation::FromExport(instance_global_idx, name));
            ctx.validator.register_global_instance(idx, global_idx)?;
            AliasIdx::Instance
        }
    };
    Ok(idx)
}

fn parse_core_export(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: Sort,
) -> ParseResult<AliasIdx> {
    trace!("parse core export alias");
    let core_inst_idx = parse_core_instance_idx(ctx)?;
    let core_inst_global_idx = ctx.validator.get_global_core_instance(core_inst_idx)?;
    let core_instance_type = ctx.validator.get_core_instance_type(core_inst_idx)?;
    let (_, name) = parse_name(ctx.reader)?;
    let Sort::Core(_) = sort else {
        return Err(ComponentParseError::InvalidSort(sort, "Core".to_string()));
    };
    let export = core_instance_type.get_export_type(&name)?;
    match export {
        CoreModuleExportType::Memory(ty) => {
            let idx = ctx.validator.add_core_memory_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_memory(global_idx, CoreMemoryRef(core_inst_global_idx, name));
            ctx.validator.register_global_core_memory(idx, global_idx)?;
            Ok(AliasIdx::CoreMemory)
        }
        CoreModuleExportType::Table(ty) => {
            let idx = ctx.validator.add_core_table_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_table(global_idx, CoreTableRef(core_inst_global_idx, name));
            ctx.validator.register_global_core_table(idx, global_idx)?;
            Ok(AliasIdx::CoreTable)
        }
        CoreModuleExportType::Func(ty) => {
            let idx = ctx.validator.add_core_func_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state.register_core_func(
                global_idx,
                Relation::FromCoreExport(core_inst_global_idx, name),
            );
            ctx.validator.register_global_core_func(idx, global_idx)?;
            Ok(AliasIdx::CoreFunc)
        }
        CoreModuleExportType::Global(ty) => {
            let idx = ctx.validator.add_core_global_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_global(global_idx, CoreGlobalRef(core_inst_global_idx, name));
            ctx.validator.register_global_core_global(idx, global_idx)?;
            Ok(AliasIdx::CoreGlobal)
        }
    }
}

fn parse_outer_export(
    ctx: &mut ParseContext<impl BinaryReader>,
    sort: Sort,
) -> ParseResult<AliasIdx> {
    trace!("parse outer export alias");
    let (_, ct) = parse_u32(ctx.reader)?;
    let (_, idx) = parse_u32(ctx.reader)?;
    match sort {
        Sort::Core(CoreSort::Module) => {
            let (ty, global_idx) = {
                let outer_validator = ctx.validator.get_outer(ct);
                let super_idx = outer_validator.validate_core_module_idx(idx)?;
                let super_type = outer_validator.get_core_module_type(super_idx)?;
                let super_global_idx = outer_validator.get_global_core_module(super_idx)?;
                (super_type, super_global_idx)
            };
            let idx = ctx.validator.add_core_module_type(ty)?;
            ctx.validator.register_global_core_module(idx, global_idx)?;
            Ok(AliasIdx::CoreModule)
        }
        Sort::Type => {
            let ty = {
                let outer = ctx.validator.get_outer(ct);
                let super_idx = outer.validate_core_type_idx(idx)?;

                outer.get_type(super_idx)?
            };
            if ty.is_resource_type() {
                return Err(ComponentParseError::InvalidSignature(
                    "Outer alias type cannot be a resource type".to_string(),
                ));
            }
            ctx.validator.add_type(ty)?;
            Ok(AliasIdx::Type)
        }
        Sort::Component => {
            let (ty, global_idx) = {
                let outer = ctx.validator.get_outer(ct);
                let super_idx = outer.validate_component_idx(idx)?;
                let super_type = outer.get_component_type(super_idx)?;
                let super_global_idx = outer.get_global_component(super_idx)?;
                (super_type, super_global_idx)
            };

            let idx = ctx.validator.add_component_type(ty)?;
            ctx.validator.register_global_component(idx, global_idx)?;
            Ok(AliasIdx::Component)
        }
        Sort::Instance => {
            let (ty, global_idx) = {
                let outer = ctx.validator.get_outer(ct);
                let super_idx = outer.validate_instance_idx(idx)?;
                let super_type = outer.get_instance_type(super_idx)?;
                let super_global_idx = outer.get_global_instance(super_idx)?;
                (super_type, super_global_idx)
            };
            let idx = ctx.validator.add_instance_type(ty)?;
            ctx.validator.register_global_instance(idx, global_idx)?;
            Ok(AliasIdx::Instance)
        }
        _ => Err(ComponentParseError::InvalidSort(
            sort,
            "Core Module, Type, Component or Instance".to_string(),
        )),
    }
}
