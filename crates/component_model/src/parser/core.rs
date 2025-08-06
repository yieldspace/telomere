use crate::parser::component::RawCoreData;
use crate::parser::idx::{RawCoreInstanceIdx, RawCoreModuleIdx};
use crate::parser::sort::CoreSort;
use crate::parser::ComponentParser;
use crate::{ComponentParseError, Result};
use binary_reader::BinaryReader;
use telomere_wasm::parser::core::parse_name;

pub enum CoreInstanceDef {
    Instantiate {
        module_idx: RawCoreModuleIdx,
        args: Vec<(String, RawCoreInstanceIdx)>,
    },
    InlineExport,
}

impl<T> ComponentParser<'_, '_, T>
where
    T: BinaryReader,
{
    pub(crate) fn parse_core_instance(&mut self) -> Result<()> {
        match self.reader.read_exact_one()? {
            0x00 => self.parse_core_instance_with_arg()?,
            0x01 => self.parse_core_instance_inline_export()?,
            x => return Err(ComponentParseError::InvalidCoreInstanceType(x)),
        }
        Ok(())
    }

    fn parse_core_instance_with_arg(&mut self) -> Result<()> {
        let module_idx = self.parse_core_module_idx()?;
        let args = {
            let mut name_unique = std::collections::HashSet::new();
            self.parse_vec(move |slf| {
                let (name, core_instance_idx) = slf.parse_core_instantiate_arg()?;
                if name_unique.contains(&name) {
                    Err(ComponentParseError::InvalidName(
                        "Duplicated core instance argument name".to_owned(),
                    ))?
                } else {
                    name_unique.insert(name.clone());
                    Ok((name, core_instance_idx))
                }
            })?
        };
        self.core_instances
            .push(RawCoreData::Defined(CoreInstanceDef::Instantiate {
                module_idx,
                args,
            }))?;
        Ok(())
    }

    fn parse_core_instantiate_arg(&mut self) -> Result<(String, RawCoreInstanceIdx)> {
        let (_, name) = parse_name(self.reader)?;
        let magic = self.reader.read_exact_one()?;
        if magic != 0x12 {
            return Err(ComponentParseError::InvalidSignature(
                Box::new([magic]),
                Box::new([0x12]),
                "core instance arg".to_string(),
            ));
        }
        let core_instance_idx = self.parse_core_instance_idx()?;
        Ok((name, core_instance_idx))
    }

    fn parse_core_instance_inline_export(&mut self) -> Result<()> {
        Ok(())
    }

    fn parese_inline_export(&mut self) -> Result<(String, CoreSort)> {
        let (_, name) = parse_name(self.reader)?;
        let cs = self.parse_core_sort()?;
        Ok((name, cs))
    }
}
