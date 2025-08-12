use crate::Result;
use crate::parser::component::{RawCoreData, RawData};
use crate::parser::idx::{RawComponentIdx, RawCoreModuleIdx, RawInstanceIdx};
use crate::parser::sort::{CoreSortType, SortType};
use crate::parser::vec::RawIdx;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;
use telomere_wasm::parser::core::{parse_name, parse_u32};

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_alias(&mut self) -> Result<()> {
        let sort = self.parse_sort_type()?;
        match (self.reader.read_exact_one()?, sort) {
            (0x00, sort) => self.parse_export_alias(sort),
            (0x01, SortType::Core(cs)) => self.parse_core_export_alias(cs),
            (0x02, sort) => self.parse_outer_export_alias(sort),
            (x, _) => Err(ComponentParseError::InvalidAliasType(x)),
        }
    }

    fn parse_export_alias(&mut self, sort: SortType) -> Result<()> {
        let instance_idx = self.parse_instance_idx()?;
        let name = self.parse_export_name()?;
        match sort {
            SortType::Core(CoreSortType::Module) => {
                self.core_modules
                    .push(RawCoreData::ReExportedModule(name, instance_idx))?;
            }
            SortType::Func => {
                self.funcs.push(RawData::ReExported(name, instance_idx))?;
            }
            SortType::Type => {
                self.components
                    .push(RawData::ReExported(name, instance_idx))?;
            }
            SortType::Component => {
                self.components
                    .push(RawData::ReExported(name, instance_idx))?;
            }
            SortType::Instance => {
                self.instances
                    .push(RawData::ReExported(name, instance_idx))?;
            }
            _ => return Err(ComponentParseError::InvalidSortType(0)),
        }
        Ok(())
    }

    fn parse_core_export_alias(&mut self, sort: CoreSortType) -> Result<()> {
        let core_instance_idx = self.parse_core_instance_idx()?;
        let (_, name) = parse_name(self.reader)?;
        match sort {
            CoreSortType::Func => {
                self.core_funcs
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
            CoreSortType::Table => {
                self.core_modules
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
            CoreSortType::Memory => {
                self.core_modules
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
            CoreSortType::Global => {
                self.core_modules
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
            CoreSortType::Type => {
                self.core_modules
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
            CoreSortType::Module => {
                self.core_modules
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
            CoreSortType::Instance => {
                self.core_instances
                    .push(RawCoreData::ReExported(name, core_instance_idx))?;
            }
        }
        Ok(())
    }

    fn parse_outer_export_alias(&mut self, sort: SortType) -> Result<()> {
        let (_, ct) = parse_u32(self.reader)?;
        let (_, idx) = parse_u32(self.reader)?;
        match sort {
            SortType::Core(CoreSortType::Module) => {
                self.core_modules
                    .push_alias(RawCoreModuleIdx::new_outer(ct, idx))?;
            }
            SortType::Func => {
                self.funcs.push_alias(RawIdx::new_outer(ct, idx))?;
            }
            SortType::Type => {
                self.components.push_alias(RawIdx::new_outer(ct, idx))?;
            }
            SortType::Component => {
                self.components
                    .push_alias(RawComponentIdx::new_outer(ct, idx))?;
            }
            SortType::Instance => {
                self.instances
                    .push_alias(RawInstanceIdx::new_outer(ct, idx))?;
            }
            _ => return Err(ComponentParseError::InvalidSortType(0)),
        };
        Ok(())
    }
}
