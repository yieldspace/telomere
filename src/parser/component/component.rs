use std::sync::Arc;
use crate::binary::BinaryReader;
use crate::parser::component::{parse_layer, parse_magic, parse_section_type, parse_vec_map, parse_version, ParseContext, SortMap};
use crate::parser::component::error::ComponentParseError;
use crate::parser::component::section::ComponentSectionType;
use crate::parser::core::parse_u32;
use crate::{Module, WasmParser};
use crate::component_model::{ComponentBuilder, CoreModule};
use crate::parser::component::core::parse_core_instance;
use crate::parser::component::vec::VecParser;

pub fn parse_component<R: BinaryReader>(reader: &mut R) -> Result<(), ComponentParseError> {
    // let sort_map = SortMap::new(None);
    parse_magic(reader)?;
    parse_version(reader)?;
    parse_layer(reader)?;

    let mut builder = ComponentBuilder::new();

    loop {
        let section_type = if let Some(st) = parse_section_type(reader)? {
            st
        } else {
            break;
        };
        let (_, section_size) = parse_u32(reader)?;
        let mut sized_reader = reader.take(section_size as usize);
        match section_type {
            ComponentSectionType::Custom => parse_custom_section(&mut sized_reader, section_size as usize)?,
            ComponentSectionType::CoreModule => parse_core_module_section(&mut sized_reader, &mut builder)?,
            ComponentSectionType::CoreInstance => parse_core_instance_section(&mut sized_reader, &mut builder)?,
            ComponentSectionType::CoreType => {}
            ComponentSectionType::Component => {}
            ComponentSectionType::Instance => {}
            ComponentSectionType::Alias => {}
            ComponentSectionType::Type => {}
            ComponentSectionType::Canon => {}
            ComponentSectionType::Start => {}
            ComponentSectionType::Import => {}
            ComponentSectionType::Export => {}
            ComponentSectionType::Value => {}
        }
    }
    Ok(())
}

fn parse_custom_section<R: BinaryReader>(reader: &mut R, size: usize) -> Result<(), ComponentParseError> {
    // Custom section parsing logic
    for _ in 0..size {
        reader.read_exact_one()?;
    }
    Ok(())
}

fn parse_core_module_section<R: BinaryReader>(reader: &mut R, builder: &mut ComponentBuilder) -> Result<(), ComponentParseError> {
    // Core module parsing logic
    let mut core_module = WasmParser::new(reader);
    let module = core_module.parse_module()?;
    builder.register_core_module(CoreModule(module));
    Ok(())
}

fn parse_core_instance_section<R: BinaryReader>(reader: &mut R, builder: &mut ComponentBuilder) -> Result<(), ComponentParseError>
where
    R: BinaryReader,
{
    // Core instance parsing logic
    let mut ctx = ParseContext::new(reader, builder);
    parse_vec_map(
        &mut ctx,
        |v| v.reader,
        parse_core_instance,
        |v, i| {
            v.builder.register_core_instance(i);
        },
    )?;
    Ok(())
}
