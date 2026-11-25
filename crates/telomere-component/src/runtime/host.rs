use super::canonical::{
    component_value_to_direct_wasm, direct_wasm_to_component_value, lift_component_args,
    lift_component_results, lower_component_args, lower_component_results,
    lowered_indirect_result_area, track_host_owned_results, validate_lifted_handle_results,
    validate_lowered_borrow_args,
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
                shared,
                handles,
            } => {
                let core_args = lower_component_args(func_type, args, options, program, store)?;
                let core_results = core.call_sync(store, &core_args)?;
                let lifted =
                    lift_component_results(func_type, &core_results, options, program, store)?;
                validate_lifted_handle_results(func_type, &lifted, program, shared, handles)?;
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
                shared,
                handles,
                ..
            } => {
                let result_area = lowered_indirect_result_area(func_type, args, program)?;
                let component_args = lift_component_args(func_type, args, options, program, store)?;
                validate_lowered_borrow_args(func_type, &component_args, program, shared, handles)?;
                let results = callable.call_sync(store, &component_args)?;
                track_host_owned_results(func_type, &results, program, shared);
                lower_component_results(func_type, &results, options, program, store, result_area)
            }
            HostBinding::ResourceNew {
                resource,
                destructor,
                handles,
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
                let handle =
                    handles
                        .borrow_mut()
                        .alloc_resource(*resource, rep, destructor.clone());
                Ok(vec![WasmValue::I32(handle as i32)])
            }
            HostBinding::ResourceDrop {
                resource,
                shared,
                handles,
                ..
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
                // Take the entry out of the table first and let the
                // borrow end here: the Ok arm invokes the resource's
                // destructor and the Err arm runs a host-registered
                // drop, and either may re-enter this instance's canon
                // resource ops, which borrow the table again.
                let dropped = handles.borrow_mut().drop_resource(*resource, handle);
                match dropped {
                    Ok((rep, destructor)) => {
                        shared.forget_host_resource(handle);
                        if let Some(dtor) = destructor {
                            dtor.call_sync(store, &[WasmValue::I32(rep)])?;
                        }
                        Ok(Vec::new())
                    }
                    Err(error) => {
                        // The index names a live entry of this instance's
                        // own table under a different resource type:
                        // `drop_resource`'s wrong-type trap stands, and the
                        // host-provided path below (for indices this table
                        // does not hold at all) must not run.
                        if handles.borrow().contains_handle(handle) {
                            return Err(error);
                        }
                        // Not a locally created resource. If this is a
                        // tracked host-provided handle with a registered
                        // destructor, release it through the host; a double
                        // drop or a handle that was never issued is not
                        // tracked and traps with the same error as a local
                        // one.
                        if shared.drop_host_resource(store, *resource, handle)? {
                            Ok(Vec::new())
                        } else {
                            Err(error)
                        }
                    }
                }
            }
            HostBinding::ResourceRep {
                resource, handles, ..
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
                    handles.borrow().resource_rep(*resource, handle)?,
                )])
            }
        }
    }
}

impl HandleTable {
    /// Allocates a handle in this instance's table: the most recently
    /// freed index is reused first (LIFO); otherwise the next fresh
    /// index in sequence from 1 is handed out.
    fn alloc_resource(
        &mut self,
        resource: ResourceId,
        rep: i32,
        destructor: Option<RuntimeCoreFunc>,
    ) -> u32 {
        let handle = match self.free_list.pop() {
            Some(handle) => handle,
            None => {
                let handle = self.next_index;
                self.next_index = self.next_index.wrapping_add(1);
                handle
            }
        };
        self.entries.insert(
            handle,
            HandleEntry {
                resource,
                record: ResourceRecord { rep, destructor },
            },
        );
        handle
    }

    /// Whether `handle` names a live entry of this table (of any
    /// resource type).
    pub(super) fn contains_handle(&self, handle: u32) -> bool {
        self.entries.contains_key(&handle)
    }

    /// Validates a handle index against this table with the canonical
    /// ABI's trap texts (#335): an index the table does not hold (never
    /// allocated, already dropped, zero, or out of range) traps
    /// `unknown handle index N`, and an index held for a different
    /// resource type traps the canonical wrong-type text. Table entries
    /// are all created by a component's `resource.new`, so the found
    /// side of the wrong-type text is always a guest-defined resource.
    pub(super) fn validate_handle(
        &self,
        resource: ResourceId,
        handle: u32,
    ) -> Result<(), ComponentError> {
        match self.entries.get(&handle) {
            Some(entry) if entry.resource == resource => Ok(()),
            Some(_) => Err(ComponentError::Trap(format!(
                "handle index {handle} used with the wrong type, expected \
                 guest-defined resource but found a different guest-defined resource"
            ))),
            None => Err(ComponentError::Trap(format!(
                "unknown handle index {handle}"
            ))),
        }
    }

    /// Removes a handle from this instance's table and frees its index
    /// for LIFO reuse. A handle that this instance never allocated, that
    /// was already dropped, or that belongs to a different resource type
    /// is invalid here.
    fn drop_resource(
        &mut self,
        resource: ResourceId,
        handle: u32,
    ) -> Result<(i32, Option<RuntimeCoreFunc>), ComponentError> {
        self.validate_handle(resource, handle)?;
        let entry = self
            .entries
            .remove(&handle)
            .expect("handle validated against this table above");
        self.free_list.push(handle);
        Ok((entry.record.rep, entry.record.destructor))
    }

    fn resource_rep(&self, resource: ResourceId, handle: u32) -> Result<i32, ComponentError> {
        self.validate_handle(resource, handle)?;
        Ok(self.entries[&handle].record.rep)
    }
}

impl SharedState {
    /// Records a handle that has entered guest code through a lowered call,
    /// under the resource type the producing function declared.
    ///
    /// The declared type is not necessarily the identity a later
    /// `canon resource.drop` names: when one imported interface re-exports
    /// another interface's resource (for example `wasi:cli/stdout`
    /// re-exporting `wasi:io/streams`' `output-stream`), the decoder assigns
    /// each import declaration its own `ResourceId` for the same underlying
    /// resource. Drop therefore looks handles up across all tracked types
    /// (see `drop_host_resource`).
    pub(super) fn track_host_resource_handle(&self, resource: ResourceId, handle: u32) {
        self.host_resources
            .borrow_mut()
            .entry(resource)
            .or_default()
            .insert(handle);
    }

    fn host_resource_drop(&self, resource: ResourceId) -> Option<ResourceDropFn> {
        self.host_resource_drops.borrow().get(&resource).cloned()
    }

    /// Whether `handle` is a live host-provided handle under any tracked
    /// resource type. The declared type a handle was tracked under is not
    /// necessarily the identity a later canon op names (see
    /// `track_host_resource_handle`), so this lookup is by index alone.
    pub(super) fn tracks_host_resource(&self, handle: u32) -> bool {
        self.host_resources
            .borrow()
            .values()
            .any(|handles| handles.contains(&handle))
    }

    /// Removes a live host-provided handle, returning whether it was live.
    fn take_host_resource(&self, resource: ResourceId, handle: u32) -> bool {
        self.host_resources
            .borrow_mut()
            .get_mut(&resource)
            .is_some_and(|handles| handles.remove(&handle))
    }

    /// Forgets every tracked host entry for `handle` (used when the handle
    /// turned out to name a locally created resource instead).
    fn forget_host_resource(&self, handle: u32) {
        for handles in self.host_resources.borrow_mut().values_mut() {
            handles.remove(&handle);
        }
    }

    /// Releases a host-provided handle through a registered destructor.
    ///
    /// Returns `Ok(false)` when the handle is not tracked, or when neither
    /// the type named by the drop nor the type the handle was tracked under
    /// has a registered destructor; the caller then reports the handle as
    /// invalid, exactly as for a locally created resource. The tracked
    /// entry is consumed only when a destructor actually runs, so a double
    /// drop traps instead of releasing twice.
    fn drop_host_resource(
        &self,
        store: &Store,
        resource: ResourceId,
        handle: u32,
    ) -> Result<bool, ComponentError> {
        let tracked = {
            let live = self.host_resources.borrow();
            if live.get(&resource).is_some_and(|h| h.contains(&handle)) {
                Some(resource)
            } else {
                live.iter()
                    .find(|(_, handles)| handles.contains(&handle))
                    .map(|(tracked, _)| *tracked)
            }
        };
        let Some(tracked) = tracked else {
            return Ok(false);
        };
        let Some(drop_fn) = self
            .host_resource_drop(resource)
            .or_else(|| self.host_resource_drop(tracked))
        else {
            return Ok(false);
        };
        self.take_host_resource(tracked, handle);
        let released = run_ready_future_sync(drop_fn(store, handle), "host resource drop")?;
        released?;
        Ok(true)
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
        let native = HostModule {
            functions: host_functions
                .iter()
                .map(|(name, binding)| HostFnDefinition {
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

fn component_host_trampoline(cx: &mut HostContext) -> VMResult<Vec<WasmValue>> {
    let key = (cx.instance_id(), cx.func_index());
    let binding = HOST_BINDINGS.with(|bindings| bindings.borrow().get(&key).cloned());
    let Some(binding) = binding else {
        return VMResult::Unlinkable;
    };
    // The public host boundary has already decoded the arguments from the
    // frame's local area against the function's declared signature (#216);
    // the component layer only forwards them to the bound host callable.
    let args = cx.args().to_vec();
    match binding.call_sync(cx.store(), &args) {
        Ok(results) => VMResult::Success(results),
        // The core host boundary cannot carry a `ComponentError`, so every
        // failure of the bound callable -- including a host future that
        // yields under the first-poll contract -- surfaces to the guest as
        // an `unreachable` trap. That is the documented behaviour for the
        // canon-lower path; component-level calls return the error itself.
        // To keep that last part true when the failure is a trap (a
        // canonical ABI trap, or a guest trap from a nested call), the
        // error is also kept in `PENDING_HOST_TRAP`, and
        // `vm_result_to_component_error` restores it when this execution's
        // result is converted: the guest still observes only `unreachable`,
        // but the component-level caller sees the original trap cause
        // instead of a bare "trapped: unreachable". Non-trap failures are
        // not kept; they keep surfacing as the plain `unreachable` trap.
        Err(error) => {
            if matches!(
                error,
                ComponentError::Trap(_) | ComponentError::GuestTrap { .. }
            ) {
                PENDING_HOST_TRAP.with(|pending| *pending.borrow_mut() = Some(error));
            }
            VMResult::Unreachable
        }
    }
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

/// The single enforcement point of the first-poll contract documented on
/// [`crate::ComponentFuture`]: a host future gets exactly one poll with a
/// no-op waker, and `Poll::Pending` is an error, never a suspension. Every
/// host-function and resource-destructor invocation in the component
/// runtime goes through here, so this is the only place the contract is
/// implemented.
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
            "{context} yielded in sync execution: component host functions must \
             complete on their first poll (the component runtime is synchronous \
             and never drives host futures past one poll); return a ready future \
             or register the synchronous form instead"
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
            // A trap a host binding raised while servicing this execution
            // was kept by `component_host_trampoline`; restore it so the
            // trap cause survives the core boundary. Every core execution's
            // non-success result is converted exactly once, through this
            // function, so a kept trap cannot outlive its own execution.
            if let Some(error) = PENDING_HOST_TRAP.with(|pending| pending.borrow_mut().take()) {
                return error;
            }
            ComponentError::GuestTrap {
                kind: telomere::TrapKind::Unreachable,
                message: format!("{context} trapped: unreachable"),
            }
        }
        CoreVMResult::StackOverflow => ComponentError::GuestTrap {
            kind: telomere::TrapKind::StackOverflow,
            message: format!("{context} trapped: stack overflow"),
        },
        CoreVMResult::MemoryIndexOutOfRange => ComponentError::GuestTrap {
            kind: telomere::TrapKind::MemoryIndexOutOfRange,
            message: format!("{context} trapped: memory index out of range"),
        },
        CoreVMResult::TableIndexOutOfRange => ComponentError::GuestTrap {
            kind: telomere::TrapKind::TableIndexOutOfRange,
            message: format!("{context} trapped: table index out of range"),
        },
        CoreVMResult::CallIndirectInvalidType => ComponentError::GuestTrap {
            kind: telomere::TrapKind::CallIndirectInvalidType,
            message: format!("{context} trapped: call indirect invalid type"),
        },
        CoreVMResult::TableUninitialized => ComponentError::GuestTrap {
            kind: telomere::TrapKind::TableUninitialized,
            message: format!("{context} trapped: table uninitialized"),
        },
        CoreVMResult::Unlinkable => ComponentError::Link(format!("{context} failed: unlinkable")),
        CoreVMResult::MemoryAllocationFailed => {
            ComponentError::Runtime(format!("{context} failed: memory allocation failed"))
        }
        CoreVMResult::InvalidOperand => {
            ComponentError::Runtime(format!("{context} failed: invalid operand"))
        }
        CoreVMResult::UnalignedAtomic => ComponentError::GuestTrap {
            kind: telomere::TrapKind::UnalignedAtomic,
            message: format!("{context} trapped: unaligned atomic"),
        },
        CoreVMResult::DivideByZero => ComponentError::GuestTrap {
            kind: telomere::TrapKind::DivideByZero,
            message: format!("{context} trapped: integer divide by zero"),
        },
        CoreVMResult::IntegerOverflow => ComponentError::GuestTrap {
            kind: telomere::TrapKind::IntegerOverflow,
            message: format!("{context} trapped: integer overflow"),
        },
        CoreVMResult::InvalidConversion => ComponentError::GuestTrap {
            kind: telomere::TrapKind::InvalidConversion,
            message: format!("{context} trapped: invalid conversion to integer"),
        },
        CoreVMResult::Unimplemented => {
            ComponentError::Runtime(format!("{context} failed: unimplemented"))
        }
        CoreVMResult::FuelExhausted => ComponentError::GuestTrap {
            kind: telomere::TrapKind::FuelExhausted,
            message: format!("{context} interrupted: fuel exhausted"),
        },
        CoreVMResult::Cancelled => ComponentError::GuestTrap {
            kind: telomere::TrapKind::Cancelled,
            message: format!("{context} interrupted: cancelled"),
        },
        CoreVMResult::ResourceLimitExceeded => ComponentError::GuestTrap {
            kind: telomere::TrapKind::ResourceLimitExceeded,
            message: format!("{context} failed: resource limit exceeded"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_trap_conversion_preserves_the_core_trap_kind() {
        // The component layer must not flatten a guest trap to a bare
        // string: callers match on the preserved TrapKind (#143).
        let cases = [
            (
                CoreVMResult::<()>::DivideByZero,
                telomere::TrapKind::DivideByZero,
                "integer divide by zero",
            ),
            (
                CoreVMResult::<()>::IntegerOverflow,
                telomere::TrapKind::IntegerOverflow,
                "integer overflow",
            ),
            (
                CoreVMResult::<()>::InvalidConversion,
                telomere::TrapKind::InvalidConversion,
                "invalid conversion to integer",
            ),
            (
                CoreVMResult::<()>::Unreachable,
                telomere::TrapKind::Unreachable,
                "unreachable",
            ),
        ];
        for (result, expected_kind, expected_message) in cases {
            let error = vm_result_to_component_error(result, "export run");
            let ComponentError::GuestTrap { kind, message } = error else {
                panic!("guest traps must convert to GuestTrap, got {error:?}");
            };
            assert_eq!(kind, expected_kind);
            assert_eq!(message, format!("export run trapped: {expected_message}"));
        }
    }

    #[test]
    fn non_trap_failures_keep_their_existing_component_categories() {
        assert!(matches!(
            vm_result_to_component_error(CoreVMResult::<()>::Unlinkable, "ctx"),
            ComponentError::Link(_)
        ));
        assert!(matches!(
            vm_result_to_component_error(CoreVMResult::<()>::InvalidOperand, "ctx"),
            ComponentError::Runtime(_)
        ));
    }
}
