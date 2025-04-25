use crate::binary::BinaryReader;
use crate::component_model::{AliasIdx, AliasType, Binding, ComponentFunction, CoreModule, CoreModuleReference, CoreModuleType, CoreType, ExportDecl, ExternDesc, InlineComponent, Instance, InstanceDecl, InstanceExportType, InstanceType, Reference, Type, TypeBound};
use crate::parser::component_model::types::{parse_export_decl, parse_type};
use crate::parser::component_model::{parse_alias, parse_core_type, parse_vec_range, ComponentParseError, ParseContext, SizedResult};
use crate::parser::component_model::types::alias::parse_alias_type;
use crate::parser::component_model::types::validator::TypeValidator;
use crate::parser::core::parse_vec;

pub fn parse_instance_type(ctx: &mut ParseContext<impl BinaryReader>, type_validator: &mut TypeValidator) -> SizedResult<InstanceType> {
    for _ in parse_vec_range(ctx)? {
        let (_, decl) = parse_instance_decl(ctx, type_validator)?;
        match decl {
            InstanceDecl::CoreModuleType(ty) => {
                type_validator.add_core_type(CoreType::ModuleType(ty));
            }
            InstanceDecl::Type(ty) => {
                type_validator.add_type(ty.clone());
            }
            InstanceDecl::Alias(ty) => match ty {
                AliasType::Type(ty) => {
                    type_validator.add_type(ty.clone());
                }
                AliasType::Instance(ty) => {
                    type_validator.add_instance_type(ty);
                }
            }
            InstanceDecl::ExportDecl(ExportDecl {name, ed}) => match ed {
                ExternDesc::Core(ty) => {
                    
                }
                ExternDesc::Func(_) => {}
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => {}
                ExternDesc::Type(_) => {}
                ExternDesc::Component(_) => {}
                ExternDesc::Instance(_) => {}
            }
        }
    }
    let (len, decls) = parse_vec(ctx, |v| v.reader, parse_instance_decl)?;
    let mut inst_type = InstanceType::new();
    for decl in decls {
        match decl {
            InstanceDecl::CoreModuleType(ty) => {
                inst_type.core_types.push(ty);
            }
            InstanceDecl::Type(ty) => {
                inst_type.types.push(ty);
            }
            InstanceDecl::Alias(ty) => match ty {
                AliasType::Type(ty) => {}
                AliasType::Instance(_) => {}
            }
            InstanceDecl::ExportDecl(decl) => match &decl.ed {
                ExternDesc::Core(idx) => {
                    let ty = ctx.validator.get_core_type(&idx)?;
                    let k = CoreModuleType::try_from(ty.clone())?;
                    inst_type.exports.insert(decl.name.clone(), InstanceExportType::CoreModule(k));
                }
                ExternDesc::Func(idx) => {
                    let ty = ctx.validator.get_type(&idx);
                    let k = ty.clone().try_into()?;
                    inst_type.exports.insert(decl.name.clone(), InstanceExportType::Func(k));
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => todo!(),
                ExternDesc::Type(bound) => match bound {
                    TypeBound::Eq(idx) => {
                        let ty = ctx.validator.get_type(&idx);
                        inst_type
                            .exports
                            .insert(decl.name.clone(), InstanceExportType::Type(ty.clone()));
                    }
                    TypeBound::Sub => {
                        inst_type
                            .exports
                            .insert(decl.name.clone(), InstanceExportType::Type(Type::UniqueResource));
                    }
                }
                ExternDesc::Component(idx) => {
                    let ty = ctx.validator.get_type(&idx);
                    let k = ty.clone().try_into()?;
                    inst_type
                        .exports
                        .insert(decl.name.clone(), InstanceExportType::Component(k));
                }
                ExternDesc::Instance(idx) => {
                    let ty = ctx.validator.get_type(&idx);
                    let k = ty.clone().try_into()?;
                    inst_type
                        .exports
                        .insert(decl.name.clone(), InstanceExportType::Instance(k));
                }
            }
        }
    }
    Ok((len, inst_type))
}

pub fn parse_instance_decl(ctx: &mut ParseContext<impl BinaryReader>, type_validator: &mut TypeValidator) -> SizedResult<InstanceDecl> {
    _parse_instance_decl(ctx, None, type_validator)
}

pub fn _parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
    byte: Option<u8>,
    type_validator: &mut TypeValidator
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
            let (_, t) = parse_type(ctx, Some(type_validator))?;
            InstanceDecl::Type(t)
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
