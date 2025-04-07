mod sort;
mod instance;

use crate::binary::{BinaryReader, Countable, Counter};
use crate::component_model::id::TypeId;
#[cfg(feature = "import_export")]
use crate::component_model::types::ValueBound;
use crate::component_model::types::{
    Case, ComponentDecl, ComponentType, DefValType, ExportDecl, ExternDesc, FuncType, ImportDecl,
    InstanceDecl, InstanceType, Label, LabelValType, PrimValType, ResourceType, Type, TypeBound,
    ValType,
};
use crate::parser::component::alias::parse_alias;
use crate::parser::component::context::ParseContext;
use crate::parser::component::core::parse_core_type;
use crate::parser::component::id::{parse_func_idx, parse_type_idx};
use crate::parser::component::import_export::{parse_export_name_dash, parse_import_name_dash};
use crate::parser::component::types::sort::TypeSort;
use crate::parser::component::{parse_option, parse_vec_map, ComponentModelParserError};
use crate::parser::core::{parse_i32, parse_name, parse_u32, parse_vec};
use crate::parser::leb128::compile_i32;
use crate::with_count;
use num_traits::FromPrimitive;
use crate::parser::component::types::instance::{_parse_instance_decl, parse_instance_decl, parse_instance_type};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

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
#[cfg(feature = "async")]
const_type!([0x66], DEFVALTYPE_STREAM);
#[cfg(feature = "async")]
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

fn parse_core_option<R: BinaryReader, V, E>(
    ctx: &mut ParseContext<R>,
    mut f: impl FnMut(&mut R) -> std::result::Result<(usize, V), E>,
) -> Result<(usize, Option<V>)>
where
    ComponentModelParserError: From<E>,
{
    match ctx.reader.read_exact_one()? {
        0x00 => Ok((1, None)),
        0x01 => {
            let (len, v) = f(ctx.reader)?;
            Ok((len + 1, Some(v)))
        }
        x => Err(ComponentModelParserError::WrongMagic(
            x,
            "core option".to_string(),
        )),
    }
}

pub fn parse_type<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, Type)> {
    let mut counter = Counter::new();
    let opcode = parse_i32(ctx.reader)?.count(&mut counter);
    let ty = match opcode {
        x if PrimValType::from_i32(x).is_some() => {
            Type::DefVal(DefValType::Primitive(PrimValType::from_i32(x).unwrap()))
        }
        DEFVALTYPE_RECORD => {
            let fields = parse_vec(ctx, |v| v.reader, parse_label_valtype)?.count(&mut counter);
            Type::DefVal(DefValType::Record(fields))
        }
        DEFVALTYPE_VARIANT => {
            let cases = parse_vec(ctx, |v| v.reader, parse_case)?.count(&mut counter);
            Type::DefVal(DefValType::Variant(cases))
        }
        DEFVALTYPE_LIST => {
            let valtype = parse_valtype(ctx)?.count(&mut counter);
            Type::DefVal(DefValType::List(valtype, None))
        }
        DEFVALTYPE_LIST_WITH_LEN => {
            let valtype = parse_valtype(ctx)?.count(&mut counter);
            let len = parse_u32(ctx.reader)?.count(&mut counter);
            Type::DefVal(DefValType::List(valtype, Some(len as usize)))
        }
        DEFVALTYPE_TUPLE => {
            let types = parse_vec(ctx, |v| v.reader, parse_valtype)?.count(&mut counter);
            Type::DefVal(DefValType::Tuple(types))
        }
        DEFVALTYPE_FLAGS => {
            let labels = parse_vec(ctx, |v| v.reader, parse_label_dash)?.count(&mut counter);
            if labels.is_empty() || labels.len() > 32 {
                return Err(ComponentModelParserError::TypeError(
                    "Flags type must have 1-32 labels".to_string(),
                ));
            }
            Type::DefVal(DefValType::Flags(labels))
        }
        DEFVALTYPE_ENUM => {
            let labels = parse_vec(ctx, |v| v.reader, parse_label_dash)?.count(&mut counter);
            if labels.is_empty() {
                return Err(ComponentModelParserError::TypeError(
                    "Enum type cannot be empty".to_string(),
                ));
            }
            Type::DefVal(DefValType::Enum(labels))
        }
        DEFVALTYPE_OPTION => {
            let t = parse_valtype(ctx)?.count(&mut counter);
            Type::DefVal(DefValType::Option(t))
        }
        DEFVALTYPE_RESULT => {
            let t = parse_option(ctx, parse_valtype)?.count(&mut counter);
            let u = parse_option(ctx, parse_valtype)?.count(&mut counter);
            Type::DefVal(DefValType::Result(t, u))
        }
        DEFVALTYPE_OWN => {
            let id = parse_type_idx(ctx)?.count(&mut counter);
            Type::DefVal(DefValType::Own(id))
        }
        DEFVALTYPE_BORROW => {
            let id = parse_type_idx(ctx)?.count(&mut counter);
            Type::DefVal(DefValType::Borrow(id))
        }
        #[cfg(feature = "async")]
        DEFVALTYPE_STREAM => {
            let t = parse_option(ctx, parse_valtype)?.count(&mut counter);
            Type::DefVal(DefValType::Stream(t))
        }
        #[cfg(feature = "async")]
        DEFVALTYPE_FUTURE => {
            let t = parse_option(ctx, parse_valtype)?.count(&mut counter);
            Type::DefVal(DefValType::Future(t))
        }
        FUNC_TYPE => {
            let ps = parse_vec(ctx, |v| v.reader, parse_label_valtype)?.count(&mut counter);
            let rs = parse_resultlist(ctx)?.count(&mut counter);
            Type::Func(FuncType {
                params: ps,
                result: rs,
            })
        }
        COMPONENT_TYPE => {
            let cd = parse_vec(ctx, |v| v.reader, parse_component_decl)?.count(&mut counter);
            Type::Component(ComponentType(cd))
        }
        INSTANCE_TYPE => {
            Type::Instance(parse_instance_type(ctx)?.1)
        }
        RESOURCE_TYPE => {
            let idx = parse_option(ctx, parse_func_idx)?.count(&mut counter);
            Type::Resource(ResourceType::Resource(idx))
        }
        RESOURCE_TYPE_WITH_ASYNC_CALLBACK => {
            let idx = parse_func_idx(ctx)?.count(&mut counter);
            let cb = parse_option(ctx, parse_func_idx)?.count(&mut counter);
            Type::Resource(ResourceType::ResourceWithAsyncCallback(idx, cb))
        }
        n => {
            return Err(ComponentModelParserError::TypeError(format!(
                "Invalid type: {n}"
            )));
        }
    };
    Ok((counter.count(), ty))
}

pub fn parse_resultlist(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, Option<ValType>)> {
    let mut counter = Counter::new();
    let t = match ctx.reader.read_exact_one()?.count(&mut counter) {
        0x00 => {
            let t = parse_valtype(ctx)?.count(&mut counter);
            Some(t)
        }
        0x01 => match ctx.reader.read_exact_one()?.count(&mut counter) {
            0x00 => None,
            x => {
                return Err(ComponentModelParserError::TypeError(format!(
                    "Invalid function result type: {x}"
                )));
            }
        },
        x => {
            return Err(ComponentModelParserError::TypeError(format!(
                "Invalid function result type: {x}"
            )));
        }
    };
    Ok((counter.count(), t))
}

pub fn parse_primvaltype<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, PrimValType)> {
    let r = with_count!(ctx.reader, {
        let n = ctx.reader.read_exact_one()?;
        if let Some(s) = PrimValType::from_u8(n) {
            s
        } else {
            return Err(ComponentModelParserError::InvalidPrimValType(n));
        }
    });
    Ok(r)
}

fn parse_label_valtype<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, LabelValType)> {
    Ok(with_count!(ctx.reader, {
        let (_, l) = parse_label_dash(ctx)?;
        LabelValType {
            label: l,
            t: parse_valtype(ctx)?.1,
        }
    }))
}

fn parse_case<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, Case)> {
    Ok(with_count!(ctx.reader, {
        let (_, l) = parse_label_dash(ctx)?;
        let (_, t) = parse_option(ctx, parse_valtype)?;
        ComponentModelParserError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "case")?;
        Case { label: l, t }
    }))
}

fn parse_valtype<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, ValType)> {
    let mut counter = Counter::new();
    let value = parse_i32(ctx.reader)?.count(&mut counter);
    if is_type_opcode(value) {
        Ok((
            counter.count(),
            ValType::Primitive(PrimValType::from_i32(value).unwrap()),
        ))
    } else {
        Ok((counter.count(), ValType::TypeId(TypeId(value))))
    }
}

fn parse_label_dash<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, Label)> {
    Ok(with_count!(ctx.reader, {
        let (len, label) = parse_name(ctx.reader)?;
        Label { len, label }
    }))
}

fn parse_component_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, ComponentDecl)> {
    Ok(with_count!(ctx.reader, {
        match ctx.reader.read_exact_one()? {
            0x03 => {
                let (_, decl) = parse_import_decl(ctx)?;
                ComponentDecl::Import(decl)
            }
            x => {
                let (_, decl) = _parse_instance_decl(ctx, Some(x))?;
                // type_sort.add_instance_decl(decl);
                ComponentDecl::Instance(decl)
            }
        }
    }))
}

fn parse_import_decl(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, ImportDecl)> {
    Ok(with_count!(ctx.reader, {
        let (_, name) = parse_import_name_dash(ctx)?;
        let (_, ed) = parse_externdesc(ctx)?;
        ImportDecl { name, ed }
    }))
}

pub fn parse_externdesc(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, ExternDesc)> {
    Ok(with_count!(ctx.reader, {
        match ctx.reader.read_exact_one()? {
            0x00 => {
                ComponentModelParserError::assert_magic(
                    [ctx.reader.read_exact_one()?],
                    [0x00],
                    "extern desc",
                )?;
                let (_, i) = parse_u32(ctx.reader)?;
                ExternDesc::Core(i as usize)
            }
            0x01 => {
                let (_, i) = parse_u32(ctx.reader)?;
                ExternDesc::Func(i as usize)
            }
            #[cfg(feature = "import_export")]
            0x02 => {
                let (_, b) = parse_valuebound(ctx)?;
                ExternDesc::Value(b)
            }
            0x03 => {
                let (_, b) = parse_typebound(ctx)?;
                ExternDesc::Type(b)
            }
            0x04 => {
                let (_, i) = parse_u32(ctx.reader)?;
                ExternDesc::Component(i as usize)
            }
            0x05 => {
                let (_, i) = parse_u32(ctx.reader)?;
                ExternDesc::Instance(i as usize)
            }
            _ => todo!(),
        }
    }))
}

fn parse_typebound(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, TypeBound)> {
    let mut counter = Counter::new();
    let r = match ctx.reader.read_exact_one()?.count(&mut counter) {
        0x00 => {
            let idx = parse_type_idx(ctx)?.count(&mut counter);
            TypeBound::Eq(idx)
        }
        0x01 => TypeBound::Sub,
        _ => todo!(),
    };
    Ok((counter.count(), r))
}

#[cfg(feature = "import_export")]
fn parse_valuebound(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, ValueBound)> {
    Ok(with_count!(ctx.reader, {
        match ctx.reader.read_exact_one()? {
            0x00 => {
                let (_, idx) = parse_u32(ctx.reader)?;
                ValueBound::Eq(idx as usize)
            }
            0x01 => {
                let (_, t) = parse_valtype(ctx)?;
                ValueBound::Type(t)
            }
            _ => todo!(),
        }
    }))
}

fn parse_export_decl(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, ExportDecl)> {
    Ok(with_count!(ctx.reader, {
        let (_, en) = parse_export_name_dash(ctx)?;
        let (_, ed) = parse_externdesc(ctx)?;
        ExportDecl { name: en, ed }
    }))
}
