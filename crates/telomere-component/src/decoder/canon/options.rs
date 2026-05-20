use super::flatten::{flatten_param_types_ctx, flatten_result_types_ctx};
use super::*;

pub(super) fn parse_canonical_options(
    ctx: &mut ParseContext<impl BinaryReader>,
    mode: CanonMode,
) -> ParseResult<CanonicalOptions> {
    let mut options = CanonicalOptions::default();
    for _ in parse_vec_range(ctx)? {
        match ctx.reader.read_exact_one()? {
            0x00 => set_unique(
                &mut options.string_encoding,
                CanonicalStringEncoding::Utf8,
                "`string-encoding` is specified more than once",
            )?,
            0x01 => set_unique(
                &mut options.string_encoding,
                CanonicalStringEncoding::Utf16,
                "`string-encoding` is specified more than once",
            )?,
            0x02 => set_unique(
                &mut options.string_encoding,
                CanonicalStringEncoding::CompactUtf16,
                "`string-encoding` is specified more than once",
            )?,
            0x03 => {
                let idx = parse_core_memory_local_idx(ctx)?;
                ctx.validator.scope().core_memories.get(idx).map_err(|_| {
                    ComponentParseError::InvalidType("memory index out of bounds".to_owned())
                })?;
                set_unique(
                    &mut options.memory,
                    ctx.state.scope().core_memories.get(idx)?,
                    "canonical option `memory` is specified more than once",
                )?;
            }
            0x04 => {
                let idx = parse_core_func_local_idx(ctx)?;
                let func_gidx = ctx.state.scope().core_funcs.get(idx)?;
                let ty = ctx
                    .validator
                    .scope()
                    .core_funcs
                    .get(idx)
                    .map_err(|_| {
                        ComponentParseError::InvalidType(
                            "core function index out of bounds".to_owned(),
                        )
                    })?
                    .clone();
                if ty
                    != CoreFuncType::new(
                        vec![
                            CoreValType::I32,
                            CoreValType::I32,
                            CoreValType::I32,
                            CoreValType::I32,
                        ],
                        vec![CoreValType::I32],
                    )
                {
                    return Err(ComponentParseError::TypeMismatch(
                        "canonical option `realloc` uses a core function with an incorrect signature"
                            .to_owned(),
                    ));
                }
                set_unique(
                    &mut options.realloc,
                    func_gidx,
                    "canonical option `realloc` is specified more than once",
                )?;
                options.realloc_signature = Some(ty);
            }
            0x05 => {
                if matches!(mode, CanonMode::Lower) {
                    return Err(ComponentParseError::TypeMismatch(
                        "canonical option `post-return` cannot be specified for lowerings"
                            .to_owned(),
                    ));
                }
                let idx = parse_core_func_local_idx(ctx)?;
                let func_gidx = ctx.state.scope().core_funcs.get(idx)?;
                let ty = ctx
                    .validator
                    .scope()
                    .core_funcs
                    .get(idx)
                    .map_err(|_| {
                        ComponentParseError::InvalidType(
                            "core function index out of bounds".to_owned(),
                        )
                    })?
                    .clone();
                set_unique(
                    &mut options.post_return,
                    func_gidx,
                    "canonical option `post-return` is specified more than once",
                )?;
                options.post_return_signature = Some(ty);
            }
            0x06 => {
                #[cfg(not(feature = "component-gated-feature-async"))]
                {
                    return Err(ComponentParseError::Unsupported(
                        "canonical option `async` requires the component-gated-feature-async feature"
                            .to_owned(),
                    ));
                }
                #[cfg(feature = "component-gated-feature-async")]
                {
                    if options.async_ {
                        return Err(ComponentParseError::TypeMismatch(
                            "canonical option `async` is specified more than once".to_owned(),
                        ));
                    }
                    options.async_ = true;
                }
            }
            0x07 => {
                let idx = parse_core_func_local_idx(ctx)?;
                #[cfg(not(feature = "component-gated-feature-async"))]
                {
                    let _ = idx;
                    return Err(ComponentParseError::Unsupported(
                        "canonical option `callback` requires the component-gated-feature-async feature"
                            .to_owned(),
                    ));
                }
                #[cfg(feature = "component-gated-feature-async")]
                {
                    let func_gidx = ctx.state.scope().core_funcs.get(idx)?;
                    let ty = ctx
                        .validator
                        .scope()
                        .core_funcs
                        .get(idx)
                        .map_err(|_| {
                            ComponentParseError::InvalidType(
                                "core function index out of bounds".to_owned(),
                            )
                        })?
                        .clone();
                    if ty
                        != CoreFuncType::new(
                            vec![CoreValType::I32, CoreValType::I32, CoreValType::I32],
                            vec![CoreValType::I32],
                        )
                    {
                        return Err(ComponentParseError::TypeMismatch(
                        "canonical option `callback` uses a core function with an incorrect signature"
                            .to_owned(),
                    ));
                    }
                    set_unique(
                        &mut options.callback,
                        func_gidx,
                        "canonical option `callback` is specified more than once",
                    )?;
                    options.callback_signature = Some(ty);
                }
            }
            0x08 => {
                let idx = parse_core_type_local_idx(ctx)?;
                let core_type_gidx = ctx.state.scope().core_types.get(idx)?;
                let ty = ctx
                    .validator
                    .scope()
                    .core_types
                    .get(idx)
                    .map_err(|_| {
                        ComponentParseError::InvalidType("core type index out of bounds".to_owned())
                    })?
                    .clone();
                let crate::ir::types::CoreType::Func(signature) = ty else {
                    return Err(ComponentParseError::TypeMismatch(
                        "canonical option `core type` must reference a core function type"
                            .to_owned(),
                    ));
                };
                set_unique(
                    &mut options.core_type,
                    core_type_gidx,
                    "canonical option `core type` is specified more than once",
                )?;
                options.core_type_signature = Some(signature);
            }
            0x09 => {
                if options.gc {
                    return Err(ComponentParseError::TypeMismatch(
                        "canonical option `gc` is specified more than once".to_owned(),
                    ));
                }
                options.gc = true;
            }
            x => {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "invalid canonical option: {x}"
                )));
            }
        }
    }
    if options.callback.is_some() && !options.async_ {
        return Err(ComponentParseError::TypeMismatch(
            "cannot specify callback without async".to_owned(),
        ));
    }
    if options.async_ && options.post_return.is_some() {
        return Err(ComponentParseError::TypeMismatch(
            "cannot specify post-return function in async".to_owned(),
        ));
    }
    if options.core_type.is_some() && !options.gc {
        return Err(ComponentParseError::TypeMismatch(
            "cannot specify `core-type` without `gc`".to_owned(),
        ));
    }
    if options.gc && options.core_type.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "cannot specify `gc` without also specifying a `core-type` for lowerings".to_owned(),
        ));
    }
    Ok(options)
}

fn set_unique<T>(slot: &mut Option<T>, value: T, message: &str) -> ParseResult<()> {
    if slot.is_some() {
        return Err(ComponentParseError::TypeMismatch(message.to_owned()));
    }
    *slot = Some(value);
    Ok(())
}

pub(super) fn validate_required_options(
    ctx: &ParseContext<impl BinaryReader>,
    func_ty: &FuncType,
    options: &CanonicalOptions,
    mode: CanonMode,
) -> ParseResult<()> {
    let direct_params = flatten_param_types_ctx(func_ty, ctx)?;
    let direct_results = flatten_result_types_ctx(func_ty, ctx)?;
    let params_need_memory = func_ty
        .params
        .iter()
        .any(|param| type_needs_memory(param, ctx));
    let results_need_memory = func_ty
        .result
        .as_ref()
        .is_some_and(|result| type_needs_memory(result, ctx));
    let param_limit = if matches!(mode, CanonMode::Lower) && options.async_ {
        super::flatten::MAX_FLAT_ASYNC_PARAMS
    } else {
        MAX_FLAT_PARAMS
    };
    let params_indirect = direct_params.len() > param_limit;
    let async_lower_with_result =
        matches!(mode, CanonMode::Lower) && options.async_ && !direct_results.is_empty();
    let results_indirect = if matches!(mode, CanonMode::Lift) && options.async_ {
        direct_results.len() > MAX_FLAT_PARAMS
    } else {
        direct_results.len() > MAX_FLAT_RESULTS
    };
    let needs_memory = params_need_memory
        || params_indirect
        || results_need_memory
        || results_indirect
        || async_lower_with_result;
    if needs_memory && options.memory.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `memory` is required".to_owned(),
        ));
    }
    let needs_realloc = match mode {
        CanonMode::Lift => params_need_memory || params_indirect,
        CanonMode::Lower if options.async_ => false,
        CanonMode::Lower => results_need_memory,
    };
    if needs_realloc && options.realloc.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `realloc` is required".to_owned(),
        ));
    }
    if options.realloc.is_some() && options.memory.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `memory` is required".to_owned(),
        ));
    }
    if matches!(mode, CanonMode::Lower) && options.callback.is_some() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `callback` cannot be specified for lowerings".to_owned(),
        ));
    }
    if matches!(mode, CanonMode::Lift) && options.core_type.is_some() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `core-type` is not allowed in `canon lift`".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn type_needs_memory(ty: &ValType, ctx: &ParseContext<impl BinaryReader>) -> bool {
    match ty {
        ValType::Primitive(PrimValType::String) => true,
        ValType::Primitive(_) => false,
        ValType::Type(type_id) => ctx
            .validator
            .get_type(*type_id)
            .map(|ty| defined_type_needs_memory(ty, ctx))
            .unwrap_or(false),
    }
}

fn defined_type_needs_memory(ty: &Type, ctx: &ParseContext<impl BinaryReader>) -> bool {
    match ty {
        Type::DefVal(def) => defval_needs_memory(def, ctx),
        Type::Resource(_) | Type::Generic(_) => false,
        Type::Func(_) | Type::Component(_) | Type::Instance(_) => false,
    }
}

fn defval_needs_memory(def: &DefValType, ctx: &ParseContext<impl BinaryReader>) -> bool {
    match def {
        DefValType::Primitive(PrimValType::String) => true,
        DefValType::Primitive(_) => false,
        DefValType::Record(fields) => fields.iter().any(|field| type_needs_memory(&field.ty, ctx)),
        DefValType::Variant(cases) => cases
            .iter()
            .filter_map(|case| case.ty.as_ref())
            .any(|ty| type_needs_memory(ty, ctx)),
        DefValType::Flags(_) => false,
        DefValType::List(elem, maybe_len) => maybe_len.is_none() || type_needs_memory(elem, ctx),
        #[cfg(feature = "component-gated-feature-async")]
        DefValType::Stream(_) | DefValType::Future(_) => false,
        DefValType::Own(_) | DefValType::Borrow(_) => false,
    }
}
