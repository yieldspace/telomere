use crate::parser::idx::RawExportId;
use crate::parser::sort::{CoreSort, Sort};
use crate::ComponentParser;
use crate::Result;
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
        match sort {
            Sort::Core(CoreSort::Module(_)) => {
                self.exports.insert(id, RawExport::CoreModule);
            }
            Sort::Func(idx) => {
                self.exports.insert(id, RawExport::Func);
                self.funcs.push_alias(idx)?;
            }
            Sort::Type(_) => {}
            Sort::Component(idx) => {
                self.exports.insert(id, RawExport::Component);
                self.components.push_alias(idx)?;
            }
            Sort::Instance(idx) => {
                self.exports.insert(id, RawExport::Instance);
                self.instances.push_alias(idx)?;
            }
            _ => panic!(),
        }
        Ok(())
    }
}
