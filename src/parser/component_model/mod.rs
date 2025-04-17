use crate::binary::BinaryReader;
use crate::parser::core::parse_u32;
use crate::WasmParserError;
use tracing::trace;

pub use alias::*;
pub use component::parse_component;
pub use context::ParseContext;
pub use core::*;
pub use error::ComponentParseError;
pub use idx::*;
pub use instance::*;
pub use section::*;
pub use sort::*;
pub(crate) use validator::Validator;
pub use validator::{ChildValidator, ComponentValidator};

mod alias;
mod component;
mod context;
mod core;
mod error;
mod idx;
mod instance;
mod section;
mod sort;
mod validator;

pub type SizedResult<T> = std::result::Result<(usize, T), ComponentParseError>;

pub fn parse_magic<R: BinaryReader>(reader: &mut R) -> Result<(), ComponentParseError> {
    let magic = reader.read_exact::<4>()?;
    if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
        Ok(())
    } else {
        Err(ComponentParseError::InvalidMagic(
            Box::new(magic),
            Box::new([0x00, 0x61, 0x73, 0x6d]),
            "component".to_string(),
        ))
    }
}

pub fn parse_version<R: BinaryReader>(reader: &mut R) -> Result<(), ComponentParseError> {
    let version = reader.read_exact::<2>()?;
    if matches!(&version, &[0x0d, 0x00]) {
        Ok(())
    } else {
        Err(ComponentParseError::InvalidVersion(version))
    }
}

pub fn parse_layer<R: BinaryReader>(reader: &mut R) -> Result<(), ComponentParseError> {
    let layer = reader.read_exact::<2>()?;
    if matches!(&layer, &[0x01, 0x00]) {
        Ok(())
    } else {
        Err(ComponentParseError::InvalidLayer(layer))
    }
}

pub fn parse_section_type<R: BinaryReader>(
    reader: &mut R,
) -> Result<Option<ComponentSectionType>, ComponentParseError> {
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
            _ => Err(ComponentParseError::InvalidSectionType(kind)),
        }
    } else {
        Ok(None)
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
