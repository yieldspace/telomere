use crate::binary::{BinaryReader, Countable, Counter};
use crate::component_model::{CoreAlias, CoreAliasTarget, CoreModuleDecl, CoreType};
use crate::parser::component::context::ParseContext;
use crate::parser::component::core::parse_core_sort;
use crate::parser::component::ComponentParseError;
use crate::parser::core::{parse_name, parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentParseError>;

/// note: rt and sub x* ct is not supported (Wasm 3.0)
pub fn parse_core_type<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, CoreType)> {
    parse_core_module_type(ctx)
}

fn parse_core_module_type(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, CoreType)> {
    let mut counter = Counter::new();
    ComponentParseError::assert_magic(
        [ctx.reader.read_exact_one()?.count(&mut counter)],
        [0x50],
        "core module type",
    )?;
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
            ComponentParseError::assert_magic(
                [ctx.reader.read_exact_one()?],
                [0x01],
                "core alias target",
            )?;
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
        t => Err(ComponentParseError::InvalidCoreModuleDecl(t)),
    }
}
