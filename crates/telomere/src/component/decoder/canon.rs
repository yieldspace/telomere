use crate::binary::BinaryReader;
use crate::common::{FuncType as CoreFuncType, ValType as CoreValType};
use crate::component::decoder::{
    parse_core_func_local_idx, parse_core_memory_local_idx, parse_func_local_idx,
    parse_type_local_idx, parse_vec_range, ComponentParseError, ParseContext, ParseResult,
};
use crate::component::ir::types::{DefValType, FuncType, PrimValType, Type, ValType};
use crate::component::ir::{
    CanonicalOptions, CanonicalStringEncoding, CoreFunc, CoreRelation, Func, Relation,
};

const MAX_FLAT_PARAMS: usize = 16;
const MAX_FLAT_RESULTS: usize = 1;

pub fn parse_canon(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    match ctx.reader.read_exact_one()? {
        0x00 => match ctx.reader.read_exact_one()? {
            0x00 => parse_lift(ctx),
            x => Err(ComponentParseError::Unsupported(format!(
                "unsupported canonical function 0x00 0x{x:02x}"
            ))),
        },
        0x01 => match ctx.reader.read_exact_one()? {
            0x00 => parse_lower(ctx),
            x => Err(ComponentParseError::Unsupported(format!(
                "unsupported canonical function 0x01 0x{x:02x}"
            ))),
        },
        0x02 => parse_resource(
            ctx,
            ResourceCanonKind::New,
            CoreFuncType::new(vec![CoreValType::I32], vec![CoreValType::I32]),
        ),
        0x03 => parse_resource(
            ctx,
            ResourceCanonKind::Drop,
            CoreFuncType::new(vec![CoreValType::I32], vec![]),
        ),
        0x04 => parse_resource(
            ctx,
            ResourceCanonKind::Rep,
            CoreFuncType::new(vec![CoreValType::I32], vec![CoreValType::I32]),
        ),
        x => Err(ComponentParseError::Unsupported(format!(
            "unsupported canonical function opcode: 0x{x:02x}"
        ))),
    }
}

fn parse_lift(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let core_func_idx = parse_core_func_local_idx(ctx)?;
    let core_func_gidx = ctx.state.scope().core_funcs.get(core_func_idx)?;
    let core_func_ty = ctx
        .validator
        .scope()
        .core_funcs
        .get(core_func_idx)
        .map_err(|_| {
            ComponentParseError::InvalidType("core function index out of bounds".to_owned())
        })?
        .clone();
    let options = parse_canonical_options(ctx, CanonMode::Lift)?;
    let type_idx = parse_type_local_idx(ctx)?;
    let type_id = ctx.validator.scope().type_indexes.get(type_idx)?;
    let func_ty = ctx.validator.get_func_type(type_id)?.clone();
    validate_required_options(ctx, &func_ty, &options, CanonMode::Lift)?;

    let expected_core_ty = flatten_func_type(&func_ty, ctx, CanonMode::Lift)?;
    if core_func_ty.0 != expected_core_ty.0 {
        return Err(ComponentParseError::TypeMismatch(format!(
            "lowered parameter types `{:?}` do not match parameter types `{:?}`",
            core_func_ty.0 .0, expected_core_ty.0 .0
        )));
    }
    if core_func_ty.1 != expected_core_ty.1 {
        return Err(ComponentParseError::TypeMismatch(format!(
            "lowered result types `{:?}` do not match result types `{:?}`",
            core_func_ty.1 .0, expected_core_ty.1 .0
        )));
    }
    if let Some(post_return_ty) = options.post_return_signature.clone() {
        let expected_post_return = CoreFuncType::new(expected_core_ty.1 .0.clone(), Vec::new());
        if post_return_ty != expected_post_return {
            return Err(ComponentParseError::TypeMismatch(
                "canonical option `post-return` uses a core function with an incorrect signature"
                    .to_owned(),
            ));
        }
    }

    let gidx = ctx
        .state
        .func_store
        .register(Relation::Defined(Func::CanonLift {
            core_func: core_func_gidx,
            type_id,
            options,
        }));
    ctx.state.scope_mut().funcs.register(gidx);
    ctx.validator.scope_mut().func_indexes.add(type_id);
    Ok(())
}

fn parse_lower(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let func_idx = parse_func_local_idx(ctx)?;
    let func_gidx = ctx.state.scope().funcs.get(func_idx)?;
    let type_id = ctx.validator.scope().func_indexes.get(func_idx)?;
    let func_ty = ctx.validator.get_func_type(type_id)?.clone();
    let options = parse_canonical_options(ctx, CanonMode::Lower)?;
    validate_required_options(ctx, &func_ty, &options, CanonMode::Lower)?;

    let lowered_ty = flatten_func_type(&func_ty, ctx, CanonMode::Lower)?;
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonLower {
            func: func_gidx,
            type_id,
            options: Box::new(options),
            signature: lowered_ty.clone(),
        }));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(lowered_ty);
    Ok(())
}

#[derive(Clone, Copy)]
enum ResourceCanonKind {
    New,
    Drop,
    Rep,
}

fn parse_resource(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: ResourceCanonKind,
    ty: CoreFuncType,
) -> ParseResult<()> {
    let type_idx = parse_type_local_idx(ctx)?;
    let type_id = ctx.validator.scope().type_indexes.get(type_idx)?;
    let ty_ref = ctx.validator.get_type(type_id)?;
    let is_resource = matches!(ty_ref, Type::Resource(_) | Type::Generic(_));
    if !is_resource {
        return Err(ComponentParseError::TypeMismatch(
            "not a resource type".to_owned(),
        ));
    }
    if matches!(kind, ResourceCanonKind::New | ResourceCanonKind::Rep) {
        let is_local = match ty_ref {
            Type::Resource(resource) => resource.owner() == ctx.validator.current_scope_id(),
            Type::Generic(_) => false,
            _ => false,
        };
        if !is_local {
            return Err(ComponentParseError::TypeMismatch(
                "not a local resource".to_owned(),
            ));
        }
    }
    let kind = match kind {
        ResourceCanonKind::New => CoreFunc::CanonResourceNew { type_id },
        ResourceCanonKind::Drop => CoreFunc::CanonResourceDrop { type_id },
        ResourceCanonKind::Rep => CoreFunc::CanonResourceRep { type_id },
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(kind));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(ty);
    Ok(())
}

#[derive(Clone, Copy)]
enum CanonMode {
    Lift,
    Lower,
}

fn parse_canonical_options(
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

fn validate_required_options(
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
    let params_indirect = direct_params.len() > MAX_FLAT_PARAMS;
    let results_indirect = direct_results.len() > MAX_FLAT_RESULTS;
    let needs_memory = match mode {
        CanonMode::Lift => results_need_memory || results_indirect,
        CanonMode::Lower => params_need_memory || params_indirect,
    };
    if needs_memory && options.memory.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `memory` is required".to_owned(),
        ));
    }
    let needs_realloc = match mode {
        CanonMode::Lift => params_need_memory || params_indirect,
        CanonMode::Lower => results_need_memory || results_indirect,
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

fn type_needs_memory(ty: &ValType, ctx: &ParseContext<impl BinaryReader>) -> bool {
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
        DefValType::List(elem, maybe_len) => maybe_len.is_none() || type_needs_memory(elem, ctx),
        DefValType::Own(_) | DefValType::Borrow(_) => false,
    }
}

fn flatten_func_type(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
    mode: CanonMode,
) -> ParseResult<CoreFuncType> {
    let mut params = abi_flatten_params(&flatten_param_types_ctx(func_ty, ctx)?);
    let results = abi_flatten_results(&flatten_result_types_ctx(func_ty, ctx)?, mode, &mut params);
    Ok(CoreFuncType::new(params, results))
}

fn flatten_param_types_ctx(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
) -> ParseResult<Vec<CoreValType>> {
    let mut params = Vec::new();
    for param in &func_ty.params {
        flatten_val_type(param, ctx, &mut params)?;
    }
    Ok(params)
}

fn flatten_result_types_ctx(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
) -> ParseResult<Vec<CoreValType>> {
    let mut results = Vec::new();
    if let Some(result) = &func_ty.result {
        flatten_val_type(result, ctx, &mut results)?;
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
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match ty {
        ValType::Primitive(prim) => {
            flatten_primitive(prim, out);
            Ok(())
        }
        ValType::Type(type_id) => flatten_type(ctx.validator.get_type(*type_id)?, ctx, out),
    }
}

fn flatten_type(
    ty: &Type,
    ctx: &ParseContext<impl BinaryReader>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match ty {
        Type::DefVal(def) => flatten_defval(def, ctx, out),
        Type::Resource(_) => {
            out.push(CoreValType::I32);
            Ok(())
        }
        Type::Generic(_) => {
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
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match def {
        DefValType::Primitive(prim) => {
            flatten_primitive(prim, out);
            Ok(())
        }
        DefValType::Record(fields) => {
            for field in fields {
                flatten_val_type(&field.ty, ctx, out)?;
            }
            Ok(())
        }
        DefValType::Variant(cases) => {
            let mut payload = Vec::new();
            for case in cases {
                if let Some(ty) = &case.ty {
                    let mut flat = Vec::new();
                    flatten_val_type(ty, ctx, &mut flat)?;
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
    }
}
