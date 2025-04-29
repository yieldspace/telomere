use crate::binary::BinaryReader;
use crate::component_model::{
    AliasType, CoreType, ExportDecl, ExternDesc, InstanceDecl, InstanceType,
};
use crate::parser::component_model::types::alias::parse_alias_type;
use crate::parser::component_model::types::parse_export_decl;
use crate::parser::component_model::{
    parse_core_type, parse_type, parse_vec_range, ParseContext, SizedResult, Validator,
};

pub fn parse_instance_type<R: BinaryReader>(
    parent_ctx: &mut ParseContext<R>,
) -> SizedResult<InstanceType> {
    let new_validator = Validator::new_child(&mut parent_ctx.validator);
    let state = &mut parent_ctx.state;
    let mut new_ctx = ParseContext::new(parent_ctx.reader, parent_ctx.instrs, new_validator, state);
    let start_count = new_ctx.reader.read_count();
    let mut inst_type = InstanceType::new();
    for _ in parse_vec_range(&mut new_ctx)? {
        let (_, decl) = parse_instance_decl(&mut new_ctx)?;
        match decl {
            InstanceDecl::CoreModuleType(ty) => {
                new_ctx.validator.add_core_module_type(ty.clone())?;
                // inst_type.core_types.push(CoreType::ModuleType(ty));
            }
            InstanceDecl::Type(ty) => {
                new_ctx.validator.add_type(ty.clone())?;
                // inst_type.types.push(ty);
            }
            InstanceDecl::Alias(ty) => match ty {
                AliasType::Type(ty) => {
                    new_ctx.validator.add_type(ty.clone())?;
                    // inst_type.types.push(ty);
                }
                AliasType::Instance(ty) => {
                    new_ctx.validator.add_instance_type(ty.clone())?;
                    // inst_type.instances.push(ty)
                }
            },
            InstanceDecl::ExportDecl(ExportDecl { name, ed }) => match ed {
                ExternDesc::CoreModule(ty) => {
                    new_ctx.validator.add_core_module_type(ty.clone())?;
                    inst_type.exports.insert(name, ExternDesc::CoreModule(ty));
                }
                ExternDesc::Func(ty) => {
                    new_ctx.validator.add_func_type(ty.clone())?;
                    inst_type.exports.insert(name, ExternDesc::Func(ty));
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => {}
                ExternDesc::Type(ty) => {
                    new_ctx.validator.add_type(ty.clone().into())?;
                    inst_type.exports.insert(name, ExternDesc::Type(ty));
                }
                ExternDesc::Component(ty) => {
                    new_ctx.validator.add_component_type(ty.clone())?;
                    inst_type.exports.insert(name, ExternDesc::Component(ty));
                }
                ExternDesc::Instance(ty) => {
                    new_ctx.validator.add_instance_type(ty.clone())?;
                    inst_type.exports.insert(name, ExternDesc::Instance(ty));
                }
            },
        }
    }

    Ok((new_ctx.reader.read_count() - start_count, inst_type))
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
            InstanceDecl::CoreModuleType(t.try_into()?)
        }
        0x01 => {
            let (_, t) = parse_type(ctx)?;
            InstanceDecl::Type(t)
        }
        0x02 => {
            let (_, a) = parse_alias_type(ctx)?;
            InstanceDecl::Alias(a)
        }
        0x04 => {
            let (_, d) = parse_export_decl(ctx)?;
            InstanceDecl::ExportDecl(d)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, d))
}
