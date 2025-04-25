use crate::binary::BinaryReader;
use crate::component_model::{Binding, ComponentExport, ComponentFunction, ComponentImport, CoreModule, CoreSortWithIdx, CoreType, ExternDesc, Idx, InlineComponent, Instance, InstanceReference, LazyValue, Reference, SortWithIdx, Type, TypeBound};
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{parse_option, parse_sort_with_idx, ComponentParseError, ParseContext, SizedResult, Validator};
use crate::parser::core::parse_name;
use crate::runtime::component_model::instantiate::{instantiate_import_core_module, InstantiateInstr};

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let (_, ed) = parse_externdesc(ctx)?;
    let import = match ed {
        ExternDesc::CoreModule(ty) => {
            let idx = ctx
                .validator
                .add_core_module(Binding::Real(CoreModule::new(
                    None,
                    ty.clone(),
                )))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_import_core_module,
            });
            ComponentImport::CoreModule(idx)
        }
        ExternDesc::Func(ty) => {
            let idx = ctx
                .validator
                .add_func(Binding::Real(ComponentFunction::new(
                    None,
                    ty.clone(),
                )))?;
            ComponentImport::Func(idx)
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
            ComponentImport::Type(idx)
        }
        ExternDesc::Component(ty) => {
            let idx = ctx
                .validator
                .add_component(Binding::Real(InlineComponent::new(None, ty.clone())))?;
            ComponentImport::Component(idx)
        }
        ExternDesc::Instance(ty) => {
            let idx = ctx
                .validator
                .add_instance(Binding::reference(
                    Instance::new(None, ty.clone()),
                    InstanceReference::Imported(name.clone()),
                ))?;
            ComponentImport::Instance(idx)
        }
    };
    ctx.validator.add_import(name, import)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_export_name_dash(ctx)?;
    let (_, si) = parse_sort_with_idx(ctx)?;
    let (_, ed) = parse_option(ctx, parse_externdesc)?;
    let sort = match si {
        SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
            if ed.is_some() {
                if let ExternDesc::CoreModule(ty) = ed.clone().unwrap() {
                    let idx = ctx
                        .validator
                        .add_core_module(Binding::Real(CoreModule::new(
                            None,
                            ty.clone(),
                        )))?;
                    SortWithIdx::Core(CoreSortWithIdx::Module(idx))
                } else {
                    return Err(ComponentParseError::InvalidSignature(format!(
                        "Invalid core type for import: {si:?}"
                    )));
                }
            } else {
                let idx = ctx
                    .validator
                    .add_core_module(Binding::Alias(idx.global()))?;
                SortWithIdx::Core(CoreSortWithIdx::Module(idx))
            }
        }
        SortWithIdx::Func(idx) => {
            let idx = ctx.validator.add_func(Binding::Alias(idx.global()))?;
            SortWithIdx::Func(idx)
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        SortWithIdx::Value(_) => todo!(),
        SortWithIdx::Type(idx) => {
            let idx = ctx.validator.add_type(Binding::Alias(idx.global()))?;
            SortWithIdx::Type(idx)
        }
        SortWithIdx::Component(idx) => {
            let idx = ctx.validator.add_component(Binding::Alias(idx.global()))?;
            SortWithIdx::Component(idx)
        }
        SortWithIdx::Instance(idx) => {
            let idx = ctx.validator.add_instance(Binding::Alias(idx.global()))?;
            SortWithIdx::Instance(idx)
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid core type for import: {si:?}"
            )));
        }
    };
    let export = ComponentExport { sort: si, desc: ed };
    ctx.validator.add_export(name, export)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_import_name_dash(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "import name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}

pub fn parse_export_name_dash(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "export name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}
