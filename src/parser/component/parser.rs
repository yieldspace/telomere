use thiserror::Error;
use crate::binary::BinaryReader;
use crate::component::Component;
use crate::WasmParserError;

pub type Result<R> = std::result::Result<R, ComponentModelParserError>;

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
}



pub struct ComponentModelParser<'a, R: BinaryReader> {
    reader: &'a mut R,
}
impl<'a, R: BinaryReader> ComponentModelParser<'a, R> {
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }

    fn parse_component(&mut self) -> Result<Component> {
        self.parse_magic()?;
        self.parse_version()?;
        self.parse_layer()?;
        Ok(Component {})
    }

    fn parse_magic(&mut self) -> Result<()> {
        let magic = self.reader.read_exact::<4>()?;
        if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
            Ok(())
        } else {
            Err(ComponentModelParserError::InvalidMagic(magic))
        }
    }

    fn parse_version(&mut self) -> Result<()> {
        let version = self.reader.read_exact::<2>()?;
        if matches!(&version, &[0x0d, 0x00]) {
            Ok(())
        } else {
            Err(ComponentModelParserError::InvalidVersion(version))
        }
    }

    fn parse_layer(&mut self) -> Result<()> {
        let layer = self.reader.read_exact::<2>()?;
        if matches!(&layer, &[0x01, 0x00]) {
            Ok(())
        } else {
            Err(ComponentModelParserError::InvalidLayer(layer))
        }
    }
}
