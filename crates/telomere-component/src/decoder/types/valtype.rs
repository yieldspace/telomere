use crate::decoder::name::parse_label_dash;
use crate::decoder::types::is_type_opcode;
use crate::decoder::{ComponentParseError, ParseContext, ParseResult, SizedResult};
use crate::ir::types::{LabelValType, PrimValType, Type, ValType};
use crate::ir::LocalIdx;
use crate::support::binary::BinaryReader;
use crate::support::parser::core::parse_i32;
use num_traits::FromPrimitive;

pub fn parse_valtype(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ValType> {
    let (_, value) = parse_i32(ctx.reader)?;
    // Drive the branch off the conversion itself. `is_type_opcode` accepts every
    // negative opcode, but only some of them name a primitive, so testing the
    // guard first and unwrapping the conversion aborts on an input that uses any
    // other negative value here.
    if let Some(prim) = PrimValType::from_i32(value) {
        Ok(ValType::Primitive(prim))
    } else if is_type_opcode(value) {
        Err(ComponentParseError::InvalidType(format!(
            "{value} is not a primitive value type opcode"
        )))
    } else {
        let tid = ctx
            .validator
            .scope()
            .type_indexes
            .get(LocalIdx::new(value as u32))?;
        let ty = ctx.validator.get_type(tid)?;
        let Type::DefVal(_) = ty else {
            return Err(ComponentParseError::TypeMismatch(
                "the typeidx of valtype must refer to defvaltype".to_string(),
            ));
        };
        Ok(ValType::Type(tid))
    }
}

pub fn parse_label_valtype(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<LabelValType> {
    let start_count = ctx.reader.read_count();
    let l = parse_label_dash(ctx)?;
    let ty = LabelValType {
        label: l,
        ty: parse_valtype(ctx)?,
    };
    Ok((ctx.reader.read_count() - start_count, ty))
}
