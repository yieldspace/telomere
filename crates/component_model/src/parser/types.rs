use crate::Result;
use crate::parser::idx::RawCoreTypeIdx;
use crate::types::resource::ResourceDef;
use crate::types::{
    ComponentTypeId, FuncTypeId, InstanceTypeId, PrimValType, ResourceDefId, TypeId, TypeIdx,
};
use crate::vec::Idx;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;
use num_traits::FromPrimitive;
use telomere_wasm::parser::core::parse_i32;
use telomere_wasm::parser::leb128::compile_i32;

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

#[derive(Debug)]
pub enum RawExternDesc {
    CoreModule(RawCoreTypeIdx),
    Func(FuncTypeId),
    #[cfg(feature = "value-imports-exports")]
    Value,
    Type(TypeBound),
    Component(ComponentTypeId),
    Instance(InstanceTypeId),
}

#[derive(Debug)]
pub enum TypeBound {
    Eq(TypeIdx),
    Sub,
}

impl RawExternDesc {
    pub fn ensure_core_module(self) -> Result<RawCoreTypeIdx> {
        match self {
            RawExternDesc::CoreModule(id) => Ok(id),
            _ => Err(ComponentParseError::TypeError(format!(
                "Expected CoreModule, found {:?}",
                self
            ))),
        }
    }

    pub fn ensure_func(self) -> Result<FuncTypeId> {
        match self {
            RawExternDesc::Func(id) => Ok(id),
            _ => Err(ComponentParseError::TypeError(format!(
                "Expected Func, found {:?}",
                self
            ))),
        }
    }

    pub fn ensure_component(self) -> Result<ComponentTypeId> {
        match self {
            RawExternDesc::Component(id) => Ok(id),
            _ => Err(ComponentParseError::TypeError(format!(
                "Expected Component, found {:?}",
                self
            ))),
        }
    }

    pub fn ensure_instance(self) -> Result<InstanceTypeId> {
        match self {
            RawExternDesc::Instance(id) => Ok(id),
            _ => Err(ComponentParseError::TypeError(format!(
                "Expected Instance, found {:?}",
                self
            ))),
        }
    }

    pub fn ensure_type(self) -> Result<TypeBound> {
        match self {
            RawExternDesc::Type(bound) => Ok(bound),
            _ => Err(ComponentParseError::TypeError(format!(
                "Expected TypeBound, found {:?}",
                self
            ))),
        }
    }

    pub fn ensure_type_eq(self) -> Result<TypeIdx> {
        match self {
            RawExternDesc::Type(TypeBound::Eq(idx)) => Ok(idx),
            _ => Err(ComponentParseError::TypeError(format!(
                "Expected TypeBound, found {:?}",
                self
            ))),
        }
    }

    pub fn ensure_type_sub(self) -> Result<()> {
        if let RawExternDesc::Type(TypeBound::Sub) = self {
            Ok(())
        } else {
            Err(ComponentParseError::TypeError(format!(
                "Expected TypeBound::Sub, found {:?}",
                self
            )))
        }
    }

    #[cfg(feature = "value-imports-exports")]
    pub fn ensure_value(self) -> Result<()> {
        if let RawExternDesc::Value = self {
            Ok(())
        } else {
            Err(ComponentParseError::TypeError(format!(
                "Expected Value, found {:?}",
                self
            )))
        }
    }
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_externdesc(&mut self) -> Result<RawExternDesc> {
        let desc = match self.reader.read_exact_one()? {
            0x00 => match self.reader.read_exact_one()? {
                0x11 => {
                    // todo
                    let id = self.parse_core_type_idx()?;
                    RawExternDesc::CoreModule(id)
                }
                _ => panic!(),
            },
            0x01 => {
                let id = self.parse_type_idx()?;
                let id = self.validator.locals.get_type(&id)?;
                RawExternDesc::Func(id.ensure_func_type()?)
            }
            #[cfg(feature = "value-imports-exports")]
            0x02 => todo!(),
            0x03 => {
                let bound = self.parse_typebound()?;
                RawExternDesc::Type(bound)
            }
            0x04 => {
                let idx = self.parse_type_idx()?;
                let id = self.validator.locals.get_type(&idx)?;
                RawExternDesc::Component(id.ensure_component_type()?)
            }
            0x05 => {
                let idx = self.parse_type_idx()?;
                let id = self.validator.locals.get_type(&idx)?;
                RawExternDesc::Instance(id.ensure_instance_type()?)
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
        if let magic = self.reader.read_exact_one()?
            && magic != 0x7f
        {
            return Err(ComponentParseError::InvalidSignature(
                Box::new([magic]),
                Box::new([0x7f]),
                "resource type".to_string(),
            ));
        }
        let dtor = self
            .parse_option()?
            .map(|slf| slf.parse_func_idx())
            .transpose()?;
        let id = self
            .validator
            .store
            .push_resource_in_type(ResourceDef::Defined { dtor });
        self.validator
            .locals
            .register_type_idx(TypeId::Resource(id));
        Ok(())
    }
}
