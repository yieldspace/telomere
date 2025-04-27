use crate::binary::BinaryReader;
use crate::component_model::{Binding, ComponentExport, CoreModule, CoreSortWithIdx, ExternDesc, Idx, SortWithIdx};
use crate::parser::component_model::{parse_externdesc, parse_option, parse_sort_with_idx, ComponentParseError, DefaultValidator, ParseContext, SizedResult, Validator};
use crate::parser::core::parse_name;

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>) -> SizedResult<()> {
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
                        .add_core_module(Binding::Real(CoreModule::new(None, ty.clone())))?;
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

pub fn parse_export_name_dash(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "export name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}
