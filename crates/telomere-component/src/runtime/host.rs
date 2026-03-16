use super::canonical::{
    component_value_to_direct_wasm, direct_wasm_to_component_value, lift_component_args,
    lift_component_results, lower_component_args, lower_component_results,
    lowered_indirect_result_area,
};
use super::*;

impl ResolvedCallable {
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

impl RuntimeCoreFunc {
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
            | HostBinding::ResourceRep { signature, .. } => signature.clone(),
        }
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
                let result_area = lowered_indirect_result_area(func_type, args, program)?;
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
        }
    }
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
}

pub(super) fn materialize_inline_core_instance(
    env: &RuntimeEnv,
    exports: &HashMap<String, CoreInstanceInlineExport>,
    store: &Store,
) -> Result<InstanceHandle, ComponentError> {
    let mut registry = Registry::new();
    let mut triplets = Vec::new();
    let mut host_functions = Vec::new();

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
                    host_functions.push((export_name.clone(), binding))
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

    match aliasing(&registry, &triplets, store) {
        CoreVMResult::Success(instance) => Ok(instance),
        other => Err(vm_result_to_component_error(other, "core aliasing")),
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
    let results =
        match with_active_component_host_gc(ctx.gc, || binding.call_sync(ctx.store, &args)) {
            Ok(results) => results,
            Err(_) => return VMResult::Unreachable,
        };
    if push_core_results(ctx, &results).is_err() {
        return VMResult::Unreachable;
    }
    let return_size = binding
        .signature()
        .1
        .iter()
        .map(|ty| ty.stack_size().u32())
        .sum::<u32>() as usize;
    let (prev_local_ref, return_addr) =
        ctx.stack.function_return(&ctx.local_reference, return_size);
    ctx.local_reference = prev_local_ref;
    VMResult::Success(return_addr)
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

fn push_core_results(
    ctx: &mut ExecuteContext,
    results: &[WasmValue],
) -> Result<(), ComponentError> {
    for value in results {
        match value {
            WasmValue::I32(v) => match ctx.stack.push_i32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::I64(v) => match ctx.stack.push_i64(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::F32(v) => match ctx.stack.push_f32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::F64(v) => match ctx.stack.push_f64(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::FuncRef(v) => match ctx.stack.push_u32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::ExternRef(v) => match ctx.stack.push_u32(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
            WasmValue::V128(v) => match ctx.stack.push_u128(*v) {
                VMResult::Success(()) => {}
                other => {
                    return Err(vm_result_to_component_error(
                        other,
                        "host trampoline push result",
                    ))
                }
            },
        }
    }
    Ok(())
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
    let gc = store.lock_gc();
    crate::support::common::instance_id(instance, store, &gc)
        .expect("instance handle belongs to another store")
}

fn call_core_export_sync(
    instance: &InstanceHandle,
    store: &Store,
    export_name: &str,
    args: &[WasmValue],
) -> Result<Vec<WasmValue>, ComponentError> {
    let result = with_active_component_host_gc_ptr(|gc| match gc {
        Some(gc) => crate::support::runtime::run_module_function_sync_with_gc(
            instance,
            store,
            gc,
            export_name,
            &ResultValue::new(args.to_vec()),
        )
        .map_err(|error| {
            ComponentError::Runtime(format!(
                "core function `{export_name}` cannot suspend during sync execution: {error:?}"
            ))
        }),
        None => Ok(block_on(run_module_function(
            instance,
            store,
            export_name,
            &ResultValue::new(args.to_vec()),
        ))),
    })?;
    match result {
        CoreVMResult::Success(values) => Ok(values.iter().copied().collect()),
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

fn with_active_component_host_gc<T>(
    gc: &mut crate::support::common::gc::MemoryPool,
    f: impl FnOnce() -> T,
) -> T {
    struct ResetGuard<'a> {
        slot: &'a Cell<*mut crate::support::common::gc::MemoryPool>,
        previous: *mut crate::support::common::gc::MemoryPool,
    }

    impl Drop for ResetGuard<'_> {
        fn drop(&mut self) {
            self.slot.set(self.previous);
        }
    }

    ACTIVE_COMPONENT_HOST_GC.with(|slot| {
        let previous = slot.replace(gc as *mut _);
        let _guard = ResetGuard { slot, previous };
        f()
    })
}

pub(super) fn with_active_component_host_gc_ptr<T>(
    f: impl FnOnce(Option<&mut crate::support::common::gc::MemoryPool>) -> T,
) -> T {
    ACTIVE_COMPONENT_HOST_GC.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            f(None)
        } else {
            // SAFETY: the pointer is set only while the current thread is executing a
            // synchronous host trampoline, so nested canonical ABI calls can reuse the same
            // mutable GC borrow before control returns to the outer VM frame.
            f(Some(unsafe { &mut *ptr }))
        }
    })
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
    }
}
