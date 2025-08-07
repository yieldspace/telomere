use crate::parser::component::{RawCoreData, RawData};
use crate::parser::idx::RawImportId;
use crate::parser::types::RawExternDesc;
use crate::ComponentParser;
use crate::Result;
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
            RawExternDesc::CoreModule => {
                self.imports.insert(id, RawImport::CoreModule);
                self.core_modules.push(RawCoreData::Imported(id))?;
            }
            RawExternDesc::Func => {
                self.imports.insert(id, RawImport::Func);
                self.funcs.push(RawData::Imported(id))?;
            }
            RawExternDesc::Type => {}
            RawExternDesc::Component => {
                self.imports.insert(id, RawImport::Component);
                self.components.push(RawData::Imported(id))?;
            }
            RawExternDesc::Instance => {
                self.imports.insert(id, RawImport::Instance);
                self.instances.push(RawData::Imported(id))?;
            }
        }
        Ok(())
    }
}
