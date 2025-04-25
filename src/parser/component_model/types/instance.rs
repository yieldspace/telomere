use crate::binary::BinaryReader;
use crate::component_model::{
    AliasIdx, AliasType, Binding, ComponentFunction, CoreModule, CoreModuleReference,
    CoreModuleType, CoreType, ExportDecl, ExternDesc, InlineComponent, Instance, InstanceDecl,
    InstanceExportType, InstanceType, Reference, Resolvable, Type, TypeBound,
};
use crate::parser::component_model::types::alias::parse_alias_type;
use crate::parser::component_model::types::validator::TypeValidator;
use crate::parser::component_model::types::{parse_export_decl, parse_type};
use crate::parser::component_model::{
    parse_alias, parse_core_type, parse_vec_range, ComponentParseError, ParseContext, SizedResult,
    Validator,
};
use crate::parser::core::parse_vec;

pub fn parse_instance_type(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<InstanceType> {
    for _ in parse_vec_range(ctx)? {
        let (_, decl) = parse_instance_decl(ctx)?;
        match decl {
            InstanceDecl::CoreModuleType(ty) => {
                // type_validator.add_core_type(CoreType::ModuleType(ty));
            }
            InstanceDecl::Type(ty) => {
                // type_validator.add_type(ty.clone());
            }
            InstanceDecl::Alias(ty) => match ty {
                AliasType::Type(ty) => {
                    // type_validator.add_type(ty.clone());
                }
                AliasType::Instance(ty) => {
                    // type_validator.add_instance_type(ty);
                }
            },
            InstanceDecl::ExportDecl(ExportDecl { name, ed }) => match ed {
                ExternDesc::CoreModule(ty) => {}
                ExternDesc::Func(_) => {}
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => {}
                ExternDesc::Type(_) => {}
                ExternDesc::Component(_) => {}
                ExternDesc::Instance(_) => {}
            },
        }
    }
    let (len, decls) = parse_vec(ctx, |v| v.reader, parse_instance_decl)?;
    let mut inst_type = InstanceType::new();
    for decl in decls {
        match decl {
            InstanceDecl::CoreModuleType(ty) => {
                inst_type.core_types.push(CoreType::ModuleType(ty));
            }
            InstanceDecl::Type(ty) => {
                inst_type.types.push(ty);
            }
            InstanceDecl::Alias(ty) => match ty {
                AliasType::Type(ty) => {}
                AliasType::Instance(_) => {}
            },
            InstanceDecl::ExportDecl(decl) => match &decl.ed {
                ExternDesc::CoreModule(ty) => {
                    inst_type.exports.insert(
                        decl.name.clone(),
                        InstanceExportType::CoreModule(ty.clone()),
                    );
                }
                ExternDesc::Func(ty) => {
                    inst_type
                        .exports
                        .insert(decl.name.clone(), InstanceExportType::Func(ty.clone()));
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => todo!(),
                ExternDesc::Type(bound) => match bound {
                    TypeBound::Eq(idx) => {
                        let ty = idx.resolve(ctx.validator)?;
                        inst_type
                            .exports
                            .insert(decl.name.clone(), InstanceExportType::Type(ty.clone()));
                    }
                    TypeBound::Sub => {
                        inst_type.exports.insert(
                            decl.name.clone(),
                            InstanceExportType::Type(Type::UniqueResource),
                        );
                    }
                },
                ExternDesc::Component(ty) => {
                    inst_type
                        .exports
                        .insert(decl.name.clone(), InstanceExportType::Component(ty.clone()));
                }
                ExternDesc::Instance(ty) => {
                    inst_type
                        .exports
                        .insert(decl.name.clone(), InstanceExportType::Instance(ty.clone()));
                }
            },
        }
    }
    Ok((len, inst_type))
}

pub fn parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<InstanceDecl> {
    _parse_instance_decl(ctx, None)
}

pub fn _parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
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
            InstanceDecl::CoreModuleType(t.try_into()?)
        }
        0x01 => {
            // let (_, t) = parse_type(ctx, Some(type_validator))?;
            // InstanceDecl::Type(t)
            todo!()
        }
        0x02 => {
            let (_, a) = parse_alias_type(ctx)?;
            InstanceDecl::Alias(a)
        }
        0x04 => {
            let (_, decl) = parse_export_decl(ctx)?;
            InstanceDecl::ExportDecl(decl)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, d))
}
