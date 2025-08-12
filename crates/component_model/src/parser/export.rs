use crate::ComponentParser;
use crate::Result;
use crate::parser::idx::RawExportId;
use crate::parser::sort::{CoreSort, Sort};
use crate::parser::types::{RawExternDesc, TypeBound};
use crate::types::TypeId;
use crate::types::component::PublicTyRef;
use crate::types::resource::ResourceDef;
use binary_reader::BinaryReader;

pub enum RawExport {
    CoreModule,
    Func,
    Component,
    Instance,
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_export(&mut self) -> Result<()> {
        let name = self.parse_export_name_dash()?;
        let sort = self.parse_sort()?;
        let desc = self
            .parse_option()?
            .map(Self::parse_externdesc)
            .transpose()?;
        let id = RawExportId::new(&name);
        // todo: sortの型がdescの型のsubsetであることを確認する
        match sort {
            Sort::Core(CoreSort::Module(_)) => {
                self.exports.insert(id, RawExport::CoreModule);

                // todo
            }
            Sort::Func(idx) => {
                self.exports.insert(id, RawExport::Func);
                let new_idx = self.funcs.push_alias(idx)?;

                let sort_type_id = self.validator.locals.get_func_type(&idx)?;
                self.validator.locals.push_func(new_idx, sort_type_id);

                let export_type_id = desc
                    .map(|x| {
                        // todo: descの型がsortの型(sort_type_id)のサブセットであることを確認する
                        x.ensure_func()
                    })
                    .transpose()?
                    .unwrap_or(sort_type_id);
                self.validator
                    .surface
                    .exports
                    .insert(name.clone(), PublicTyRef::Func(export_type_id));
            }
            Sort::Type(type_idx) => {
                let inner_id = *self.validator.locals.get_type(&type_idx)?;
                self.validator.locals.register_type_idx(inner_id);

                match desc.map(|x| x.ensure_type()).transpose()? {
                    None => {
                        self.validator
                            .surface
                            .exports
                            .insert(name.clone(), PublicTyRef::TypeEq(inner_id));
                    }
                    Some(TypeBound::Eq(idx)) => {
                        let id = *self.validator.locals.get_type(&idx)?;
                        self.validator
                            .surface
                            .exports
                            .insert(name.clone(), PublicTyRef::TypeEq(id));
                    }
                    Some(TypeBound::Sub) => {
                        let id = self.validator.store.push_resource_in_type(
                            ResourceDef::ExportSubResource {
                                export_name: name.clone(),
                            },
                        );
                        self.validator
                            .locals
                            .register_type_idx(TypeId::Resource(id));
                        self.validator
                            .surface
                            .exports
                            .insert(name.clone(), PublicTyRef::TypeSubResource(id));
                    }
                }
            }
            Sort::Component(idx) => {
                self.exports.insert(id, RawExport::Component);
                let new_idx = self.components.push_alias(idx)?;

                let sort_type_id = self.validator.locals.get_component_type(&idx)?;
                self.validator.locals.push_component(new_idx, sort_type_id);

                let export_type_id = desc
                    .map(|x| {
                        // todo: descの型がsortの型(sort_type_id)のサブセットであることを確認する
                        x.ensure_component()
                    })
                    .transpose()?
                    .unwrap_or(sort_type_id);
                self.validator
                    .surface
                    .exports
                    .insert(name.clone(), PublicTyRef::Component(export_type_id));
            }
            Sort::Instance(idx) => {
                self.exports.insert(id, RawExport::Instance);
                let new_idx = self.instances.push_alias(idx)?;

                let sort_type_id = self.validator.locals.get_instance_type(&idx)?;
                self.validator.locals.push_instance(new_idx, sort_type_id);
                let export_type_id = desc
                    .map(|x| {
                        // todo: descの型がsortの型(sort_type_id)のサブセットであることを確認する
                        x.ensure_instance()
                    })
                    .transpose()?
                    .unwrap_or(sort_type_id);
                self.validator
                    .surface
                    .exports
                    .insert(name.clone(), PublicTyRef::Instance(export_type_id));
            }
            _ => panic!(),
        }
        Ok(())
    }
}
