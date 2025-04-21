use crate::binary::BinaryReader;
use crate::component_model::{
    Binding, ComponentExport, ComponentFunction, ComponentImport, CoreModule, CoreSortWithIdx,
    CoreType, ExternDesc, Idx, InlineComponent, Instance, Reference, SortWithIdx, Type, TypeBound,
};
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{
    parse_option, parse_sort_with_idx, ComponentParseError, ParseContext, SizedResult,
};
use crate::parser::core::parse_name;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let (_, ed) = parse_externdesc(ctx)?;
    let import = match ed {
        ExternDesc::Core(idx) => {
            let ty = ctx.validator.get_core_type(&idx);
            if let CoreType::ModuleType(mod_type) = ty {
                let idx = ctx
                    .validator
                    .add_core_module(Binding::Real(CoreModule::Typed(
                        mod_type.clone(),
                        Reference::Imported(name.clone()),
                    )))?;
                ComponentImport::CoreModule(name, idx)
            } else {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid core type for import: {ty:?}"
                )));
            }
        }
        ExternDesc::Func(idx) => {
            let ty = ctx.validator.get_type(&idx);
            if let Type::Func(func_type) = ty {
                let idx = ctx
                    .validator
                    .add_func(Binding::Real(ComponentFunction::Typed(
                        func_type.clone(),
                        Reference::Imported(name.clone()),
                    )))?;
                ComponentImport::Func(name, idx)
            } else {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid core type for import: {ty:?}"
                )));
            }
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        ExternDesc::Value(_) => todo!(),
        ExternDesc::Type(bound) => {
            let idx = match bound {
                TypeBound::Eq(idx) => ctx.validator.add_type(Binding::Real(Type::Referenced(
                    Box::new(Type::Eq(idx)),
                    Reference::Imported(name.clone()),
                )))?,
                TypeBound::Sub => ctx.validator.add_type(Binding::Real(Type::Referenced(
                    Box::new(Type::UniqueResource),
                    Reference::Imported(name.clone()),
                )))?,
            };
            ComponentImport::Type(name, idx)
        }
        ExternDesc::Component(idx) => {
            let ty = ctx.validator.get_type(&idx);
            if let Type::Component(comp_type) = ty {
                let idx = ctx
                    .validator
                    .add_component(Binding::Real(InlineComponent::Typed(
                        comp_type.clone(),
                        Reference::Imported(name.clone()),
                    )))?;
                ComponentImport::Component(name, idx)
            } else {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid core type for import: {ty:?}"
                )));
            }
        }
        ExternDesc::Instance(idx) => {
            let ty = ctx.validator.get_type(&idx);
            if let Type::Instance(inst_type) = ty {
                let idx = ctx.validator.add_instance(Binding::Real(Instance::Typed(
                    inst_type.clone(),
                    Reference::Imported(name.clone()),
                )))?;
                ComponentImport::Instance(name, idx)
            } else {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid core type for import: {ty:?}"
                )));
            }
        }
    };
    ctx.validator.add_import(import)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_export_name_dash(ctx)?;
    let (_, si) = parse_sort_with_idx(ctx)?;
    let (_, ed) = parse_option(ctx, parse_externdesc)?;
    match si {
        SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
            if ed.is_some() {
                if let ExternDesc::Core(type_idx) = ed.clone().unwrap() {
                    if let CoreType::ModuleType(ty) = ctx.validator.get_core_type(&type_idx) {
                        ctx.validator
                            .add_core_module(Binding::Real(CoreModule::SuperTyped(
                                ty.clone(),
                                idx.clone(),
                                Reference::Exported(name.clone()),
                            )))?;
                    } else {
                        return Err(ComponentParseError::InvalidSignature(format!(
                            "Invalid core type for import: {si:?}"
                        )));
                    }
                } else {
                    return Err(ComponentParseError::InvalidSignature(format!(
                        "Invalid core type for import: {si:?}"
                    )));
                }
            } else {
                ctx.validator
                    .add_core_module(Binding::Alias(idx.global()))?;
            }
        }
        SortWithIdx::Func(idx) => {
            ctx.validator.add_func(Binding::Alias(idx.global()))?;
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        SortWithIdx::Value(_) => todo!(),
        SortWithIdx::Type(idx) => {
            ctx.validator.add_type(Binding::Alias(idx.global()))?;
        }
        SortWithIdx::Component(idx) => {
            ctx.validator.add_component(Binding::Alias(idx.global()))?;
        }
        SortWithIdx::Instance(idx) => {
            ctx.validator.add_instance(Binding::Alias(idx.global()))?;
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid core type for import: {si:?}"
            )));
        }
    }
    let export = ComponentExport {
        name,
        sort: si,
        desc: ed,
    };
    ctx.validator.add_export(export)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_import_name_dash(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "import name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}

pub fn parse_export_name_dash(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "export name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}
