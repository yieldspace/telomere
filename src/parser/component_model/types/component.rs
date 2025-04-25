use crate::binary::BinaryReader;
use crate::component_model::{ComponentDecl, ComponentType};
use crate::parser::component_model::{parse_vec_range, ParseContext, SizedResult, Validator, _parse_instance_decl};
use crate::parser::component_model::types::parse_import_decl;
use crate::parser::component_model::types::validator::TypeValidator;

pub fn parse_component_type(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<ComponentType> {
    let start_count = ctx.reader.read_count();

    let mut component_type = ComponentType::new();

    for _ in parse_vec_range(ctx)? {
        let (_, decl) = parse_component_decl(ctx)?;
    }
    
    Ok((ctx.reader.read_count() - start_count, component_type))
}

fn parse_component_decl(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<ComponentDecl> {
    let start_count = ctx.reader.read_count();
    let decl = match ctx.reader.read_exact_one()? {
        0x03 => {
            let (_, decl) = parse_import_decl(ctx)?;
            ComponentDecl::Import(decl)
        }
        x => {
            let (_, decl) = _parse_instance_decl(ctx, Some(x))?;
            ComponentDecl::Instance(decl)
        }
    };
    Ok((ctx.reader.read_count() - start_count, decl))
}
