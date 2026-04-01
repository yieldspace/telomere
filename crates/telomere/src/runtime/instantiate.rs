use crate::{
    common::{
        execute_elem_init_const_expr, store::FunctionBody as RuntimeFunctionBody,
        AsyncHostFunction, AsyncHostFunctionDefinition, AsyncNativeModule, CallFrameCache,
        CodeSection, ConstExpr, DataMode, DataSection, ElemInit, ElemMode, ElementSection,
        ExecuteContext, Export, ExportDesc, ExportSection, FuncIdx, FunctionBody,
        FunctionInstanceData, GlobalIdx, HostFunction, HostFunctionDefinition, ImportDesc,
        ImportSection, InstanceData, InstanceHandle, Instr, Limits, LocalReference, MemIdx,
        ModuleInstance, NativeModule, ObjectRef, StablePc, StoreInner, TableIdx, TypeIdx,
        TypeSection, PAGE_SIZE_MAX,
    },
    runtime::{
        scheduler::{ReadyFlag, Scheduler, Task},
        vm,
    },
    Instance, Module, Registry, Stack, Store, VMResult,
};

pub(crate) fn init_global(
    gc: &mut StoreInner,
    init: &ConstExpr,
    globals: &[ObjectRef],
    funcs: &[ObjectRef],
) -> VMResult<ObjectRef> {
    tracing::trace!("global init: {init:?}");

    let res = match init {
        ConstExpr::I32(v) => gc.new_global_data4(*v as u32),
        ConstExpr::I64(v) => gc.new_global_data8(*v as u64),
        ConstExpr::F32(v) => gc.new_global_data4(v.to_bits()),
        ConstExpr::F64(v) => gc.new_global_data8(v.to_bits()),
        ConstExpr::V128(v) => gc.new_global_data16(*v),

        ConstExpr::RefNull(_t) => gc.new_global_ref(ObjectRef(0)),
        ConstExpr::FuncRef(v) => {
            let addr = funcs.get(*v as usize);
            if let Some(addr) = addr {
                gc.new_global_ref(*addr)
            } else {
                return VMResult::InvalidOperand;
            }
        }
        ConstExpr::GlobalGet(idx) => {
            let idx = *idx as usize;
            let addr = globals[idx];
            gc.copy_object(addr)
        }
    };
    VMResult::Success(res)
}

fn validate_limit(import_limit: Limits, real: u32, export_limit: Limits) -> VMResult<()> {
    if import_limit.min > real {
        tracing::trace!("invalid import_limit min");

        return VMResult::Unlinkable;
    }
    match export_limit.max {
        None => {
            if import_limit.max.is_some() {
                tracing::trace!("invalid import_limit max");

                return VMResult::Unlinkable;
            }
        }
        Some(export_max) => {
            if let Some(import_max) = import_limit.max {
                if export_max > import_max {
                    tracing::trace!("invalid import_limit max");

                    return VMResult::Unlinkable;
                }
            }
        }
    }
    VMResult::Success(())
}
fn execute_offset_const_expr(
    gc: &mut StoreInner,
    globals: &[ObjectRef],
    exprs: &[ConstExpr],
) -> VMResult<u32> {
    if exprs.len() != 1 {
        return VMResult::Unlinkable;
    }
    match &exprs[0] {
        ConstExpr::I32(v) => VMResult::Success(*v as u32),
        ConstExpr::GlobalGet(idx) => {
            let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                VMResult::Unlinkable
            }));
            let Ok(buf): Result<[u8; 4], _> = gc.get_global(addr).try_into() else {
                return VMResult::Unlinkable;
            };
            VMResult::Success(u32::from_le_bytes(buf))
        }
        _ => VMResult::Unlinkable,
    }
}

fn convert_native_module_to_module(m: NativeModule) -> Module {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    for HostFunctionDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(fp));
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    Module {
        codes: CodeSection(codes),
        functions,
        fts: TypeSection(fts),
        data: DataSection(vec![]),
        elems: ElementSection(vec![]),
        imports: ImportSection(vec![]),
        mems: vec![],
        globals: vec![],
        global_init: vec![],
        exs: ExportSection(exs),
        tables: vec![],
        start: None,
        name: None,
    }
}

fn async_host_placeholder(_ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    VMResult::Unreachable
}

fn convert_async_native_module_to_module(m: AsyncNativeModule) -> (Module, Vec<AsyncHostFunction>) {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    let mut async_functions = Vec::with_capacity(m.functions.len());
    for AsyncHostFunctionDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(async_host_placeholder));
        async_functions.push(fp);
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    (
        Module {
            codes: CodeSection(codes),
            functions,
            fts: TypeSection(fts),
            data: DataSection(vec![]),
            elems: ElementSection(vec![]),
            imports: ImportSection(vec![]),
            mems: vec![],
            globals: vec![],
            global_init: vec![],
            exs: ExportSection(exs),
            tables: vec![],
            start: None,
            name: None,
        },
        async_functions,
    )
}
pub async fn instantiate_native_module(
    m: NativeModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    instantiate(convert_native_module_to_module(m), store, registry).await
}

pub async fn instantiate_native_async_module(
    m: AsyncNativeModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let (module, async_functions) = convert_async_native_module_to_module(m);
    let instance = vm_try!(instantiate(module, store, registry).await);
    for (funcidx, fp) in async_functions.into_iter().enumerate() {
        link_async_host_function_with_function_idx(&instance, funcidx as u32, fp, store);
    }
    VMResult::Success(instance)
}

pub async fn instantiate(
    m: Module,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("instantiate is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let Module {
        fts,
        functions,
        imports,
        mems,
        globals: m_globals,
        global_init,
        exs,
        tables: m_tables,
        elems: m_elems,
        codes,
        data,
        start,
        ..
    } = m;

    let mut scheduler = Scheduler::new(store);
    let (addr, has_start) = {
        let mut gc = store.lock_gc();
        let instance_id = store.new_instance_id();

        // -> addr
        let mut memories: Vec<ObjectRef> = Vec::new();
        let mut globals = vec![];
        let mut funcs: Vec<ObjectRef> = vec![];
        let mut tables = vec![];

        for import in &imports.0 {
            tracing::trace!("processing import: {import:?}");
            let ext_inst_addr =
                vm_try!(VMResult::from_option(registry.get(&import.module), || {
                    tracing::error!("unknown instance");
                    VMResult::Unlinkable
                }));
            let instance_object_ref = vm_try!(VMResult::from_option(
                ext_inst_addr.object_ref_for_store(store),
                || {
                    tracing::error!("instance handle belongs to another store");
                    VMResult::Unlinkable
                }
            ));
            let ext_inst = unsafe { &*gc.get_instance_unchecked(instance_object_ref) };
            let ext_module = gc.get_module(ext_inst.module_addr);
            let export = vm_try!(VMResult::from_option(
                ext_module.exports.find(&import.name),
                || {
                    tracing::error!("unknown export");
                    VMResult::Unlinkable
                }
            ));
            match (&import.desc, export) {
                (ImportDesc::TypeIdx(tidx), ExportDesc::Func(funcidx)) => {
                    let import_ft = fts.get(*tidx).unwrap();
                    let export_ft_idx = ext_module.functions[funcidx.0 as usize];
                    let export_ft = ext_module
                        .function_types
                        .get(export_ft_idx.0 as usize)
                        .unwrap();
                    if import_ft != export_ft {
                        tracing::trace!("import function type");
                        return VMResult::Unlinkable;
                    }
                    let funcaddr = ext_inst.funcs.as_slice()[funcidx.0 as usize];
                    let funcidx = funcs.len();
                    funcs.push(funcaddr);
                    tracing::trace!("linking: {funcidx} => {funcaddr:?}")
                }
                (ImportDesc::GlobalType(import_gt), ExportDesc::Global(global_idx)) => {
                    let export_gt = ext_module.globals.get(global_idx.0 as usize).unwrap();
                    if import_gt != export_gt {
                        tracing::trace!("import global type");
                        return VMResult::Unlinkable;
                    }
                    globals.push(ext_inst.globals.as_slice()[global_idx.0 as usize]);
                }
                (ImportDesc::TableType(import_tt), ExportDesc::Table(idx)) => {
                    let export_tt = ext_module.tables[idx.0 as usize];
                    tracing::trace!("{export_tt:?}");

                    if import_tt.reftype != export_tt.reftype {
                        tracing::trace!("import table type");
                        return VMResult::Unlinkable;
                    }
                    let addr = ext_inst.tables.as_slice()[idx.0 as usize];
                    vm_try!(validate_limit(
                        import_tt.limits,
                        gc.get_table(addr).1.len() as u32,
                        export_tt.limits
                    ));
                    tables.push(ext_inst.tables.as_slice()[idx.0 as usize]);
                }
                (ImportDesc::MemType(mt), ExportDesc::Mem(_idx)) => {
                    let memory_addr = *vm_try!(VMResult::from_option(
                        ext_inst.mems.as_slice().get(_idx.0 as usize),
                        || {
                            tracing::trace!("invalid instance memory");
                            VMResult::Unlinkable
                        }
                    ));
                    let limits = ext_module.mems[_idx.0 as usize];

                    if mt.shared != limits.shared {
                        tracing::trace!("import shared memory flag mismatch");
                        return VMResult::Unlinkable;
                    }
                    let handle = gc.memory_handle(memory_addr);
                    vm_try!(validate_limit(
                        mt.limits,
                        gc.memory_page_size(handle),
                        limits.limits
                    ));
                    memories.push(memory_addr);
                }
                _ => {
                    tracing::trace!("import other type objects");
                    return VMResult::Unlinkable;
                }
            }
        }

        let mod_addr = gc.new_module(ModuleInstance {
            function_types: fts.0.clone(),
            functions: functions.clone(),
            exports: exs.clone(),
            tables: m_tables.clone(),
            globals: m_globals.clone(),
            mems: mems.clone(),
        });
        let inst_id = gc.alloc_instance(InstanceData {
            instance_id,
            module_addr: mod_addr,
            globals: Vec::new(),
            funcs: Vec::new(),
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });
        let inst_addr = gc.object_ref_for_instance(inst_id);

        for mem in mems.iter().skip(memories.len()) {
            let limits = mem.limits;
            memories.push(if mem.shared {
                gc.new_shared_memory(limits.min, limits.max.unwrap_or(PAGE_SIZE_MAX as u32))
            } else {
                gc.new_memory(limits.min, limits.max.unwrap_or(PAGE_SIZE_MAX as u32))
            });
        }

        for (idx, d) in (0..).zip(data.0.into_iter()) {
            match &d.mode {
                DataMode::Active(mem, offset) => {
                    let offset =
                        vm_try!(execute_offset_const_expr(&mut gc, &globals, offset)) as usize;
                    let memory =
                        *vm_try!(VMResult::from_option(memories.get(mem.0 as usize), || {
                            VMResult::MemoryIndexOutOfRange
                        }));
                    vm_try!(gc.with_memory_by_addr(memory, |memory| {
                        if let Some(slice) = memory.get_mut(offset..offset + d.init.len()) {
                            slice.copy_from_slice(&d.init);
                            VMResult::Success(())
                        } else {
                            VMResult::MemoryIndexOutOfRange
                        }
                    }));
                    store.lock_segments().data.insert((instance_id, idx), d);
                }
                DataMode::Passive => {
                    store.lock_segments().data.insert((instance_id, idx), d);
                }
            }
        }

        let mut local_wasm_funcs = Vec::new();
        for func in codes.0.into_iter() {
            let funcidx = funcs.len() as u32;

            let func_addr = match func {
                FunctionBody::Wasm(code) => {
                    let materialized = code.lowered.materialize();
                    let func_addr = gc.new_func(&FunctionInstanceData {
                        instance: inst_id,
                        body: RuntimeFunctionBody::Wasm {
                            locals: code.locals,
                            code: materialized.instrs.into(),
                            lowered: code.lowered.clone(),
                        },
                        funcidx,
                    });
                    local_wasm_funcs.push(func_addr);
                    func_addr
                }
                FunctionBody::Host(fp) => gc.new_func(&FunctionInstanceData {
                    instance: inst_id,
                    body: RuntimeFunctionBody::Host(fp),
                    funcidx,
                }),
            };

            funcs.push(func_addr);
            tracing::trace!("linking: {funcidx} => {func_addr:?}");
        }

        for init in &global_init {
            globals.push(vm_try!(init_global(&mut gc, init, &globals, &funcs)));
        }
        let mut table_instances: Vec<ObjectRef> =
            m_tables.iter().map(|v| gc.new_table(*v)).collect();
        tables.append(&mut table_instances);

        let res = (|| {
            for (idx, elem) in (0u32..).zip(m_elems.0.into_iter()) {
                match &elem.mode {
                    ElemMode::Active(idx, offset) => match &elem.init {
                        ElemInit::FuncIdx(idxs) => {
                            let offset =
                                vm_try!(execute_offset_const_expr(&mut gc, &globals, offset))
                                    as usize;
                            let table_addr = tables[idx.0 as usize];
                            let instance = gc.get_table(table_addr);

                            if instance.0.reftype != elem.kind {
                                panic!("reftype mismatch")
                            }
                            if offset + idxs.len() > instance.1.len() {
                                return VMResult::TableIndexOutOfRange;
                            }
                            for (idx, funcidx) in idxs.iter().enumerate() {
                                instance.1[offset + idx] = funcs[*funcidx as usize].get();
                            }
                        }
                        ElemInit::ConstExpr(idxs) => {
                            let offset =
                                vm_try!(execute_offset_const_expr(&mut gc, &globals, offset))
                                    as usize;
                            let table_addr = tables[idx.0 as usize];
                            let instance = gc.get_table(table_addr);
                            if offset + idxs.len() > instance.1.len() {
                                return VMResult::TableIndexOutOfRange;
                            }
                            let rt = instance.0.reftype;

                            for (idx, idx_expr) in idxs.iter().enumerate() {
                                let elem_addr = vm_try!(execute_elem_init_const_expr(
                                    &mut gc, &globals, &funcs, idx_expr, rt
                                ));
                                let instance = gc.get_table(table_addr);
                                instance.1[offset + idx] = elem_addr.get();
                            }
                        }
                    },
                    ElemMode::Passive => {
                        store.lock_segments().elems.insert((instance_id, idx), elem);
                    }
                    ElemMode::Declarative => {}
                }
            }
            VMResult::Success(())
        })();

        let instance = Instance {
            module_addr: mod_addr,
            instance_id,
            memory: memories,
            tables,
            globals,
            funcs,
        };

        unsafe {
            gc.place_instance_unchecked(inst_addr, &instance);
        }
        vm_try!(res);
        let recipe_slots = instance
            .funcs
            .iter()
            .map(|&funcaddr| gc.call_recipe_slot_for_func(funcaddr))
            .collect::<Vec<_>>();
        for func_addr in local_wasm_funcs {
            let materialized = match &gc.get_func(func_addr).body {
                RuntimeFunctionBody::Wasm { lowered, .. } => {
                    lowered.materialize_with_recipe_slots(&recipe_slots)
                }
                RuntimeFunctionBody::Host(_) | RuntimeFunctionBody::AsyncHost(_) => continue,
            };
            let func = gc.get_func_mut(func_addr);
            let RuntimeFunctionBody::Wasm { code, .. } = &mut func.body else {
                unreachable!("materialized local wasm function must remain wasm")
            };
            *code = materialized.instrs.into();
        }
        for &funcaddr in &instance.funcs {
            let recipe = gc.build_call_recipe(funcaddr);
            gc.set_call_recipe_for_func(funcaddr, recipe);
        }
        let addr = InstanceHandle::new(store, inst_id, instance_id);

        let has_start = if let Some(start) = start {
            let mut stack = Stack::new(128 * 1024);
            let funcaddr = instance.funcs[start.0 as usize];
            let funcinst = gc.get_func(funcaddr);
            let func_instance = gc.instance(funcinst.instance);
            if funcinst.is_host_func() {
                let local_reference = vm_try!(stack.function_call(
                    0,
                    0,
                    CallFrameCache::from_parts(
                        funcaddr,
                        funcinst,
                        func_instance
                            .memory_slots
                            .first()
                            .copied()
                            .and_then(|slot| slot.handle()),
                    ),
                    LocalReference {
                        local_size: 0,
                        local_top: 0
                    },
                    &vm::VM_END,
                    &gc,
                ));

                scheduler.push(Task {
                    task_id: 0,
                    stack,
                    local_reference,
                    ready_flag: ReadyFlag::Ready,
                    fp: StablePc::from_stable_ptr(vm::START_HOST_FUNCTION_PROGRAM.as_ptr()),
                    pending_effects: 0,
                    terminal_result: None,
                });
            } else {
                let locals = funcinst.locals();
                let local_reference = vm_try!(stack.function_call(
                    0,
                    locals.byte_size(),
                    CallFrameCache::from_parts(
                        funcaddr,
                        funcinst,
                        func_instance
                            .memory_slots
                            .first()
                            .copied()
                            .and_then(|slot| slot.handle()),
                    ),
                    LocalReference {
                        local_size: 0,
                        local_top: 0
                    },
                    &vm::VM_END,
                    &gc,
                ));

                scheduler.push(Task {
                    fp: StablePc::from_relative_index(0),
                    task_id: 0,
                    stack,
                    local_reference,
                    ready_flag: ReadyFlag::Ready,
                    pending_effects: 0,
                    terminal_result: None,
                });
            }
            true
        } else {
            false
        };

        (addr, has_start)
    };

    if has_start {
        scheduler.run().await;
        vm_try!(scheduler.completed_tasks.pop().unwrap().result);
    }

    VMResult::Success(addr)
}
#[allow(dead_code)]
pub fn aliasing(
    registry: &Registry,
    triplets: &[(String, String, String)],
    store: &Store,
) -> VMResult<InstanceHandle> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("aliasing is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let mut gc = store.lock_gc();
    let inst_id = store.new_instance_id();
    let mut functions = vec![];
    let mut function_types = vec![];
    let mut globals = vec![];
    let mut memories = vec![];
    let mut tables = vec![];
    let mut function_addrs = vec![];
    let mut global_addrs = vec![];
    let mut memory_addrs = vec![];
    let mut table_addrs = vec![];
    let mut exports = vec![];
    for (modname, importname, exportname) in triplets {
        let instance_addr = vm_try!(VMResult::from_option(registry.get(modname), || {
            VMResult::Unlinkable
        }));

        let object_ref = vm_try!(VMResult::from_option(
            instance_addr.object_ref_for_store(store),
            || { VMResult::Unlinkable }
        ));
        let ext_instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
        let ext_module = gc.get_module(ext_instance.module_addr);
        let export_desc = vm_try!(VMResult::from_option(
            ext_module.exports.find(importname),
            || { VMResult::Unlinkable }
        ));
        let exportname = (*exportname).to_owned();
        match export_desc {
            ExportDesc::Func(idx) => {
                let tidx = ext_module.functions[idx.0 as usize];
                let ft = &ext_module.function_types[tidx.0 as usize];
                let new_tidx = function_types.len();
                let new_funcidx = functions.len();
                function_types.push(ft.clone());
                functions.push(TypeIdx(new_tidx as u32));
                let addr = ext_instance.funcs.as_slice()[idx.0 as usize];
                function_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Func(FuncIdx(new_funcidx as u32)),
                ));
            }
            ExportDesc::Global(idx) => {
                let gt = ext_module.globals[idx.0 as usize];
                let new_gidx = globals.len();
                globals.push(gt);
                let addr = ext_instance.globals.as_slice()[idx.0 as usize];
                global_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Global(GlobalIdx(new_gidx as u32)),
                ));
            }
            ExportDesc::Mem(idx) => {
                let mt = ext_module.mems[idx.0 as usize];
                let new_memidx = memories.len();
                memories.push(mt);
                let addr = ext_instance.mems.as_slice()[idx.0 as usize];
                memory_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Mem(MemIdx(new_memidx as u32)),
                ));
            }
            ExportDesc::Table(idx) => {
                let tt = ext_module.tables[idx.0 as usize];
                let new_tableidx = tables.len();
                tables.push(tt);
                table_addrs.push(ext_instance.tables.as_slice()[idx.0 as usize]);
                exports.push(Export(
                    exportname,
                    ExportDesc::Table(TableIdx(new_tableidx as u32)),
                ));
            }
        }
    }
    let mod_addr = gc.new_module(ModuleInstance {
        exports: ExportSection(exports),
        tables,
        globals,
        functions,
        function_types,
        mems: memories,
    });
    let inst_id_handle = gc.alloc_instance(InstanceData {
        module_addr: mod_addr,
        mems: memory_addrs,
        globals: global_addrs,
        funcs: function_addrs,
        tables: table_addrs,
        instance_id: inst_id,
        memory_slots: Vec::new(),
    });
    VMResult::Success(InstanceHandle::new(store, inst_id_handle, inst_id))
}
pub fn link_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_host_function_with_function_idx is unsupported while the same store GC is already active"
        );
        return;
    }
    let mut gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
    let funcaddr = instance.funcs.as_slice()[funcidx as usize];
    {
        let func = gc.get_func_mut(funcaddr);
        func.replace_host_code_pointer(f);
    }
    let recipe = gc.build_call_recipe(funcaddr);
    gc.set_call_recipe_for_func(funcaddr, recipe);
}
pub fn link_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: HostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_host_function_with_export_name is unsupported while the same store GC is already active"
        );
        return;
    }
    let gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
    let module = gc.get_module(instance.module_addr);
    let export = &module.exports.find(name).unwrap();
    let func_idx = if let ExportDesc::Func(v) = export {
        v.0
    } else {
        unreachable!()
    };
    link_host_function_with_function_idx(addr, func_idx, f, store);
}

pub fn link_async_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: AsyncHostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_async_host_function_with_function_idx is unsupported while the same store GC is already active"
        );
        return;
    }
    let mut gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
    let funcaddr = instance.funcs.as_slice()[funcidx as usize];
    {
        let func = gc.get_func_mut(funcaddr);
        func.replace_async_host_code_pointer(f);
    }
    let recipe = gc.build_call_recipe(funcaddr);
    gc.set_call_recipe_for_func(funcaddr, recipe);
}

pub fn link_async_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: AsyncHostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_async_host_function_with_export_name is unsupported while the same store GC is already active"
        );
        return;
    }
    let gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
    let module = gc.get_module(instance.module_addr);
    let export = &module.exports.find(name).unwrap();
    let func_idx = if let ExportDesc::Func(v) = export {
        v.0
    } else {
        unreachable!()
    };
    drop(gc);
    link_async_host_function_with_function_idx(addr, func_idx, f, store);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IoReadBinaryReader, WasmParser};

    async fn instantiate_wat_for_test(wat_src: &str) -> (Store, InstanceHandle) {
        let bytes = wat::parse_str(wat_src).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        let store = Store::new();
        let registry = Registry::new();
        let VMResult::Success(instance) = instantiate(module, &store, &registry).await else {
            panic!("module must instantiate");
        };
        (store, instance)
    }

    #[test]
    fn execute_offset_const_expr_fail_closes_non_i32_const() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let result = execute_offset_const_expr(&mut gc, &[], &[ConstExpr::F64(1.0)]);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[test]
    fn execute_offset_const_expr_fail_closes_non_i32_global_get() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let global = gc.new_global_data8(42);
        let result = execute_offset_const_expr(&mut gc, &[global], &[ConstExpr::GlobalGet(0)]);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[test]
    fn lowered_materialize_with_recipe_slots_resolves_direct_call_operands() {
        let lowered = crate::common::LoweredFunction::from_materialized(
            vec![
                Instr { op: vm::op_call },
                Instr {
                    operand: crate::common::Operand {
                        call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(1),
                    },
                },
                Instr { op: vm::op_end },
            ],
            vec![2, 1],
        );
        let materialized = lowered.materialize_with_recipe_slots(&[7, 13]);
        let target = unsafe { materialized.instrs[1].operand.call_recipe_ref };
        assert_eq!(target.funcidx, 1);
        assert_eq!(target.resolved_recipe_slot(), Some(13));
    }

    #[tokio::test]
    async fn instantiate_rewrites_return_call_with_matching_recipe_slot() {
        let (store, instance) = instantiate_wat_for_test(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0
                i32.const 0
                call 1)
              (func (param $n i32) (param $acc i32) (result i32)
                local.get $n
                i32.eqz
                if
                  local.get $acc
                  return
                end
                local.get $n
                i32.const 1
                i32.sub
                local.get $acc
                local.get $n
                i32.add
                return_call 1))
            "#,
        )
        .await;

        let gc = store.lock_gc();
        let object_ref = instance
            .object_ref_for_store(&store)
            .expect("instance must belong to store");
        let instance_data = gc.get_instance(object_ref);
        let funcaddr = instance_data.funcs[1];
        let expected_slot = gc.call_recipe_slot_for_func(funcaddr);
        let func = gc.get_func(funcaddr);
        let crate::common::store::FunctionBody::Wasm { code, .. } = &func.body else {
            panic!("expected wasm function");
        };
        let call_index = code
            .iter()
            .position(|instr| {
                std::ptr::fn_addr_eq(
                    unsafe { instr.op },
                    vm::op_return_call
                        as unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>,
                )
            })
            .expect("return_call must exist");
        let target = unsafe { code[call_index + 1].operand.call_recipe_ref };
        assert_eq!(target.funcidx, 1);
        assert_eq!(target.resolved_recipe_slot(), Some(expected_slot));
    }
}
