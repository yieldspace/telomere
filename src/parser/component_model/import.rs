use crate::binary::BinaryReader;
use crate::component_model::{Binding, ComponentFunction, ComponentImport, CoreModule, CoreType, ExternDesc, InlineComponent, Instance, InstanceReference, Reference, Resolvable, Resolver, Type};
use crate::parser::component_model::{parse_core_type_idx, parse_core_type_idx_resolved, parse_externdesc, parse_type_idx, parse_type_idx_resolved, ComponentParseError, DefaultValidator, ParseContext, SizedResult, Validator};
use crate::parser::core::parse_name;
use crate::runtime::component_model::instantiate::{instantiate_import_core_module, InstantiateInstr};

fn parse_externdesc_import(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<ExternDesc> {
    let start_count = ctx.reader.read_count();
    let start_count = ctx.reader.read_count();
    let desc = match ctx.reader.read_exact_one()? {
        0x00 => {
            ComponentParseError::assert_magic(
                [ctx.reader.read_exact_one()?],
                [0x00],
                "extern desc",
            )?;
            let (_, i) = parse_core_type_idx_resolved(ctx)?;
            ExternDesc::CoreModule(i.try_into()?)
        }
        0x01 => {
            let (_, i) = parse_type_idx_resolved(ctx)?;
            ExternDesc::Func(i.try_into()?)
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        0x02 => {
            let (_, b) = crate::parser::component_model::types::parse_valuebound(ctx)?;
            ExternDesc::Value(b)
        }
        0x03 => {
            // let (_, b) = crate::parser::component_model::types::parse_typebound(ctx)?;
            // ExternDesc::Type(b)
            todo!()
        }
        0x04 => {
            let (_, i) = parse_type_idx_resolved(ctx)?;
            ExternDesc::Component(i.try_into()?)
        }
        0x05 => {
            let (_, i) = parse_type_idx_resolved(ctx)?;
            ExternDesc::Instance(i.try_into()?)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, desc))
}


pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let (_, ed) = parse_externdesc(ctx)?;
    let import = match ed {
        ExternDesc::CoreModule(ty) => {
            let idx = ctx
                .validator
                .add_core_module(Binding::Real(CoreModule::new(None, ty.clone())))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_import_core_module,
            });
            ComponentImport::CoreModule(idx)
        }
        ExternDesc::Func(ty) => {
            let idx = ctx
                .validator
                .add_func(Binding::Real(ComponentFunction::new(None, ty.clone())))?;
            ComponentImport::Func(idx)
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        ExternDesc::Value(_) => todo!(),
        ExternDesc::Type(ty) => {
            // let idx = match bound {
            //     TypeBound::Eq(idx) => ctx.validator.add_type(Binding::Real(Type::Referenced(
            //         Box::new(Type::Eq(idx)),
            //         Reference::Imported(name.clone()),
            //     )))?,
            //     TypeBound::Sub => ctx.validator.add_type(Binding::Real(Type::Referenced(
            //         Box::new(Type::UniqueResource),
            //         Reference::Imported(name.clone()),
            //     )))?,
            // };
            let idx = ctx
                .validator
                .add_type(Binding::Real(Type::Referenced(
                    Box::new(ty.clone()),
                    Reference::Imported(name.clone()),
                )))?;
            ComponentImport::Type(idx)
        }
        ExternDesc::Component(ty) => {
            let idx = ctx
                .validator
                .add_component(Binding::Real(InlineComponent::new(None, ty.clone())))?;
            ComponentImport::Component(idx)
        }
        ExternDesc::Instance(ty) => {
            let idx = ctx.validator.add_instance(Binding::reference(
                Instance::new(None, ty.clone()),
                InstanceReference::Imported(name.clone()),
            ))?;
            ComponentImport::Instance(idx)
        }
    };
    ctx.validator.add_import(name, import)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_import_name_dash(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "import name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}
