use crate::binary::BinaryReader;
use crate::component_model::types::FuncType;
use crate::parser::component_model::{ParseContext, ParseResult};

pub fn parse_func_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<FuncType> {
    // todo(type) add func type
    ctx.validator.push_scope();
    ctx.validator.pop_scope();
    Ok(FuncType {
        params: Default::default(),
        result: Default::default(),
    })
}
