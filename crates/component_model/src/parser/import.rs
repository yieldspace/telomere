use crate::ComponentParser;
use crate::Result;
use crate::parser::component::{RawCoreData, RawData};
use crate::parser::idx::RawImportId;
use crate::parser::types::{RawExternDesc, TypeBound};
use crate::types::TypeId;
use crate::types::component::PublicTyRef;
use crate::types::resource::ResourceDef;
use binary_reader::BinaryReader;

pub enum RawImport {
    CoreModule,
    Func,
    Component,
    Instance,
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_import(&mut self) -> Result<()> {
        let name = self.parse_import_name_dash()?;
        let ed = self.parse_externdesc()?;
        let id = RawImportId::new(&name);
        match ed {
            RawExternDesc::CoreModule(_) => {
                self.imports.insert(id, RawImport::CoreModule);
                self.core_modules.push(RawCoreData::Imported(id))?;
                todo!("ty ref")
            }
            RawExternDesc::Func(tid) => {
                self.imports.insert(id, RawImport::Func);
                let idx = self.funcs.push(RawData::Imported(id))?;

                self.validator
                    .surface
                    .imports
                    .insert(name.clone(), PublicTyRef::Func(tid));
                self.validator.locals.push_func(idx, tid);
            }
            RawExternDesc::Type(bound) => match bound {
                TypeBound::Eq(idx) => {
                    let id = self.validator.locals.get_type(&idx)?;
                    self.validator
                        .surface
                        .imports
                        .insert(name.clone(), PublicTyRef::TypeEq(*id));
                }
                TypeBound::Sub => {
                    let id = self.validator.store.push_resource_in_type(
                        ResourceDef::ImportSubResource {
                            import_name: name.clone(),
                        },
                    );
                    self.validator
                        .locals
                        .register_type_idx(TypeId::Resource(id));
                    self.validator
                        .surface
                        .imports
                        .insert(name.clone(), PublicTyRef::TypeSubResource(id));
                }
            },
            RawExternDesc::Component(tid) => {
                self.imports.insert(id, RawImport::Component);
                let idx = self.components.push(RawData::Imported(id))?;

                self.validator
                    .surface
                    .imports
                    .insert(name.clone(), PublicTyRef::Component(tid));
                self.validator.locals.push_component(idx, tid);
            }
            RawExternDesc::Instance(tid) => {
                self.imports.insert(id, RawImport::Instance);
                let idx = self.instances.push(RawData::Imported(id))?;

                self.validator
                    .surface
                    .imports
                    .insert(name.clone(), PublicTyRef::Instance(tid));

                self.validator.locals.push_instance(idx, tid);
            }
        }
        Ok(())
    }
}
