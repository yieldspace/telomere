use crate::binary::BinaryReader;
use crate::component_model::{AliasIdx, InstanceDecl, InstanceType};
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
                AliasIdx::Type(_) => {}
                AliasIdx::Instance(_) => {}
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
            InstanceDecl::ExportDecl(decl)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, d))
}
