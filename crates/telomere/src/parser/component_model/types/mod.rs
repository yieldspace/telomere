mod alias;
mod component;
/// This module contains the parsing logic for various types in the component model.
/// ParseResult<()>を返す関数はすべて型情報をその関数内で更新しています
mod externdesc;
mod instance;
mod instance_decl;
mod interface;
mod valtype;

use crate::binary::BinaryReader;
use crate::component_model::types::{DefValType, PrimValType, Type};
use crate::parser::component_model::types::instance::parse_instance_type;
use crate::parser::component_model::{ComponentParseError, ParseContext, ParseResult};
use crate::parser::core::parse_i32;
use crate::parser::leb128::compile_i32;
pub use alias::*;
pub use component::*;
pub use externdesc::*;
pub use instance_decl::*;
pub use interface::*;
use num_traits::FromPrimitive;
use tracing::trace;

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
#[cfg(feature = "component-gated-feature-async")]
const_type!([0x66], DEFVALTYPE_STREAM);
#[cfg(feature = "component-gated-feature-async")]
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

pub fn parse_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<Type> {
    let start_count = ctx.reader.read_count();
    trace!("parse type");
    let (_, opcode) = parse_i32(ctx.reader)?;

    let may_prim_val_type = PrimValType::from_i32(opcode);
    let ty = match opcode {
        _ if may_prim_val_type.is_some() => {
            // Type::DefVal(Box::from(DefValType::Primitive(may_prim_val_type.unwrap())))
            todo!()
        }
        COMPONENT_TYPE => Type::Component(parse_component_type(ctx)?),
        INSTANCE_TYPE => Type::Instance(parse_instance_type(ctx)?),

        _ => unreachable!(),
    };

    Ok(ty)
}
