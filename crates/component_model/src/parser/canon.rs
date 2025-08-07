use crate::canon::StringEncoding;
use crate::parser::component::{RawCoreData, RawData};
use crate::parser::idx::{RawCoreFuncIdx, RawCoreMemoryIdx, RawFuncIdx, RawTypeIdx};
use crate::types::TypeIdx;
use crate::Result;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;

#[derive(Default)]
pub struct RawCanonOpt {
    pub string_encoding: Option<StringEncoding>,
    pub memory: Option<RawCoreMemoryIdx>,
    pub realloc: Option<RawCoreFuncIdx>,
    pub post_return: Option<RawCoreFuncIdx>,
    #[cfg(feature = "async")]
    pub is_async: Option<bool>,
    #[cfg(feature = "async")]
    pub callback: Option<RawCoreFuncIdx>,
}

impl RawCanonOpt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_string_encoding(&mut self, encoding: StringEncoding) -> Result<()> {
        if self.string_encoding.is_some() {
            return Err(ComponentParseError::InvalidCanonOpt(
                "string encoding already set".into(),
            ));
        }
        self.string_encoding = Some(encoding);
        Ok(())
    }

    pub fn set_memory(&mut self, memory: RawCoreMemoryIdx) -> Result<()> {
        if self.memory.is_some() {
            return Err(ComponentParseError::InvalidCanonOpt(
                "memory already set".into(),
            ));
        }
        self.memory = Some(memory);
        Ok(())
    }

    pub fn set_realloc(&mut self, realloc: RawCoreFuncIdx) -> Result<()> {
        if self.realloc.is_some() {
            return Err(ComponentParseError::InvalidCanonOpt(
                "realloc already set".into(),
            ));
        }
        self.realloc = Some(realloc);
        Ok(())
    }

    pub fn set_post_return(&mut self, post_return: RawCoreFuncIdx) -> Result<()> {
        if self.post_return.is_some() {
            return Err(ComponentParseError::InvalidCanonOpt(
                "post_return already set".into(),
            ));
        }
        self.post_return = Some(post_return);
        Ok(())
    }
}

pub struct RawFunction {
    pub core_func_index: RawCoreFuncIdx,
    pub opt: RawCanonOpt,
    pub ft: RawTypeIdx,
    // type
}

pub enum RawLowerAdaptor {
    Lower(RawFuncIdx, RawCanonOpt),
    ResourceNew(RawTypeIdx),
    ResourceDrop(RawTypeIdx),
    ResourceRep(RawTypeIdx),
}

pub enum RawCoreFunction {
    Ref(),
    Lower(RawLowerAdaptor),
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_canon(&mut self) -> Result<()> {
        match self.reader.read_exact_one()? {
            0x00 if self.reader.read_exact_one()? == 0x00 => self.parse_canon_lift(),
            0x01 if self.reader.read_exact_one()? == 0x00 => self.parse_canon_lower(),
            0x02 => self.parse_resource_new(),
            0x03 => self.parse_resource_drop(),
            0x04 => self.parse_resource_rep(),
            x => Err(ComponentParseError::InvalidCanonType(x)),
        }
    }

    fn parse_canon_lift(&mut self) -> Result<()> {
        let core_func_index = self.parse_core_func_idx()?;
        let opt = self.parse_canon_opts()?;
        let ft = self.parse_type_idx()?;
        let func = RawFunction {
            core_func_index,
            opt,
            ft,
        };
        self.funcs.push(RawData::Defined(func))?;
        Ok(())
    }

    fn parse_canon_lower(&mut self) -> Result<()> {
        let func_idx = self.parse_func_idx()?;
        let opt = self.parse_canon_opts()?;
        let adaptor = RawLowerAdaptor::Lower(func_idx, opt);
        self.core_funcs
            .push(RawCoreData::Defined(RawCoreFunction::Lower(adaptor)))?;
        Ok(())
    }

    fn parse_resource_new(&mut self) -> Result<()> {
        let type_idx = self.parse_type_idx()?;
        let adaptor = RawLowerAdaptor::ResourceNew(type_idx);
        self.core_funcs
            .push(RawCoreData::Defined(RawCoreFunction::Lower(adaptor)))?;
        Ok(())
    }

    fn parse_resource_drop(&mut self) -> Result<()> {
        let type_idx = self.parse_type_idx()?;
        let adaptor = RawLowerAdaptor::ResourceDrop(type_idx);
        self.core_funcs
            .push(RawCoreData::Defined(RawCoreFunction::Lower(adaptor)))?;
        Ok(())
    }

    fn parse_resource_rep(&mut self) -> Result<()> {
        let type_idx = self.parse_type_idx()?;
        let adaptor = RawLowerAdaptor::ResourceRep(type_idx);
        self.core_funcs
            .push(RawCoreData::Defined(RawCoreFunction::Lower(adaptor)))?;
        Ok(())
    }

    fn parse_canon_opts(&mut self) -> Result<RawCanonOpt> {
        let mut opt = RawCanonOpt::new();
        self.parse_vec(|slf| match slf.reader.read_exact_one()? {
            0x00 => opt.set_string_encoding(StringEncoding::Utf8),
            0x01 => opt.set_string_encoding(StringEncoding::Utf16),
            0x02 => opt.set_string_encoding(StringEncoding::Latin1Utf16),
            0x03 => {
                let memory_idx = slf.parse_core_memory_idx()?;
                opt.set_memory(memory_idx)
            }
            0x04 => {
                let realloc_idx = slf.parse_core_func_idx()?;
                opt.set_realloc(realloc_idx)
            }
            0x05 => {
                let post_return_idx = slf.parse_core_func_idx()?;
                opt.set_post_return(post_return_idx)
            }
            #[cfg(feature = "async")]
            0x06 => todo!(),
            #[cfg(feature = "async")]
            0x07 => todo!(),
            x => Err(ComponentParseError::InvalidCanonOpt(x.to_string())),
        })?;
        Ok(opt)
    }
}
