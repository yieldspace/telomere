use crate::binary::BinaryReader;
use crate::component_model::types::{InstanceDecl, InstanceType};
use crate::parser::component::alias::parse_alias;
use crate::parser::component::core::parse_core_type;
use crate::parser::component::error::ComponentParseError;
use crate::parser::component::types::sort::InstanceTypeSort;
use crate::parser::component::types::{parse_export_decl, parse_type};
use crate::parser::component::{parse_vec_map, ParseContext};

type Result<R> = std::result::Result<R, ComponentParseError>;

pub fn parse_instance_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, InstanceType)> {
    // let mut sort = InstanceTypeSort::with_parent(&mut ctx.sort);
    todo!();
    parse_vec_map(ctx, |v| v.reader, parse_instance_decl, |c, decl| {})?;
    Ok((0, InstanceType(vec![])))
}

pub fn parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, InstanceDecl)> {
    _parse_instance_decl(ctx, None)
}

pub fn _parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
    byte: Option<u8>,
) -> Result<(usize, InstanceDecl)> {
    let start = ctx.start_count();
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
            // todo: validate alias sort is in [type, instance]
            InstanceDecl::Alias(a)
        }
        0x04 => {
            let (_, decl) = parse_export_decl(ctx)?;
            InstanceDecl::ExportDecl(decl)
        }
        _ => todo!(),
    };
    let end = ctx.end_count(start);
    Ok((end, d))
}
