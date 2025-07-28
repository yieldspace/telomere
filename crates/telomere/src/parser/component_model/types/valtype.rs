use crate::component_model::types::{LabelValType, PrimValType, Type, ValType};
use crate::component_model::LocalIdx;
use crate::parser::component_model::name::parse_label_dash;
use crate::parser::component_model::types::is_type_opcode;
use crate::parser::component_model::{ComponentParseError, ParseContext, ParseResult, SizedResult};
use binary_reader::BinaryReader;
use num_traits::FromPrimitive;
use telomere_wasm::parser::core::parse_i32;

pub fn parse_valtype(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ValType> {
    let (_, value) = parse_i32(ctx.reader)?;
    if is_type_opcode(value) {
        Ok(ValType::Primitive(PrimValType::from_i32(value).unwrap()))
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
