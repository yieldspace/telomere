use super::canonical::{
    collect_borrowed_handles, component_value_to_direct_wasm, direct_wasm_to_component_value,
    lift_component_args, lift_component_results, lower_component_args, lower_component_results,
    lowered_indirect_result_area, resource_is_local_definition, retire_lent_handles,
    track_host_owned_results, transfer_handles, HandleTransfer,
};
use super::*;

impl ResolvedCallable {
    pub(super) fn call_sync(
        &self,
        store: &Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_sync_impl(store, args, true)
    }

    /// Calls with the argument borrows already registered against the
    /// callee's handle table by the caller. The canon-lower boundary
    /// path registers them itself: after the handle transfer it knows
    /// each borrow's provenance — the rep when the borrow crossed into
    /// the defining instance, a borrowed registration otherwise — which
    /// the transferred values alone no longer distinguish from a plain
    /// handle index.
    pub(super) fn call_sync_borrows_preregistered(
        &self,
        store: &Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.call_sync_impl(store, args, false)
    }

    fn call_sync_impl(
        &self,
        store: &Store,
        args: &[ComponentValue],
        register_borrows: bool,
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
                // Register the borrows lent to this call against this
                // instance's handle table for the duration of the call:
                // dropping the corresponding own handle while a borrow
                // is outstanding traps (#334). Resolving the argument
                // values against this table assumes they name handles
                // in it, which holds when no call boundary rewrote
                // them — direct calls through the public API and
                // same-instance calls; the canon-lower boundary path
                // registers the borrows itself, with the transfer's
                // provenance, and enters through
                // `call_sync_borrows_preregistered` instead. A borrow
                // that resolves to no owned entry here is not
                // registered. The table borrow is never held across the
                // call itself, since the called code re-enters this
                // instance's canon resource ops.
                let mut registered: Vec<(ResourceId, u32)> = Vec::new();
                if register_borrows {
                    let mut table = handles.borrow_mut();
                    for (resource, handle) in
                        collect_borrowed_handles(func_type, args, program, shared)
                    {
                        if let Some(entry) = table.note_borrow(resource, handle) {
                            registered.push((resource, entry));
                        }
                    }
                }
                let outcome = (|| {
                    let core_args = lower_component_args(func_type, args, options, program, store)?;
                    let core_results = core.call_sync(store, &core_args)?;
                    let lifted =
                        lift_component_results(func_type, &core_results, options, program, store)?;
                    if let Some(post_return) = &options.post_return {
                        post_return.call_sync(store, &core_results)?;
                    }
                    Ok(lifted)
                })();
                {
                    let mut table = handles.borrow_mut();
                    for (resource, handle) in &registered {
                        table.release_borrow(*resource, *handle);
                    }
                }
                outcome
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
                let mut component_args =
                    lift_component_args(func_type, args, options, program, store)?;
                // When the callee is another component instance's lifted
                // function, the call crosses a handle-table boundary:
                // handles in the argument values are indices into this
                // instance's table and must be transferred into the
                // callee's, and result handles transferred back (#342).
                // Host and raw-core callees own no table; their handles
                // keep the pass-through treatment (host-provided handles
                // are tracked in `SharedState` instead). A callee in the
                // same instance shares this table and needs no transfer.
                let boundary = match callable.as_ref() {
                    ResolvedCallable::Lifted {
                        func_type: callee_type,
                        handles: callee_handles,
                        ..
                    } if !Rc::ptr_eq(handles, callee_handles) => {
                        Some((callee_type, callee_handles))
                    }
                    _ => None,
                };
                let mut lent: Vec<u32> = Vec::new();
                // Borrows this call lends to the callee, registered
                // against the callee's table for the call's duration
                // and released when it returns (#334).
                let mut registered: Vec<(ResourceId, u32)> = Vec::new();
                if let Some((callee_type, callee_handles)) = boundary {
                    let mut transfer = HandleTransfer {
                        program,
                        shared,
                        src: handles,
                        dst: callee_handles,
                        lent: Some(&mut lent),
                        src_reps: false,
                    };
                    if let Err(error) = transfer_handles(
                        &mut component_args,
                        &func_type.params,
                        &callee_type.params,
                        &mut transfer,
                    ) {
                        retire_lent_handles(callee_handles, &lent);
                        return Err(error);
                    }
                    // Register the borrows lent to the callee against
                    // its table. The transfer has just rewritten every
                    // borrow argument, and its provenance decides how
                    // the value resolves: a borrow of a resource the
                    // callee defines crossed as the rep and resolves
                    // by rep only — the same integer may also be an
                    // unrelated live handle index in the callee's
                    // table — while any other borrow crossed as a
                    // borrowed registration in the callee's table and
                    // resolves through that registration.
                    {
                        let mut table = callee_handles.borrow_mut();
                        for (resource, handle) in
                            collect_borrowed_handles(callee_type, &component_args, program, shared)
                        {
                            let entry = if resource_is_local_definition(resource) {
                                table.note_borrow_rep(resource, handle as i32)
                            } else {
                                table.note_borrow(resource, handle)
                            };
                            if let Some(entry) = entry {
                                registered.push((resource, entry));
                            }
                        }
                    }
                }
                let call_outcome = if boundary.is_some() {
                    callable.call_sync_borrows_preregistered(store, &component_args)
                } else {
                    callable.call_sync(store, &component_args)
                };
                if let Some((_, callee_handles)) = boundary {
                    let mut table = callee_handles.borrow_mut();
                    for (resource, handle) in &registered {
                        table.release_borrow(*resource, *handle);
                    }
                }
                let mut results = match call_outcome {
                    Ok(results) => results,
                    Err(error) => {
                        if let Some((_, callee_handles)) = boundary {
                            retire_lent_handles(callee_handles, &lent);
                        }
                        return Err(error);
                    }
                };
                if let Some((callee_type, callee_handles)) = boundary {
                    if let (Some(result_ty), Some(callee_result_ty)) =
                        (&func_type.result, &callee_type.result)
                    {
                        if let Some(result) = results.first_mut() {
                            let mut transfer = HandleTransfer {
                                program,
                                shared,
                                src: callee_handles,
                                dst: handles,
                                lent: None,
                                src_reps: true,
                            };
                            if let Err(error) = transfer_handles(
                                std::slice::from_mut(result),
                                std::slice::from_ref(callee_result_ty),
                                std::slice::from_ref(result_ty),
                                &mut transfer,
                            ) {
                                retire_lent_handles(callee_handles, &lent);
                                return Err(error);
                            }
                        }
                    }
                    retire_lent_handles(callee_handles, &lent);
                }
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
        self.insert_entry(resource, ResourceRecord { rep, destructor }, false)
    }

    /// Inserts an entry under a fresh or LIFO-reused index and returns
    /// the index. Used by `resource.new` and by the handle transfers at
    /// component-call boundaries (#342).
    pub(super) fn insert_entry(
        &mut self,
        resource: ResourceId,
        record: ResourceRecord,
        borrowed: bool,
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
                record,
                borrows: 0,
                borrowed,
            },
        );
        handle
    }

    /// Removes the entry for `handle`, freeing its index for LIFO
    /// reuse, and returns its record; `None` when this instance's table
    /// holds no such handle. Ownership transfers at a call boundary use
    /// this to move an entry between tables without destroying the
    /// resource (#342).
    pub(super) fn take_entry(&mut self, handle: u32) -> Option<ResourceRecord> {
        let entry = self.entries.remove(&handle)?;
        self.free_list.push(handle);
        Some(entry.record)
    }

    /// Whether `handle` names an owned entry of this table with
    /// borrows currently lent to a call in flight (#334). Ownership
    /// transfers at a call boundary consult this before taking the
    /// entry: a transfer removes the entry from this table just as a
    /// drop does, so a borrowed own cannot be transferred either.
    pub(super) fn has_outstanding_borrows(&self, handle: u32) -> bool {
        matches!(
            self.entries.get(&handle),
            Some(entry) if !entry.borrowed && entry.borrows > 0
        )
    }

    /// Returns a copy of the record for `handle`, or `None` when this
    /// instance's table holds no such handle.
    pub(super) fn record_of(&self, handle: u32) -> Option<ResourceRecord> {
        self.entries.get(&handle).map(|entry| entry.record.clone())
    }

    /// Removes the entry for `handle` if present, freeing its index.
    /// Used to retire borrow entries registered for a single call
    /// (#342); a borrow the callee already dropped is simply absent.
    pub(super) fn remove_entry(&mut self, handle: u32) {
        if self.entries.remove(&handle).is_some() {
            self.free_list.push(handle);
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
        let invalid = || ComponentError::Trap(format!("resource handle {handle} is invalid"));
        match self.entries.get(&handle) {
            Some(entry) if entry.resource == resource => {
                // The canonical ABI forbids removing an owned resource
                // while a borrow of it is still lent to a call in
                // flight (#334). A borrowed entry only registers
                // another instance's resource (#342) and is not owned,
                // so the check does not apply to it.
                if !entry.borrowed && entry.borrows > 0 {
                    return Err(ComponentError::Trap(
                        "cannot remove owned resource while borrowed".to_owned(),
                    ));
                }
            }
            _ => return Err(invalid()),
        }
        let entry = self.entries.remove(&handle).ok_or_else(invalid)?;
        self.free_list.push(handle);
        // A borrowed entry only registers another instance's resource
        // for the duration of a call: dropping it removes the
        // registration and never runs the destructor (#342).
        let destructor = if entry.borrowed {
            None
        } else {
            entry.record.destructor
        };
        Ok((entry.record.rep, destructor))
    }

    /// Registers one borrow lent to a call in flight and returns the
    /// index of the owned entry it was registered against, if any.
    ///
    /// The `handle` a borrow argument carries names the borrowed
    /// resource in one of three ways, depending on how the value
    /// reached this instance: directly, when it is an index of an
    /// owned entry in this table (the same-instance path); through a
    /// borrowed registration (#342), when a call boundary lent the
    /// resource into this table and the argument names that
    /// registration, whose record carries the rep; or as the rep
    /// itself, when the borrowed resource is defined by this instance
    /// and a boundary crossing passed the rep in place of a handle.
    /// In the latter two shapes the borrow is registered against the
    /// live owned entry holding that rep, since that is the entry a
    /// `resource.drop` during the call would remove. A borrow that
    /// resolves to no owned entry in this table belongs to another
    /// instance's table (or to the host) and is not registered here;
    /// it cannot be dropped through this table either.
    fn note_borrow(&mut self, resource: ResourceId, handle: u32) -> Option<u32> {
        let rep = match self.entries.get(&handle) {
            Some(entry) if entry.resource == resource && !entry.borrowed => {
                let entry = self.entries.get_mut(&handle)?;
                entry.borrows = entry.borrows.saturating_add(1);
                return Some(handle);
            }
            Some(entry) if entry.resource == resource => entry.record.rep,
            Some(_) => return None,
            None => handle as i32,
        };
        self.note_borrow_on_rep(resource, rep)
    }

    /// Registers one borrow that crossed a call boundary into the
    /// instance defining its resource, where the argument carries the
    /// rep rather than a handle index (#342). The borrow registers
    /// against the owned entry holding that rep; the integer is never
    /// interpreted as a handle index, since the same value may also be
    /// a live index of an unrelated entry in this table.
    fn note_borrow_rep(&mut self, resource: ResourceId, rep: i32) -> Option<u32> {
        self.note_borrow_on_rep(resource, rep)
    }

    /// Registers one borrow against the live owned entry holding
    /// `rep` and returns that entry's index, if such an entry exists.
    fn note_borrow_on_rep(&mut self, resource: ResourceId, rep: i32) -> Option<u32> {
        let target = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.resource == resource && !entry.borrowed && entry.record.rep == rep
            })
            .map(|(index, _)| *index)
            .min()?;
        let entry = self.entries.get_mut(&target)?;
        entry.borrows = entry.borrows.saturating_add(1);
        Some(target)
    }

    /// Releases one borrow previously registered with
    /// [`HandleTable::note_borrow`]. The entry may already be gone if
    /// the resource was dropped through a path that does not consult
    /// this table; the count simply has nothing to release then.
    fn release_borrow(&mut self, resource: ResourceId, handle: u32) {
        if let Some(entry) = self.entries.get_mut(&handle) {
            if entry.resource == resource {
                entry.borrows = entry.borrows.saturating_sub(1);
            }
        }
    }

    fn resource_rep(&self, resource: ResourceId, handle: u32) -> Result<i32, ComponentError> {
        self.entries
            .get(&handle)
            .filter(|entry| entry.resource == resource)
            .map(|entry| entry.record.rep)
            .ok_or_else(|| ComponentError::Trap(format!("resource handle {handle} is invalid")))
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
    fn dropping_a_borrowed_resource_is_rejected_with_the_canonical_message() {
        // #334: `resource.drop` of an owned resource that is currently
        // borrowed traps with the canonical message; once the borrow is
        // released, the same drop succeeds.
        let resource = ResourceId::synthetic();
        let mut table = HandleTable::default();
        let handle = table.alloc_resource(resource, 7, None);
        assert_eq!(table.note_borrow(resource, handle), Some(handle));
        let error = match table.drop_resource(resource, handle) {
            Ok(_) => panic!("dropping a borrowed resource must be rejected"),
            Err(error) => error,
        };
        assert!(
            matches!(&error, ComponentError::Trap(message) if message == "cannot remove owned resource while borrowed"),
            "expected the canonical borrowed-drop trap, got {error:?}"
        );
        table.release_borrow(resource, handle);
        let (rep, _) = table
            .drop_resource(resource, handle)
            .expect("dropping after the borrow is released should succeed");
        assert_eq!(rep, 7);
    }

    #[test]
    fn borrow_reaching_the_defining_instance_registers_by_rep_not_by_index() {
        // The rep a boundary-crossed borrow carries may equal the
        // index of an unrelated owned entry in the defining table:
        // registration by rep must protect the entry holding the rep
        // and leave the entry at that index alone.
        let resource = ResourceId::synthetic();
        let mut table = HandleTable::default();
        let decoy = table.alloc_resource(resource, 100, None);
        let lent = table.alloc_resource(resource, 1, None);
        assert_eq!((decoy, lent), (1, 2));
        assert_eq!(table.note_borrow_rep(resource, 1), Some(lent));
        // The borrowed entry cannot be dropped while borrowed...
        assert!(table.drop_resource(resource, lent).is_err());
        // ...while the unrelated entry at index 1 drops fine.
        let (rep, _) = table
            .drop_resource(resource, decoy)
            .expect("the decoy entry at the rep's index is not borrowed");
        assert_eq!(rep, 100);
        table.release_borrow(resource, lent);
        assert!(table.drop_resource(resource, lent).is_ok());
        // A rep no live entry holds registers nothing.
        assert_eq!(table.note_borrow_rep(resource, 555), None);
    }

    #[test]
    fn ownership_transfer_check_tracks_outstanding_borrows() {
        let resource = ResourceId::synthetic();
        let mut table = HandleTable::default();
        let handle = table.alloc_resource(resource, 7, None);
        assert!(!table.has_outstanding_borrows(handle));
        assert_eq!(table.note_borrow(resource, handle), Some(handle));
        assert!(table.has_outstanding_borrows(handle));
        table.release_borrow(resource, handle);
        assert!(!table.has_outstanding_borrows(handle));
        assert!(!table.has_outstanding_borrows(999));
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
