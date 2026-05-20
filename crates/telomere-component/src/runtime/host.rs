use super::canonical::{
    component_value_to_direct_wasm, direct_wasm_to_component_value, element_stride,
    lift_component_args, lift_component_results, lift_error_context_debug_message,
    lower_component_args, lower_component_results, lower_error_context_debug_message,
    lowered_indirect_result_area, read_value_from_memory, write_memory, write_value_to_memory,
    COPY_BLOCKED, COPY_COMPLETED,
};
use super::*;

type PendingStreamCompletion = (u32, PendingStreamRead, Vec<Option<ComponentValue>>);

impl ResolvedCallable {
    pub(super) fn call<'a>(
        &'a self,
        store: &'a Store,
        args: &'a [ComponentValue],
    ) -> ComponentFuture<'a, Result<Vec<ComponentValue>, ComponentError>> {
        Box::pin(async move {
            match self {
                ResolvedCallable::Host(func) => func(store, args).await,
                ResolvedCallable::Core(export) => {
                    let core_args = args
                        .iter()
                        .map(component_value_to_direct_wasm)
                        .collect::<Result<Vec<_>, _>>()?;
                    let results =
                        call_core_export(&export.instance, store, &export.export_name, &core_args)
                            .await?;
                    results
                        .into_iter()
                        .map(direct_wasm_to_component_value)
                        .collect()
                }
                ResolvedCallable::Lifted {
                    core,
                    func_type,
                    options,
                    program,
                } => {
                    let core_args = lower_component_args(func_type, args, options, program, store)?;
                    if options.async_ {
                        options.shared.begin_task_return();
                        let core_result = if options.callback.is_some() {
                            run_async_callback_lift(core, options, store, &core_args).await
                        } else {
                            core.call(store, &core_args).await
                        };
                        let task_return = options.shared.finish_task_return();
                        core_result?;
                        return task_return
                            .and_then(|result| validate_task_return(func_type, result));
                    }
                    let core_results = core.call(store, &core_args).await?;
                    let lifted =
                        lift_component_results(func_type, &core_results, options, program, store)?;
                    if let Some(post_return) = &options.post_return {
                        post_return.call(store, &core_results).await?;
                    }
                    Ok(lifted)
                }
            }
        })
    }

    pub(super) fn call_sync(
        &self,
        store: &Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        match self {
            ResolvedCallable::Host(func) => {
                run_ready_future_sync(func(store, args), "host component call")?
            }
            ResolvedCallable::Core(export) => {
                let core_args = args
                    .iter()
                    .map(component_value_to_direct_wasm)
                    .collect::<Result<Vec<_>, _>>()?;
                let results = call_core_export_sync(
                    &export.instance,
                    store,
                    &export.export_name,
                    &core_args,
                )?;
                results
                    .into_iter()
                    .map(direct_wasm_to_component_value)
                    .collect()
            }
            ResolvedCallable::Lifted {
                core,
                func_type,
                options,
                program,
            } => {
                let core_args = lower_component_args(func_type, args, options, program, store)?;
                if options.async_ {
                    if options.callback.is_some() {
                        return Err(ComponentError::Unsupported(
                            "async canonical callback lift requires async component execution"
                                .to_owned(),
                        ));
                    }
                    options.shared.begin_task_return();
                    let core_result = core.call_sync(store, &core_args);
                    let task_return = options.shared.finish_task_return();
                    core_result?;
                    return task_return.and_then(|result| validate_task_return(func_type, result));
                }
                let core_results = core.call_sync(store, &core_args)?;
                let lifted =
                    lift_component_results(func_type, &core_results, options, program, store)?;
                if let Some(post_return) = &options.post_return {
                    post_return.call_sync(store, &core_results)?;
                }
                Ok(lifted)
            }
        }
    }
}

fn validate_task_return(
    func_type: &FuncType,
    result: Vec<ComponentValue>,
) -> Result<Vec<ComponentValue>, ComponentError> {
    match (&func_type.result, result.len()) {
        (None, 0) | (Some(_), 1) => Ok(result),
        (None, len) => Err(ComponentError::Runtime(format!(
            "async task returned {len} values for a void function"
        ))),
        (Some(_), len) => Err(ComponentError::Runtime(format!(
            "async task returned {len} values for a single-result function"
        ))),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallbackCode {
    Exit,
    Yield,
    Wait,
}

fn unpack_callback_result(
    result: Vec<WasmValue>,
    context: &str,
) -> Result<(CallbackCode, u32), ComponentError> {
    let packed = match result.as_slice() {
        [WasmValue::I32(packed)] => *packed as u32,
        [other] => {
            return Err(ComponentError::Runtime(format!(
                "{context} must return i32, got {other:?}"
            )))
        }
        values => {
            return Err(ComponentError::Runtime(format!(
                "{context} must return exactly one value, got {}",
                values.len()
            )))
        }
    };
    let code = match packed & 0x0f {
        0 => CallbackCode::Exit,
        1 => CallbackCode::Yield,
        2 => CallbackCode::Wait,
        other => {
            return Err(ComponentError::Trap(format!(
                "{context} returned invalid callback code {other}"
            )))
        }
    };
    Ok((code, packed >> 4))
}

async fn run_async_callback_lift(
    core: &RuntimeCoreFunc,
    options: &RuntimeCanonicalOptions,
    store: &Store,
    core_args: &[WasmValue],
) -> Result<Vec<WasmValue>, ComponentError> {
    let callback = options
        .callback
        .as_ref()
        .ok_or_else(|| ComponentError::Runtime("async canonical callback is missing".to_owned()))?;
    let mut state =
        unpack_callback_result(core.call(store, core_args).await?, "async callback callee")?;
    for _ in 0..1024 {
        if state.0 == CallbackCode::Exit {
            return Ok(Vec::new());
        }
        let event = match state.0 {
            CallbackCode::Exit => unreachable!(),
            CallbackCode::Yield => WaitableEvent {
                code: WaitableEventCode::None,
                index: 0,
                payload: 0,
            },
            CallbackCode::Wait => {
                let event = options.shared.poll_waitable_set(state.1)?;
                if matches!(event.code, WaitableEventCode::None) {
                    return Err(ComponentError::Unsupported(
                        "async canonical callback WAIT requires a ready local event".to_owned(),
                    ));
                }
                event
            }
        };
        state = unpack_callback_result(
            callback
                .call(
                    store,
                    &[
                        WasmValue::I32(event.code as i32),
                        WasmValue::I32(event.index as i32),
                        WasmValue::I32(event.payload as i32),
                    ],
                )
                .await?,
            "async canonical callback",
        )?;
    }
    Err(ComponentError::Trap(
        "async canonical callback did not exit".to_owned(),
    ))
}

impl RuntimeCoreFunc {
    pub(super) fn call<'a>(
        &'a self,
        store: &'a Store,
        args: &'a [WasmValue],
    ) -> ComponentFuture<'a, Result<Vec<WasmValue>, ComponentError>> {
        Box::pin(async move {
            match self {
                RuntimeCoreFunc::Export {
                    instance,
                    export_name,
                    ..
                } => call_core_export(instance, store, export_name, args).await,
                RuntimeCoreFunc::Host(binding) => binding.call(store, args).await,
            }
        })
    }

    pub(super) fn call_sync(
        &self,
        store: &Store,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, ComponentError> {
        match self {
            RuntimeCoreFunc::Export {
                instance,
                export_name,
                ..
            } => call_core_export_sync(instance, store, export_name, args),
            RuntimeCoreFunc::Host(binding) => binding.call_sync(store, args),
        }
    }
}

impl HostBinding {
    fn signature(&self) -> CoreFuncType {
        match self {
            HostBinding::Lower { signature, .. }
            | HostBinding::ResourceNew { signature, .. }
            | HostBinding::ResourceDrop { signature, .. }
            | HostBinding::ResourceRep { signature, .. }
            | HostBinding::ErrorContextNew { signature, .. }
            | HostBinding::ErrorContextDebugMessage { signature, .. }
            | HostBinding::ErrorContextDrop { signature, .. }
            | HostBinding::TaskCancel { signature, .. }
            | HostBinding::SubtaskCancel { signature, .. }
            | HostBinding::SubtaskDrop { signature, .. }
            | HostBinding::WaitableSetNew { signature, .. }
            | HostBinding::WaitableSetWait { signature, .. }
            | HostBinding::WaitableSetPoll { signature, .. }
            | HostBinding::WaitableSetDrop { signature, .. }
            | HostBinding::WaitableJoin { signature, .. }
            | HostBinding::TaskReturn { signature, .. }
            | HostBinding::StreamFutureNew { signature, .. }
            | HostBinding::StreamRead { signature, .. }
            | HostBinding::StreamWrite { signature, .. }
            | HostBinding::StreamFutureCancel { signature, .. }
            | HostBinding::StreamFutureDrop { signature, .. }
            | HostBinding::FutureRead { signature, .. }
            | HostBinding::FutureWrite { signature, .. } => signature.clone(),
        }
    }

    fn call<'a>(
        &'a self,
        store: &'a Store,
        args: &'a [WasmValue],
    ) -> ComponentFuture<'a, Result<Vec<WasmValue>, ComponentError>> {
        Box::pin(async move {
            match self {
                HostBinding::Lower {
                    callable,
                    func_type,
                    options,
                    program,
                    ..
                } => {
                    let result_area =
                        lowered_indirect_result_area(func_type, args, options, program)?;
                    let component_args =
                        lift_component_args(func_type, args, options, program, store)?;
                    let results = callable.call(store, &component_args).await?;
                    lower_component_results(
                        func_type,
                        &results,
                        options,
                        program,
                        store,
                        result_area,
                    )
                }
                HostBinding::ResourceNew { .. }
                | HostBinding::ResourceRep { .. }
                | HostBinding::ErrorContextNew { .. }
                | HostBinding::ErrorContextDebugMessage { .. }
                | HostBinding::ErrorContextDrop { .. }
                | HostBinding::TaskCancel { .. }
                | HostBinding::SubtaskCancel { .. }
                | HostBinding::SubtaskDrop { .. }
                | HostBinding::WaitableSetNew { .. }
                | HostBinding::WaitableSetWait { .. }
                | HostBinding::WaitableSetPoll { .. }
                | HostBinding::WaitableSetDrop { .. }
                | HostBinding::WaitableJoin { .. }
                | HostBinding::TaskReturn { .. }
                | HostBinding::StreamFutureNew { .. }
                | HostBinding::StreamRead { .. }
                | HostBinding::StreamWrite { .. }
                | HostBinding::StreamFutureCancel { .. }
                | HostBinding::StreamFutureDrop { .. }
                | HostBinding::FutureRead { .. }
                | HostBinding::FutureWrite { .. } => self.call_sync(store, args),
                HostBinding::ResourceDrop {
                    resource, shared, ..
                } => {
                    let handle = match args.first() {
                        Some(WasmValue::I32(v)) => *v as u32,
                        Some(other) => {
                            return Err(ComponentError::Runtime(format!(
                                "resource.drop expects i32, got {other:?}"
                            )))
                        }
                        None => {
                            return Err(ComponentError::Runtime(
                                "resource.drop missing handle".to_owned(),
                            ))
                        }
                    };
                    let (rep, destructor) = shared.drop_resource(*resource, handle)?;
                    if let Some(dtor) = destructor {
                        dtor.call(store, &[WasmValue::I32(rep)]).await?;
                    }
                    Ok(Vec::new())
                }
            }
        })
    }

    fn call_sync(
        &self,
        store: &Store,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, ComponentError> {
        match self {
            HostBinding::Lower {
                callable,
                func_type,
                options,
                program,
                ..
            } => {
                if options.async_ {
                    return Err(ComponentError::Unsupported(
                        "async canonical lower requires async component execution".to_owned(),
                    ));
                }
                let result_area = lowered_indirect_result_area(func_type, args, options, program)?;
                let component_args = lift_component_args(func_type, args, options, program, store)?;
                let results = callable.call_sync(store, &component_args)?;
                lower_component_results(func_type, &results, options, program, store, result_area)
            }
            HostBinding::ResourceNew {
                resource,
                destructor,
                shared,
                ..
            } => {
                let rep = match args.first() {
                    Some(WasmValue::I32(v)) => *v,
                    Some(other) => {
                        return Err(ComponentError::Runtime(format!(
                            "resource.new expects i32, got {other:?}"
                        )))
                    }
                    None => {
                        return Err(ComponentError::Runtime(
                            "resource.new missing rep argument".to_owned(),
                        ))
                    }
                };
                let handle = shared.alloc_resource(*resource, rep, destructor.clone());
                Ok(vec![WasmValue::I32(handle as i32)])
            }
            HostBinding::ResourceDrop {
                resource, shared, ..
            } => {
                let handle = match args.first() {
                    Some(WasmValue::I32(v)) => *v as u32,
                    Some(other) => {
                        return Err(ComponentError::Runtime(format!(
                            "resource.drop expects i32, got {other:?}"
                        )))
                    }
                    None => {
                        return Err(ComponentError::Runtime(
                            "resource.drop missing handle".to_owned(),
                        ))
                    }
                };
                let (rep, destructor) = shared.drop_resource(*resource, handle)?;
                if let Some(dtor) = destructor {
                    dtor.call_sync(store, &[WasmValue::I32(rep)])?;
                }
                Ok(Vec::new())
            }
            HostBinding::ResourceRep {
                resource, shared, ..
            } => {
                let handle = match args.first() {
                    Some(WasmValue::I32(v)) => *v as u32,
                    Some(other) => {
                        return Err(ComponentError::Runtime(format!(
                            "resource.rep expects i32, got {other:?}"
                        )))
                    }
                    None => {
                        return Err(ComponentError::Runtime(
                            "resource.rep missing handle".to_owned(),
                        ))
                    }
                };
                Ok(vec![WasmValue::I32(
                    shared.resource_rep(*resource, handle)?,
                )])
            }
            HostBinding::ErrorContextNew {
                options, shared, ..
            } => {
                let (ptr, len) = expect_i32_pair(args, "error-context.new")?;
                let debug_message =
                    lift_error_context_debug_message(options, store, ptr as u32, len as u32)?;
                let handle = shared.alloc_error_context(debug_message);
                Ok(vec![WasmValue::I32(handle as i32)])
            }
            HostBinding::ErrorContextDebugMessage {
                options, shared, ..
            } => {
                let (handle, ptr) = expect_i32_pair(args, "error-context.debug-message")?;
                let debug_message = shared.error_context_debug_message(handle as u32)?;
                lower_error_context_debug_message(options, store, &debug_message, ptr as u32)?;
                Ok(Vec::new())
            }
            HostBinding::ErrorContextDrop { shared, .. } => {
                let handle = expect_i32_arg(args, "error-context.drop")?;
                shared.drop_error_context(handle as u32)?;
                Ok(Vec::new())
            }
            HostBinding::TaskCancel { .. } => {
                expect_no_args(args, "task.cancel")?;
                Ok(Vec::new())
            }
            HostBinding::SubtaskCancel { .. } => {
                let _ = expect_i32_arg(args, "subtask.cancel")?;
                Err(ComponentError::Unsupported(
                    "subtask.cancel requires async-lowered subtask handles, which are not implemented yet"
                        .to_owned(),
                ))
            }
            HostBinding::SubtaskDrop { .. } => {
                let _ = expect_i32_arg(args, "subtask.drop")?;
                Err(ComponentError::Unsupported(
                    "subtask.drop requires async-lowered subtask handles, which are not implemented yet"
                        .to_owned(),
                ))
            }
            HostBinding::WaitableSetNew { shared, .. } => {
                expect_no_args(args, "waitable-set.new")?;
                Ok(vec![WasmValue::I32(shared.alloc_waitable_set() as i32)])
            }
            HostBinding::WaitableSetWait { memory, shared, .. } => {
                let (set, ptr) = expect_i32_pair(args, "waitable-set.wait")?;
                let event = shared.poll_waitable_set(set as u32)?;
                if matches!(event.code, WaitableEventCode::None) {
                    return Err(ComponentError::Unsupported(
                        "blocking waitable-set.wait requires scheduler event delivery".to_owned(),
                    ));
                }
                write_waitable_event(store, memory, ptr as u32, event)?;
                Ok(vec![WasmValue::I32(event.code as i32)])
            }
            HostBinding::WaitableSetPoll { memory, shared, .. } => {
                let (set, ptr) = expect_i32_pair(args, "waitable-set.poll")?;
                let event = shared.poll_waitable_set(set as u32)?;
                write_waitable_event(store, memory, ptr as u32, event)?;
                Ok(vec![WasmValue::I32(event.code as i32)])
            }
            HostBinding::WaitableSetDrop { shared, .. } => {
                let handle = expect_i32_arg(args, "waitable-set.drop")?;
                shared.drop_waitable_set(handle as u32)?;
                Ok(Vec::new())
            }
            HostBinding::WaitableJoin { shared, .. } => {
                let (waitable, set) = expect_i32_pair(args, "waitable.join")?;
                shared.join_waitable(waitable as u32, set as u32)?;
                Ok(Vec::new())
            }
            HostBinding::TaskReturn {
                result_func_type,
                options,
                program,
                shared,
                ..
            } => {
                let results = lift_component_args(result_func_type, args, options, program, store)?;
                shared.set_task_return(results)?;
                Ok(Vec::new())
            }
            HostBinding::StreamFutureNew {
                type_id,
                kind,
                shared,
                ..
            } => {
                expect_no_args(args, stream_future_context(*kind, None, "new"))?;
                let (readable, writable) = shared.alloc_stream_future(*type_id, *kind);
                Ok(vec![WasmValue::I64(
                    i64::from(readable) | (i64::from(writable) << 32),
                )])
            }
            HostBinding::StreamWrite {
                type_id,
                payload,
                options,
                program,
                shared,
                ..
            } => {
                let (handle, ptr, count) = expect_i32_triple(args, "stream.write")?;
                let values = read_stream_payloads_from_memory(
                    payload.as_ref(),
                    options,
                    program,
                    store,
                    ptr as u32,
                    count as u32,
                )?;
                let written = shared.write_stream_payloads(handle as u32, *type_id, values)?;
                if let Some((readable, request, values)) =
                    shared.take_ready_pending_stream_read_for_writable(handle as u32, *type_id)?
                {
                    write_stream_payloads_to_memory(
                        request.payload.as_ref(),
                        &request.options,
                        &request.program,
                        store,
                        request.ptr,
                        &values,
                    )?;
                    shared.set_waitable_event(
                        handle as u32,
                        WaitableEvent {
                            code: WaitableEventCode::StreamRead,
                            index: readable,
                            payload: copy_result_completed(values.len() as u32) as u32,
                        },
                    )?;
                }
                Ok(vec![WasmValue::I32(copy_result_completed(written))])
            }
            HostBinding::StreamRead {
                type_id,
                payload,
                options,
                program,
                shared,
                ..
            } => {
                let (handle, ptr, count) = expect_i32_triple(args, "stream.read")?;
                let values = shared.read_stream_payloads(handle as u32, *type_id, count as u32)?;
                if values.is_empty() && count != 0 && options.async_ {
                    shared.register_pending_stream_read(
                        handle as u32,
                        PendingStreamRead {
                            type_id: *type_id,
                            payload: payload.clone(),
                            options: options.clone(),
                            program: program.clone(),
                            ptr: ptr as u32,
                            count: count as u32,
                        },
                    )?;
                    return Ok(vec![WasmValue::I32(COPY_BLOCKED)]);
                }
                if values.is_empty() && count != 0 {
                    return Err(ComponentError::Trap(format!(
                        "stream handle {handle} is not ready"
                    )));
                }
                write_stream_payloads_to_memory(
                    payload.as_ref(),
                    options,
                    program,
                    store,
                    ptr as u32,
                    &values,
                )?;
                Ok(vec![WasmValue::I32(copy_result_completed(
                    values.len() as u32
                ))])
            }
            HostBinding::StreamFutureCancel {
                type_id,
                kind,
                end,
                async_,
                shared,
                ..
            } => {
                let handle =
                    expect_i32_arg(args, stream_future_context(*kind, Some(*end), "cancel"))?;
                let status =
                    shared.cancel_stream_future(handle as u32, *type_id, *kind, *end, *async_)?;
                Ok(vec![WasmValue::I32(status)])
            }
            HostBinding::StreamFutureDrop {
                type_id,
                kind,
                end,
                shared,
                ..
            } => {
                let context = stream_future_context(*kind, Some(*end), "drop");
                let handle = expect_i32_arg(args, context)?;
                shared.drop_stream_future(handle as u32, *type_id, *kind, *end)?;
                Ok(Vec::new())
            }
            HostBinding::FutureWrite {
                type_id,
                payload,
                options,
                program,
                shared,
                ..
            } => {
                let (handle, payload_ptr) = expect_i32_pair(args, "future.write")?;
                let value = match payload {
                    Some(ty) => {
                        let memory = options.memory.as_ref().ok_or_else(|| {
                            ComponentError::Runtime(
                                "canonical option `memory` is required".to_owned(),
                            )
                        })?;
                        Some(read_value_from_memory(
                            ty,
                            options,
                            program,
                            store,
                            memory,
                            payload_ptr as u32,
                        )?)
                    }
                    None => None,
                };
                if let Some((readable, request, value)) =
                    shared.write_future_payload(handle as u32, *type_id, value)?
                {
                    write_future_payload_to_memory(&request, value, store)?;
                    shared.set_waitable_event(
                        readable,
                        WaitableEvent {
                            code: WaitableEventCode::FutureRead,
                            index: readable,
                            payload: COPY_COMPLETED as u32,
                        },
                    )?;
                }
                Ok(vec![WasmValue::I32(COPY_COMPLETED)])
            }
            HostBinding::FutureRead {
                type_id,
                payload,
                options,
                program,
                shared,
                ..
            } => {
                let (handle, payload_ptr) = expect_i32_pair(args, "future.read")?;
                let Some(value) =
                    shared.read_future_payload(handle as u32, *type_id, options.async_)?
                else {
                    shared.register_pending_future_read(
                        handle as u32,
                        PendingFutureRead {
                            type_id: *type_id,
                            payload: payload.clone(),
                            options: options.clone(),
                            program: program.clone(),
                            ptr: payload_ptr as u32,
                        },
                    )?;
                    return Ok(vec![WasmValue::I32(COPY_BLOCKED)]);
                };
                match (payload, value) {
                    (Some(ty), Some(value)) => {
                        options.memory.as_ref().ok_or_else(|| {
                            ComponentError::Runtime(
                                "canonical option `memory` is required".to_owned(),
                            )
                        })?;
                        write_value_to_memory(
                            &value,
                            ty,
                            options,
                            program,
                            store,
                            payload_ptr as u32,
                        )?;
                    }
                    (None, None) => {}
                    (Some(_), None) => {
                        return Err(ComponentError::Trap("future payload is missing".to_owned()))
                    }
                    (None, Some(_)) => {
                        return Err(ComponentError::Trap(
                            "void future carried a payload".to_owned(),
                        ))
                    }
                }
                Ok(vec![WasmValue::I32(COPY_COMPLETED)])
            }
        }
    }
}

fn stream_future_context(
    kind: StreamFutureKind,
    end: Option<StreamFutureEnd>,
    op: &str,
) -> &'static str {
    match (kind, end, op) {
        (StreamFutureKind::Stream, None, "new") => "stream.new",
        (StreamFutureKind::Future, None, "new") => "future.new",
        (StreamFutureKind::Stream, Some(StreamFutureEnd::Readable), "drop") => {
            "stream.drop-readable"
        }
        (StreamFutureKind::Stream, Some(StreamFutureEnd::Writable), "drop") => {
            "stream.drop-writable"
        }
        (StreamFutureKind::Stream, Some(StreamFutureEnd::Readable), "cancel") => {
            "stream.cancel-read"
        }
        (StreamFutureKind::Stream, Some(StreamFutureEnd::Writable), "cancel") => {
            "stream.cancel-write"
        }
        (StreamFutureKind::Future, Some(StreamFutureEnd::Readable), "drop") => {
            "future.drop-readable"
        }
        (StreamFutureKind::Future, Some(StreamFutureEnd::Writable), "drop") => {
            "future.drop-writable"
        }
        (StreamFutureKind::Future, Some(StreamFutureEnd::Readable), "cancel") => {
            "future.cancel-read"
        }
        (StreamFutureKind::Future, Some(StreamFutureEnd::Writable), "cancel") => {
            "future.cancel-write"
        }
        _ => "stream/future",
    }
}

fn expect_no_args(args: &[WasmValue], context: &str) -> Result<(), ComponentError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(ComponentError::Runtime(format!(
            "{context} expects 0 arguments, got {}",
            args.len()
        )))
    }
}

fn expect_i32_arg(args: &[WasmValue], context: &str) -> Result<i32, ComponentError> {
    match args {
        [WasmValue::I32(value)] => Ok(*value),
        [other] => Err(ComponentError::Runtime(format!(
            "{context} expects i32, got {other:?}"
        ))),
        _ => Err(ComponentError::Runtime(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_i32_pair(args: &[WasmValue], context: &str) -> Result<(i32, i32), ComponentError> {
    match args {
        [WasmValue::I32(lhs), WasmValue::I32(rhs)] => Ok((*lhs, *rhs)),
        [lhs, rhs] => Err(ComponentError::Runtime(format!(
            "{context} expects i32/i32, got {lhs:?}/{rhs:?}"
        ))),
        _ => Err(ComponentError::Runtime(format!(
            "{context} expects 2 arguments, got {}",
            args.len()
        ))),
    }
}

fn expect_i32_triple(args: &[WasmValue], context: &str) -> Result<(i32, i32, i32), ComponentError> {
    match args {
        [WasmValue::I32(a), WasmValue::I32(b), WasmValue::I32(c)] => Ok((*a, *b, *c)),
        [a, b, c] => Err(ComponentError::Runtime(format!(
            "{context} expects i32/i32/i32, got {a:?}/{b:?}/{c:?}"
        ))),
        _ => Err(ComponentError::Runtime(format!(
            "{context} expects 3 arguments, got {}",
            args.len()
        ))),
    }
}

fn copy_result_completed(count: u32) -> i32 {
    ((count as i32) << 4) | COPY_COMPLETED
}

fn read_stream_payloads_from_memory(
    payload: Option<&ValType>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
    count: u32,
) -> Result<Vec<Option<ComponentValue>>, ComponentError> {
    let Some(ty) = payload else {
        return Ok((0..count).map(|_| None).collect());
    };
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let stride = element_stride(ty, program)?;
    (0..count)
        .map(|index| {
            read_value_from_memory(
                ty,
                options,
                program,
                store,
                memory,
                ptr + index.saturating_mul(stride),
            )
            .map(Some)
        })
        .collect()
}

fn write_stream_payloads_to_memory(
    payload: Option<&ValType>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
    values: &[Option<ComponentValue>],
) -> Result<(), ComponentError> {
    let Some(ty) = payload else {
        return Ok(());
    };
    let stride = element_stride(ty, program)?;
    for (index, value) in values.iter().enumerate() {
        let value = value
            .as_ref()
            .ok_or_else(|| ComponentError::Trap("stream payload is missing".to_owned()))?;
        write_value_to_memory(
            value,
            ty,
            options,
            program,
            store,
            ptr + (index as u32).saturating_mul(stride),
        )?;
    }
    Ok(())
}

fn write_future_payload_to_memory(
    request: &PendingFutureRead,
    value: Option<ComponentValue>,
    store: &Store,
) -> Result<(), ComponentError> {
    match (&request.payload, value) {
        (Some(ty), Some(value)) => write_value_to_memory(
            &value,
            ty,
            &request.options,
            &request.program,
            store,
            request.ptr,
        ),
        (None, None) => Ok(()),
        (Some(_), None) => Err(ComponentError::Trap("future payload is missing".to_owned())),
        (None, Some(_)) => Err(ComponentError::Trap(
            "void future carried a payload".to_owned(),
        )),
    }
}

fn write_waitable_event(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    event: WaitableEvent,
) -> Result<(), ComponentError> {
    write_memory(store, memory, ptr, &event.index.to_le_bytes())?;
    write_memory(store, memory, ptr + 4, &event.payload.to_le_bytes())
}

impl SharedState {
    fn alloc_resource(
        &self,
        resource: ResourceId,
        rep: i32,
        destructor: Option<RuntimeCoreFunc>,
    ) -> u32 {
        let handle = self.next_resource_handle.get() + 1;
        self.next_resource_handle.set(handle);
        self.resources
            .borrow_mut()
            .entry(resource)
            .or_default()
            .insert(handle, ResourceRecord { rep, destructor });
        handle
    }

    fn drop_resource(
        &self,
        resource: ResourceId,
        handle: u32,
    ) -> Result<(i32, Option<RuntimeCoreFunc>), ComponentError> {
        self.resources
            .borrow_mut()
            .get_mut(&resource)
            .and_then(|records| records.remove(&handle))
            .map(|record| (record.rep, record.destructor))
            .ok_or_else(|| ComponentError::Trap(format!("resource handle {handle} is invalid")))
    }

    fn resource_rep(&self, resource: ResourceId, handle: u32) -> Result<i32, ComponentError> {
        self.resources
            .borrow()
            .get(&resource)
            .and_then(|records| records.get(&handle))
            .map(|record| record.rep)
            .ok_or_else(|| ComponentError::Trap(format!("resource handle {handle} is invalid")))
    }

    fn alloc_error_context(&self, debug_message: String) -> u32 {
        let handle = self.next_resource_handle.get() + 1;
        self.next_resource_handle.set(handle);
        self.error_contexts
            .borrow_mut()
            .insert(handle, debug_message);
        handle
    }

    fn error_context_debug_message(&self, handle: u32) -> Result<String, ComponentError> {
        self.error_contexts
            .borrow()
            .get(&handle)
            .cloned()
            .ok_or_else(|| {
                ComponentError::Trap(format!("error-context handle {handle} is invalid"))
            })
    }

    fn drop_error_context(&self, handle: u32) -> Result<(), ComponentError> {
        self.error_contexts
            .borrow_mut()
            .remove(&handle)
            .map(|_| ())
            .ok_or_else(|| {
                ComponentError::Trap(format!("error-context handle {handle} is invalid"))
            })
    }

    fn alloc_waitable_set(&self) -> u32 {
        let handle = self.next_resource_handle.get() + 1;
        self.next_resource_handle.set(handle);
        self.waitable_sets
            .borrow_mut()
            .insert(handle, WaitableSet::default());
        handle
    }

    fn drop_waitable_set(&self, handle: u32) -> Result<(), ComponentError> {
        let mut sets = self.waitable_sets.borrow_mut();
        let Some(set) = sets.get(&handle) else {
            return Err(ComponentError::Trap(format!(
                "waitable-set handle {handle} is invalid"
            )));
        };
        if !set.members.is_empty() {
            return Err(ComponentError::Trap(format!(
                "waitable-set handle {handle} is not empty"
            )));
        }
        sets.remove(&handle);
        Ok(())
    }

    fn join_waitable(&self, waitable: u32, set: u32) -> Result<(), ComponentError> {
        let mut handles = self.stream_future_handles.borrow_mut();
        let Some(record) = handles.get_mut(&waitable) else {
            return Err(ComponentError::Trap(format!(
                "waitable handle {waitable} is invalid"
            )));
        };
        let old_set = record.waitable_set.take();
        if let Some(old_set) = old_set {
            if let Some(set_record) = self.waitable_sets.borrow_mut().get_mut(&old_set) {
                set_record.members.retain(|member| *member != waitable);
            }
        }
        if set == 0 {
            return Ok(());
        }
        let mut sets = self.waitable_sets.borrow_mut();
        let Some(set_record) = sets.get_mut(&set) else {
            return Err(ComponentError::Trap(format!(
                "waitable-set handle {set} is invalid"
            )));
        };
        if !set_record.members.contains(&waitable) {
            set_record.members.push(waitable);
        }
        record.waitable_set = Some(set);
        Ok(())
    }

    fn poll_waitable_set(&self, set: u32) -> Result<WaitableEvent, ComponentError> {
        let members = self
            .waitable_sets
            .borrow()
            .get(&set)
            .map(|set| set.members.clone())
            .ok_or_else(|| ComponentError::Trap(format!("waitable-set handle {set} is invalid")))?;
        let mut events = self.waitable_events.borrow_mut();
        for member in members {
            if let Some(event) = events.remove(&member) {
                return Ok(event);
            }
        }
        Ok(WaitableEvent {
            code: WaitableEventCode::None,
            index: 0,
            payload: 0,
        })
    }

    fn set_waitable_event(&self, handle: u32, event: WaitableEvent) -> Result<(), ComponentError> {
        self.stream_future_handles
            .borrow()
            .get(&handle)
            .ok_or_else(|| ComponentError::Trap(format!("waitable handle {handle} is invalid")))?;
        self.waitable_events.borrow_mut().insert(handle, event);
        Ok(())
    }

    fn alloc_stream_future(&self, type_id: TypeId, kind: StreamFutureKind) -> (u32, u32) {
        let readable = self.next_resource_handle.get() + 1;
        let writable = readable + 1;
        self.next_resource_handle.set(writable);
        let mut handles = self.stream_future_handles.borrow_mut();
        handles.insert(
            readable,
            StreamFutureHandle {
                type_id,
                kind,
                end: StreamFutureEnd::Readable,
                peer: writable,
                waitable_set: None,
            },
        );
        handles.insert(
            writable,
            StreamFutureHandle {
                type_id,
                kind,
                end: StreamFutureEnd::Writable,
                peer: readable,
                waitable_set: None,
            },
        );
        (readable, writable)
    }

    fn drop_stream_future(
        &self,
        handle: u32,
        type_id: TypeId,
        kind: StreamFutureKind,
        end: StreamFutureEnd,
    ) -> Result<(), ComponentError> {
        let mut handles = self.stream_future_handles.borrow_mut();
        let Some(record) = handles.get(&handle).copied() else {
            return Err(ComponentError::Trap(format!(
                "{} handle {handle} is invalid",
                stream_future_kind_name(kind)
            )));
        };
        if record.type_id != type_id || record.kind != kind || record.end != end {
            return Err(ComponentError::Trap(format!(
                "{} handle {handle} has the wrong type or endpoint",
                stream_future_kind_name(kind)
            )));
        }
        handles.remove(&handle);
        if let Some(set) = record.waitable_set {
            if let Some(set_record) = self.waitable_sets.borrow_mut().get_mut(&set) {
                set_record.members.retain(|member| *member != handle);
            }
        }
        self.waitable_events.borrow_mut().remove(&handle);
        self.pending_stream_reads.borrow_mut().remove(&handle);
        self.pending_future_reads.borrow_mut().remove(&handle);
        if kind == StreamFutureKind::Future && end == StreamFutureEnd::Readable {
            self.future_payloads.borrow_mut().remove(&handle);
        } else if kind == StreamFutureKind::Stream && end == StreamFutureEnd::Readable {
            self.stream_payloads.borrow_mut().remove(&handle);
        }
        Ok(())
    }

    fn cancel_stream_future(
        &self,
        handle: u32,
        type_id: TypeId,
        kind: StreamFutureKind,
        end: StreamFutureEnd,
        async_: bool,
    ) -> Result<i32, ComponentError> {
        self.stream_future_handle(handle, type_id, kind, end)?;
        let mut event_handle = handle;
        let cancelled = match (kind, end) {
            (StreamFutureKind::Stream, StreamFutureEnd::Readable) => self
                .pending_stream_reads
                .borrow_mut()
                .remove(&handle)
                .is_some(),
            (StreamFutureKind::Future, StreamFutureEnd::Readable) => self
                .pending_future_reads
                .borrow_mut()
                .remove(&handle)
                .is_some(),
            (StreamFutureKind::Stream, StreamFutureEnd::Writable) => {
                let record = self.stream_future_handle(handle, type_id, kind, end)?;
                event_handle = record.peer;
                self.stream_payloads
                    .borrow_mut()
                    .remove(&record.peer)
                    .is_some()
            }
            (StreamFutureKind::Future, StreamFutureEnd::Writable) => {
                let record = self.stream_future_handle(handle, type_id, kind, end)?;
                event_handle = record.peer;
                self.future_payloads
                    .borrow_mut()
                    .remove(&record.peer)
                    .is_some()
            }
        };
        self.waitable_events.borrow_mut().remove(&event_handle);
        if !cancelled && async_ {
            Ok(COPY_BLOCKED)
        } else {
            Ok(COPY_COMPLETED)
        }
    }

    fn write_stream_payloads(
        &self,
        handle: u32,
        type_id: TypeId,
        values: Vec<Option<ComponentValue>>,
    ) -> Result<u32, ComponentError> {
        let record = self.stream_future_handle(
            handle,
            type_id,
            StreamFutureKind::Stream,
            StreamFutureEnd::Writable,
        )?;
        self.stream_future_handle(
            record.peer,
            type_id,
            StreamFutureKind::Stream,
            StreamFutureEnd::Readable,
        )?;
        let count = values.len() as u32;
        self.stream_payloads
            .borrow_mut()
            .entry(record.peer)
            .or_default()
            .extend(values);
        Ok(count)
    }

    fn register_pending_stream_read(
        &self,
        handle: u32,
        request: PendingStreamRead,
    ) -> Result<(), ComponentError> {
        self.stream_future_handle(
            handle,
            request.type_id,
            StreamFutureKind::Stream,
            StreamFutureEnd::Readable,
        )?;
        if self.pending_stream_reads.borrow().contains_key(&handle) {
            return Err(ComponentError::Trap(format!(
                "stream handle {handle} already has a pending read"
            )));
        }
        self.pending_stream_reads
            .borrow_mut()
            .insert(handle, request);
        Ok(())
    }

    fn take_ready_pending_stream_read_for_writable(
        &self,
        writable: u32,
        type_id: TypeId,
    ) -> Result<Option<PendingStreamCompletion>, ComponentError> {
        let record = self.stream_future_handle(
            writable,
            type_id,
            StreamFutureKind::Stream,
            StreamFutureEnd::Writable,
        )?;
        let readable = record.peer;
        let Some(request) = self.pending_stream_reads.borrow_mut().remove(&readable) else {
            return Ok(None);
        };
        let values = self.read_stream_payloads(readable, type_id, request.count)?;
        if values.is_empty() {
            self.pending_stream_reads
                .borrow_mut()
                .insert(readable, request);
            return Ok(None);
        }
        Ok(Some((readable, request, values)))
    }

    fn read_stream_payloads(
        &self,
        handle: u32,
        type_id: TypeId,
        count: u32,
    ) -> Result<Vec<Option<ComponentValue>>, ComponentError> {
        self.stream_future_handle(
            handle,
            type_id,
            StreamFutureKind::Stream,
            StreamFutureEnd::Readable,
        )?;
        let mut payloads = self.stream_payloads.borrow_mut();
        let Some(queue) = payloads.get_mut(&handle) else {
            return Ok(Vec::new());
        };
        let limit = count as usize;
        let mut values = Vec::with_capacity(limit.min(queue.len()));
        for _ in 0..limit {
            let Some(value) = queue.pop_front() else {
                break;
            };
            values.push(value);
        }
        if queue.is_empty() {
            payloads.remove(&handle);
        }
        Ok(values)
    }

    fn write_future_payload(
        &self,
        handle: u32,
        type_id: TypeId,
        value: Option<ComponentValue>,
    ) -> Result<Option<(u32, PendingFutureRead, Option<ComponentValue>)>, ComponentError> {
        let record = self.stream_future_handle(
            handle,
            type_id,
            StreamFutureKind::Future,
            StreamFutureEnd::Writable,
        )?;
        self.stream_future_handle(
            record.peer,
            type_id,
            StreamFutureKind::Future,
            StreamFutureEnd::Readable,
        )?;
        if let Some(request) = self.pending_future_reads.borrow_mut().remove(&record.peer) {
            return Ok(Some((record.peer, request, value)));
        }
        let mut payloads = self.future_payloads.borrow_mut();
        if payloads.contains_key(&record.peer) {
            return Err(ComponentError::Trap(format!(
                "future handle {} already has a payload",
                record.peer
            )));
        }
        payloads.insert(record.peer, value.into_iter().collect());
        Ok(None)
    }

    fn register_pending_future_read(
        &self,
        handle: u32,
        request: PendingFutureRead,
    ) -> Result<(), ComponentError> {
        self.stream_future_handle(
            handle,
            request.type_id,
            StreamFutureKind::Future,
            StreamFutureEnd::Readable,
        )?;
        if self.pending_future_reads.borrow().contains_key(&handle) {
            return Err(ComponentError::Trap(format!(
                "future handle {handle} already has a pending read"
            )));
        }
        self.pending_future_reads
            .borrow_mut()
            .insert(handle, request);
        Ok(())
    }

    fn read_future_payload(
        &self,
        handle: u32,
        type_id: TypeId,
        async_: bool,
    ) -> Result<Option<Option<ComponentValue>>, ComponentError> {
        self.stream_future_handle(
            handle,
            type_id,
            StreamFutureKind::Future,
            StreamFutureEnd::Readable,
        )?;
        let mut payloads = self.future_payloads.borrow_mut();
        let Some(payload) = payloads.remove(&handle) else {
            if async_ {
                return Ok(None);
            }
            return Err(ComponentError::Trap(format!(
                "future handle {handle} is not ready"
            )));
        };
        match payload.as_slice() {
            [] => Ok(Some(None)),
            [value] => Ok(Some(Some(value.clone()))),
            _ => Err(ComponentError::Trap(
                "future payload contains more than one value".to_owned(),
            )),
        }
    }

    fn stream_future_handle(
        &self,
        handle: u32,
        type_id: TypeId,
        kind: StreamFutureKind,
        end: StreamFutureEnd,
    ) -> Result<StreamFutureHandle, ComponentError> {
        let handles = self.stream_future_handles.borrow();
        let Some(record) = handles.get(&handle).copied() else {
            return Err(ComponentError::Trap(format!(
                "{} handle {handle} is invalid",
                stream_future_kind_name(kind)
            )));
        };
        if record.type_id != type_id || record.kind != kind || record.end != end {
            return Err(ComponentError::Trap(format!(
                "{} handle {handle} has the wrong type or endpoint",
                stream_future_kind_name(kind)
            )));
        }
        Ok(record)
    }

    fn begin_task_return(&self) {
        self.task_returns.borrow_mut().push(None);
    }

    fn set_task_return(&self, values: Vec<ComponentValue>) -> Result<(), ComponentError> {
        let mut task_returns = self.task_returns.borrow_mut();
        let Some(slot) = task_returns.last_mut() else {
            return Err(ComponentError::Trap(
                "task.return called outside an async canonical lift".to_owned(),
            ));
        };
        if slot.is_some() {
            return Err(ComponentError::Trap(
                "task.return called more than once".to_owned(),
            ));
        }
        *slot = Some(values);
        Ok(())
    }

    fn finish_task_return(&self) -> Result<Vec<ComponentValue>, ComponentError> {
        self.task_returns
            .borrow_mut()
            .pop()
            .ok_or_else(|| {
                ComponentError::Runtime("async canonical lift task scope is missing".to_owned())
            })?
            .ok_or_else(|| {
                ComponentError::Runtime(
                    "async canonical lift completed without task.return".to_owned(),
                )
            })
    }
}

fn stream_future_kind_name(kind: StreamFutureKind) -> &'static str {
    match kind {
        StreamFutureKind::Stream => "stream",
        StreamFutureKind::Future => "future",
    }
}

pub(super) fn materialize_inline_core_instance(
    env: &RuntimeEnv,
    exports: &HashMap<String, CoreInstanceInlineExport>,
    store: &Store,
) -> Result<InstanceHandle, ComponentError> {
    let mut registry = Registry::new();
    let mut triplets = Vec::new();
    let mut host_functions = Vec::new();
    let mut async_host_functions = Vec::new();

    for (export_name, export) in exports {
        match export {
            CoreInstanceInlineExport::Func(idx) => match env.resolve_core_func(*idx, store)? {
                RuntimeCoreFunc::Export {
                    instance,
                    export_name: source_name,
                    ..
                } => {
                    let module_name = format!("core-func-{}-{}", export_name, triplets.len());
                    registry.register(module_name.clone(), instance.clone());
                    triplets.push((module_name, source_name, export_name.clone()));
                }
                RuntimeCoreFunc::Host(binding) => {
                    if binding.requires_async_trampoline() {
                        async_host_functions.push((export_name.clone(), binding));
                    } else {
                        host_functions.push((export_name.clone(), binding));
                    }
                }
            },
            CoreInstanceInlineExport::Memory(idx) => {
                let memory = env.resolve_core_memory(*idx, store)?;
                let module_name = format!("core-memory-{}-{}", export_name, triplets.len());
                registry.register(module_name.clone(), memory.instance.clone());
                triplets.push((module_name, memory.export_name, export_name.clone()));
            }
            CoreInstanceInlineExport::Table(idx) => {
                let table = env.resolve_core_table(*idx, store)?;
                let module_name = format!("core-table-{}-{}", export_name, triplets.len());
                registry.register(module_name.clone(), table.instance.clone());
                triplets.push((module_name, table.export_name, export_name.clone()));
            }
            CoreInstanceInlineExport::Global(_)
            | CoreInstanceInlineExport::Type(_)
            | CoreInstanceInlineExport::Instance(_)
            | CoreInstanceInlineExport::Module(_) => {
                return Err(ComponentError::Unsupported(
                    "runtime inline core instances only support func/memory/table exports"
                        .to_owned(),
                ));
            }
        }
    }

    if !host_functions.is_empty() {
        let native = NativeModule {
            functions: host_functions
                .iter()
                .map(|(name, binding)| HostFunctionDefinition {
                    name: Some(name.clone()),
                    signature: binding.signature(),
                    fp: component_host_trampoline,
                })
                .collect(),
        };
        let host_instance =
            match block_on(instantiate_native_module(native, store, &Registry::new())) {
                CoreVMResult::Success(instance) => instance,
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline instantiation",
                    ))
                }
            };
        register_host_bindings(&host_instance, &host_functions, store);
        let module_name = format!("host-inline-{}", triplets.len());
        registry.register(module_name.clone(), host_instance.clone());
        for (name, _) in host_functions {
            triplets.push((module_name.clone(), name.clone(), name));
        }
    }

    if !async_host_functions.is_empty() {
        let native = AsyncNativeModule {
            functions: async_host_functions
                .iter()
                .map(|(name, binding)| AsyncHostFunctionDefinition {
                    name: Some(name.clone()),
                    signature: binding.signature(),
                    fp: component_async_host_trampoline,
                })
                .collect(),
        };
        let host_instance = match block_on(instantiate_native_async_module(
            native,
            store,
            &Registry::new(),
        )) {
            CoreVMResult::Success(instance) => instance,
            other => {
                return Err(vm_result_to_component_error(
                    other,
                    "async host trampoline instantiation",
                ))
            }
        };
        register_host_bindings(&host_instance, &async_host_functions, store);
        let module_name = format!("async-host-inline-{}", triplets.len());
        registry.register(module_name.clone(), host_instance.clone());
        for (name, _) in async_host_functions {
            triplets.push((module_name.clone(), name.clone(), name));
        }
    }

    match aliasing(&registry, &triplets, store) {
        CoreVMResult::Success(instance) => Ok(instance),
        other => Err(vm_result_to_component_error(other, "core aliasing")),
    }
}

impl HostBinding {
    fn requires_async_trampoline(&self) -> bool {
        matches!(self, HostBinding::Lower { options, .. } if options.async_)
    }
}

fn register_host_bindings(
    instance: &InstanceHandle,
    bindings: &[(String, Rc<HostBinding>)],
    store: &Store,
) {
    let instance_id = instance_id(instance, store);
    HOST_BINDINGS.with(|host_bindings| {
        let mut host_bindings = host_bindings.borrow_mut();
        for (funcidx, (_, binding)) in bindings.iter().enumerate() {
            host_bindings.insert((instance_id, funcidx as u32), binding.clone());
        }
    });
}

fn component_host_trampoline(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let key = (ctx.instance_id(), ctx.func().funcidx);
    let binding = HOST_BINDINGS.with(|bindings| bindings.borrow().get(&key).cloned());
    let Some(binding) = binding else {
        return VMResult::Unlinkable;
    };
    let args = match read_core_args_from_locals(ctx, &binding.signature()) {
        Ok(args) => args,
        Err(_) => return VMResult::Unreachable,
    };
    let results = match binding.call_sync(ctx.store, &args) {
        Ok(results) => results,
        Err(_) => return VMResult::Unreachable,
    };
    let slot = ctx.return_slot();
    let offset = write_core_results_to_slot(&slot, &results);
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, offset, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn component_async_host_trampoline(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    let key = (ctx.instance_id(), ctx.func().funcidx);
    let binding = HOST_BINDINGS.with(|bindings| bindings.borrow().get(&key).cloned());
    let Some(binding) = binding else {
        return Box::pin(async { VMResult::Unlinkable });
    };
    let signature = binding.signature();
    let args = match read_core_args_from_locals(ctx, &signature) {
        Ok(args) => args,
        Err(_) => return Box::pin(async { VMResult::Unreachable }),
    };
    let return_size = core_return_size(&signature);
    let slot = ctx.return_slot();
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, return_size, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    let store = ctx.store as *const Store;
    Box::pin(async move {
        let store = unsafe { &*store };
        let results = match binding.call(store, &args).await {
            Ok(results) => results,
            Err(_) => return VMResult::Unreachable,
        };
        if write_core_results_to_slot(&slot, &results) != return_size {
            return VMResult::Unreachable;
        }
        VMResult::Success(return_addr)
    })
}

fn core_return_size(signature: &CoreFuncType) -> usize {
    signature
        .1
        .iter()
        .map(|ty| ty.stack_size().u32() as usize)
        .sum()
}

fn write_core_results_to_slot(slot: &ReturnSlot, results: &[WasmValue]) -> usize {
    let mut offset = 0usize;
    for value in results {
        match value {
            WasmValue::I32(v) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        v.to_le_bytes().as_ptr(),
                        slot.as_mut_ptr().add(offset),
                        4,
                    )
                };
                offset += 4;
            }
            WasmValue::I64(v) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        v.to_le_bytes().as_ptr(),
                        slot.as_mut_ptr().add(offset),
                        8,
                    )
                };
                offset += 8;
            }
            WasmValue::F32(v) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        v.to_le_bytes().as_ptr(),
                        slot.as_mut_ptr().add(offset),
                        4,
                    )
                };
                offset += 4;
            }
            WasmValue::F64(v) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        v.to_le_bytes().as_ptr(),
                        slot.as_mut_ptr().add(offset),
                        8,
                    )
                };
                offset += 8;
            }
            WasmValue::FuncRef(v) | WasmValue::ExternRef(v) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        v.to_le_bytes().as_ptr(),
                        slot.as_mut_ptr().add(offset),
                        4,
                    )
                };
                offset += 4;
            }
            WasmValue::V128(v) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        v.to_le_bytes().as_ptr(),
                        slot.as_mut_ptr().add(offset),
                        16,
                    )
                };
                offset += 16;
            }
        }
    }
    offset
}

fn read_core_args_from_locals(
    ctx: &mut ExecuteContext,
    signature: &CoreFuncType,
) -> Result<Vec<WasmValue>, ComponentError> {
    let mut offset = 0u32;
    let mut args = Vec::with_capacity(signature.0 .0.len());
    for ty in signature.0.iter() {
        match ty {
            CoreValType::I32 | CoreValType::F32 | CoreValType::FuncRef | CoreValType::ExternRef => {
                match ctx
                    .stack
                    .local_get(&ctx.local_reference(), offset as usize, 4)
                {
                    VMResult::Success(()) => {}
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "host trampoline local_get",
                        ))
                    }
                }
            }
            CoreValType::I64 | CoreValType::F64 => {
                match ctx
                    .stack
                    .local_get(&ctx.local_reference(), offset as usize, 8)
                {
                    VMResult::Success(()) => {}
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "host trampoline local_get",
                        ))
                    }
                }
            }
            CoreValType::V128 => {
                match ctx
                    .stack
                    .local_get(&ctx.local_reference(), offset as usize, 16)
                {
                    VMResult::Success(()) => {}
                    other => {
                        return Err(vm_result_to_component_error(
                            other,
                            "host trampoline local_get",
                        ))
                    }
                }
            }
        }
        let value = match ty {
            CoreValType::I32 => WasmValue::I32(ctx.stack.pop_i32()),
            CoreValType::I64 => WasmValue::I64(ctx.stack.pop_i64()),
            CoreValType::F32 => WasmValue::F32(ctx.stack.pop_f32()),
            CoreValType::F64 => WasmValue::F64(ctx.stack.pop_f64()),
            CoreValType::FuncRef => WasmValue::FuncRef(ctx.stack.pop_u32()),
            CoreValType::ExternRef => WasmValue::ExternRef(ctx.stack.pop_u32()),
            CoreValType::V128 => WasmValue::V128(ctx.stack.pop_u128()),
        };
        args.push(value);
        offset += ty.stack_size().u32();
    }
    Ok(args)
}

pub(super) fn linker_binding_to_callable(binding: LinkerBinding) -> ResolvedCallable {
    match binding {
        LinkerBinding::Host(func) => ResolvedCallable::Host(func),
        LinkerBinding::Core(binding) => ResolvedCallable::Core(CoreExportRef {
            instance: binding.instance,
            export_name: binding.export_name,
        }),
    }
}

fn instance_id(instance: &InstanceHandle, store: &Store) -> u32 {
    crate::support::common::instance_id(instance, store)
        .expect("instance handle belongs to another store")
}

fn call_core_export_sync(
    instance: &InstanceHandle,
    store: &Store,
    export_name: &str,
    args: &[WasmValue],
) -> Result<Vec<WasmValue>, ComponentError> {
    let result = crate::support::runtime::run_core_export_sync_reentrant(
        instance,
        store,
        export_name,
        &ResultValue::new(args.to_vec()),
    )
    .map_err(|error| {
        ComponentError::Runtime(format!(
            "core function `{export_name}` cannot suspend during sync execution: {error:?}"
        ))
    })?;
    match result {
        CoreVMResult::Success(values) => Ok(values.iter().copied().collect::<Vec<_>>()),
        other => Err(vm_result_to_component_error(other, export_name)),
    }
}

async fn call_core_export(
    instance: &InstanceHandle,
    store: &Store,
    export_name: &str,
    args: &[WasmValue],
) -> Result<Vec<WasmValue>, ComponentError> {
    match crate::support::runtime::run_module_function(
        instance,
        store,
        export_name,
        &ResultValue::new(args.to_vec()),
    )
    .await
    {
        CoreVMResult::Success(values) => Ok(values.iter().copied().collect::<Vec<_>>()),
        other => Err(vm_result_to_component_error(other, export_name)),
    }
}

fn run_ready_future_sync<F, T>(future: F, context: &str) -> Result<T, ComponentError>
where
    F: Future<Output = T>,
{
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = pin!(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => Ok(output),
        Poll::Pending => Err(ComponentError::Runtime(format!(
            "{context} yielded in sync execution"
        ))),
    }
}

pub(super) fn vm_result_to_component_error(
    result: CoreVMResult<impl Sized>,
    context: &str,
) -> ComponentError {
    match result {
        CoreVMResult::Success(_) => unreachable!(),
        CoreVMResult::Unreachable => {
            ComponentError::Trap(format!("{context} trapped: unreachable"))
        }
        CoreVMResult::StackOverflow => {
            ComponentError::Trap(format!("{context} trapped: stack overflow"))
        }
        CoreVMResult::MemoryIndexOutOfRange => {
            ComponentError::Trap(format!("{context} trapped: memory index out of range"))
        }
        CoreVMResult::TableIndexOutOfRange => {
            ComponentError::Trap(format!("{context} trapped: table index out of range"))
        }
        CoreVMResult::CallIndirectInvalidType => {
            ComponentError::Trap(format!("{context} trapped: call indirect invalid type"))
        }
        CoreVMResult::TableUninitialized => {
            ComponentError::Trap(format!("{context} trapped: table uninitialized"))
        }
        CoreVMResult::Unlinkable => ComponentError::Link(format!("{context} failed: unlinkable")),
        CoreVMResult::InvalidOperand => {
            ComponentError::Runtime(format!("{context} failed: invalid operand"))
        }
        CoreVMResult::UnalignedAtomic => {
            ComponentError::Trap(format!("{context} trapped: unaligned atomic"))
        }
        CoreVMResult::Unimplemented => {
            ComponentError::Runtime(format!("{context} failed: unimplemented"))
        }
    }
}
