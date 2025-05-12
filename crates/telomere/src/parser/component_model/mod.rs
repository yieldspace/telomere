use crate::binary::BinaryReader;
use crate::parser::core::parse_u32;
use crate::WasmParserError;
use std::ops::Range;
use tracing::trace;

use crate::component_model::ComponentSection;
pub use component::parse_component;
pub use context::ParseContext;
pub use core::*;
pub use error::ComponentParseError;
pub use idx::*;
pub use validator::{Validator, ValidatorState};

mod component;
mod context;
mod error;
mod export;
mod idx;
mod import;
mod name;
mod sort;
mod types;
mod validator;
mod instance;
pub use validator::ScopeGuard;

pub type SizedResult<T> = std::result::Result<(usize, T), ComponentParseError>;
pub type ParseResult<T> = std::result::Result<T, ComponentParseError>;

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
) -> Result<Option<ComponentSection>, ComponentParseError> {
    if let Some(kind) = reader.read_one()? {
        match kind {
            0x00 => Ok(Some(ComponentSection::Custom)),
            0x01 => Ok(Some(ComponentSection::CoreModule)),
            0x02 => Ok(Some(ComponentSection::CoreInstance)),
            0x03 => Ok(Some(ComponentSection::CoreType)),
            0x04 => Ok(Some(ComponentSection::Component)),
            0x05 => Ok(Some(ComponentSection::Instance)),
            0x06 => Ok(Some(ComponentSection::Alias)),
            0x07 => Ok(Some(ComponentSection::Type)),
            0x08 => Ok(Some(ComponentSection::Canon)),
            0x09 => Ok(Some(ComponentSection::Start)),
            0x0a => Ok(Some(ComponentSection::Import)),
            0x0b => Ok(Some(ComponentSection::Export)),
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            0x0c => Ok(Some(ComponentSection::Value)),
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
    mut map: impl FnMut(&mut A, V),
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

pub(crate) fn parse_option<R: BinaryReader, T, E>(
    ctx: &mut ParseContext<R>,
    mut f: impl FnMut(&mut ParseContext<R>) -> Result<T, E>,
) -> ParseResult<Option<T>>
where
    ComponentParseError: From<E>,
{
    match ctx.reader.read_exact_one()? {
        0x00 => Ok(None),
        0x01 => {
            let t = f(ctx)?;
            Ok(Some(t))
        }
        x => {
            println!("{x}");
            Err(ComponentParseError::WrongMagic(x, "option".to_string()))
        }
    }
}

pub(crate) fn parse_vec_range(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<Range<u32>, ComponentParseError> {
    let (_, size) = parse_u32(ctx.reader)?;
    Ok(0..size)
}
