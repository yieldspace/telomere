use crate::types::{CoreTypeId, TypeBound, TypeIdx};
use crate::ComponentParser;
use crate::Result;
use binary_reader::BinaryReader;

pub enum RawExternDesc {
    CoreModule,
    Func,
    #[cfg(feature = "value-imports-exports")]
    Value,
    Type,
    Component,
    Instance,
}

impl<T> ComponentParser<'_, '_, T>
where
    T: BinaryReader,
{
    pub fn parse_externdesc(&mut self) -> Result<RawExternDesc> {
        let desc = match self.reader.read_exact_one()? {
            0x00 => match self.reader.read_exact_one()? {
                0x11 => {
                    let id = self.parse_core_type_idx()?;
                    RawExternDesc::CoreModule
                }
                _ => panic!(),
            },
            0x01 => {
                let id = self.parse_type_idx()?;
                RawExternDesc::Func
            }
            #[cfg(feature = "value-imports-exports")]
            0x02 => todo!(),
            0x03 => {
                let bound = self.parse_typebound()?;
                RawExternDesc::Type
            }
            0x04 => {
                let id = self.parse_type_idx()?;
                RawExternDesc::Component
            }
            0x05 => {
                let id = self.parse_type_idx()?;
                RawExternDesc::Instance
            }
            _ => panic!(),
        };
        Ok(desc)
    }

    fn parse_typebound(&mut self) -> Result<TypeBound> {
        match self.reader.read_exact_one()? {
            0x00 => {
                let _id = self.parse_type_idx()?;
                todo!()
                // Ok(TypeBound::Eq(id))
            }
            0x01 => Ok(TypeBound::Sub),
            _ => panic!(),
        }
    }
}
