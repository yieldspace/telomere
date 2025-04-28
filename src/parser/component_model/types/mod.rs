mod alias;
mod component;
mod instance;

use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueBound;
use crate::component_model::{
    Case, DefValType, ExportDecl,
    ExternDesc, FuncType, ImportDecl, Label,
    LabelValType, PrimValType, ResourceType, Type, ValType,
};
use crate::parser::component_model::export::parse_export_name_dash;
use crate::parser::component_model::import::parse_import_name_dash;
use crate::parser::component_model::{
    parse_core_type_idx, parse_func_idx, parse_option, parse_type_idx, ComponentParseError,
    ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::{parse_i32, parse_name, parse_u32, parse_vec};
use crate::parser::leb128::compile_i32;
pub use component::*;
pub use instance::*;
use num_traits::FromPrimitive;
use std::sync::atomic::{AtomicUsize, Ordering};

static RESOURCE_HANDLE: AtomicUsize = AtomicUsize::new(0);

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
const_type!([0x3f, 0x7f], RESOURCE_TYPE);
const_type!([0x3e, 0x7f], RESOURCE_TYPE_WITH_ASYNC_CALLBACK);

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

pub fn parse_type(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<Type> {
    let start_count = ctx.reader.read_count();
    let (_, opcode) = parse_i32(ctx.reader)?;

    let may_prim_val_type = PrimValType::from_i32(opcode);
    let ty = match opcode {
        _ if may_prim_val_type.is_some() => {
            Type::DefVal(Box::from(DefValType::Primitive(may_prim_val_type.unwrap())))
        }
        DEFVALTYPE_RECORD => {
            let (_, fields) = parse_vec(ctx, |v| v.reader, parse_label_valtype)?;
            Type::DefVal(Box::from(DefValType::Record(fields)))
        }
        DEFVALTYPE_VARIANT => {
            let (_, cases) = parse_vec(ctx, |v| v.reader, parse_case)?;
            Type::DefVal(Box::from(DefValType::Variant(cases)))
        }
        DEFVALTYPE_LIST => {
            let valtype = parse_valtype(ctx)?;
            Type::DefVal(Box::from(DefValType::List(valtype, None)))
        }
        DEFVALTYPE_LIST_WITH_LEN => {
            let valtype = parse_valtype(ctx)?;
            let (_, len) = parse_u32(ctx.reader)?;
            Type::DefVal(Box::from(DefValType::List(valtype, Some(len as usize))))
        }
        DEFVALTYPE_TUPLE => {
            let (_, types) = parse_vec(
                ctx,
                |v| v.reader,
                |ctx| SizedResult::Ok((0, parse_valtype(ctx)?)),
            )?;
            Type::DefVal(Box::from(DefValType::Tuple(types)))
        }
        DEFVALTYPE_FLAGS => {
            let (_, labels) = parse_vec(ctx, |v| v.reader, parse_label_dash)?;
            if labels.is_empty() || labels.len() > 32 {
                return Err(ComponentParseError::InvalidSignature(
                    "Flags type must have 1-32 labels".to_string(),
                ));
            }
            Type::DefVal(Box::from(DefValType::Flags(labels)))
        }
        DEFVALTYPE_ENUM => {
            let (_, labels) = parse_vec(ctx, |v| v.reader, parse_label_dash)?;
            if labels.is_empty() {
                return Err(ComponentParseError::InvalidSignature(
                    "Enum type cannot be empty".to_string(),
                ));
            }
            Type::DefVal(Box::from(DefValType::Enum(labels)))
        }
        DEFVALTYPE_OPTION => {
            let t = parse_valtype(ctx)?;
            Type::DefVal(Box::from(DefValType::Option(t)))
        }
        DEFVALTYPE_RESULT => {
            let t = parse_option(ctx, parse_valtype)?;
            let u = parse_option(ctx, parse_valtype)?;
            Type::DefVal(Box::from(DefValType::Result(t, u)))
        }
        DEFVALTYPE_OWN => {
            let idx = parse_type_idx(ctx)?;
            Type::DefVal(Box::from(DefValType::Own(ctx.validator.get_type(idx)?)))
        }
        DEFVALTYPE_BORROW => {
            let idx = parse_type_idx(ctx)?;
            Type::DefVal(Box::from(DefValType::Borrow(ctx.validator.get_type(idx)?)))
        }
        #[cfg(feature = "component-gated-feature-async")]
        DEFVALTYPE_STREAM => {
            let t = parse_option(ctx, parse_valtype)?;
            Type::DefVal(Box::from(DefValType::Stream(t)))
        }
        #[cfg(feature = "component-gated-feature-async")]
        DEFVALTYPE_FUTURE => {
            let t = parse_option(ctx, parse_valtype)?;
            Type::DefVal(Box::from(DefValType::Future(t)))
        }
        FUNC_TYPE => {
            let (_, ps) = parse_vec(ctx, |v| v.reader, parse_label_valtype)?;
            let (_, rs) = parse_resultlist(ctx)?;
            Type::Func(FuncType {
                params: ps,
                result: rs.map(|x| Box::from(x)),
            })
        }
        COMPONENT_TYPE => Type::Component(parse_component_type(ctx)?.1),
        INSTANCE_TYPE => Type::Instance(parse_instance_type(ctx)?.1),
        RESOURCE_TYPE => {
            if let Some(idx) = parse_option(ctx, parse_func_idx)? {
                Type::Resource(ResourceType::Resource(Some(
                    ctx.validator.get_func_type(idx)?,
                )))
            } else {
                Type::Resource(ResourceType::Resource(None))
            }
        }
        RESOURCE_TYPE_WITH_ASYNC_CALLBACK => {
            let func = {
                let idx = parse_func_idx(ctx)?;
                ctx.validator.get_func_type(idx)?
            };
            let cb = if let Some(idx) = parse_option(ctx, parse_func_idx)? {
                Some(ctx.validator.get_func_type(idx)?)
            } else {
                None
            };
            Type::Resource(ResourceType::ResourceWithAsyncCallback(func, cb))
        }
        _ => unreachable!(),
    };
    // let idx = ctx.validator.add_type(Binding::Real(ty))?;

    Ok((ctx.reader.read_count() - start_count, ty))
}

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
    let (_, l) = parse_label_dash(ctx)?;
    let ty = LabelValType {
        label: l,
        t: parse_valtype(ctx)?,
    };
    Ok((ctx.reader.read_count() - start_count, ty))
}

fn parse_case<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<Case> {
    let start_count = ctx.reader.read_count();
    let (_, l) = parse_label_dash(ctx)?;
    let t = parse_option(ctx, parse_valtype)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "case")?;
    Ok((ctx.reader.read_count() - start_count, Case { label: l, t }))
}

fn parse_valtype<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<ValType> {
    let start_count = ctx.reader.read_count();
    let (_, value) = parse_i32(ctx.reader)?;
    if is_type_opcode(value) {
        Ok(ValType::Primitive(PrimValType::from_i32(value).unwrap()))
    } else {
        Ok(ValType::Type(ctx.validator.get_type(
            ctx.validator.validate_type_idx(value as u32)?,
        )?))
    }
}

fn parse_label_dash<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<Label> {
    let (len, label) = parse_name(ctx.reader)?;
    Ok((len, Label { len, label }))
}

fn parse_import_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<ImportDecl> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ImportDecl { name, ed },
    ))
}

pub fn parse_externdesc<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<ExternDesc> {
    let start_count = ctx.reader.read_count();
    let desc = match ctx.reader.read_exact_one()? {
        0x00 => {
            ComponentParseError::assert_magic(
                [ctx.reader.read_exact_one()?],
                [0x00],
                "extern desc",
            )?;
            let idx = parse_core_type_idx(ctx)?;
            let ty = ctx.validator.get_core_type(idx)?;
            ExternDesc::CoreModule(ty.try_into()?)
        }
        0x01 => {
            let idx = parse_type_idx(ctx)?;
            let ty = ctx.validator.get_type(idx)?;
            ExternDesc::Func(ty.try_into()?)
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        0x02 => {
            let (_, b) = parse_valuebound(ctx)?;
            ExternDesc::Value(b)
        }
        0x03 => {
            let (_, b) = parse_typebound(ctx)?;
            ExternDesc::Type(b)
        }
        0x04 => {
            let idx = parse_type_idx(ctx)?;
            let ty = ctx.validator.get_type(idx)?;
            ExternDesc::Component(ty.try_into()?)
        }
        0x05 => {
            let idx = parse_type_idx(ctx)?;
            let ty = ctx.validator.get_type(idx)?;
            ExternDesc::Instance(ty.try_into()?)
        }
        _ => todo!(),
    };
    Ok(desc)
}

fn parse_typebound<R: BinaryReader>(ctx: &mut ParseContext<R>) -> SizedResult<Type> {
    let start_count = ctx.reader.read_count();
    let bound = match ctx.reader.read_exact_one()? {
        0x00 => {
            let idx = parse_type_idx(ctx)?;
            let ty = ctx.validator.get_type(idx)?;
            ty
        }
        0x01 => {
            let resource_id = RESOURCE_HANDLE.fetch_add(1, Ordering::Relaxed);
            Type::UniqueResource(resource_id)
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
    let (_, en) = parse_export_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ExportDecl { name: en, ed },
    ))
}
