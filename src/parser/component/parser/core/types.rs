use crate::assert_magic;
use crate::binary::BinaryReader;
use crate::component_model::{CoreAlias, CoreAliasTarget, CoreModuleDecl, CoreType};
use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::core::parse_core_sort;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_name, parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

/// note: rt and sub x* ct is not supported (Wasm 3.0)
pub fn parse_core_type<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> crate::parser::component::parser::Result<(usize, CoreType)> {
    assert_magic!(
        ctx.reader.read_exact_one()?,
        0x50,
        ComponentModelParserError::InvalidCoreModuleTypeMagic
    );
    let (len, decls) = parse_vec(ctx, |v| v.reader, parse_core_module_decl)?;
    Ok((len + 1, CoreType::CoreModuleType(decls)))
}

fn parse_core_module_decl<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreModuleDecl)> {
    match ctx.reader.read_exact_one()? {
        0x00 => todo!("parse core import"),
        0x01 => {
            let (len, core_type) = parse_core_type(ctx)?;
            Ok((len, CoreModuleDecl::Type(core_type)))
        }
        0x02 => {
            let (sort_len, sort) = parse_core_sort(ctx)?;
            assert_magic!(
                ctx.reader.read_exact_one()?,
                0x01,
                ComponentModelParserError::InvalidCoreAliasTargetMagic
            );
            let (ct_len, ct) = parse_u32(ctx.reader)?;
            let (idx_len, idx) = parse_u32(ctx.reader)?;
            Ok((
                sort_len + 1 + ct_len + idx_len,
                CoreModuleDecl::Alias(CoreAlias {
                    sort,
                    target: CoreAliasTarget::Outer(ct, idx as usize),
                }),
            ))
        }
        0x03 => {
            let (name_len, name) = parse_name(ctx.reader)?;
            todo!("parse core import desc")
        }
        t => Err(ComponentModelParserError::InvalidCoreModuleDecl(t)),
    }
}
