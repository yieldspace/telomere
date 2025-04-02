use crate::binary::BinaryReader;
use crate::component_model::Component;
pub use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::core::{parse_core_instance, parse_core_type};
use crate::parser::component::parser::instance::parse_instance;
use crate::parser::component::section::ComponentSectionType;
use crate::parser::core::{parse_name, parse_u32, parse_vec};
use crate::parser::leb128::Leb128Parser;
use crate::{Module, WasmParser, WasmParserError};
use thiserror::Error;

mod alias;
mod context;
mod core;
mod id;
mod import_export;
mod instance;
mod sort;
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

pub fn parse_component<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<Component> {
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
                let module = parse_core_module(ctx.reader, size as usize)?;
            }
            ComponentSectionType::CoreInstance => {
                let (_, instances) = parse_vec(ctx, |v| v.reader, parse_core_instance)?;
            }
            ComponentSectionType::CoreType => {
                let (_, core_types) = parse_vec(ctx, |v| v.reader, parse_core_type)?;
            }
            ComponentSectionType::Component => {
                let component = parse_component(ctx)?;
            }
            ComponentSectionType::Instance => {
                let (_, instances) = parse_vec(ctx, |v| v.reader, parse_instance)?;
            }
            ComponentSectionType::Alias => {
                let (_, aliases) = parse_vec(ctx, |v| v.reader, alias::parse_alias)?;
            }
            ComponentSectionType::Type => {
                let (_, types) = parse_vec(ctx, |v| v.reader, types::parse_type)?;
            }
            ComponentSectionType::Canon => {}
            ComponentSectionType::Start => {}
            ComponentSectionType::Import => {}
            ComponentSectionType::Export => {}
            ComponentSectionType::Value => {}
        }
    }
    Ok(Component {
        modules: vec![],
        core_instances: vec![],
        core_types: vec![],
        components: vec![],
        instances: vec![],
        aliases: vec![],
        types: vec![],
    })
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
