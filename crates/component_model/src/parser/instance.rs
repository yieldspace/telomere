use crate::name::{ExportName, ImportName};
use crate::parser::component::RawData;
use crate::parser::idx::RawComponentIdx;
use crate::parser::sort::Sort;
use crate::parser::ComponentParser;
use crate::{ComponentParseError, Result};
use binary_reader::BinaryReader;
use std::collections::HashSet;
use tracing::trace;

pub struct RawInstance {
    pub component_idx: RawComponentIdx,
    pub args: Vec<(ImportName, Sort)>,
}

pub struct RawInstanceInlineExport {
    pub exports: Vec<(ExportName, Sort)>,
}

pub enum RawInstanceDef {
    Instantiate(RawInstance),
    InlineExport(RawInstanceInlineExport),
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub(crate) fn parse_instance(&mut self) -> Result<()> {
        trace!("parse_instance");
        match self.reader.read_exact_one()? {
            0x00 => self.parse_instantiate(),
            0x01 => self.parse_instantiate_inline_export(),
            x => Err(ComponentParseError::InvalidInstanceType(x)),
        }
    }

    fn parse_instantiate(&mut self) -> Result<()> {
        trace!("parse_instantiate");
        let component_idx = self.parse_component_idx()?;
        let args = {
            let mut name_unique = HashSet::new();
            self.parse_vec(move |slf| {
                let (name, sort) = slf.parse_instantiate_arg()?;
                if name_unique.contains(&name) {
                    Err(ComponentParseError::InvalidName(
                        "Duplicated target import name".to_owned(),
                    ))?
                } else {
                    name_unique.insert(name.clone());
                    Ok((name, sort))
                }
            })?
        };
        let instance = RawInstance {
            component_idx,
            args,
        };
        self.instances
            .push(RawData::Defined(RawInstanceDef::Instantiate(instance)))?;
        Ok(())
    }

    fn parse_instantiate_arg(&mut self) -> Result<(ImportName, Sort)> {
        let name = self.parse_import_name()?;
        let sort = self.parse_sort()?;
        Ok((name, sort))
    }

    fn parse_instantiate_inline_export(&mut self) -> Result<()> {
        let mut name_unique = HashSet::new();
        let exports = self.parse_vec(|slf| {
            let (name, sort) = slf.parse_inline_export_arg()?;
            if name_unique.contains(&name) {
                Err(ComponentParseError::InvalidName(
                    "Duplicated inline export name".to_owned(),
                ))?
            } else {
                name_unique.insert(name.clone());
                Ok((name, sort))
            }
        })?;
        let instance = RawInstanceInlineExport { exports };
        self.instances
            .push(RawData::Defined(RawInstanceDef::InlineExport(instance)))
            .unwrap();
        Ok(())
    }

    fn parse_inline_export_arg(&mut self) -> Result<(ExportName, Sort)> {
        let name = self.parse_export_name()?;
        let sort = self.parse_sort()?;
        Ok((name, sort))
    }
}
