use crate::binary::BinaryReader;
use crate::component::decoder::alias::parse_alias;
use crate::component::decoder::canon::parse_canon;
use crate::component::decoder::export::parse_export;
use crate::component::decoder::import::parse_import;
use crate::component::decoder::instance::parse_instance;
use crate::component::decoder::types::parse_type;
use crate::component::decoder::validator::ParseState;
use crate::component::decoder::{
    parse_core_instance, parse_core_type, parse_layer, parse_magic, parse_section_type,
    parse_vec_range, parse_version, ComponentParseError, ParseContext, ParseResult, Validator,
};
use crate::component::ir::types::{CoreModuleType, Type};
use crate::component::ir::{ComponentSection, CoreModule, CoreRelation, Relation};
use crate::parser::core::parse_u32;
use crate::WasmParser;
use std::collections::HashSet;

pub fn parse_component(
    reader: &mut impl BinaryReader,
    state: &mut ParseState,
    validator: &mut Validator,
) -> Result<(), ComponentParseError> {
    tracing::trace!("parse_component_root");
    let mut ctx = ParseContext::new(reader, state, validator);
    ctx.validator.push_scope();
    _parse_component(&mut ctx)?;
    let component_ty = ctx.validator.make_component();
    ctx.validator.validate_component_surface(&component_ty)?;
    ctx.validator.pop_scope();
    Ok(())
}

fn _parse_component(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(), ComponentParseError> {
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
                    ctx.validator.validate_effective_type_size(id)?;
                    ctx.validator.scope_mut().type_indexes.add(id);
                }
            }
            ComponentSection::CoreModule => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let mut core_module = WasmParser::new(&mut sized_reader);
                let module = core_module.parse_module()?;
                validate_core_module_contract(&module)?;
                let ty = CoreModuleType::from(&module);
                let idx = ctx
                    .state
                    .core_module_store
                    .register(CoreRelation::Defined(CoreModule { module }));
                ctx.state.scope_mut().core_modules.register(idx);
                ctx.validator.scope_mut().core_modules.add(ty);
            }
            ComponentSection::CoreType => {
                for _ in parse_vec_range(ctx)? {
                    let (_, ty) = parse_core_type(ctx)?;
                    let idx = ctx
                        .state
                        .core_type_store
                        .register(CoreRelation::Defined(ty.clone()));
                    ctx.state.scope_mut().core_types.register(idx);
                    ctx.validator.scope_mut().core_types.add(ty);
                }
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
                let component_ty = ctx.validator.make_component();
                let component = ctx.state.scope().make_component();

                ctx.validator.pop_scope();
                ctx.state.pop_scope();

                let component_type_id = ctx.validator.new_type(Type::Component(component_ty));
                ctx.validator
                    .validate_effective_type_size(component_type_id)?;
                ctx.validator
                    .validate_component_type_definition(component_type_id)?;
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
            ComponentSection::Canon => {
                for _ in parse_vec_range(ctx)? {
                    parse_canon(ctx)?;
                }
            }
            v => {
                return Err(ComponentParseError::Unsupported(format!(
                    "unsupported component section: {v:?}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_core_module_contract(module: &crate::Module) -> ParseResult<()> {
    let mut import_names = HashSet::new();
    for import in &module.imports.0 {
        let key = format!("{}:{}", import.module, import.name);
        if !import_names.insert(key.clone()) {
            return Err(ComponentParseError::TypeMismatch(format!(
                "duplicate import name `{key}`"
            )));
        }
    }

    let mut export_names = HashSet::new();
    for export in &module.exs.0 {
        if !export_names.insert(export.0.clone()) {
            return Err(ComponentParseError::TypeMismatch(format!(
                "export name `{}` already defined",
                export.0
            )));
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
