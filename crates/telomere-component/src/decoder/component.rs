use crate::decoder::alias::parse_alias;
use crate::decoder::canon::parse_canon;
use crate::decoder::export::parse_export;
use crate::decoder::import::parse_import;
use crate::decoder::instance::parse_instance;
use crate::decoder::types::parse_type;
use crate::decoder::validator::ParseState;
use crate::decoder::value::parse_value_def;
use crate::decoder::{
    parse_core_instance, parse_core_type, parse_layer, parse_magic, parse_section_type,
    parse_vec_range, parse_version, ComponentParseError, ParseContext, ParseResult, Validator,
};
use crate::ir::types::{CoreModuleType, Type};
use crate::ir::{ComponentSection, CoreModule, CoreRelation, Relation};
use crate::support::binary::BinaryReader;
use crate::support::common::{module_exports, module_imports};
use crate::support::parser::core::{parse_name, parse_u32};
use std::collections::HashSet;
use telomere::WasmParser;

pub fn parse_component(
    reader: &mut impl BinaryReader,
    state: &mut ParseState,
    validator: &mut Validator,
) -> Result<(), ComponentParseError> {
    tracing::trace!("parse_component_root");
    let mut ctx = ParseContext::new(reader, state, validator);
    ctx.validator.push_scope();
    _parse_component(&mut ctx, 0)?;
    let component_ty = ctx.validator.make_component();
    ctx.validator.validate_component_surface(&component_ty)?;
    ctx.validator.pop_scope();
    Ok(())
}

fn _parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
    depth: u32,
) -> Result<(), ComponentParseError> {
    if depth > crate::MAX_COMPONENT_NESTING_DEPTH {
        return Err(ComponentParseError::NestingTooDeep {
            limit: crate::MAX_COMPONENT_NESTING_DEPTH,
        });
    }

    tracing::trace!("_parse_component");
    parse_magic(ctx.reader)?;
    parse_version(ctx.reader)?;
    parse_layer(ctx.reader)?;

    while let Some(st) = parse_section_type(ctx.reader)? {
        let (_, section_size) = parse_u32(ctx.reader)?;
        let section_size = section_size as usize;
        let section_start = ctx.reader.read_count();
        // Every section body is parsed through a reader limited to the
        // declared size and must consume it exactly: contents may not
        // spill into the following sections, and a body that ends early
        // (or a declared size that runs past the end of the input) is
        // malformed instead of being silently resynchronised.
        match st {
            ComponentSection::Custom => {
                let mut sized_reader = ctx.reader.take(section_size);
                parse_custom_section(&mut sized_reader, section_size)?;
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Type => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        let ty = parse_type(&mut ctx, depth)?;
                        let id = ctx.validator.new_type(ty);
                        ctx.validator.validate_effective_type_size(id)?;
                        ctx.validator.scope_mut().type_indexes.add(id);
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::CoreModule => {
                let mut sized_reader = ctx.reader.take(section_size);
                let module = parse_core_module_section(ctx.state, &mut sized_reader)?;
                finish_section(&mut sized_reader, section_start, section_size)?;
                validate_core_module_contract(&module)?;
                let ty = CoreModuleType::from(&module);
                let idx = ctx
                    .state
                    .core_module_store
                    .register(CoreRelation::Defined(CoreModule { module }));
                #[cfg(feature = "aot-bundle")]
                ctx.state.bind_core_module_substitution(idx);
                ctx.state.scope_mut().core_modules.register(idx);
                ctx.validator.scope_mut().core_modules.add(ty);
            }
            ComponentSection::CoreType => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        // One `core:deftype` can define several core
                        // types (a `rec` group); register each of them.
                        let (_, types) = parse_core_type(&mut ctx)?;
                        for (ty, is_final, identity) in types {
                            let idx = ctx
                                .state
                                .core_type_store
                                .register(CoreRelation::Defined(ty.clone()));
                            ctx.state.scope_mut().core_types.register(idx);
                            ctx.validator.scope_mut().core_types.add_with_records(
                                ty,
                                Some(is_final),
                                identity,
                            );
                        }
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::CoreInstance => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_core_instance(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Component => {
                ctx.state.push_scope();
                ctx.validator.push_scope();
                {
                    let mut sized_reader = ctx.reader.take(section_size);
                    {
                        let mut ctx =
                            ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                        _parse_component(&mut ctx, depth + 1)?;
                    }
                    finish_section(&mut sized_reader, section_start, section_size)?;
                }
                let component_ty = ctx.validator.make_component();
                let component = ctx
                    .state
                    .scope()
                    .make_component(&ctx.state.core_instance_store, &ctx.state.instance_store);

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
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_export(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Import => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_import(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Instance => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_instance(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Alias => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_alias(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Canon => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_canon(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
            }
            ComponentSection::Value => {
                let mut sized_reader = ctx.reader.take(section_size);
                {
                    let mut ctx = ParseContext::new(&mut sized_reader, ctx.state, ctx.validator);
                    for _ in parse_vec_range(&mut ctx)? {
                        parse_value_def(&mut ctx)?;
                    }
                }
                finish_section(&mut sized_reader, section_start, section_size)?;
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

/// Verifies that a section parsed through a size-limited reader
/// consumed exactly its declared size.
///
/// The limiting reader caps every read at the declared size, so a
/// section body can only under-consume. The remainder is discarded to
/// tell the two malformations apart: if the underlying input ends
/// first, the declared size itself ran past the end of the input
/// (`unexpected end-of-file`); if the bytes are present but were not
/// consumed, the section body did not fill its declared size.
fn finish_section<R: BinaryReader>(
    sized_reader: &mut R,
    section_start: usize,
    section_size: usize,
) -> ParseResult<()> {
    let consumed = sized_reader.read_count().saturating_sub(section_start);
    let mut remaining = section_size.saturating_sub(consumed);
    while remaining > 0 {
        match sized_reader.read_one()? {
            Some(_) => remaining -= 1,
            None => {
                return Err(ComponentParseError::IoError(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "unexpected end-of-file",
                )));
            }
        }
    }
    if consumed != section_size {
        return Err(ComponentParseError::InvalidSectionSize);
    }
    Ok(())
}

/// Parses one CoreModule section's bytes into a core module.
///
/// When a component bundle substitution session is active on the
/// parse state (#201), the section is consumed without parsing and
/// the bundle artifact's deserialized module is returned instead;
/// the caller records the substitution against the registered index
/// via `bind_core_module_substitution`. Without a session this is
/// exactly the plain parse.
fn parse_core_module_section(
    state: &mut ParseState,
    sized_reader: &mut impl BinaryReader,
) -> ParseResult<crate::support::Module> {
    #[cfg(feature = "aot-bundle")]
    if let Some(module) = state.substitute_core_module(sized_reader)? {
        return Ok(module);
    }
    #[cfg(not(feature = "aot-bundle"))]
    let _ = state;
    let mut core_module = WasmParser::new(sized_reader);
    Ok(core_module.parse_module()?)
}

fn validate_core_module_contract(module: &crate::support::Module) -> ParseResult<()> {
    let mut import_names = HashSet::new();
    for import in module_imports(module) {
        let key = format!("{}:{}", import.module, import.name);
        if !import_names.insert(key.clone()) {
            return Err(ComponentParseError::TypeMismatch(format!(
                "duplicate import name `{key}`"
            )));
        }
    }

    let mut export_names = HashSet::new();
    for export in module_exports(module) {
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
    // A custom section is a name followed by opaque bytes. The name is
    // part of the binary grammar, so it must be well-formed — a length
    // running past the section or invalid UTF-8 makes the whole
    // component malformed — while the remaining bytes are not
    // interpreted at all.
    let (name_len, _name) = parse_name(reader)?;
    let mut remaining = size
        .checked_sub(name_len)
        .ok_or(ComponentParseError::InvalidSectionSize)?;
    let mut scratch = [0u8; 4096];
    while remaining > 0 {
        let chunk = remaining.min(scratch.len());
        let read = reader.read_slice(&mut scratch[..chunk])?;
        if read == 0 {
            return Err(ComponentParseError::IoError(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected end-of-file",
            )));
        }
        remaining -= read;
    }
    Ok(())
}
