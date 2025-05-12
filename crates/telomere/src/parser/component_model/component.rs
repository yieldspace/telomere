use crate::binary::BinaryReader;
use crate::component_model::types::Type;
use crate::component_model::{ComponentSection, Relation};
use crate::parser::component_model::export::parse_export;
use crate::parser::component_model::import::parse_import;
use crate::parser::component_model::types::parse_type;
use crate::parser::component_model::validator::ValidatorState;
use crate::parser::component_model::{
    parse_layer, parse_magic, parse_section_type, parse_vec_range, parse_version,
    ComponentParseError, ParseContext, Validator,
};
use crate::parser::component_model::instance::parse_instance;
use crate::parser::core::parse_u32;

pub fn parse_component(
    reader: &mut impl BinaryReader,
    state: &mut ValidatorState,
    validator: &mut Validator,
) -> Result<(), ComponentParseError> {
    let mut ctx = ParseContext::new(reader, state, validator);
    _parse_component(&mut ctx)?;
    Ok(())
}

pub fn _parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    parse_magic(ctx.reader)?;
    parse_version(ctx.reader)?;
    parse_layer(ctx.reader)?;

    while let Some(st) = parse_section_type(ctx.reader)? {
        let (_, section_size) = parse_u32(ctx.reader)?;
        match st {
            ComponentSection::Custom => parse_custom_section(ctx.reader, section_size as usize)?,
            ComponentSection::Type => {
                for _ in parse_vec_range(ctx)? {
                    let ty = parse_type(ctx)?;
                    ctx.validator.with_scope(|scope| {
                        let id = scope.add_type(ty);
                        scope.types.register(id);
                    });
                }
            }
            ComponentSection::Component => {
                ctx.validator.new_scope();
                {
                    let mut sized_reader = ctx.reader.take(section_size as usize);
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    _parse_component(&mut ctx)?;
                }
                // todo ここでcomponent登録
                let component = ctx.validator.scope().make_component();
                let ty = ctx.validator.scope().make_component_type();
                ctx.validator.merge_types_into_parent();
                ctx.validator.merge_globals_into_parent();
                ctx.validator.pop_scope();
                let scope = ctx.validator.scope_mut();
                let id = scope.add_type(Type::Component(ty));
                scope
                    .components
                    .register_with_data(id, Relation::Defined(component));
            }
            ComponentSection::Export => {
                for _ in parse_vec_range(ctx)? {
                    parse_export(ctx)?;
                }
            }
            ComponentSection::Import => {
                for _ in parse_vec_range(ctx)? {
                    parse_import(ctx)?;
                }
            }
            ComponentSection::Instance => {
                for _ in parse_vec_range(ctx)? {
                    parse_instance(ctx)?;
                }
            }
            _ => todo!(),
        }
    }
    Ok(())
}

#[inline]
fn parse_custom_section<R: BinaryReader>(
    reader: &mut R,
    size: usize,
) -> Result<(), ComponentParseError> {
    // Custom section parsing logic
    for _ in 0..size {
        reader.read_exact_one()?;
    }
    Ok(())
}
