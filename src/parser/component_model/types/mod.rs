mod alias;
mod component;
mod instance;
mod validator;

use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueBound;
use crate::component_model::{
    Case, ComponentFunction, ComponentIdx, CoreModule, CoreModuleIdx, CoreType, CoreTypeIdx,
    DefValType, ExportDecl, ExternDesc, FuncIdx, FuncType, ImportDecl, InlineComponent, Instance,
    InstanceIdx, Label, LabelValType, PrimValType, Resolvable, Resolver, ResourceType, Type,
    TypeBound, TypeIdx, ValType,
};
use crate::parser::component_model::export::parse_export_name_dash;
use crate::parser::component_model::import::parse_import_name_dash;
pub(super) use crate::parser::component_model::types::validator::TypeValidatorState;
use crate::parser::component_model::validator::{IdxValidator, ValidatorStateImpl};
use crate::parser::component_model::{parse_core_type_idx, parse_core_type_idx_resolved, parse_func_idx, parse_func_idx_resolved, parse_option, parse_type_idx, parse_type_idx_resolved, ComponentParseError, ParseContext, ParseResult, SizedResult, Validator};
use crate::parser::core::{parse_i32, parse_name, parse_u32, parse_vec};
use crate::parser::leb128::compile_i32;
pub use component::*;
pub use instance::*;
use num_traits::FromPrimitive;
use std::sync::atomic::{AtomicUsize, Ordering};
pub use crate::parser::component_model::types::validator::TypeSuperValidator;

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

pub fn parse_type(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl ValidatorStateImpl
            + IdxValidator<TypeIdx, Resolved = Type>
            + Resolver<Type, Error = ComponentParseError>
            + IdxValidator<FuncIdx, Resolved = ComponentFunction>
            + IdxValidator<CoreModuleIdx, Resolved = CoreModule>
            + IdxValidator<InstanceIdx, Resolved = Instance>
            + IdxValidator<ComponentIdx, Resolved = InlineComponent>
            + Resolver<ComponentFunction, Error = ComponentParseError>
            + IdxValidator<CoreTypeIdx, Resolved = CoreType> + TypeSuperValidator,
    >,
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
            let valtype = parse_valtype(ctx)?;
            Type::DefVal(Box::from(DefValType::List(valtype, None)))
        }
        DEFVALTYPE_LIST_WITH_LEN => {
            let valtype = parse_valtype(ctx)?;
            let (_, len) = parse_u32(ctx.reader)?;
            Type::DefVal(Box::from(DefValType::List(valtype, Some(len as usize))))
        }
        DEFVALTYPE_TUPLE => {
            let (_, types) = parse_vec(ctx, |v| v.reader, |ctx| SizedResult::Ok((0, parse_valtype(ctx)?)))?;
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
            let ty = parse_type_idx_resolved(ctx)?;
            Type::DefVal(Box::from(DefValType::Own(ty)))
        }
        DEFVALTYPE_BORROW => {
            let ty = parse_type_idx_resolved(ctx)?;
            Type::DefVal(Box::from(DefValType::Borrow(ty)))
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
            let func = parse_option(ctx, parse_func_idx_resolved)?;
            Type::Resource(ResourceType::Resource(func.map(|x| x.ty)))
        }
        RESOURCE_TYPE_WITH_ASYNC_CALLBACK => {
            let func = parse_func_idx_resolved(ctx)?;
            let cb = parse_option(ctx, parse_func_idx_resolved)?;
            Type::Resource(ResourceType::ResourceWithAsyncCallback(
                func.ty,
                cb.map(|x| x.ty),
            ))
        }
        _ => unreachable!(),
    };
    // let idx = ctx.validator.add_type(Binding::Real(ty))?;

    Ok((ctx.reader.read_count() - start_count, ty))
}

pub fn parse_resultlist<
    R: BinaryReader,
    S: ValidatorStateImpl
        + IdxValidator<TypeIdx, Resolved = Type>
        + Resolver<Type, Error = ComponentParseError>,
>(
    ctx: &mut ParseContext<R, S>,
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

fn parse_label_valtype<
    R: BinaryReader,
    S: ValidatorStateImpl
        + IdxValidator<TypeIdx, Resolved = Type>
        + Resolver<Type, Error = ComponentParseError>,
>(
    ctx: &mut ParseContext<R, S>,
) -> SizedResult<LabelValType> {
    let start_count = ctx.reader.read_count();
    let (_, l) = parse_label_dash(ctx)?;
    let ty = LabelValType {
        label: l,
        t: parse_valtype(ctx)?,
    };
    Ok((ctx.reader.read_count() - start_count, ty))
}

fn parse_case<
    R: BinaryReader,
    S: ValidatorStateImpl
        + IdxValidator<TypeIdx, Resolved = Type>
        + Resolver<Type, Error = ComponentParseError>,
>(
    ctx: &mut ParseContext<R, S>,
) -> SizedResult<Case> {
    let start_count = ctx.reader.read_count();
    let (_, l) = parse_label_dash(ctx)?;
    let t = parse_option(ctx, parse_valtype)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "case")?;
    Ok((ctx.reader.read_count() - start_count, Case { label: l, t }))
}

fn parse_valtype<
    R: BinaryReader,
    S: ValidatorStateImpl
        + IdxValidator<TypeIdx, Resolved = Type>
        + Resolver<Type, Error = ComponentParseError>,
>(
    ctx: &mut ParseContext<R, S>,
) -> ParseResult<ValType> {
    let start_count = ctx.reader.read_count();
    let (_, value) = parse_i32(ctx.reader)?;
    if is_type_opcode(value) {
        Ok(ValType::Primitive(PrimValType::from_i32(value).unwrap()))
    } else {
        Ok(ValType::Type(ctx.validator.validate_idx_resolved(value as u32)?))
    }
}

fn parse_label_dash<R: BinaryReader, S: ValidatorStateImpl>(
    ctx: &mut ParseContext<R, S>,
) -> SizedResult<Label> {
    let (len, label) = parse_name(ctx.reader)?;
    Ok((len, Label { len, label }))
}

fn parse_import_decl<
    R: BinaryReader,
    S: ValidatorStateImpl
        + IdxValidator<CoreTypeIdx, Resolved = CoreType>
        + Resolver<CoreType, Error = ComponentParseError>
    + IdxValidator<TypeIdx, Resolved = Type>
    + Resolver<Type, Error = ComponentParseError>
>(
    ctx: &mut ParseContext<R, S>,
) -> SizedResult<ImportDecl> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ImportDecl { name, ed },
    ))
}

pub fn parse_externdesc<
    R: BinaryReader,
    S: ValidatorStateImpl
        + IdxValidator<CoreTypeIdx, Resolved = CoreType>
        + Resolver<CoreType, Error = ComponentParseError>
    + IdxValidator<TypeIdx, Resolved = Type>
    + Resolver<Type, Error = ComponentParseError>,
>(
    ctx: &mut ParseContext<R, S>,
) -> ParseResult<ExternDesc> {
    let start_count = ctx.reader.read_count();
    let desc = match ctx.reader.read_exact_one()? {
        0x00 => {
            ComponentParseError::assert_magic(
                [ctx.reader.read_exact_one()?],
                [0x00],
                "extern desc",
            )?;
            let ty = parse_core_type_idx_resolved(ctx)?;
            ExternDesc::CoreModule(ty.try_into()?)
        }
        0x01 => {
            let ty = parse_type_idx_resolved(ctx)?;
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
            let ty = parse_type_idx_resolved(ctx)?;
            ExternDesc::Component(ty.try_into()?)
        }
        0x05 => {
            let ty = parse_type_idx_resolved(ctx)?;
            ExternDesc::Instance(ty.try_into()?)
        }
        _ => todo!(),
    };
    Ok(desc)
}

fn parse_typebound<R: BinaryReader, S: ValidatorStateImpl + IdxValidator<TypeIdx, Resolved = Type>
+ Resolver<Type, Error = ComponentParseError>>(
    ctx: &mut ParseContext<R, S>,
) -> SizedResult<Type> {
    let start_count = ctx.reader.read_count();
    let bound = match ctx.reader.read_exact_one()? {
        0x00 => {
            let ty = parse_type_idx_resolved(ctx)?;
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
fn parse_valuebound(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> SizedResult<ValueBound> {
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

fn parse_export_decl<R: BinaryReader, S: ValidatorStateImpl + IdxValidator<CoreTypeIdx, Resolved = CoreType>
+ Resolver<CoreType, Error = ComponentParseError>
+ IdxValidator<TypeIdx, Resolved = Type>
+ Resolver<Type, Error = ComponentParseError>>(
    ctx: &mut ParseContext<R, S>,
) -> SizedResult<ExportDecl> {
    let start_count = ctx.reader.read_count();
    let (_, en) = parse_export_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok((
        ctx.reader.read_count() - start_count,
        ExportDecl { name: en, ed },
    ))
}
