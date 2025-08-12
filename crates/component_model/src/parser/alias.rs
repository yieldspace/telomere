use crate::Result;
use crate::parser::component::{RawCoreData, RawData};
use crate::parser::idx::{RawComponentIdx, RawCoreModuleIdx, RawInstanceIdx};
use crate::parser::sort::{CoreSortType, SortType};
use crate::parser::vec::RawIdx;
use crate::types::{AliasTarget, Relation, TypeId, TypeIdx};
use crate::vec::Idx;
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
        let instance_id = self.validator.locals.get_instance_type(&instance_idx)?;
        let name = self.parse_export_name()?;
        let id = self
            .validator
            .store
            .push_alias_in_type(AliasTarget::InstanceExportType {
                instance_type_id: instance_id,
                name: name.clone(),
            });
        match sort {
            SortType::Core(CoreSortType::Module) => {
                self.core_modules
                    .push(RawCoreData::ReExportedModule(name, instance_idx))?;
            }
            SortType::Func => {
                let idx = self.funcs.push(RawData::ReExported(name, instance_idx))?;
                let func_id = self.validator.store.push_func_in_type(Relation::Alias(id));
                self.validator.locals.push_func(idx, func_id);
            }
            SortType::Type => {
                self.components
                    .push(RawData::ReExported(name, instance_idx))?;
                self.validator.locals.register_type_idx(TypeId::Alias(id));
            }
            SortType::Component => {
                let idx = self
                    .components
                    .push(RawData::ReExported(name, instance_idx))?;
                self.validator.locals.push_component(
                    idx,
                    self.validator
                        .store
                        .push_component_in_type(Relation::Alias(id)),
                );
            }
            SortType::Instance => {
                let idx = self
                    .instances
                    .push(RawData::ReExported(name, instance_idx))?;

                self.validator.locals.push_instance(
                    idx,
                    self.validator
                        .store
                        .push_instance_in_type(Relation::Alias(id)),
                );
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
                let idx = self
                    .core_funcs
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
        let levels = self
            .outer
            .clone()
            .drain(..self.outer.len() - (ct as usize))
            .collect();
        let id = self
            .validator
            .store
            .push_alias_in_type(AliasTarget::OuterType { levels, index: idx });
        match sort {
            SortType::Core(CoreSortType::Module) => {
                self.core_modules
                    .push_alias(RawCoreModuleIdx::new_outer(ct, idx))?;
            }
            SortType::Func => {
                let idx = self.funcs.push_alias(RawIdx::new_outer(ct, idx))?;
                let id = self.validator.store.push_func_in_type(Relation::Alias(id));
                self.validator.locals.push_func(idx, id);
            }
            SortType::Type => {
                self.components.push_alias(RawIdx::new_outer(ct, idx))?;
                self.validator.locals.register_type_idx(TypeId::Alias(id));
            }
            SortType::Component => {
                let idx = self
                    .components
                    .push_alias(RawComponentIdx::new_outer(ct, idx))?;
                let id = self
                    .validator
                    .store
                    .push_component_in_type(Relation::Alias(id));
                self.validator.locals.push_component(idx, id);
            }
            SortType::Instance => {
                let idx = self
                    .instances
                    .push_alias(RawInstanceIdx::new_outer(ct, idx))?;
                let id = self
                    .validator
                    .store
                    .push_instance_in_type(Relation::Alias(id));
                self.validator.locals.push_instance(idx, id);
            }
            _ => return Err(ComponentParseError::InvalidSortType(0)),
        };
        Ok(())
    }
}
