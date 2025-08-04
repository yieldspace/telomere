mod component;
mod core;
mod idx;
mod instance;
mod name;
mod section_type;
mod sort;
mod vec;

use crate::parser::component::{RawComponent, RawCoreData, RawData};
use crate::parser::idx::{RawComponentIdx, RawCoreInstanceIdx, RawCoreModuleIdx, RawInstanceIdx};
use crate::parser::instance::RawInstance;
use crate::parser::section_type::ComponentSection;
use crate::parser::vec::RawIndexVec;
use crate::{Component, ComponentParseError, InstantiateContext, Result};
use binary_reader::BinaryReader;
use telomere_wasm::parser::core::parse_u32;
use telomere_wasm::WasmParser;

pub struct ComponentParser<'a, T: BinaryReader> {
    reader: &'a mut T,
    components: RawIndexVec<RawComponentIdx, RawData<RawComponent>>,
    instances: RawIndexVec<RawInstanceIdx, RawData<RawInstance>>,
    core_modules: RawIndexVec<RawCoreModuleIdx, RawCoreData<telomere_wasm::Module>>,
    core_instances: RawIndexVec<RawCoreInstanceIdx, RawCoreData<()>>,
}

impl<'a, T> ComponentParser<'a, T>
where
    T: BinaryReader,
{
    pub fn new(reader: &'a mut T) -> Self {
        Self {
            reader,
            components: RawIndexVec::with_capacity(256),
            instances: RawIndexVec::with_capacity(256),
            core_modules: RawIndexVec::with_capacity(256),
            core_instances: RawIndexVec::with_capacity(256),
        }
    }

    pub fn parse_vec<V>(&mut self, mut func: impl FnMut(&mut Self) -> Result<V>) -> Result<Vec<V>> {
        let (_, count) = parse_u32(self.reader)?;
        let mut vec = Vec::with_capacity(count as usize);
        for _ in 0..count {
            vec.push(func(self)?);
        }
        Ok(vec)
    }

    fn parse_magic(&mut self) -> Result<()> {
        let magic = self.reader.read_exact::<4>()?;
        if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
            Ok(())
        } else {
            Err(ComponentParseError::InvalidSignature(
                Box::new(magic),
                Box::new([0x00, 0x61, 0x73, 0x6d]),
                "magic".to_string(),
            ))
        }
    }

    fn parse_version(&mut self) -> Result<()> {
        let version = self.reader.read_exact::<2>()?;
        if matches!(&version, &[0x0d, 0x00]) {
            Ok(())
        } else {
            Err(ComponentParseError::InvalidSignature(
                Box::new(version),
                Box::new([0x0d, 0x00]),
                "version".to_string(),
            ))
        }
    }

    fn parse_layer(&mut self) -> Result<()> {
        let layer = self.reader.read_exact::<2>()?;
        if matches!(&layer, &[0x01, 0x00]) {
            Ok(())
        } else {
            Err(ComponentParseError::InvalidSignature(
                Box::new(layer),
                Box::new([0x01, 0x00]),
                "layer".to_string(),
            ))
        }
    }

    fn parse_section_type(&mut self) -> Result<Option<ComponentSection>> {
        if let Some(kind) = self.reader.read_one()? {
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
                #[cfg(feature = "value-imports-exports")]
                0x0c => Ok(Some(ComponentSection::Value)),
                _ => Err(ComponentParseError::InvalidSignature(
                    Box::new([kind]),
                    Box::new([0x00]),
                    "section type".into(),
                )),
            }
        } else {
            Ok(None)
        }
    }

    pub fn parse(mut self) -> Result<RawComponent> {
        self.parse_magic()?;
        self.parse_version()?;
        self.parse_layer()?;

        while let Some(st) = self.parse_section_type()? {
            let (_, section_size) = parse_u32(self.reader)?;
            match st {
                ComponentSection::Custom => {
                    let mut data = Vec::with_capacity(section_size as usize);
                    assert_eq!(
                        section_size as usize,
                        self.reader.read_slice(data.as_mut_slice())?
                    );
                }
                ComponentSection::CoreModule => {
                    let module = {
                        let mut sized_reader = self.reader.take(section_size as usize);
                        let mut core_module = WasmParser::new(&mut sized_reader);
                        core_module.parse_module()
                    }?;
                    let _idx = self.core_modules.push(RawCoreData::Defined(module));
                }
                ComponentSection::CoreInstance => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_core_instance()?;
                    }
                }
                ComponentSection::CoreType => todo!(),
                ComponentSection::Component => {
                    let component = {
                        let mut sized_reader = self.reader.take(section_size as usize);
                        let parser = ComponentParser::new(&mut sized_reader);
                        parser.parse()?
                    };
                    let _idx = self.components.push(RawData::Defined(component));
                }
                ComponentSection::Instance => {
                    let (_, count) = parse_u32(self.reader)?;
                    for _ in 0..count {
                        self.parse_instance()?
                    }
                }
                ComponentSection::Alias => {
                    // Parse alias section
                    // Implementation omitted for brevity
                }
                ComponentSection::Type => {
                    // Parse type section
                    // Implementation omitted for brevity
                }
                ComponentSection::Canon => {
                    // Parse canon section
                    // Implementation omitted for brevity
                }
                ComponentSection::Start => {
                    // Parse start section
                    // Implementation omitted for brevity
                }
                ComponentSection::Import => {
                    // Parse import section
                    // Implementation omitted for brevity
                }
                ComponentSection::Export => {
                    // Parse export section
                    // Implementation omitted for brevity
                }
            }
        }
        Ok(RawComponent {
            imports: Default::default(),
            exports: Default::default(),
        })
    }
}
