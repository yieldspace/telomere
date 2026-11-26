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
                return Err(ComponentParseError::Unsupported(
                    "async canonical ABI is not supported".to_owned(),
                ));
            }
            0x07 => {
                return Err(ComponentParseError::Unsupported(
                    "canonical callback is not supported".to_owned(),
                ));
            }
            0x08 => {
                return Err(ComponentParseError::Unsupported(
                    "canonical `core type` option is not supported".to_owned(),
                ));
            }
            0x09 => {
                return Err(ComponentParseError::Unsupported(
                    "canonical GC ABI is not supported".to_owned(),
                ));
            }
            x => {
                return Err(ComponentParseError::InvalidSignature(format!(
                    "invalid canonical option: {x}"
                )));
            }
        }
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
        .any(|param| ctx.validator.type_needs_memory(param));
    let results_need_memory = func_ty
        .result
        .as_ref()
        .is_some_and(|result| ctx.validator.type_needs_memory(result));
    let params_indirect = direct_params.len() > MAX_FLAT_PARAMS;
    let results_indirect = direct_results.len() > MAX_FLAT_RESULTS;
    let needs_memory =
        params_need_memory || params_indirect || results_need_memory || results_indirect;
    if needs_memory && options.memory.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `memory` is required".to_owned(),
        ));
    }
    let needs_realloc = match mode {
        CanonMode::Lift => params_need_memory || params_indirect,
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
    Ok(())
}
