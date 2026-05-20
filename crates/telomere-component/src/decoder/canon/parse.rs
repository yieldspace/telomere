use super::flatten::flatten_func_type_with_options;
use super::options::{parse_canonical_options, type_needs_memory, validate_required_options};
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
        0x05 => parse_task_cancel(ctx),
        0x06 => parse_subtask_cancel(ctx),
        0x07 => unsupported_canonical("resource.drop async"),
        0x08 => unsupported_canonical("backpressure.set"),
        0x09 => parse_task_return(ctx),
        0x0a => unsupported_canonical("context.get"),
        0x0b => unsupported_canonical("context.set"),
        0x0c => unsupported_canonical("thread.yield"),
        0x0d => parse_subtask_drop(ctx),
        0x0e => parse_stream_or_future_new(ctx, StreamFutureKind::Stream),
        0x0f => parse_stream_read_write(ctx, StreamIoKind::Read),
        0x10 => parse_stream_read_write(ctx, StreamIoKind::Write),
        0x11 => {
            parse_stream_or_future_cancel(ctx, StreamFutureKind::Stream, StreamFutureEnd::Readable)
        }
        0x12 => {
            parse_stream_or_future_cancel(ctx, StreamFutureKind::Stream, StreamFutureEnd::Writable)
        }
        0x13 => {
            parse_stream_or_future_drop(ctx, StreamFutureKind::Stream, StreamFutureEnd::Readable)
        }
        0x14 => {
            parse_stream_or_future_drop(ctx, StreamFutureKind::Stream, StreamFutureEnd::Writable)
        }
        0x15 => parse_stream_or_future_new(ctx, StreamFutureKind::Future),
        0x16 => parse_future_read_write(ctx, FutureIoKind::Read),
        0x17 => parse_future_read_write(ctx, FutureIoKind::Write),
        0x18 => {
            parse_stream_or_future_cancel(ctx, StreamFutureKind::Future, StreamFutureEnd::Readable)
        }
        0x19 => {
            parse_stream_or_future_cancel(ctx, StreamFutureKind::Future, StreamFutureEnd::Writable)
        }
        0x1a => {
            parse_stream_or_future_drop(ctx, StreamFutureKind::Future, StreamFutureEnd::Readable)
        }
        0x1b => {
            parse_stream_or_future_drop(ctx, StreamFutureKind::Future, StreamFutureEnd::Writable)
        }
        0x1c => parse_error_context_new(ctx),
        0x1d => parse_error_context_debug_message(ctx),
        0x1e => parse_error_context_drop(ctx),
        0x1f => parse_waitable_set_new(ctx),
        0x20 => parse_waitable_set_wait_poll(ctx, WaitableSetIoKind::Wait),
        0x21 => parse_waitable_set_wait_poll(ctx, WaitableSetIoKind::Poll),
        0x22 => parse_waitable_set_drop(ctx),
        0x23 => parse_waitable_join(ctx),
        0x24 => unsupported_canonical("backpressure.inc"),
        0x25 => unsupported_canonical("backpressure.dec"),
        0x26 => unsupported_canonical("thread.index"),
        0x27 => unsupported_canonical("thread.new-indirect"),
        0x28 => unsupported_canonical("thread.switch-to"),
        0x29 => unsupported_canonical("thread.suspend"),
        0x2a => unsupported_canonical("thread.resume-later"),
        0x2b => unsupported_canonical("thread.yield-to"),
        0x40 => unsupported_canonical("thread.spawn-ref"),
        0x41 => unsupported_canonical("thread.spawn-indirect"),
        0x42 => unsupported_canonical("thread.available-parallelism"),
        x => Err(ComponentParseError::Unsupported(format!(
            "unsupported canonical function opcode: 0x{x:02x}"
        ))),
    }
}

fn unsupported_canonical(name: &str) -> ParseResult<()> {
    Err(ComponentParseError::Unsupported(format!(
        "canonical function `{name}` is not implemented"
    )))
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

    let expected_core_ty =
        flatten_func_type_with_options(&func_ty, ctx, CanonMode::Lift, &options)?;
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

    let lowered_ty = flatten_func_type_with_options(&func_ty, ctx, CanonMode::Lower, &options)?;
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

fn parse_error_context_new(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let options = parse_error_context_options(ctx, ErrorContextCanonKind::New)?;
    let gidx =
        ctx.state
            .core_func_store
            .register(CoreRelation::Defined(CoreFunc::CanonErrorContextNew {
                options: Box::new(options),
            }));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32, CoreValType::I32],
        vec![CoreValType::I32],
    ));
    Ok(())
}

fn parse_error_context_debug_message(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let options = parse_error_context_options(ctx, ErrorContextCanonKind::DebugMessage)?;
    let gidx = ctx.state.core_func_store.register(CoreRelation::Defined(
        CoreFunc::CanonErrorContextDebugMessage {
            options: Box::new(options),
        },
    ));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32, CoreValType::I32],
        vec![],
    ));
    Ok(())
}

fn parse_error_context_drop(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonErrorContextDrop));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![CoreValType::I32], vec![]));
    Ok(())
}

fn parse_waitable_set_new(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonWaitableSetNew));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![], vec![CoreValType::I32]));
    Ok(())
}

#[derive(Clone, Copy)]
enum WaitableSetIoKind {
    Wait,
    Poll,
}

fn parse_waitable_set_wait_poll(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: WaitableSetIoKind,
) -> ParseResult<()> {
    let cancellable = match ctx.reader.read_exact_one()? {
        0 => false,
        1 => true,
        value => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid waitable-set cancellable flag: {value}"
            )))
        }
    };
    let memory_idx = parse_core_memory_local_idx(ctx)?;
    ctx.validator
        .scope()
        .core_memories
        .get(memory_idx)
        .map_err(|_| ComponentParseError::InvalidType("memory index out of bounds".to_owned()))?;
    let memory = ctx.state.scope().core_memories.get(memory_idx)?;
    let func = match kind {
        WaitableSetIoKind::Wait => CoreFunc::CanonWaitableSetWait {
            cancellable,
            memory,
        },
        WaitableSetIoKind::Poll => CoreFunc::CanonWaitableSetPoll {
            cancellable,
            memory,
        },
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(func));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32, CoreValType::I32],
        vec![CoreValType::I32],
    ));
    Ok(())
}

fn parse_waitable_set_drop(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonWaitableSetDrop));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![CoreValType::I32], vec![]));
    Ok(())
}

fn parse_waitable_join(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonWaitableJoin));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32, CoreValType::I32],
        vec![],
    ));
    Ok(())
}

fn parse_task_cancel(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonTaskCancel));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![], vec![]));
    Ok(())
}

fn parse_subtask_cancel(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let async_ = match ctx.reader.read_exact_one()? {
        0 => false,
        1 => true,
        value => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid subtask.cancel async flag: {value}"
            )))
        }
    };
    let gidx =
        ctx.state
            .core_func_store
            .register(CoreRelation::Defined(CoreFunc::CanonSubtaskCancel {
                async_,
            }));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32],
        vec![CoreValType::I32],
    ));
    Ok(())
}

fn parse_subtask_drop(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(CoreFunc::CanonSubtaskDrop));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![CoreValType::I32], vec![]));
    Ok(())
}

fn parse_task_return(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let result = parse_task_return_result(ctx)?;
    let options = parse_canonical_options(ctx, CanonMode::Lower)?;
    if options.realloc.is_some()
        || options.post_return.is_some()
        || options.async_
        || options.callback.is_some()
        || options.core_type.is_some()
        || options.gc
    {
        return Err(ComponentParseError::TypeMismatch(
            "canonical task.return only allows memory and string-encoding options".to_owned(),
        ));
    }
    let result_func_type = FuncType {
        params: result.iter().cloned().collect(),
        param_names: Vec::new(),
        result: None,
    };
    validate_required_options(ctx, &result_func_type, &options, CanonMode::Lower)?;
    let signature =
        flatten_func_type_with_options(&result_func_type, ctx, CanonMode::Lower, &options)?;
    let gidx =
        ctx.state
            .core_func_store
            .register(CoreRelation::Defined(CoreFunc::CanonTaskReturn {
                result,
                options: Box::new(options),
                signature: signature.clone(),
            }));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(signature);
    Ok(())
}

fn parse_task_return_result(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<Option<ValType>> {
    match ctx.reader.read_exact_one()? {
        0x00 => Ok(Some(parse_valtype(ctx)?)),
        0x01 => match ctx.reader.read_exact_one()? {
            0x00 => Ok(None),
            x => Err(ComponentParseError::InvalidSignature(format!(
                "Invalid task.return result type: {x}"
            ))),
        },
        x => Err(ComponentParseError::InvalidSignature(format!(
            "Invalid task.return result type: {x}"
        ))),
    }
}

#[derive(Clone, Copy)]
enum StreamFutureKind {
    Stream,
    Future,
}

#[derive(Clone, Copy)]
enum StreamFutureEnd {
    Readable,
    Writable,
}

fn parse_stream_or_future_new(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: StreamFutureKind,
) -> ParseResult<()> {
    let type_id = parse_stream_or_future_type(ctx, kind)?;
    let func = match kind {
        StreamFutureKind::Stream => CoreFunc::CanonStreamNew { type_id },
        StreamFutureKind::Future => CoreFunc::CanonFutureNew { type_id },
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(func));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![], vec![CoreValType::I64]));
    Ok(())
}

fn parse_stream_or_future_drop(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: StreamFutureKind,
    end: StreamFutureEnd,
) -> ParseResult<()> {
    let type_id = parse_stream_or_future_type(ctx, kind)?;
    let func = match (kind, end) {
        (StreamFutureKind::Stream, StreamFutureEnd::Readable) => {
            CoreFunc::CanonStreamDropReadable { type_id }
        }
        (StreamFutureKind::Stream, StreamFutureEnd::Writable) => {
            CoreFunc::CanonStreamDropWritable { type_id }
        }
        (StreamFutureKind::Future, StreamFutureEnd::Readable) => {
            CoreFunc::CanonFutureDropReadable { type_id }
        }
        (StreamFutureKind::Future, StreamFutureEnd::Writable) => {
            CoreFunc::CanonFutureDropWritable { type_id }
        }
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(func));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator
        .scope_mut()
        .core_funcs
        .add(CoreFuncType::new(vec![CoreValType::I32], vec![]));
    Ok(())
}

fn parse_stream_or_future_cancel(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: StreamFutureKind,
    end: StreamFutureEnd,
) -> ParseResult<()> {
    let type_id = parse_stream_or_future_type(ctx, kind)?;
    let async_ = match ctx.reader.read_exact_one()? {
        0 => false,
        1 => true,
        value => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid stream/future cancel async flag: {value}"
            )))
        }
    };
    let func = match (kind, end) {
        (StreamFutureKind::Stream, StreamFutureEnd::Readable) => {
            CoreFunc::CanonStreamCancelRead { type_id, async_ }
        }
        (StreamFutureKind::Stream, StreamFutureEnd::Writable) => {
            CoreFunc::CanonStreamCancelWrite { type_id, async_ }
        }
        (StreamFutureKind::Future, StreamFutureEnd::Readable) => {
            CoreFunc::CanonFutureCancelRead { type_id, async_ }
        }
        (StreamFutureKind::Future, StreamFutureEnd::Writable) => {
            CoreFunc::CanonFutureCancelWrite { type_id, async_ }
        }
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(func));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32],
        vec![CoreValType::I32],
    ));
    Ok(())
}

#[derive(Clone, Copy)]
enum StreamIoKind {
    Read,
    Write,
}

fn parse_stream_read_write(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: StreamIoKind,
) -> ParseResult<()> {
    let type_id = parse_stream_or_future_type(ctx, StreamFutureKind::Stream)?;
    let payload = stream_or_future_payload(ctx, type_id, StreamFutureKind::Stream)?;
    let options = parse_canonical_options(ctx, CanonMode::Lower)?;
    validate_stream_io_options(ctx, payload.as_ref(), &options, kind)?;
    let func = match kind {
        StreamIoKind::Read => CoreFunc::CanonStreamRead {
            type_id,
            options: Box::new(options),
        },
        StreamIoKind::Write => CoreFunc::CanonStreamWrite {
            type_id,
            options: Box::new(options),
        },
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(func));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32, CoreValType::I32, CoreValType::I32],
        vec![CoreValType::I32],
    ));
    Ok(())
}

#[derive(Clone, Copy)]
enum FutureIoKind {
    Read,
    Write,
}

fn parse_future_read_write(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: FutureIoKind,
) -> ParseResult<()> {
    let type_id = parse_stream_or_future_type(ctx, StreamFutureKind::Future)?;
    let payload = stream_or_future_payload(ctx, type_id, StreamFutureKind::Future)?;
    let options = parse_canonical_options(ctx, CanonMode::Lower)?;
    validate_future_io_options(ctx, payload.as_ref(), &options, kind)?;
    let func = match kind {
        FutureIoKind::Read => CoreFunc::CanonFutureRead {
            type_id,
            options: Box::new(options),
        },
        FutureIoKind::Write => CoreFunc::CanonFutureWrite {
            type_id,
            options: Box::new(options),
        },
    };
    let gidx = ctx
        .state
        .core_func_store
        .register(CoreRelation::Defined(func));
    ctx.state.scope_mut().core_funcs.register(gidx);
    ctx.validator.scope_mut().core_funcs.add(CoreFuncType::new(
        vec![CoreValType::I32, CoreValType::I32],
        vec![CoreValType::I32],
    ));
    Ok(())
}

fn parse_stream_or_future_type(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: StreamFutureKind,
) -> ParseResult<TypeId> {
    #[cfg(not(feature = "component-gated-feature-async"))]
    {
        let _ = ctx;
        let _ = kind;
        return Err(ComponentParseError::Unsupported(
            "stream/future canonical built-ins require the component-gated-feature-async feature"
                .to_owned(),
        ));
    }

    #[cfg(feature = "component-gated-feature-async")]
    {
        let idx = parse_type_local_idx(ctx)?;
        let type_id = ctx.validator.scope().type_indexes.get(idx)?;
        let ty = ctx.validator.get_type(type_id)?;
        let matches_kind = matches!(
            (kind, ty),
            (
                StreamFutureKind::Stream,
                Type::DefVal(DefValType::Stream(_))
            ) | (
                StreamFutureKind::Future,
                Type::DefVal(DefValType::Future(_))
            )
        );
        if matches_kind {
            Ok(type_id)
        } else {
            let expected = match kind {
                StreamFutureKind::Stream => "stream",
                StreamFutureKind::Future => "future",
            };
            Err(ComponentParseError::TypeMismatch(format!(
                "canonical {expected} built-in requires a {expected} type"
            )))
        }
    }
}

fn stream_or_future_payload(
    ctx: &ParseContext<impl BinaryReader>,
    type_id: TypeId,
    kind: StreamFutureKind,
) -> ParseResult<Option<ValType>> {
    #[cfg(not(feature = "component-gated-feature-async"))]
    {
        let _ = ctx;
        let _ = type_id;
        let _ = kind;
        Err(ComponentParseError::Unsupported(
            "stream/future canonical built-ins require the component-gated-feature-async feature"
                .to_owned(),
        ))
    }

    #[cfg(feature = "component-gated-feature-async")]
    {
        match (kind, ctx.validator.get_type(type_id)?) {
            (StreamFutureKind::Stream, Type::DefVal(DefValType::Stream(payload)))
            | (StreamFutureKind::Future, Type::DefVal(DefValType::Future(payload))) => {
                Ok(payload.clone())
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "stream/future canonical built-in used with the wrong type".to_owned(),
            )),
        }
    }
}

fn validate_future_io_options(
    ctx: &ParseContext<impl BinaryReader>,
    payload: Option<&ValType>,
    options: &CanonicalOptions,
    kind: FutureIoKind,
) -> ParseResult<()> {
    if options.callback.is_some()
        || options.post_return.is_some()
        || options.core_type.is_some()
        || options.gc
    {
        return Err(ComponentParseError::TypeMismatch(
            "canonical future.read/write only allow async, memory, realloc, and string-encoding options"
                .to_owned(),
        ));
    }
    if let Some(payload) = payload {
        if options.memory.is_none() {
            return Err(ComponentParseError::TypeMismatch(
                "canonical option `memory` is required".to_owned(),
            ));
        }
        if matches!(kind, FutureIoKind::Read)
            && type_needs_memory(payload, ctx)
            && options.realloc.is_none()
        {
            return Err(ComponentParseError::TypeMismatch(
                "canonical option `realloc` is required".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_stream_io_options(
    ctx: &ParseContext<impl BinaryReader>,
    payload: Option<&ValType>,
    options: &CanonicalOptions,
    kind: StreamIoKind,
) -> ParseResult<()> {
    if options.callback.is_some()
        || options.post_return.is_some()
        || options.core_type.is_some()
        || options.gc
    {
        return Err(ComponentParseError::TypeMismatch(
            "canonical stream.read/write only allow async, memory, realloc, and string-encoding options"
                .to_owned(),
        ));
    }
    if let Some(payload) = payload {
        if options.memory.is_none() {
            return Err(ComponentParseError::TypeMismatch(
                "canonical option `memory` is required".to_owned(),
            ));
        }
        if matches!(kind, StreamIoKind::Read)
            && type_needs_memory(payload, ctx)
            && options.realloc.is_none()
        {
            return Err(ComponentParseError::TypeMismatch(
                "canonical option `realloc` is required".to_owned(),
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ErrorContextCanonKind {
    New,
    DebugMessage,
}

fn parse_error_context_options(
    ctx: &mut ParseContext<impl BinaryReader>,
    kind: ErrorContextCanonKind,
) -> ParseResult<CanonicalOptions> {
    let options = parse_canonical_options(ctx, CanonMode::Lift)?;
    if options.async_ || options.callback.is_some() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical error-context built-ins do not allow async options".to_owned(),
        ));
    }
    if options.post_return.is_some() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical error-context built-ins do not allow post-return".to_owned(),
        ));
    }
    if options.core_type.is_some() || options.gc {
        return Err(ComponentParseError::TypeMismatch(
            "canonical error-context built-ins do not allow gc/core-type options".to_owned(),
        ));
    }
    if options.memory.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `memory` is required".to_owned(),
        ));
    }
    if matches!(kind, ErrorContextCanonKind::DebugMessage) && options.realloc.is_none() {
        return Err(ComponentParseError::TypeMismatch(
            "canonical option `realloc` is required".to_owned(),
        ));
    }
    Ok(options)
}
