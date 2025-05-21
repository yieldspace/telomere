use crate::binary::BinaryReader;
use crate::component_model::types::{ComponentType, CoreModuleType, Type};
use crate::component_model::{ComponentSection, CoreModule, CoreRelation, Relation};
use crate::parser::component_model::alias::parse_alias;
use crate::parser::component_model::export::parse_export;
use crate::parser::component_model::import::parse_import;
use crate::parser::component_model::instance::parse_instance;
use crate::parser::component_model::types::parse_type;
use crate::parser::component_model::validator::ParseState;
use crate::parser::component_model::{
    parse_core_instance, parse_layer, parse_magic, parse_section_type, parse_vec_range,
    parse_version, ComponentParseError, ParseContext, Validator,
};
use crate::parser::core::parse_u32;
use crate::WasmParser;

pub fn parse_component(
    reader: &mut impl BinaryReader,
    state: &mut ParseState,
    validator: &mut Validator,
) -> Result<(), ComponentParseError> {
    tracing::trace!("parse_component_root");
    let mut ctx = ParseContext::new(reader, state, validator);
    ctx.validator.push_scope();
    _parse_component(&mut ctx)?;
    ctx.validator.pop_scope();
    Ok(())
}

pub fn _parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    tracing::trace!("_parse_component");
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
                    let id = ctx.validator.new_type(ty);
                    ctx.validator.scope_mut().type_indexes.add(id);
                }
            }
            ComponentSection::CoreModule => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let mut core_module = WasmParser::new(&mut sized_reader);
                let module = core_module.parse_module()?;
                let ty = CoreModuleType::from(&module);
                let idx = ctx
                    .state
                    .core_module_store
                    .register(CoreRelation::Defined(CoreModule { module }));
                ctx.state.scope_mut().core_modules.register(idx);
                ctx.validator.scope_mut().core_modules.add(ty);
            }
            ComponentSection::CoreInstance => {
                for _ in parse_vec_range(ctx)? {
                    parse_core_instance(ctx)?;
                }
            }
            ComponentSection::Component => {
                ctx.state.push_scope();
                ctx.validator.push_scope();
                {
                    let mut sized_reader = ctx.reader.take(section_size as usize);
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    _parse_component(&mut ctx)?;
                }
                let component_ty = ctx.validator.scope().make_component();
                let component = ctx.state.scope().make_component();

                ctx.validator.pop_scope();
                ctx.state.pop_scope();

                let component_type_id = ctx.validator.new_type(Type::Component(component_ty));
                ctx.validator
                    .scope_mut()
                    .component_indexes
                    .add(component_type_id);

                let idx = ctx
                    .state
                    .component_store
                    .register(Relation::Defined(component));
                ctx.state.scope_mut().components.register(idx);
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
            ComponentSection::Alias => {
                for _ in parse_vec_range(ctx)? {
                    parse_alias(ctx)?;
                }
            }
            v => todo!("unimplemented: {:?}", v),
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
