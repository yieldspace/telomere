use crate::binary::BinaryReader;
use crate::component_model::{Binding, Component, Idx};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::parse_core_instance;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::instance::parse_instance;
use crate::parser::component_model::section::ComponentSectionType;
use crate::parser::component_model::types::parse_type;
use crate::parser::component_model::validator::{ChildValidator, Validator};
use crate::parser::component_model::{
    parse_alias, parse_layer, parse_magic, parse_section_type, parse_version,
};
use crate::parser::core::{parse_u32, parse_vec};
use crate::runtime::component_model::instantiate::{instantiate_special_end, InstantiateInstr};
use crate::{Module, WasmParser};

pub fn parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    _parse_component(ctx)?;
    ctx.push_instr(InstantiateInstr {
        op: instantiate_special_end,
    });
    Ok(())
}

pub fn _parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
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
                let module = {
                    let mut sized_reader = ctx.reader.take(section_size as usize);
                    parse_core_module_section(&mut sized_reader)?
                };
                ctx.validator.add_core_module(Binding::Real(module))?;
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
                    _parse_component(&mut child_ctx)?;
                }
                ctx.validator
                    .add_component(Binding::Real(Component::new(instrs)))?;
            }
            ComponentSectionType::Instance => parse_instance_section(ctx)?,
            ComponentSectionType::Alias => parse_alias_section(ctx)?,
            ComponentSectionType::Type => parse_type_section(ctx)?,
            ComponentSectionType::Canon => {}
            ComponentSectionType::Start => todo!(),
            ComponentSectionType::Import => {}
            ComponentSectionType::Export => {}
            ComponentSectionType::Value => todo!(),
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

#[inline]
fn parse_core_module_section<R: BinaryReader>(
    reader: &mut R,
) -> Result<Module, ComponentParseError> {
    // Core module parsing logic
    let mut core_module = WasmParser::new(reader);
    let module = core_module.parse_module()?;

    Ok(module)
}

#[inline]
fn parse_core_instance_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    parse_vec(ctx, |v| v.reader, parse_core_instance)?;
    Ok(())
}

#[inline]
fn parse_instance_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    parse_vec(ctx, |v| v.reader, parse_instance)?;
    Ok(())
}

#[inline]
fn parse_alias_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Alias parsing logic
    parse_vec(ctx, |v| v.reader, parse_alias)?;
    Ok(())
}

#[inline]
fn parse_type_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Type parsing logic
    parse_vec(ctx, |v| v.reader, parse_type)?;
    Ok(())
}
