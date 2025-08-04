use crate::name::ImportName;
use crate::parser::sort::Sort;
use crate::parser::ComponentParser;
use crate::{ComponentParseError, Result};
use binary_reader::BinaryReader;
use std::collections::HashSet;

pub struct RawInstance {}

impl<'a, T> ComponentParser<'a, T>
where
    T: BinaryReader,
{
    pub(crate) fn parse_instance(&mut self) -> Result<()> {
        match self.reader.read_exact_one()? {
            0x00 => self.parse_instantiate(),
            0x01 => self.parse_instantiate_inline_export(),
            x => Err(ComponentParseError::InvalidInstanceType(x)),
        }
    }

    fn parse_instantiate(&mut self) -> Result<()> {
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
        Ok(())
    }

    fn parse_instantiate_arg(&mut self) -> Result<(ImportName, Sort)> {
        let name = self.parse_import_name()?;
        let sort = self.parse_sort()?;
        Ok((name, sort))
    }

    fn parse_instantiate_inline_export(&mut self) -> Result<()> {
        // Implementation for parsing an inline export instance
        // This will involve reading the inline export data and populating the RawInstance struct
        Ok(())
    }
}
