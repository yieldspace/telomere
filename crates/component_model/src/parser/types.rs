use num_traits::FromPrimitive;
use crate::types::{PrimValType, ResourceDefId, TypeBound};
use crate::{ComponentParseError, ComponentParser};
use crate::Result;
use binary_reader::BinaryReader;
use telomere_wasm::parser::core::parse_i32;
use telomere_wasm::parser::leb128::compile_i32;
use crate::vec::Idx;

/// Macro to define a constant type with a given value and name.
///
/// # Parameters
/// - `$value`: The value to be assigned to the constant.
/// - `$name`: The identifier for the constant.
///
/// The macro uses the `compile_i32` function to compile the provided value into an `i32` constant.
macro_rules! const_type {
    ($value:expr, $name:ident) => {
        const $name: i32 = compile_i32($value);
    };
}
const_type!([0x72], DEFVALTYPE_RECORD);
const_type!([0x71], DEFVALTYPE_VARIANT);
const_type!([0x70], DEFVALTYPE_LIST);
const_type!([0x67], DEFVALTYPE_LIST_WITH_LEN);
const_type!([0x6f], DEFVALTYPE_TUPLE);
const_type!([0x6e], DEFVALTYPE_FLAGS);
const_type!([0x6d], DEFVALTYPE_ENUM);
const_type!([0x6b], DEFVALTYPE_OPTION);
const_type!([0x6a], DEFVALTYPE_RESULT);
const_type!([0x69], DEFVALTYPE_OWN);
const_type!([0x68], DEFVALTYPE_BORROW);
#[cfg(feature = "async")]
const_type!([0x66], DEFVALTYPE_STREAM);
#[cfg(feature = "async")]
const_type!([0x65], DEFVALTYPE_FUTURE);
const_type!([0x40], FUNC_TYPE);
const_type!([0x41], COMPONENT_TYPE);
const_type!([0x42], INSTANCE_TYPE);
const_type!([0x3f], RESOURCE_TYPE);
const_type!([0x3e], RESOURCE_TYPE_WITH_ASYNC_CALLBACK);

/// Checks if the given opcode is a type opcode.
///
/// # Parameters
/// - `opcode`: The opcode to check.
///
/// # Returns
/// - `true` if the opcode is a type opcode (i.e., less than or equal to -1).
/// - `false` otherwise.
fn is_type_opcode(opcode: i32) -> bool {
    opcode <= -1
}

pub enum RawExternDesc {
    CoreModule,
    Func,
    #[cfg(feature = "value-imports-exports")]
    Value,
    Type,
    Component,
    Instance,
}

impl<T> ComponentParser<'_, T>
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
    
    pub fn parse_type(&mut self) -> Result<()> {
        let (_, opcode) = parse_i32(self.reader)?;
        let may_prim_val_type = PrimValType::from_i32(opcode);
        match opcode {
            _ if may_prim_val_type.is_some() => {
                todo!();
            }
            RESOURCE_TYPE => self.parse_resource_type(),
            _ => {
                panic!()
            }
        }
    }

    fn parse_resource_type(&mut self) -> Result<()> {
        let idx = self.validator.new_raw_type_idx();
        if let magic = self.reader.read_exact_one()? && magic != 0x7f {
            return Err(ComponentParseError::InvalidSignature(
                Box::new([magic]),
                Box::new([0x7f]),
                "resource type".to_string(),
            ))
        }
        let dtor = self.parse_option()?.map(|slf| slf.parse_func_idx()).transpose()?;
        self.validator.types.push_from_type_section(idx, dtor);
        Ok(())
    }
}
