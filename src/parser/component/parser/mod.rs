use crate::binary::BinaryReader;
use crate::component_model::Component;
pub use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::core::{parse_core_instance, parse_core_type};
use crate::parser::component::parser::instance::parse_instance;
use crate::parser::component::section::ComponentSectionType;
use crate::parser::component::sort::SortMap;
use crate::parser::core::{parse_u32, parse_vec};
use crate::{Module, WasmParser, WasmParserError};
use std::sync::{Arc, Weak};
use thiserror::Error;
use tracing::trace;

mod alias;
mod canon;
mod context;
mod core;
mod id;
mod import_export;
mod instance;
mod types;

#[macro_export]
macro_rules! assert_magic {
    ($magic:expr, $expected:expr, $err:expr) => {{
        let magic = $magic;
        if magic != $expected {
            return Err($err(magic));
        }
    }};
}

type Result<R> = std::result::Result<R, ComponentModelParserError>;

#[derive(Error, Debug)]
pub enum ComponentModelParserError {
    #[error("module can't set multiple times")]
    MultipleModule,
    #[error("invalid magic: {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("invalid version: {0:?}")]
    InvalidVersion([u8; 2]),
    #[error("invalid layer: {0:?}")]
    InvalidLayer([u8; 2]),
    #[error("error at core wasm module")]
    CoreWasmError(#[from] WasmParserError),
    #[error("error from underlying layer")]
    IoError(#[from] std::io::Error),
    #[error("invalid section type: {0:?}")]
    InvalidSectionType(u8),
    #[error("invalid instance expression: {0:?}")]
    InvalidInstanceExpr(u8),
    #[error("invalid core sort: {0:?}")]
    InvalidCoreSort(u8),
    #[error("invalid instantiate arg magic: {0:?}")]
    InvalidInstantiateArgMagic(u8),
    #[error("invalid core module type magic: {0:?}")]
    InvalidCoreModuleTypeMagic(u8),
    #[error("invalid core module decl type: {0:?}")]
    InvalidCoreModuleDecl(u8),
    #[error("invalid core alias target magic: {0:?}")]
    InvalidCoreAliasTargetMagic(u8),
    #[error("invalid sort: {0:?}")]
    InvalidSort(u8),
    #[error("invalid alias target: {0:?}")]
    InvalidAliasTarget(u8),
    #[error("invalid {0} idx: {1:?}")]
    InvalidIdx(String, u32),
    #[error("invalid module id: {0:?}")]
    InvalidModuleId(u32),
    #[error("invalid instance id: {0:?}")]
    InvalidInstanceId(u32),
    #[error("invalid component id: {0:?}")]
    InvalidComponentId(u32),
    #[error("invalid prim val type: {0:?}")]
    InvalidPrimValType(u8),
    #[error("type error: {0:?}")]
    TypeError(String),
    #[error("invalid option magic: {0:?}")]
    InvalidOptionMagic(u8),
    #[error("invalid case magic: {0:?}")]
    InvalidCaseMagic(u8),
    #[error("invalid import name magic: {0:?}")]
    InvalidImportNameMagic(u8),
    #[error("invalid extern desc magic: {0:?}")]
    InvalidExternDescMagic(u8),
}

pub fn parse_component<R: BinaryReader>(reader: &mut R) -> Result<Component> {
    let sort_map = SortMap::new(None);
    let mut ctx = ParseContext::new(reader, sort_map);
    _parse_component(&mut ctx)?;
    Ok(Component::from(ctx.sort))
}

pub fn _parse_component<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<()> {
    parse_magic(ctx.reader)?;
    parse_version(ctx.reader)?;
    parse_layer(ctx.reader)?;
    loop {
        let section_type = if let Some(st) = parse_section_type(ctx.reader)? {
            st
        } else {
            break;
        };
        let (_, size) = parse_u32(ctx.reader)?;
        match section_type {
            ComponentSectionType::Custom => {
                for _ in 0..size {
                    ctx.reader.read_exact_one()?;
                }
            }
            ComponentSectionType::CoreModule => {
                let module = Arc::new(parse_core_module(ctx.reader, size as usize)?);
                ctx.sort.add_core_module(module)
            }
            ComponentSectionType::CoreInstance => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    parse_core_instance,
                    |v, i| {
                        v.sort.add_core_instance(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::CoreType => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    parse_core_type,
                    |v, i| {
                        v.sort.add_core_type(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Component => {
                let map = SortMap::new(Some(&ctx.sort));
                let mut child_context = ParseContext::new(ctx.reader, map);
                _parse_component(&mut child_context)?;
                ctx.sort
                    .add_component(Arc::new(Component::from(child_context.sort)));
            }
            ComponentSectionType::Instance => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    parse_instance,
                    |v, i| {
                        v.sort.add_instance(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Alias => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    alias::parse_alias,
                    |v, i| {
                        v.sort.add_alias(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Type => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    types::parse_type,
                    |v, i| {
                        v.sort.add_type(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Canon => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    canon::parse_canon,
                    |v, i| {
                        v.sort.add_canon(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Start => {
                unimplemented!()
            }
            ComponentSectionType::Import => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    import_export::parse_import,
                    |v, i| {
                        v.sort.add_import(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Export => {
                parse_vec_map(
                    ctx,
                    |v| v.reader,
                    import_export::parse_export,
                    |v, i| {
                        v.sort.add_export(Arc::new(i));
                    },
                )?;
            }
            ComponentSectionType::Value => {
                unimplemented!()
            }
        }
    }
    Ok(())
}

pub fn parse_magic<R: BinaryReader>(reader: &mut R) -> Result<()> {
    let magic = reader.read_exact::<4>()?;
    if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
        Ok(())
    } else {
        Err(ComponentModelParserError::InvalidMagic(magic))
    }
}

pub fn parse_version<R: BinaryReader>(reader: &mut R) -> Result<()> {
    let version = reader.read_exact::<2>()?;
    if matches!(&version, &[0x0d, 0x00]) {
        Ok(())
    } else {
        Err(ComponentModelParserError::InvalidVersion(version))
    }
}

pub fn parse_layer<R: BinaryReader>(reader: &mut R) -> Result<()> {
    let layer = reader.read_exact::<2>()?;
    if matches!(&layer, &[0x01, 0x00]) {
        Ok(())
    } else {
        Err(ComponentModelParserError::InvalidLayer(layer))
    }
}

pub fn parse_section_type<R: BinaryReader>(reader: &mut R) -> Result<Option<ComponentSectionType>> {
    if let Some(kind) = reader.read_one()? {
        match kind {
            0x00 => Ok(Some(ComponentSectionType::Custom)),
            0x01 => Ok(Some(ComponentSectionType::CoreModule)),
            0x02 => Ok(Some(ComponentSectionType::CoreInstance)),
            0x03 => Ok(Some(ComponentSectionType::CoreType)),
            0x04 => Ok(Some(ComponentSectionType::Component)),
            0x05 => Ok(Some(ComponentSectionType::Instance)),
            0x06 => Ok(Some(ComponentSectionType::Alias)),
            0x07 => Ok(Some(ComponentSectionType::Type)),
            0x08 => Ok(Some(ComponentSectionType::Canon)),
            0x09 => Ok(Some(ComponentSectionType::Start)),
            0x0a => Ok(Some(ComponentSectionType::Import)),
            0x0b => Ok(Some(ComponentSectionType::Export)),
            0x0c => Ok(Some(ComponentSectionType::Value)),
            _ => Err(ComponentModelParserError::InvalidSectionType(kind)),
        }
    } else {
        Ok(None)
    }
}

pub fn parse_core_module<R: BinaryReader>(reader: &mut R, size: usize) -> Result<Module> {
    let mut core_reader = reader.take(size);
    let mut core_module = WasmParser::new(&mut core_reader);
    let module = core_module.parse_module()?;
    Ok(module)
}

pub fn parse_option<R: BinaryReader, V, E>(
    ctx: &mut ParseContext<R>,
    mut f: impl FnMut(&mut ParseContext<R>) -> std::result::Result<(usize, V), E>,
) -> Result<(usize, Option<V>)>
where
    ComponentModelParserError: From<E>,
{
    match ctx.reader.read_exact_one()? {
        0x00 => Ok((1, None)),
        0x01 => {
            let (len, v) = f(ctx)?;
            Ok((len + 1, Some(v)))
        }
        x => Err(ComponentModelParserError::InvalidOptionMagic(x)),
    }
}

pub fn parse_vec_map<A, R: BinaryReader, V, E>(
    env: &mut A,
    reader: impl FnOnce(&mut A) -> &mut R,
    mut f: impl FnMut(&mut A) -> std::result::Result<(usize, V), E>,
    mut map: impl FnMut(&mut A, V) -> (),
) -> std::result::Result<(usize, ()), E>
where
    E: From<WasmParserError>,
{
    let mut read_bytes = 0;

    let (len_len, len) = parse_u32(reader(env))?;
    trace!("parse_vec_map: {len_len} {len}");
    read_bytes += len_len;
    for _i in 0..len {
        let (len, v) = f(env)?;
        map(env, v);
        read_bytes += len;
    }
    Ok((read_bytes, ()))
}
