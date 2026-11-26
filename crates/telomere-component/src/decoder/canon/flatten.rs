use super::*;
use crate::ir::types::{Generic, GenericBound};
use crate::ir::TypeId;
use std::collections::HashSet;

/// Upper bound on the flat representation retained per type.
///
/// Every consumer of a flattened type only distinguishes lengths up
/// to `MAX_FLAT_PARAMS`: longer parameter and result lists are passed
/// indirectly through memory instead. Retaining the first
/// `MAX_FLAT_PARAMS + 1` values is therefore enough — a cached vector
/// of exactly that length means "at least this many", and prefixes
/// compose exactly (concatenation is positional and the variant join
/// is element-wise, so a join or concatenation of prefixes is a
/// prefix of the true result). Without the bound, a chain of
/// single-field wrappers around a wide type each retained a complete
/// copy of the wide representation, so peak memory grew with
/// depth x width on inputs whose flattened values are never used
/// (the lift is rejected for a missing `memory` option only after
/// flattening). Measured on PR #372: 20 doubling tuple definitions
/// followed by 128 single-element wrappers peaked at ~71 MB resident
/// versus ~5 MB without the wrappers.
const MAX_CACHED_FLAT_LEN: usize = MAX_FLAT_PARAMS + 1;

pub(super) fn flatten_func_type(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
    mode: CanonMode,
) -> ParseResult<CoreFuncType> {
    let mut params = abi_flatten_params(&flatten_param_types_ctx(func_ty, ctx)?);
    let results = abi_flatten_results(&flatten_result_types_ctx(func_ty, ctx)?, mode, &mut params);
    Ok(CoreFuncType::new(params, results))
}

pub(super) fn flatten_param_types_ctx(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
) -> ParseResult<Vec<CoreValType>> {
    let mut visiting = HashSet::new();
    let mut params = Vec::new();
    for param in &func_ty.params {
        flatten_val_type(param, ctx, &mut visiting, &mut params)?;
    }
    Ok(params)
}

pub(super) fn flatten_result_types_ctx(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
) -> ParseResult<Vec<CoreValType>> {
    let mut visiting = HashSet::new();
    let mut results = Vec::new();
    if let Some(result) = &func_ty.result {
        flatten_val_type(result, ctx, &mut visiting, &mut results)?;
    }
    Ok(results)
}

fn abi_flatten_params(flat: &[CoreValType]) -> Vec<CoreValType> {
    if flat.len() > MAX_FLAT_PARAMS {
        vec![CoreValType::I32]
    } else {
        flat.to_vec()
    }
}

fn abi_flatten_results(
    flat: &[CoreValType],
    mode: CanonMode,
    params: &mut Vec<CoreValType>,
) -> Vec<CoreValType> {
    if flat.len() > MAX_FLAT_RESULTS {
        match mode {
            CanonMode::Lift => vec![CoreValType::I32],
            CanonMode::Lower => {
                params.push(CoreValType::I32);
                Vec::new()
            }
        }
    } else {
        flat.to_vec()
    }
}

fn flatten_val_type(
    ty: &ValType,
    ctx: &ParseContext<impl BinaryReader>,
    visiting: &mut HashSet<TypeId>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match ty {
        ValType::Primitive(prim) => {
            flatten_primitive(prim, out);
            Ok(())
        }
        ValType::Type(type_id) => flatten_type_by_id(*type_id, ctx, visiting, out),
    }
}

/// Flattens the type behind `type_id` into `out`, memoising the result
/// per type in the validator's type arena.
///
/// The flat representation of a type is a pure function of the type
/// graph reachable from it, and arena definitions are append-only, so a
/// completed result stays valid for the rest of the parse. Without the
/// cache, shared references are re-expanded at every visit: a chain of
/// variants whose two cases both reference the preceding type took
/// exponential time to flatten even though every type in the chain is
/// small (issue #367; measured pre-fix in release mode, the chain's
/// final type as a canon lift parameter took ~1.2 s at 24 definitions,
/// growing exponentially, while the definitions alone stayed flat).
/// The discipline matches the validator's size / alignment / borrow
/// caches (PR #364): only completed results are cached and errors are
/// not cached. The retained representation is bounded by
/// `MAX_CACHED_FLAT_LEN`; see its documentation for why a prefix is
/// all any consumer needs. A type already on the current traversal
/// stack is never
/// cached either; defined types cannot actually be recursive (a
/// definition only references earlier types), so the re-entry guard is
/// defensive and reports an error rather than recursing without bound,
/// which is what the uncached walk did.
fn flatten_type_by_id(
    type_id: TypeId,
    ctx: &ParseContext<impl BinaryReader>,
    visiting: &mut HashSet<TypeId>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    if let Some(flat) = ctx.validator.flattened_types(type_id) {
        out.extend_from_slice(&flat);
        return Ok(());
    }
    if !visiting.insert(type_id) {
        return Err(ComponentParseError::TypeMismatch(
            "recursive defined type cannot be flattened".to_owned(),
        ));
    }
    let mut flat = Vec::new();
    let result = flatten_type(ctx.validator.get_type(type_id)?, ctx, visiting, &mut flat);
    visiting.remove(&type_id);
    result?;
    flat.truncate(MAX_CACHED_FLAT_LEN);
    out.extend_from_slice(&flat);
    ctx.validator.set_flattened_types(type_id, flat);
    Ok(())
}

fn flatten_type(
    ty: &Type,
    ctx: &ParseContext<impl BinaryReader>,
    visiting: &mut HashSet<TypeId>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match ty {
        Type::DefVal(def) => flatten_defval(def, ctx, visiting, out),
        // An `(eq)` type is definitionally its inner type, so it
        // flattens as the inner type: a plain export's face over a
        // record flattens as the record, not as a handle. (An eq
        // over a resource or a `(sub)` generic still flattens to
        // the single handle slot those arms produce.)
        Type::Generic(Generic {
            bound: GenericBound::Eq(inner),
            ..
        }) => flatten_type_by_id(*inner, ctx, visiting, out),
        Type::Resource(_) | Type::Generic(_) => {
            out.push(CoreValType::I32);
            Ok(())
        }
        _ => Err(ComponentParseError::TypeMismatch(
            "not a function type".to_owned(),
        )),
    }
}

fn flatten_defval(
    def: &DefValType,
    ctx: &ParseContext<impl BinaryReader>,
    visiting: &mut HashSet<TypeId>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match def {
        DefValType::Primitive(prim) => {
            flatten_primitive(prim, out);
            Ok(())
        }
        DefValType::Record(fields) => {
            for field in fields {
                flatten_val_type(&field.ty, ctx, visiting, out)?;
            }
            Ok(())
        }
        DefValType::Variant(cases) => {
            let mut payload = Vec::new();
            for case in cases.iter() {
                if let Some(ty) = &case.ty {
                    let mut flat = Vec::new();
                    flatten_val_type(ty, ctx, visiting, &mut flat)?;
                    for (index, ty) in flat.into_iter().enumerate() {
                        if let Some(current) = payload.get_mut(index) {
                            *current = join_core_types(*current, ty);
                        } else {
                            payload.push(ty);
                        }
                    }
                }
            }
            out.push(CoreValType::I32);
            out.extend(payload);
            Ok(())
        }
        DefValType::Flags(labels) => {
            for _ in 0..labels.len().div_ceil(32) {
                out.push(CoreValType::I32);
            }
            Ok(())
        }
        DefValType::List(_, _) => {
            out.push(CoreValType::I32);
            out.push(CoreValType::I32);
            Ok(())
        }
        DefValType::Own(_) | DefValType::Borrow(_) => {
            out.push(CoreValType::I32);
            Ok(())
        }
    }
}

fn join_core_types(lhs: CoreValType, rhs: CoreValType) -> CoreValType {
    if lhs == rhs {
        lhs
    } else if matches!(
        (lhs, rhs),
        (CoreValType::I32, CoreValType::F32) | (CoreValType::F32, CoreValType::I32)
    ) {
        CoreValType::I32
    } else {
        CoreValType::I64
    }
}

fn flatten_primitive(prim: &PrimValType, out: &mut Vec<CoreValType>) {
    match prim {
        PrimValType::Bool
        | PrimValType::S8
        | PrimValType::U8
        | PrimValType::S16
        | PrimValType::U16
        | PrimValType::S32
        | PrimValType::U32
        | PrimValType::Char => out.push(CoreValType::I32),
        PrimValType::S64 | PrimValType::U64 => out.push(CoreValType::I64),
        PrimValType::F32 => out.push(CoreValType::F32),
        PrimValType::F64 => out.push(CoreValType::F64),
        PrimValType::String => {
            out.push(CoreValType::I32);
            out.push(CoreValType::I32);
        }
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => out.push(CoreValType::I32),
    }
}

#[cfg(all(test, feature = "component-gated-feature-async"))]
mod tests {
    use super::*;

    #[test]
    fn error_context_flattens_to_i32() {
        let mut flattened = Vec::new();
        flatten_primitive(&PrimValType::ErrorContext, &mut flattened);

        assert_eq!(flattened, vec![CoreValType::I32]);
    }
}
