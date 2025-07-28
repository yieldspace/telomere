use crate::common::custom_section::{
    FuncNameSubSec, LocalNameSubSec, ModuleNameSubSec, NameSubSection,
};
use binary_reader::BinaryReader;

use super::base::WasmBaseParser;
use super::{Result, WasmParserError};

type IndirectNameAssoc = (u32, Vec<(u32, String)>);
pub struct CustomSectionParser<'a, R: BinaryReader> {
    reader: &'a mut R,
}
impl<R: BinaryReader> WasmBaseParser<R> for CustomSectionParser<'_, R> {
    fn reader(&mut self) -> &mut R {
        self.reader
    }
}
#[repr(u8)]
enum NameSubsectionId {
    ModuleName = 0,
    FunctionName = 1,
    LocalName = 2,
    Unknown,
}
impl<'a, R: BinaryReader> CustomSectionParser<'a, R> {
    fn parse_name_subsec_id(&mut self) -> Result<Option<NameSubsectionId>> {
        let v = self.reader.read_one()?;
        let r = match v {
            Some(0) => Some(NameSubsectionId::ModuleName),
            Some(1) => Some(NameSubsectionId::FunctionName),
            Some(2) => Some(NameSubsectionId::LocalName),
            Some(_other) => Some(NameSubsectionId::Unknown),
            None => None,
        };
        Ok(r)
    }
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }
    pub fn parse_name_subsec(&mut self) -> Result<NameSubSection> {
        let mut mod_name_subsec = None;
        let mut func_name_subsec = None;
        let mut local_name_subsec = None;
        while let Some(id) = self.parse_name_subsec_id()? {
            match id {
                NameSubsectionId::Unknown => {
                    self.parse_unknown_sub_sec()?;
                }
                NameSubsectionId::ModuleName => {
                    mod_name_subsec = Some(self.parse_module_name_sub_sec()?);
                }
                NameSubsectionId::FunctionName => {
                    func_name_subsec = Some(self.parse_func_name_sub_sec()?);
                }
                NameSubsectionId::LocalName => {
                    local_name_subsec = Some(self.parse_local_name_sub_sec()?);
                }
            }
        }
        Ok(NameSubSection {
            function_name: func_name_subsec,
            local_name: local_name_subsec,
            module_name: mod_name_subsec,
        })
    }
    fn parse_module_name_sub_sec(&mut self) -> Result<ModuleNameSubSec> {
        let (_len, size) = self.parse_u32()?;
        let (len, name) = self.parse_name()?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ModuleNameSubSec(name))
    }
    fn parse_name_assoc(&mut self) -> Result<(usize, (u32, String))> {
        let (len, idx) = self.parse_u32()?;
        let (len2, name) = self.parse_name()?;
        Ok((len + len2, (idx, name)))
    }
    fn parse_indirect_name_assoc(&mut self) -> Result<(usize, IndirectNameAssoc)> {
        let (len1, idx) = self.parse_u32()?;
        let (len2, namemap) = self.parse_vec(Self::parse_name_assoc)?;
        Ok((len1 + len2, (idx, namemap)))
    }

    fn parse_local_name_sub_sec(&mut self) -> Result<LocalNameSubSec> {
        let (_len, size) = self.parse_u32()?;
        let (len, namemap) = self.parse_vec(Self::parse_indirect_name_assoc)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(LocalNameSubSec(namemap))
    }
    fn parse_func_name_sub_sec(&mut self) -> Result<FuncNameSubSec> {
        let (_len, size) = self.parse_u32()?;
        let (len, namemap) = self.parse_vec(Self::parse_name_assoc)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(FuncNameSubSec(namemap))
    }
    fn parse_unknown_sub_sec(&mut self) -> Result<()> {
        let (_len, size) = self.parse_u32()?;
        self.skip_section(size)?;
        Ok(())
    }
    fn skip_section(&mut self, size: u32) -> Result<()> {
        for _idx in 0..size {
            self.reader.read_exact_one()?;
        }
        Ok(())
    }
}
