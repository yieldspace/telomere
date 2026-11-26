//! Decoding for the component-model value index space (sort `0x02`).
//!
//! The value section (section `0x0c`) defines statically encoded literals:
//! each entry is `t:<valtype> len:<u32> v:<val(t)>`, where `val(t)` is the
//! grammar from the component-model binary format (design/mvp/Binary.md).
//! Defined values are stored as [`ComponentValue`]s, the canonical ABI value
//! representation, so the runtime can resolve them without further decoding.

use crate::decoder::types::parse_valtype;
use crate::decoder::{ComponentParseError, ParseContext, ParseResult, Validator};
use crate::ir::types::{DefValType, Generic, GenericBound, PrimValType, Type, ValType};
use crate::ir::{Relation, TypeId, Value};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::{parse_name, parse_u32};
use crate::value::ComponentValue;
use std::collections::HashSet;

/// Deep value literals nest one decoder frame per structural level; cap the
/// nesting so a hostile binary cannot overflow the stack.
const MAX_VALUE_NESTING: u32 = 256;

/// Budget for materialising one defined value, on top of the bytes its
/// encoding actually occupies.
///
/// Most decoded nodes consume at least one input byte, so their count is
/// bounded by the declared encoding length. Some shapes consume none: an
/// empty record, a flags type with no labels, and above all the elements of
/// a list whose element type has an empty encoding (a fixed-length list of
/// length 0, nested arbitrarily). A count such as `0xffffffff` over such an
/// element would otherwise materialise billions of `ComponentValue`s from a
/// handful of input bytes and abort the embedding process on allocation.
/// Every node whose decoding consumed no bytes is charged against this
/// budget, and list element counts are checked against it (plus the bytes
/// remaining in the encoding) before any element is materialised.
const MAX_ZERO_WIDTH_VALUE_NODES: usize = 4096;

/// Per-entry decode budget; see [`MAX_ZERO_WIDTH_VALUE_NODES`].
struct ValueBudget {
    /// Global stream position where this entry's encoding starts.
    entry_start: usize,
    /// Declared byte length of this entry's encoding.
    entry_len: usize,
    /// Zero-width nodes that may still be materialised for this entry.
    zero_width_remaining: usize,
}

impl ValueBudget {
    fn new(entry_start: usize, entry_len: usize) -> Self {
        Self {
            entry_start,
            entry_len,
            // Byte-backed encodings earn extra room: a long encoding may
            // legitimately contain many empty sub-values among its bytes.
            zero_width_remaining: MAX_ZERO_WIDTH_VALUE_NODES.saturating_add(entry_len),
        }
    }

    /// Charges one node that was materialised without consuming any bytes.
    fn charge_zero_width(&mut self) -> ParseResult<()> {
        if self.zero_width_remaining == 0 {
            return Err(ComponentParseError::InvalidSignature(
                "value has too many zero-width elements".to_owned(),
            ));
        }
        self.zero_width_remaining -= 1;
        Ok(())
    }

    /// Rejects a list element count that the rest of the encoding cannot
    /// account for: each element either consumes at least one of the
    /// remaining bytes or is a zero-width node charged against the budget,
    /// so a larger count can never decode successfully.
    fn check_list_count(&self, reader: &impl BinaryReader, count: usize) -> ParseResult<()> {
        let consumed = reader.read_count().saturating_sub(self.entry_start);
        let remaining = self.entry_len.saturating_sub(consumed);
        let accountable = (remaining as u64).saturating_add(self.zero_width_remaining as u64);
        if count as u64 > accountable {
            return Err(ComponentParseError::InvalidSignature(format!(
                "list element count {count} exceeds the value decode budget"
            )));
        }
        Ok(())
    }
}

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

/// Resolves an `(eq)` generic -- an alias, or the face of a plain
/// type export -- to the type index it denotes. Any other type
/// resolves to itself.
fn resolve_eq_type_id(validator: &Validator, mut id: TypeId) -> ParseResult<TypeId> {
    while let Type::Generic(Generic {
        bound: GenericBound::Eq(inner),
        ..
    }) = validator.get_type(id)?
    {
        id = *inner;
    }
    Ok(id)
}

fn validate_type_id(
    validator: &Validator,
    id: TypeId,
    allow_own: bool,
    visiting: &mut HashSet<TypeId>,
) -> ParseResult<()> {
    // An `(eq)` type denotes the type it resolves to; judge (and
    // track) the denoted type.
    let id = resolve_eq_type_id(validator, id)?;
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
        DefValType::Variant(variant) => {
            for case in variant.iter() {
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
        let mut budget = ValueBudget::new(start, len as usize);
        decode_value(ctx.validator, &ty, &mut sized, 0, &mut budget)?
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
    budget: &mut ValueBudget,
) -> ParseResult<ComponentValue> {
    if depth > MAX_VALUE_NESTING {
        return Err(ComponentParseError::InvalidSignature(
            "value nesting is too deep".to_owned(),
        ));
    }
    let start = reader.read_count();
    let value = match ty {
        ValType::Primitive(prim) => decode_prim(prim, reader)?,
        ValType::Type(id) => match validator.get_type(resolve_eq_type_id(validator, *id)?)? {
            Type::DefVal(def) => {
                let def = def.clone();
                decode_defval(validator, &def, reader, depth, budget)?
            }
            _ => {
                return Err(ComponentParseError::TypeMismatch(
                    "value type does not resolve to a defined value type".to_owned(),
                ));
            }
        },
    };
    // A node that consumed no input bytes is not bounded by the encoding
    // length; charge it against the per-entry zero-width budget.
    if reader.read_count() == start {
        budget.charge_zero_width()?;
    }
    Ok(value)
}

fn decode_defval(
    validator: &Validator,
    def: &DefValType,
    reader: &mut impl BinaryReader,
    depth: u32,
    budget: &mut ValueBudget,
) -> ParseResult<ComponentValue> {
    match def {
        DefValType::Primitive(prim) => decode_prim(prim, reader),
        DefValType::Record(fields) => {
            let mut values = Vec::with_capacity(fields.len());
            for field in fields {
                let value = decode_value(validator, &field.ty, reader, depth + 1, budget)?;
                values.push((field.label.flat(), value));
            }
            Ok(ComponentValue::Record(values))
        }
        DefValType::Variant(variant) => {
            let (_, index) = parse_u32(reader)?;
            let case = variant.get(index as usize).ok_or_else(|| {
                ComponentParseError::InvalidSignature(format!(
                    "variant case index {index} is out of range"
                ))
            })?;
            let value = match &case.ty {
                Some(ty) => Some(Box::new(decode_value(
                    validator,
                    ty,
                    reader,
                    depth + 1,
                    budget,
                )?)),
                None => None,
            };
            // Defined values keep the plain `Variant` representation for
            // every variant-shaped type, including ones declared with the
            // sugar constructors: the stored form of a value-section
            // constant is not a lift (issue #314 covers the dynamic API's
            // function-boundary lifts, not defined values).
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
            // Reject counts the remaining encoding cannot account for
            // before materialising any element (see `ValueBudget`).
            budget.check_list_count(reader, count)?;
            let mut values = Vec::new();
            for _ in 0..count {
                values.push(decode_value(validator, elem, reader, depth + 1, budget)?);
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
