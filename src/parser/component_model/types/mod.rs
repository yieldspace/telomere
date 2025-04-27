mod alias;
mod component;
mod instance;
mod validator;

use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueBound;
use crate::component_model::{Case, CoreType, CoreTypeIdx, DefValType, ExportDecl, ExternDesc, FuncType, ImportDecl, Instance, InstanceIdx, Label, LabelValType, PrimValType, Resolvable, ResourceType, Type, TypeBound, TypeIdx, ValType};
use crate::parser::component_model::export::parse_export_name_dash;
use crate::parser::component_model::import::parse_import_name_dash;
pub(super) use crate::parser::component_model::types::validator::TypeValidator;
use crate::parser::component_model::validator::IdxValidator;
use crate::parser::component_model::{parse_core_type_idx, parse_core_type_idx_resolved, parse_func_idx, parse_func_idx_resolved, parse_option, parse_type_idx, parse_type_idx_resolved, ComponentParseError, DefaultValidator, ParseContext, SizedResult, Validator};
use crate::parser::core::{parse_i32, parse_name, parse_u32, parse_vec};
use crate::parser::leb128::compile_i32;
pub use component::*;
pub use instance::*;
use num_traits::FromPrimitive;
use std::sync::atomic::AtomicUsize;

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

pub fn parse_type<'a, 'b>(
    ctx: &'a mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<Type> {
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
            let (_, valtype) = parse_valtype(ctx)?;
            Type::DefVal(Box::from(DefValType::List(valtype, None)))
        }
        DEFVALTYPE_LIST_WITH_LEN => {
            let (_, valtype) = parse_valtype(ctx)?;
            let (_, len) = parse_u32(ctx.reader)?;
            Type::DefVal(Box::from(DefValType::List(valtype, Some(len as usize))))
        }
        DEFVALTYPE_TUPLE => {
            let (_, types) = parse_vec(ctx, |v| v.reader, parse_valtype)?;
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
            let (_, t) = parse_valtype(ctx)?;
            Type::DefVal(Box::from(DefValType::Option(t)))
        }
        DEFVALTYPE_RESULT => {
            let (_, t) = parse_option(ctx, parse_valtype)?;
            let (_, u) = parse_option(ctx, parse_valtype)?;
            Type::DefVal(Box::from(DefValType::Result(t, u)))
        }
        DEFVALTYPE_OWN => {
            let (_, id) = parse_type_idx_resolved(ctx)?;
            Type::DefVal(Box::from(DefValType::Own(id)))
        }
        DEFVALTYPE_BORROW => {
            let (_, id) = parse_type_idx_resolved(ctx)?;
            Type::DefVal(Box::from(DefValType::Borrow(id)))
        }
        #[cfg(feature = "component-gated-feature-async")]
        DEFVALTYPE_STREAM => {
            let (_, t) = parse_option(ctx, parse_valtype)?;
            Type::DefVal(Box::from(DefValType::Stream(t)))
        }
        #[cfg(feature = "component-gated-feature-async")]
        DEFVALTYPE_FUTURE => {
            let (_, t) = parse_option(ctx, parse_valtype)?;
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
            let (_, func) = parse_option(ctx, parse_func_idx_resolved)?;
            Type::Resource(ResourceType::Resource(func.map(|x| x.ty)))
        }
        RESOURCE_TYPE_WITH_ASYNC_CALLBACK => {
            let (_, idx) = parse_func_idx(ctx)?;
            let (_, cb) = parse_option(ctx, parse_func_idx_resolved)?;
            Type::Resource(ResourceType::ResourceWithAsyncCallback(idx, cb.map(|x| x.ty)))
        }
        _ => unreachable!(),
    };
    // let idx = ctx.validator.add_type(Binding::Real(ty))?;

    Ok((ctx.reader.read_count() - start_count, ty))
}

pub fn parse_resultlist(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<TypeIdx, Type>>,
) -> SizedResult<Option<ValType>> {
    let start_count = ctx.reader.read_count();
    let t = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, t) = parse_valtype(ctx)?;
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

fn parse_label_valtype(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<TypeIdx, Type>>,
) -> SizedResult<LabelValType> {
    let start_count = ctx.reader.read_count();
    let (_, l) = parse_label_dash(ctx)?;
    let ty = LabelValType {
        label: l,
        t: parse_valtype(ctx)?.1,
    };
    Ok((ctx.reader.read_count() - start_count, ty))
}

fn parse_case(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<TypeIdx, Type>>,
) -> SizedResult<Case> {
    let start_count = ctx.reader.read_count();
    let (_, l) = parse_label_dash(ctx)?;
    let (_, t) = parse_option(ctx, parse_valtype)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "case")?;
    Ok((ctx.reader.read_count() - start_count, Case { label: l, t }))
}

fn parse_valtype(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<TypeIdx, Type>>,
) -> SizedResult<ValType> {
    let start_count = ctx.reader.read_count();
    let (_, value) = parse_i32(ctx.reader)?;
    if is_type_opcode(value) {
        Ok((
            ctx.reader.read_count() - start_count,
            ValType::Primitive(PrimValType::from_i32(value).unwrap()),
        ))
    } else {
        Ok((
            ctx.reader.read_count() - start_count,
            ValType::Type(ctx.validator.validate_idx_resolved(value as u32)?),
        ))
    }
}

fn parse_label_dash(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<Label> {
    let (len, label) = parse_name(ctx.reader)?;
    Ok((len, Label { len, label }))
}

fn parse_import_decl(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<ImportDecl> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let (_, ed) = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ImportDecl { name, ed },
    ))
}

pub fn parse_externdesc(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl Validator + IdxValidator<CoreTypeIdx, CoreType> + IdxValidator<TypeIdx, Type>,
    >,
) -> SizedResult<ExternDesc> {
    let start_count = ctx.reader.read_count();
    let desc = match ctx.reader.read_exact_one()? {
        0x00 => {
            ComponentParseError::assert_magic(
                [ctx.reader.read_exact_one()?],
                [0x00],
                "extern desc",
            )?;
            let (_, i) = parse_core_type_idx_resolved(ctx)?;
            ExternDesc::CoreModule(i.try_into()?)
        }
        0x01 => {
            let (_, i) = parse_type_idx_resolved(ctx)?;
            ExternDesc::Func(i.try_into()?)
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
            let (_, i) = parse_type_idx_resolved(ctx)?;
            ExternDesc::Component(i.try_into()?)
        }
        0x05 => {
            let (_, i) = parse_type_idx_resolved(ctx)?;
            ExternDesc::Instance(i.try_into()?)
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, desc))
}

fn parse_typebound(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<TypeIdx, Type>>,
) -> SizedResult<Type> {
    let start_count = ctx.reader.read_count();
    let bound = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, ty) = parse_type_idx_resolved(ctx)?;
            ty
        }
        0x01 => {
            todo!()
        }
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, bound))
}

#[cfg(feature = "component-gated-feature-value-imports-exports")]
fn parse_valuebound(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>) -> SizedResult<ValueBound> {
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

fn parse_export_decl(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<ExportDecl> {
    let start_count = ctx.reader.read_count();
    let (_, en) = parse_export_name_dash(ctx)?;
    let (_, ed) = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ExportDecl { name: en, ed },
    ))
}
