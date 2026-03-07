use crate::decoder::{ComponentParseError, ParseResult};
use crate::support::parser::leb128::compile_i32;
use num_derive::FromPrimitive;

#[derive(Debug, FromPrimitive, Clone, Hash, PartialEq, Eq)]
#[repr(i32)]
pub enum PrimValType {
    Bool = compile_i32([0x7f]),
    S8 = compile_i32([0x7e]),
    U8 = compile_i32([0x7d]),
    S16 = compile_i32([0x7c]),
    U16 = compile_i32([0x7b]),
    S32 = compile_i32([0x7a]),
    U32 = compile_i32([0x79]),
    S64 = compile_i32([0x78]),
    U64 = compile_i32([0x77]),
    F32 = compile_i32([0x76]),
    F64 = compile_i32([0x75]),
    Char = compile_i32([0x74]),
    String = compile_i32([0x73]),
    #[cfg(feature = "component-gated-feature-async")]
    ErrorContext = compile_i32([0x64]),
}
impl PrimValType {
    pub fn assert_subtype_of(&self, parent: &Self) -> ParseResult<()> {
        if self != parent {
            Err(ComponentParseError::TypeMismatch(
                "prim val type mismatch".to_owned(),
            ))?
        }
        Ok(())
    }
}
