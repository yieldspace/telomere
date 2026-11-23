//! Decoding for the component-model value index space (sort `0x02`).
//!
//! The value section (section `0x0c`) defines statically encoded literals:
//! each entry is `t:<valtype> len:<u32> v:<val(t)>`, where `val(t)` is the
//! grammar from the component-model binary format (design/mvp/Binary.md).
//! Defined values are stored as [`ComponentValue`]s, the canonical ABI value
//! representation, so the runtime can resolve them without further decoding.

use crate::decoder::types::parse_valtype;
use crate::decoder::{ComponentParseError, ParseContext, ParseResult, Validator};
use crate::ir::types::{DefValType, PrimValType, Type, ValType};
use crate::ir::{Relation, TypeId, Value};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::{parse_name, parse_u32};
use crate::value::ComponentValue;
use std::collections::HashSet;

/// Deep value literals nest one decoder frame per structural level; cap the
/// nesting so a hostile binary cannot overflow the stack.
const MAX_VALUE_NESTING: u32 = 256;

/// Whether a value type may appear in the value index space at all.
///
/// `val(t)` has no production for resource handles, so a *defined* value can
/// never contain `own`/`borrow`. Imported and exported values may carry an
/// `own` handle (the other side supplies the resource), but a `borrow` can
/// only exist for the duration of a call and is rejected for every
/// index-space entry.
pub(crate) fn validate_value_type(
    validator: &Validator,
    ty: &ValType,
    allow_own: bool,
) -> ParseResult<()> {
    let mut visiting = HashSet::new();
    validate_valtype(validator, ty, allow_own, &mut visiting)
}

fn validate_valtype(
    validator: &Validator,
    ty: &ValType,
    allow_own: bool,
    visiting: &mut HashSet<TypeId>,
) -> ParseResult<()> {
    match ty {
        ValType::Primitive(_) => Ok(()),
        ValType::Type(id) => validate_type_id(validator, *id, allow_own, visiting),
    }
}

fn validate_type_id(
    validator: &Validator,
    id: TypeId,
    allow_own: bool,
    visiting: &mut HashSet<TypeId>,
) -> ParseResult<()> {
    if !visiting.insert(id) {
        // Recursive type: the cycle is already being checked on the stack.
        return Ok(());
    }
    let result = match validator.get_type(id)? {
        Type::DefVal(def) => validate_defval(validator, def, allow_own, visiting),
        _ => Err(ComponentParseError::TypeMismatch(
            "value type does not resolve to a defined value type".to_owned(),
        )),
    };
    visiting.remove(&id);
    result
}

fn validate_defval(
    validator: &Validator,
    def: &DefValType,
    allow_own: bool,
    visiting: &mut HashSet<TypeId>,
) -> ParseResult<()> {
    match def {
        DefValType::Primitive(_) | DefValType::Flags(_) => Ok(()),
        DefValType::Record(fields) => {
            for field in fields {
                validate_valtype(validator, &field.ty, allow_own, visiting)?;
            }
            Ok(())
        }
        DefValType::Variant(cases) => {
            for case in cases {
                if let Some(ty) = &case.ty {
                    validate_valtype(validator, ty, allow_own, visiting)?;
                }
            }
            Ok(())
        }
        DefValType::List(elem, _) => validate_valtype(validator, elem, allow_own, visiting),
        DefValType::Own(_) => {
            if allow_own {
                Ok(())
            } else {
                Err(ComponentParseError::TypeMismatch(
                    "defined values cannot contain resource handles".to_owned(),
                ))
            }
        }
        DefValType::Borrow(_) => Err(ComponentParseError::TypeMismatch(
            "value types cannot contain borrowed resource handles".to_owned(),
        )),
    }
}

/// Parses one value-section entry and registers it in the value index space.
pub fn parse_value_def(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let ty = parse_valtype(ctx)?;
    validate_value_type(ctx.validator, &ty, false)?;
    let (_, len) = parse_u32(ctx.reader)?;
    // `take` limits are absolute end offsets and `read_count` is the global
    // stream position, so exact consumption is checked as a delta.
    let start = ctx.reader.read_count();
    let value = {
        let mut sized = ctx.reader.take(len as usize);
        decode_value(ctx.validator, &ty, &mut sized, 0)?
    };
    if ctx.reader.read_count() - start != len as usize {
        return Err(ComponentParseError::InvalidSignature(
            "value encoding does not match its declared length".to_owned(),
        ));
    }
    let idx = ctx.state.value_store.register(Relation::Defined(Value {
        ty: ty.clone(),
        value,
    }));
    ctx.state.scope_mut().values.register(idx);
    ctx.validator.scope_mut().value_indexes.add(ty);
    Ok(())
}

fn decode_value(
    validator: &Validator,
    ty: &ValType,
    reader: &mut impl BinaryReader,
    depth: u32,
) -> ParseResult<ComponentValue> {
    if depth > MAX_VALUE_NESTING {
        return Err(ComponentParseError::InvalidSignature(
            "value nesting is too deep".to_owned(),
        ));
    }
    match ty {
        ValType::Primitive(prim) => decode_prim(prim, reader),
        ValType::Type(id) => match validator.get_type(*id)? {
            Type::DefVal(def) => {
                let def = def.clone();
                decode_defval(validator, &def, reader, depth)
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "value type does not resolve to a defined value type".to_owned(),
            )),
        },
    }
}

fn decode_defval(
    validator: &Validator,
    def: &DefValType,
    reader: &mut impl BinaryReader,
    depth: u32,
) -> ParseResult<ComponentValue> {
    match def {
        DefValType::Primitive(prim) => decode_prim(prim, reader),
        DefValType::Record(fields) => {
            let mut values = Vec::with_capacity(fields.len());
            for field in fields {
                let value = decode_value(validator, &field.ty, reader, depth + 1)?;
                values.push((field.label.flat(), value));
            }
            Ok(ComponentValue::Record(values))
        }
        DefValType::Variant(cases) => {
            let (_, index) = parse_u32(reader)?;
            let case = cases.get(index as usize).ok_or_else(|| {
                ComponentParseError::InvalidSignature(format!(
                    "variant case index {index} is out of range"
                ))
            })?;
            let value = match &case.ty {
                Some(ty) => Some(Box::new(decode_value(validator, ty, reader, depth + 1)?)),
                None => None,
            };
            Ok(ComponentValue::Variant {
                case: case.label.flat(),
                value,
            })
        }
        DefValType::Flags(labels) => {
            let bytes = labels.len().div_ceil(8);
            let mut selected = Vec::new();
            for i in 0..bytes {
                let byte = reader.read_exact_one()?;
                for bit in 0..8 {
                    let index = i * 8 + bit;
                    if byte & (1 << bit) == 0 {
                        continue;
                    }
                    match labels.get(index) {
                        Some(label) => selected.push(label.flat()),
                        None => {
                            return Err(ComponentParseError::InvalidSignature(
                                "flags value sets a bit beyond the declared flags".to_owned(),
                            ));
                        }
                    }
                }
            }
            Ok(ComponentValue::Flags(selected))
        }
        DefValType::List(elem, len) => {
            let count = match len {
                Some(len) => *len,
                None => parse_u32(reader)?.1 as usize,
            };
            let mut values = Vec::new();
            for _ in 0..count {
                values.push(decode_value(validator, elem, reader, depth + 1)?);
            }
            Ok(ComponentValue::List(values))
        }
        // Rejected by `validate_value_type` before decoding begins; kept as a
        // hard error so a definition can never smuggle in a handle.
        DefValType::Own(_) | DefValType::Borrow(_) => Err(ComponentParseError::TypeMismatch(
            "defined values cannot contain resource handles".to_owned(),
        )),
    }
}

fn decode_prim(prim: &PrimValType, reader: &mut impl BinaryReader) -> ParseResult<ComponentValue> {
    Ok(match prim {
        PrimValType::Bool => match reader.read_exact_one()? {
            0x00 => ComponentValue::Bool(false),
            0x01 => ComponentValue::Bool(true),
            x => {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "invalid bool value byte: {x}"
                )));
            }
        },
        PrimValType::U8 => ComponentValue::U8(reader.read_exact_one()?),
        PrimValType::S8 => ComponentValue::S8(reader.read_exact_one()? as i8),
        PrimValType::U16 => ComponentValue::U16(read_uleb(reader, 16)? as u16),
        PrimValType::S16 => ComponentValue::S16(read_sleb(reader, 16)? as i16),
        PrimValType::U32 => ComponentValue::U32(read_uleb(reader, 32)? as u32),
        PrimValType::S32 => ComponentValue::S32(read_sleb(reader, 32)? as i32),
        PrimValType::U64 => ComponentValue::U64(read_uleb(reader, 64)?),
        PrimValType::S64 => ComponentValue::S64(read_sleb(reader, 64)?),
        PrimValType::F32 => ComponentValue::F32(f32::from_le_bytes(reader.read_exact::<4>()?)),
        PrimValType::F64 => ComponentValue::F64(f64::from_le_bytes(reader.read_exact::<8>()?)),
        PrimValType::Char => ComponentValue::Char(read_char(reader)?),
        PrimValType::String => ComponentValue::String(parse_name(reader)?.1),
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => {
            return Err(ComponentParseError::Unsupported(
                "error-context values are not supported".to_owned(),
            ));
        }
    })
}

/// Reads an unsigned LEB128 value that must fit in `bits` bits, enforcing the
/// core side condition that unused bits of the final byte are zero.
fn read_uleb(reader: &mut impl BinaryReader, bits: u32) -> ParseResult<u64> {
    let max_bytes = (bits as usize).div_ceil(7);
    let mut result: u64 = 0;
    let mut shift = 0;
    for i in 0..max_bytes {
        let byte = reader.read_exact_one()?;
        let low = (byte & 0x7f) as u64;
        if i == max_bytes - 1 {
            let used = bits - 7 * (max_bytes as u32 - 1);
            if used < 7 && low >> used != 0 {
                return Err(ComponentParseError::InvalidSignature(
                    "integer representation is too long".to_owned(),
                ));
            }
        }
        result |= low << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(ComponentParseError::InvalidSignature(
        "integer representation is too long".to_owned(),
    ))
}

/// Reads a signed LEB128 value that must fit in `bits` bits, enforcing the
/// core side condition that the final byte's unused bits are a correct sign
/// extension of the value.
fn read_sleb(reader: &mut impl BinaryReader, bits: u32) -> ParseResult<i64> {
    let max_bytes = (bits as usize).div_ceil(7);
    let mut result: i64 = 0;
    let mut shift = 0u32;
    for i in 0..max_bytes {
        let byte = reader.read_exact_one()?;
        let payload = byte & 0x7f;
        if i == max_bytes - 1 && bits % 7 != 0 {
            // Final byte: the payload bits past `used` must be a sign
            // extension of the value's sign bit.
            let used = bits - 7 * (max_bytes as u32 - 1);
            let sign = (payload >> (used - 1)) & 1;
            let expected = if sign == 1 {
                (1u8 << (7 - used)) - 1
            } else {
                0
            };
            if payload >> used != expected {
                return Err(ComponentParseError::InvalidSignature(
                    "integer representation is too long".to_owned(),
                ));
            }
        }
        result |= (payload as i64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && payload & 0x40 != 0 {
                result |= -1i64 << shift;
            }
            if bits < 64 {
                let min = -(1i64 << (bits - 1));
                let max = (1i64 << (bits - 1)) - 1;
                if !(min..=max).contains(&result) {
                    return Err(ComponentParseError::InvalidSignature(
                        "integer representation is too long".to_owned(),
                    ));
                }
            }
            return Ok(result);
        }
    }
    Err(ComponentParseError::InvalidSignature(
        "integer representation is too long".to_owned(),
    ))
}

/// Reads the UTF-8 encoding of exactly one char.
fn read_char(reader: &mut impl BinaryReader) -> ParseResult<char> {
    let mut buf = [0u8; 4];
    let mut len = 0;
    loop {
        buf[len] = reader.read_exact_one()?;
        len += 1;
        match std::str::from_utf8(&buf[..len]) {
            Ok(s) => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    return Ok(c);
                }
                return Err(ComponentParseError::InvalidSignature(
                    "char value is not a single character".to_owned(),
                ));
            }
            Err(e) if e.error_len().is_none() && len < 4 => continue,
            Err(_) => {
                return Err(ComponentParseError::InvalidSignature(
                    "char value is not valid UTF-8".to_owned(),
                ));
            }
        }
    }
}
