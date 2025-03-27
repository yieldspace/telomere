use crate::binary::BinaryReader;
use crate::component::{
    Component, CoreInstance, CoreInstanceArg, CoreInstanceInlineExport, CoreInstantiate, CoreSort,
};
use crate::parser::component::section::ComponentSectionType;
use crate::parser::core::{parse_name, parse_u32};
use crate::parser::leb128::Leb128Parser;
use crate::{Module, WasmParser, WasmParserError};
use thiserror::Error;

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
    #[error("invalid section type: {0:?}")]
    InvalidSectionType(u8),
    #[error("invalid instance expression: {0:?}")]
    InvalidInstanceExpr(u8),
    #[error("invalid instantiate arg magic: {0:?}")]
    InvalidInstantiateArgMagic(u8),
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
        loop {
            let section_type = if let Some(st) = self.parse_section_type()? {
                st
            } else {
                break;
            };
            let (_, size) = Leb128Parser::new(self.reader).parse_u32(size_of::<u32>() * 8)?;
            match section_type {
                ComponentSectionType::Custom => {
                    for _ in 0..size {
                        self.reader.read_exact_one()?;
                    }
                }
                ComponentSectionType::CoreModule => {
                    let module = self.parse_core_module(size as usize)?;
                }
                ComponentSectionType::CoreInstance => {
                    let instance = self.parse_core_instance(size as usize)?;
                }
                ComponentSectionType::CoreType => {}
                ComponentSectionType::Component => {}
                ComponentSectionType::Instance => {}
                ComponentSectionType::Alias => {}
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

    fn parse_section_type(&mut self) -> Result<Option<ComponentSectionType>> {
        if let Some(kind) = self.reader.read_one()? {
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

    fn parse_core_module(&mut self, size: usize) -> Result<Module> {
        let mut core_reader = self.reader.take(size as usize);
        let mut core_module = WasmParser::new(&mut core_reader);
        let module = core_module.parse_module()?;
        Ok(module)
    }

    fn parse_core_instance(&mut self, size: usize) -> Result<CoreInstance> {
        let mut view = self.reader.take(size);
        match self.reader.read_exact_one()? {
            0x00 => {
                let (_, idx) = parse_u32(&mut view)?;
                // parse args
                let mut read = 0;
                let mut args = Vec::new();
                while read < size {
                    let (name_len, name) = parse_name(&mut view)?;
                    read += name_len;
                    match view.read_exact_one()? {
                        0x12 => {}
                        magic => {
                            return Err(ComponentModelParserError::InvalidInstantiateArgMagic(
                                magic,
                            ))
                        }
                    }
                    read += 1;
                    let (idx_len, instance_idx) = parse_u32(&mut view)?;
                    read += idx_len;
                    args.push(CoreInstanceArg {
                        name,
                        instance_idx: instance_idx as usize,
                    })
                }
                Ok(CoreInstance::Instantiate(CoreInstantiate {
                    module_idx: idx as usize,
                    args,
                }))
            }
            0x01 => {
                // parse inline export
                let mut read = 0;
                let mut inline_exports = Vec::new();
                while read < size {
                    let (name_len, name) = parse_name(&mut view)?;
                    read += name_len;
                    let sort = match view.read_exact_one()? {
                        0x00 => CoreSort::Func,
                        0x01 => CoreSort::Table,
                        0x02 => CoreSort::Memory,
                        0x03 => CoreSort::Global,
                        0x10 => CoreSort::Type,
                        0x11 => CoreSort::Module,
                        0x12 => CoreSort::Instance,
                        magic => return Err(ComponentModelParserError::InvalidInstanceExpr(magic)),
                    };
                    read += 1;
                    let (idx_len, sort_idx) = parse_u32(&mut view)?;
                    read += idx_len;
                    inline_exports.push(CoreInstanceInlineExport {
                        name,
                        sort,
                        sort_idx: sort_idx as usize,
                    });
                }
                Ok(CoreInstance::InlineExport(inline_exports))
            }
            magic => Err(ComponentModelParserError::InvalidInstanceExpr(magic)),
        }
    }
}
