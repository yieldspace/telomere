use std::collections::HashMap;

use crate::binary::BinaryReader;
use crate::component_model::types::{InstanceExportType, InstanceType};
use crate::parser::component_model::types::parse_instance_decl;
use crate::parser::component_model::{parse_vec_range, ParseContext, ParseResult};
struct ExportSinkInstance(HashMap<String,InstanceExportType>);

pub fn parse_instance_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<InstanceType> {
    ctx.validator.push_scope();
    let mut instance_exports =HashMap::new();
    for _ in parse_vec_range(ctx)? {
        parse_instance_decl(ctx)?;
    }

    let ty = InstanceType {
        exports: instance_exports,
    };

    ctx.validator.pop_scope();

    Ok(ty)
}
