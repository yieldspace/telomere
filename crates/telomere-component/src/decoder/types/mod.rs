mod alias;
mod component;
/// This module contains the parsing logic for various types in the component model.
/// ParseResult<()>を返す関数はすべて型情報をその関数内で更新しています
mod externdesc;
mod instance;
mod instance_decl;
mod interface;
mod valtype;
mod variant;

use crate::decoder::name::{is_kebab_label, parse_label_dash};
use crate::decoder::parse_core_func_local_idx;
use crate::decoder::types::instance::parse_instance_type;
use crate::decoder::types::valtype::{parse_label_valtype, parse_valtype};
use crate::decoder::{
    parse_option, parse_type_local_idx, parse_vec_range, ComponentParseError, ParseContext,
    ParseResult, SizedResult,
};
use crate::ir::types::{Case, DefValType, FuncType, LabelValType, PrimValType, Type, ValType};
use crate::ir::{Label, ResourceId};
use crate::support::binary::BinaryReader;
use crate::support::common::ValType as CoreValType;
use crate::support::parser::core::{parse_i32, parse_u32, parse_vec};
use crate::support::parser::leb128::compile_i32;
pub use component::*;
pub use externdesc::*;
pub use instance_decl::*;
pub use interface::*;
use num_traits::FromPrimitive;
use std::collections::HashSet;
use tracing::trace;
pub use variant::*;

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
    trace!("parse type");
    let (_, opcode) = parse_i32(ctx.reader)?;

    let may_prim_val_type = PrimValType::from_i32(opcode);
    let ty = match opcode {
        _ if may_prim_val_type.is_some() => {
            Type::DefVal(DefValType::Primitive(may_prim_val_type.unwrap()))
        }
        DEFVALTYPE_RECORD => {
            let mut name_set = HashSet::new();
            let mut fields = vec![];
            for _ in parse_vec_range(ctx)? {
                let (_, field) = parse_label_valtype(ctx)?;
                ensure_kebab_label(&field.label, "record field name")?;
                if !name_set.insert(field.label.flat()) {
                    return Err(ComponentParseError::RedundantRecordFieldName);
                }
                fields.push(field);
            }
            if fields.is_empty() {
                return Err(ComponentParseError::EmptyRecord);
            }
            Type::DefVal(DefValType::Record(fields))
        }
        DEFVALTYPE_VARIANT => {
            let mut name_set = HashSet::new();
            let mut cases = vec![];
            for _ in parse_vec_range(ctx)? {
                let case = parse_case(ctx)?;
                ensure_kebab_label(&case.label, "variant case name")?;
                if !name_set.insert(case.label.flat()) {
                    return Err(ComponentParseError::RedundantVariantCaseName);
                }
                cases.push(case);
            }
            if cases.is_empty() {
                return Err(ComponentParseError::EmptyVariant);
            }
            Type::DefVal(DefValType::Variant(cases))
        }
        DEFVALTYPE_LIST => {
            let valtype = parse_valtype(ctx)?;
            Type::DefVal(DefValType::List(valtype, None))
        }
        DEFVALTYPE_LIST_WITH_LEN => {
            let valtype = parse_valtype(ctx)?;
            let (_, len) = parse_u32(ctx.reader)?;
            Type::DefVal(DefValType::List(valtype, Some(len as usize)))
        }
        DEFVALTYPE_TUPLE => {
            let (_, types) = parse_vec(
                ctx,
                |v| v.reader,
                |ctx| SizedResult::Ok((0, parse_valtype(ctx)?)),
            )?;
            if types.is_empty() {
                return Err(ComponentParseError::TypeMismatch(
                    "tuple type must have at least one type".to_owned(),
                ));
            }
            Type::DefVal(DefValType::Record(
                types
                    .into_iter()
                    .enumerate()
                    .map(|(nth, t)| LabelValType::new(Label::new(nth.to_string()), t))
                    .collect(),
            ))
        }
        DEFVALTYPE_FLAGS => {
            let mut name_set = HashSet::new();
            let mut labels = vec![];
            for _ in parse_vec_range(ctx)? {
                let label = parse_label_dash(ctx)?;
                ensure_kebab_label(&label, "flag name")?;
                if !name_set.insert(label.flat()) {
                    return Err(ComponentParseError::RedundantFlagsVariantName);
                }
                labels.push(LabelValType::new(
                    label,
                    ValType::Primitive(PrimValType::Bool),
                ));
            }
            if labels.is_empty() {
                return Err(ComponentParseError::EmptyFlags);
            } else if labels.len() > 32 {
                return Err(ComponentParseError::TooManyFlagNames);
            }
            Type::DefVal(DefValType::Flags(
                labels.into_iter().map(|entry| entry.label).collect(),
            ))
        }
        DEFVALTYPE_ENUM => {
            let mut name_set = HashSet::new();
            let mut labels = vec![];
            for _ in parse_vec_range(ctx)? {
                let label = parse_label_dash(ctx)?;
                ensure_kebab_label(&label, "enum tag name")?;
                if !name_set.insert(label.flat()) {
                    return Err(ComponentParseError::RedundantEnumVariantName);
                }
                labels.push(Case::new(label, None));
            }
            if labels.is_empty() {
                return Err(ComponentParseError::EmptyEnum);
            }
            Type::DefVal(DefValType::Variant(labels))
        }
        DEFVALTYPE_OPTION => {
            let t = parse_valtype(ctx)?;
            Type::DefVal(DefValType::Variant(vec![
                Case::new(Label::new("none".to_string()), None),
                Case::new(Label::new("some".to_string()), Some(t)),
            ]))
        }
        DEFVALTYPE_RESULT => {
            let t = parse_option(ctx, parse_valtype)?;
            let u = parse_option(ctx, parse_valtype)?;
            Type::DefVal(DefValType::Variant(vec![
                Case::new(Label::new("ok".to_string()), t),
                Case::new(Label::new("err".to_string()), u),
            ]))
        }
        DEFVALTYPE_OWN => {
            let idx = parse_type_local_idx(ctx)?;
            let type_id = ctx.validator.scope().type_indexes.get(idx)?;
            if !is_resource_like_type(ctx, type_id)? {
                return Err(ComponentParseError::TypeMismatch(
                    "not a resource type".to_owned(),
                ));
            }
            Type::DefVal(DefValType::Own(type_id))
        }
        DEFVALTYPE_BORROW => {
            let idx = parse_type_local_idx(ctx)?;
            let type_id = ctx.validator.scope().type_indexes.get(idx)?;
            if !is_resource_like_type(ctx, type_id)? {
                return Err(ComponentParseError::TypeMismatch(
                    "not a resource type".to_owned(),
                ));
            }
            Type::DefVal(DefValType::Borrow(type_id))
        }
        FUNC_TYPE => {
            let (_, ps) = parse_vec(ctx, |v| v.reader, parse_label_valtype)?;
            let mut param_names = HashSet::new();
            for param in &ps {
                ensure_kebab_label(&param.label, "function parameter name")?;
                if let Some(previous) = param_names.replace(param.label.flat()) {
                    return Err(ComponentParseError::TypeMismatch(format!(
                        "function parameter name `{}` conflicts with previous parameter name `{previous}`",
                        param.label
                    )));
                }
            }
            let rs = {
                match ctx.reader.read_exact_one()? {
                    0x00 => {
                        let t = parse_valtype(ctx)?;
                        if valtype_contains_borrow(ctx, &t)? {
                            return Err(ComponentParseError::TypeMismatch(
                                "function result cannot contain a `borrow` type".to_owned(),
                            ));
                        }
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
                }
            };
            let param_names = ps.iter().map(|v| v.label.clone()).collect();
            Type::Func(FuncType {
                params: ps.into_iter().map(|v| v.ty).collect(),
                param_names,
                result: rs,
            })
        }
        COMPONENT_TYPE => Type::Component(parse_component_type(ctx)?),
        INSTANCE_TYPE => Type::Instance(parse_instance_type(ctx)?),
        RESOURCE_TYPE => {
            if !ctx.validator.in_concrete_scope() {
                return Err(ComponentParseError::TypeMismatch(
                    "resources can only be defined within a concrete component".to_owned(),
                ));
            }
            let magic = ctx.reader.read_exact_one()?;
            if magic != 0x7f {
                Err(ComponentParseError::WrongMagic(
                    magic,
                    "resource".to_string(),
                ))?
            }
            if let Some(idx) = parse_option(ctx, parse_core_func_local_idx)? {
                let ty = ctx.validator.scope().core_funcs.get(idx)?.clone();
                if ty != crate::support::common::FuncType::new(vec![CoreValType::I32], vec![]) {
                    return Err(ComponentParseError::TypeMismatch(
                        "wrong signature for a destructor".to_owned(),
                    ));
                }
                let dtor = ctx.state.scope().core_funcs.get(idx)?;
                Type::Resource(ResourceId::with_dtor(
                    ctx.validator.current_scope_id(),
                    Some(dtor.into()),
                ))
            } else {
                Type::Resource(ResourceId::new(ctx.validator.current_scope_id()))
            }
        }
        RESOURCE_TYPE_WITH_ASYNC_CALLBACK => {
            return Err(ComponentParseError::Unsupported(
                "async resource callbacks are not supported".to_owned(),
            ));
        }
        _ => {
            return Err(ComponentParseError::InvalidType(format!(
                "unknown component type opcode: {opcode}"
            )));
        }
    };

    Ok(ty)
}

fn valtype_contains_borrow(
    ctx: &ParseContext<impl BinaryReader>,
    ty: &ValType,
) -> ParseResult<bool> {
    match ty {
        ValType::Primitive(_) => Ok(false),
        ValType::Type(type_id) => type_contains_borrow(ctx, ctx.validator.get_type(*type_id)?),
    }
}

fn ensure_kebab_label(label: &Label, kind: &str) -> ParseResult<()> {
    if label.0.is_empty() {
        return Err(ComponentParseError::InvalidLabel(
            "name cannot be empty".to_owned(),
        ));
    }
    if is_kebab_label(&label.0) {
        Ok(())
    } else {
        Err(ComponentParseError::InvalidLabel(format!(
            "{kind} `{}` is not in kebab case",
            label
        )))
    }
}

fn type_contains_borrow(ctx: &ParseContext<impl BinaryReader>, ty: &Type) -> ParseResult<bool> {
    Ok(match ty {
        Type::DefVal(def) => match def {
            DefValType::Primitive(_) | DefValType::Own(_) => false,
            DefValType::Borrow(_) => true,
            DefValType::Record(fields) => fields.iter().try_fold(false, |found, field| {
                if found {
                    Ok(true)
                } else {
                    valtype_contains_borrow(ctx, &field.ty)
                }
            })?,
            DefValType::Variant(cases) => cases
                .iter()
                .filter_map(|case| case.ty.as_ref())
                .try_fold(false, |found, ty| {
                    if found {
                        Ok(true)
                    } else {
                        valtype_contains_borrow(ctx, ty)
                    }
                })?,
            DefValType::Flags(_) => false,
            DefValType::List(ty, _) => valtype_contains_borrow(ctx, ty)?,
        },
        Type::Generic(_)
        | Type::Resource(_)
        | Type::Func(_)
        | Type::Component(_)
        | Type::Instance(_) => false,
    })
}

fn is_resource_like_type(
    ctx: &ParseContext<impl BinaryReader>,
    type_id: crate::ir::TypeId,
) -> ParseResult<bool> {
    Ok(match ctx.validator.get_type(type_id)? {
        Type::Resource(_) | Type::Generic(_) => true,
        Type::DefVal(DefValType::Own(inner)) | Type::DefVal(DefValType::Borrow(inner)) => {
            is_resource_like_type(ctx, *inner)?
        }
        Type::DefVal(_) | Type::Func(_) | Type::Component(_) | Type::Instance(_) => false,
    })
}
