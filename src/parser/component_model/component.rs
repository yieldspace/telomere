use crate::binary::BinaryReader;
use crate::component_model::{Binding, CoreModule, CoreModuleType, InlineComponent, InlineComponentValue};
use crate::parser::component_model::canon::parse_canon;
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::parse_core_instance;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::export::parse_export;
use crate::parser::component_model::import::parse_import;
use crate::parser::component_model::instance::parse_instance;
use crate::parser::component_model::section::ComponentSectionType;
use crate::parser::component_model::types::parse_type;
use crate::parser::component_model::validator::{DefaultValidatorState, ValidatorState};
use crate::parser::component_model::{parse_alias, parse_layer, parse_magic, parse_section_type, parse_vec_range, parse_version, TypeSuperValidator, Validator};
use crate::parser::core::{parse_u32, parse_vec};
use crate::runtime::component_model::instantiate::{instantiate_special_end, InstantiateInstr};
use crate::{Module, WasmParser};

pub fn parse_component(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    _parse_component(ctx)?;
    ctx.push_instr(InstantiateInstr {
        op: instantiate_special_end,
    });
    Ok(())
}

pub fn _parse_component(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    parse_magic(ctx.reader)?;
    parse_version(ctx.reader)?;
    parse_layer(ctx.reader)?;

    while let Some(st) = parse_section_type(ctx.reader)? {
        let (_, section_size) = parse_u32(ctx.reader)?;
        match st {
            ComponentSectionType::Custom => {
                parse_custom_section(ctx.reader, section_size as usize)?
            }
            ComponentSectionType::CoreModule => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let mut core_module = WasmParser::new(&mut sized_reader);
                let module = core_module.parse_module()?;
                let ty = CoreModuleType::from_module(&module);
                ctx.validator.state
                    .add_core_module(Binding::Real(
                        CoreModule::new(Some(module), ty) 
                    ))?;
            }
            ComponentSectionType::CoreInstance => parse_core_instance_section(ctx)?,
            ComponentSectionType::CoreType => todo!(),
            ComponentSectionType::Component => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let mut validator =
                    Validator::new(ValidatorState::new_child(&mut ctx.validator.state));
                let mut instrs = Vec::new();
                {
                    let mut child_ctx =
                        ParseContext::new(&mut sized_reader, &mut instrs, &mut validator);
                    _parse_component(&mut child_ctx)?;
                }

                // let imports = validator.get_local_store().imports.clone();
                // let exports = validator.get_local_store().exports.clone();
                // let ty = validator.get_local_store().make_component_type();
                // ctx.validator
                //     .add_component(Binding::Real(InlineComponent::new(
                //         Some(InlineComponentValue::new(instrs, imports, exports)),
                //         ty,
                //     )))?;
                todo!()
            }
            ComponentSectionType::Instance => parse_instance_section(ctx)?,
            ComponentSectionType::Alias => parse_alias_section(ctx)?,
            ComponentSectionType::Type => parse_type_section(ctx)?,
            ComponentSectionType::Canon => parse_canon_section(ctx)?,
            ComponentSectionType::Start => todo!(),
            ComponentSectionType::Import => parse_import_section(ctx)?,
            ComponentSectionType::Export => parse_export_section(ctx)?,
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
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
fn parse_core_instance_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    for _ in parse_vec_range(ctx)? {
        let (_, value) = parse_core_instance(ctx)?;
        ctx.validator.state.add_core_instance(Binding::Real(value))?;
    }
    Ok(())
}

#[inline]
fn parse_instance_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    parse_vec(ctx, |v| v.reader, parse_instance)?;
    Ok(())
}

#[inline]
fn parse_alias_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Alias parsing logic
    parse_vec(ctx, |v| v.reader, parse_alias)?;
    Ok(())
}

#[inline]
fn parse_type_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Type parsing logic
    for _ in parse_vec_range(ctx)? {
        let (_, ty) = parse_type(ctx)?;
        ctx.validator.state.add_type(Binding::Real(ty))?;
    }
    Ok(())
}

#[inline]
fn parse_canon_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Canon parsing logic
    parse_vec(ctx, |v| v.reader, parse_canon)?;
    Ok(())
}

#[inline]
fn parse_import_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Import parsing logic
    for _ in parse_vec_range(ctx)? {
        let (name, import) = parse_import(ctx)?;
        ctx.validator.state.add_import(name, import)?;
    }
    Ok(())
}

#[inline]
fn parse_export_section(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> Result<(), ComponentParseError> {
    // Export parsing logic
    for _ in parse_vec_range(ctx)? {
        let (name, export) = parse_export(ctx)?;
        ctx.validator.state.add_export(name, export)?;
    }
    Ok(())
}
