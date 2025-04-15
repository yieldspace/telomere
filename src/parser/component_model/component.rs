use crate::binary::BinaryReader;
use crate::component_model::{Component, FlattenComponent};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::parse_core_instance;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::section::ComponentSectionType;
use crate::parser::component_model::validator::{ChildValidator, ComponentValidator, Validator};
use crate::parser::component_model::{
    parse_layer, parse_magic, parse_section_type, parse_vec_map, parse_version,
};
use crate::parser::core::{parse_u32, parse_vec};
use crate::{Module, WasmParser};
use typed_arena::Arena;

pub fn parse_component<R: BinaryReader, V: Validator>(
    ctx: &mut ParseContext<R, V>,
) -> Result<(), ComponentParseError> {
    parse_magic(ctx.reader)?;
    parse_version(ctx.reader)?;
    parse_layer(ctx.reader)?;

    loop {
        let section_type = if let Some(st) = parse_section_type(ctx.reader)? {
            st
        } else {
            break;
        };
        let (_, section_size) = parse_u32(ctx.reader)?;
        match section_type {
            ComponentSectionType::Custom => {
                parse_custom_section(ctx.reader, section_size as usize)?
            }
            ComponentSectionType::CoreModule => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let module = parse_core_module_section(&mut sized_reader)?;
                ctx.validator.add_core_module(module)?;
            }
            ComponentSectionType::CoreInstance => parse_core_instance_section(ctx)?,
            ComponentSectionType::CoreType => todo!(),
            ComponentSectionType::Component => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let mut validator = ChildValidator::new(ctx.validator);
                let mut instrs = Vec::new();
                {
                    let mut child_ctx =
                        ParseContext::new(&mut sized_reader, &mut instrs, &mut validator);
                    parse_component(&mut child_ctx)?;
                }
                ctx.validator.add_component(Component::new(instrs))?;
            }
            ComponentSectionType::Instance => {}
            ComponentSectionType::Alias => {}
            ComponentSectionType::Type => {}
            ComponentSectionType::Canon => {}
            ComponentSectionType::Start => todo!(),
            ComponentSectionType::Import => {}
            ComponentSectionType::Export => {}
            ComponentSectionType::Value => todo!(),
        }
    }
    Ok(())
}

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

fn parse_core_module_section<R: BinaryReader>(
    reader: &mut R,
) -> Result<Module, ComponentParseError> {
    // Core module parsing logic
    let mut core_module = WasmParser::new(reader);
    let module = core_module.parse_module()?;
    Ok(module)
}

fn parse_core_instance_section(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    parse_vec(ctx, |v| v.reader, parse_core_instance)?;
    Ok(())
}
