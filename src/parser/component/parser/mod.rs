use crate::binary::BinaryReader;
use crate::component::{
    Component, CoreAlias, CoreAliasTarget, CoreInstance, CoreInstanceInlineExport, CoreInstantiate,
    CoreInstantiateArg, CoreModuleDecl, CoreSort, CoreType,
};
use crate::parser::component::parser::instance::{parse_core_instance, parse_instance};
use crate::parser::component::parser::types::parse_core_type;
use crate::parser::component::section::ComponentSectionType;
use crate::parser::core::{parse_name, parse_u32, parse_vec};
use crate::parser::leb128::Leb128Parser;
use crate::{Module, WasmParser, WasmParserError};
use thiserror::Error;

mod alias;
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
}

pub fn parse_component<R: BinaryReader>(reader: &mut R) -> Result<Component> {
    parse_magic(reader)?;
    parse_version(reader)?;
    parse_layer(reader)?;
    loop {
        let section_type = if let Some(st) = parse_section_type(reader)? {
            st
        } else {
            break;
        };
        let (_, size) = Leb128Parser::new(reader).parse_u32(size_of::<u32>() * 8)?;
        match section_type {
            ComponentSectionType::Custom => {
                for _ in 0..size {
                    reader.read_exact_one()?;
                }
            }
            ComponentSectionType::CoreModule => {
                let module = parse_core_module(reader, size as usize)?;
            }
            ComponentSectionType::CoreInstance => {
                let (_, instances) = parse_vec(reader, |v| v, parse_core_instance)?;
            }
            ComponentSectionType::CoreType => {
                let (_, core_types) = parse_vec(reader, |v| v, parse_core_type)?;
            }
            ComponentSectionType::Component => {
                let component = parse_component(reader)?;
            }
            ComponentSectionType::Instance => {
                let (_, instances) = parse_vec(reader, |v| v, parse_instance)?;
            }
            ComponentSectionType::Alias => {
                let (_, aliases) = parse_vec(reader, |v| v, alias::parse_alias)?;
            }
            ComponentSectionType::Type => {}
            ComponentSectionType::Canon => {}
            ComponentSectionType::Start => {}
            ComponentSectionType::Import => {}
            ComponentSectionType::Export => {}
            ComponentSectionType::Value => {}
        }
    }
    Ok(Component {})
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
