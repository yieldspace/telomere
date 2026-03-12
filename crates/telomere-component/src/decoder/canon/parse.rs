use super::flatten::flatten_func_type;
use super::options::{parse_canonical_options, validate_required_options};
use super::*;

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
