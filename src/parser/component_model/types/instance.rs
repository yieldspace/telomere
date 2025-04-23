use crate::binary::BinaryReader;
use crate::component_model::{
    AliasIdx, Binding, ComponentFunction, CoreModule, CoreType, ExternDesc, InlineComponent,
    Instance, InstanceDecl, InstanceType, Reference, Type, TypeBound,
};
use crate::parser::component_model::types::{parse_export_decl, parse_type};
use crate::parser::component_model::{
    parse_alias, parse_core_type, ComponentParseError, ParseContext, SizedResult,
};
use crate::parser::core::parse_vec;

pub fn parse_instance_type(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<InstanceType> {
    let (len, decls) = parse_vec(ctx, |v| v.reader, parse_instance_decl)?;
    Ok((len, InstanceType(decls)))
}

pub fn parse_instance_decl(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<InstanceDecl> {
    _parse_instance_decl(ctx, None)
}

pub fn _parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
    byte: Option<u8>,
) -> SizedResult<InstanceDecl> {
    let start_count = ctx.reader.read_count();
    let b = match byte {
        Some(b) => b,
        None => ctx.reader.read_exact_one()?,
    };
    let d = match b {
        0x00 => {
            let (_, t) = parse_core_type(ctx)?;
            InstanceDecl::CoreType(t)
        }
        0x01 => {
            let (_, t) = parse_type(ctx)?;
            InstanceDecl::Type(t)
        }
        0x02 => {
            let (_, a) = parse_alias(ctx)?;
            // validate alias sort is in [type, instance]
            match a {
                AliasIdx::Type(_idx) => {}
                AliasIdx::Instance(_idx) => {}
                _ => {
                    return Err(ComponentParseError::InvalidSignature(format!(
                        "Invalid alias type for instance decl: {a:?}"
                    )));
                }
            }
            InstanceDecl::Alias(a)
        }
        0x04 => {
            let (_, decl) = parse_export_decl(ctx)?;
            match decl.ed.clone() {
                ExternDesc::Core(idx) => {
                    let ty = ctx.validator.get_core_type(&idx);
                    if let CoreType::ModuleType(mod_type) = ty {
                        ctx.validator
                            .add_core_module(Binding::Real(CoreModule::Typed(
                                mod_type.clone(),
                                Reference::Exported(decl.name.clone()),
                            )))?;
                    } else {
                        return Err(ComponentParseError::InvalidSignature(format!(
                            "Invalid core type for import: {ty:?}"
                        )));
                    }
                }
                ExternDesc::Func(idx) => {
                    let ty = ctx.validator.get_type(&idx);
                    if let Type::Func(func_type) = ty {
                        ctx.validator
                            .add_func(Binding::Real(ComponentFunction::Typed(
                                func_type.clone(),
                                Reference::Exported(decl.name.clone()),
                            )))?;
                    } else {
                        return Err(ComponentParseError::InvalidSignature(format!(
                            "Invalid core type for import: {ty:?}"
                        )));
                    }
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(bound) => todo!(),
                ExternDesc::Type(bound) => match bound {
                    TypeBound::Eq(idx) => {
                        ctx.validator.add_type(Binding::Real(Type::Referenced(
                            Box::new(Type::Eq(idx)),
                            Reference::Exported(decl.name.clone()),
                        )))?;
                    }
                    TypeBound::Sub => {
                        ctx.validator.add_type(Binding::Real(Type::Referenced(
                            Box::new(Type::UniqueResource),
                            Reference::Exported(decl.name.clone()),
                        )))?;
                    }
                },
                ExternDesc::Component(idx) => {
                    let ty = ctx.validator.get_type(&idx);
                    if let Type::Component(comp_type) = ty {
                        ctx.validator
                            .add_component(Binding::Real(InlineComponent::Typed(
                                comp_type.clone(),
                                Reference::Exported(decl.name.clone()),
                            )))?;
                    } else {
                        return Err(ComponentParseError::InvalidSignature(format!(
                            "Invalid core type for import: {ty:?}"
                        )));
                    }
                }
                ExternDesc::Instance(idx) => {
                    let ty = ctx.validator.get_type(&idx);
                    if let Type::Instance(inst_type) = ty {
                        ctx.validator.add_instance(Binding::Real(Instance::Typed(
                            inst_type.clone(),
                            Reference::Exported(decl.name.clone()),
                        )))?;
                    } else {
                        return Err(ComponentParseError::InvalidSignature(format!(
                            "Invalid core type for import: {ty:?}"
                        )));
                    }
                }
            }
            InstanceDecl::ExportDecl(decl)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, d))
}
