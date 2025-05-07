mod alias;
mod component;
mod instance;

use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueBound;
use crate::component_model::{
    ExternDesc
};
use crate::parser::component_model::name::{parse_export_name_dash, parse_import_name_dash};
use crate::parser::component_model::{
    parse_core_type_idx, parse_func_idx, parse_label_dash, parse_option, parse_type_idx,
    parse_vec_range, ComponentParseError, ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::{parse_i32, parse_u32, parse_vec};
use crate::parser::leb128::compile_i32;
pub use component::*;
pub use instance::*;
use num_traits::FromPrimitive;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::trace;
use component_model::types::{DefValType, FuncType, PrimValType, Type};

pub fn parse_resultlist<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> SizedResult<Option<ValType>> {
    let start_count = ctx.reader.read_count();
    let t = match ctx.reader.read_exact_one()? {
        0x00 => {
            let t = parse_valtype(ctx)?;
            Some(t)
        }
        0x01 => match ctx.reader.read_exact_one()? {
            0x00 => None,
            x => {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "Invalid function result type: {x}"
                )));
            }
        },
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid function result type: {x}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start_count, t))
}

fn parse_label_valtype<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<LabelValType> {
    let start_count = ctx.reader.read_count();
    let l = parse_label_dash(ctx)?;
    let ty = LabelValType {
        label: l,
        t: parse_valtype(ctx)?,
    };
    Ok((ctx.reader.read_count() - start_count, ty))
}

fn parse_case<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<Case> {
    let start_count = ctx.reader.read_count();
    let l = parse_label_dash(ctx)?;
    let t = parse_option(ctx, parse_valtype)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "case")?;
    Ok((ctx.reader.read_count() - start_count, Case { label: l, t }))
}

fn parse_valtype<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<ValType> {
    let (_, value) = parse_i32(ctx.reader)?;
    if is_type_opcode(value) {
        Ok(ValType::Primitive(PrimValType::from_i32(value).unwrap()))
    } else {
        let ty = ctx
            .validator
            .get_type(ctx.validator.validate_type_idx(value as u32)?)?;
        let Type::DefVal(ty) = ty else {
            return Err(ComponentParseError::TypeMismatch(
                "the typeidx of valtype must refer to defvaltype".to_string(),
            ));
        };
        Ok(ValType::Type(ty))
    }
}

fn parse_import_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<ImportDecl> {
    let start_count = ctx.reader.read_count();
    let name = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ImportDecl { name, ed },
    ))
}

pub fn parse_externdesc<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<ExternDesc> {
    let desc = match ctx.reader.read_exact_one()? {
        0x00 => {
            ComponentParseError::assert_magic(
                [ctx.reader.read_exact_one()?],
                [0x00],
                "extern desc",
            )?;
            let idx = parse_core_type_idx(ctx)?;
            let ty = ctx.validator.get_core_type(idx)?;
            ExternDesc::CoreModule(ty)
        }
        0x01 => {
            let idx = parse_type_idx(ctx)?;
            let id = ctx.validator.validate_type_idx(idx)?;
            let ty = ctx.validator.get_type(&id)?;
            if !ty.is_func_type() {
                return Err(ComponentParseError::TypeMismatch("export func type mismatch".to_string()))
            }
            ExternDesc::Func(id)
        }
        0x03 => {
            let (_, b) = parse_typebound(ctx)?;
            b
        }
        0x04 => {
            let idx = parse_type_idx(ctx)?;
            let id = ctx.validator.validate_type_idx(idx)?;
            let ty = ctx.validator.get_type(&id)?;
            if !ty.is_component_type() {
                return Err(ComponentParseError::TypeMismatch("export component type mismatch".to_string()))
            }
            ExternDesc::Component(id)
        }
        0x05 => {
            let idx = parse_type_idx(ctx)?;
            let id = ctx.validator.validate_type_idx(idx)?;
            let ty = ctx.validator.get_type(&id)?;
            if !ty.is_instance_type() {
                return Err(ComponentParseError::TypeMismatch("export instance type mismatch".to_string()))
            }
            ExternDesc::Instance(id)
        }
        _ => todo!(),
    };
    Ok(desc)
}

fn parse_typebound<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<ExternDesc> {
    let start_count = ctx.reader.read_count();
    let bound = match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_type_idx(ctx)?;

            ExternDesc::TypeEq(idx)
        }
        0x01 => {
            ExternDesc::TypeSub
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, bound))
}

#[cfg(feature = "component-gated-feature-value-imports-exports")]
fn parse_valuebound(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<ValueBound> {
    let start_count = ctx.reader.read_count();
    let bound = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, idx) = parse_u32(ctx.reader)?;
            ValueBound::Eq(idx as usize)
        }
        0x01 => {
            let (_, t) = parse_valtype(ctx)?;
            ValueBound::Type(t)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, bound))
}

fn parse_export_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<ExportDecl> {
    let start_count = ctx.reader.read_count();
    let en = parse_export_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ExportDecl { name: en, ed },
    ))
}
