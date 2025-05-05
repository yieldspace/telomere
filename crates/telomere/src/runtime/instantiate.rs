use std::rc::Rc;

use crate::{
    common::{
        execute_elem_init_const_expr,
        gc::{
            FunctionInstanceData, GcRef, GcRootHandle, Header, InstanceData, MemoryPool, ObjectType,
        },
        word_size, CodeSection, ConstExpr, DataMode, DataSection, ElemInit, ElemMode,
        ElementSection, ExecuteContext, Export, ExportDesc, ExportSection, FuncIdx, FunctionBody,
        GlobalIdx, HostFunction, HostFunctionDefinition, ImportDesc, ImportSection, InstanceHandle,
        Instr, Limits, LocalReference, MemIdx, ModuleInstance, NativeModule, TableIdx, TypeIdx,
        TypeSection, PAGE_SIZE_MAX,
    },
    runtime::vm,
    Instance, Module, Registry, Stack, Store, VMResult,
};

pub(crate) fn init_global(
    gc: &mut MemoryPool,
    init: &ConstExpr,
    globals: &[GcRef],
    funcs: &[GcRef],
) -> VMResult<GcRef> {
    tracing::trace!("global init: {init:?}");

    let res = match init {
        ConstExpr::I32(v) => gc.new_global_data4(*v as u32),
        ConstExpr::I64(v) => gc.new_global_data8(*v as u64),
        ConstExpr::F32(v) => gc.new_global_data4(v.to_bits()),
        ConstExpr::F64(v) => gc.new_global_data8(v.to_bits()),
        ConstExpr::V128(v) => gc.new_global_data16(*v),

        ConstExpr::RefNull(_t) => gc.new_global_ref(GcRef(0)),
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
    gc: &mut MemoryPool,
    globals: &[GcRef],
    exprs: &[ConstExpr],
) -> VMResult<u32> {
    for expr in exprs {
        return VMResult::Success(match expr {
            ConstExpr::I32(v) => *v as u32,
            ConstExpr::GlobalGet(idx) => {
                let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                    VMResult::Unlinkable
                }));
                let mut buf = [0u8; 4];
                buf.copy_from_slice(unsafe { gc.get_global(addr) });
                u32::from_le_bytes(buf)
            }
            _ => {
                todo!()
            }
        });
    }
    VMResult::Unlinkable
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
pub fn instantiate_native_module(
    m: NativeModule,
    store: &mut Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    instantiate(convert_native_module_to_module(m), store, registry)
}

pub fn instantiate(m: Module, store: &mut Store, registry: &Registry) -> VMResult<InstanceHandle> {
    let instance_id = store.new_instance_id();
    let gc = store.gc.clone();
    let mut gc = gc.borrow_mut();
    let gc = &mut gc;
    // -> addr
    let mut memory: Option<GcRef> = None;
    let mut globals = vec![];
    let mut funcs: Vec<GcRef> = vec![];
    let mut tables = vec![];
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
    for import in &imports.0 {
        tracing::trace!("processing import: {import:?}");
        let ext_inst_addr = vm_try!(VMResult::from_option(registry.get(&import.module), || {
            tracing::error!("unknown instance");
            VMResult::Unlinkable
        }));
        let instance_gc_ref = ext_inst_addr.get_gc_ref_with_pool(gc);
        let ext_inst = unsafe { &*gc.get_instance_unchecked(instance_gc_ref) };
        let ext_module = unsafe { gc.get_module(ext_inst.module_addr) };
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
                let funcaddr = ext_inst.funcs.as_slice(gc)[funcidx.0 as usize];
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
                globals.push(ext_inst.globals.as_slice(gc)[global_idx.0 as usize]);
            }
            (ImportDesc::TableType(import_tt), ExportDesc::Table(idx)) => {
                let export_tt = ext_module.tables[idx.0 as usize];
                tracing::trace!("{export_tt:?}");

                if import_tt.reftype != export_tt.reftype {
                    tracing::trace!("import table type");

                    return VMResult::Unlinkable;
                }
                let addr = ext_inst.tables.as_slice(gc)[idx.0 as usize];
                vm_try!(validate_limit(
                    import_tt.limits,
                    unsafe { gc.get_table(addr) }.1.len() as u32,
                    export_tt.limits
                ));
                tables.push(ext_inst.tables.as_slice(gc)[idx.0 as usize]);
            }
            (ImportDesc::MemType(mt), ExportDesc::Mem(_idx)) => {
                memory = ext_inst.mems.as_slice(gc).first().copied();
                let limits = ext_module.mems[0].0;

                if let Some(memory_addr) = memory {
                    let memory = unsafe { gc.get_memory(memory_addr) };
                    vm_try!(validate_limit(mt.0, memory.page_size(), limits))
                } else {
                    tracing::trace!("invalid instance memory");
                    return VMResult::Unlinkable;
                }
            }
            _ => {
                tracing::trace!("import other type objects");
                return VMResult::Unlinkable;
            }
        }
    }
    let inst_addr = gc.allocate(Header::new(
        ObjectType::Instance,
        word_size::<InstanceData>(),
    ));
    if memory.is_none() {
        if let Some(mem) = mems.first() {
            memory = Some(gc.new_memory(mem.0.min, mem.0.max.unwrap_or(PAGE_SIZE_MAX as u32)))
        }
    }

    for (idx, d) in (0..).zip(data.0.into_iter()) {
        match &d.mode {
            DataMode::Active(mem, offset) => {
                assert_eq!(mem.0, 0);
                let offset = vm_try!(execute_offset_const_expr(gc, &globals, offset)) as usize;
                if let Some(memory) = &memory {
                    let memory = unsafe { gc.get_memory(*memory) };
                    if let Some(slice) = memory.get_mut(offset..offset + d.init.len()) {
                        slice.copy_from_slice(&d.init);
                    } else {
                        return VMResult::MemoryIndexOutOfRange;
                    }
                } else {
                    return VMResult::MemoryIndexOutOfRange;
                }
                store.data.insert((instance_id, idx), d);
            }
            DataMode::Passive => {
                store.data.insert((instance_id, idx), d);
            }
        }
    }

    for func in codes.0.into_iter() {
        let funcidx = funcs.len() as u32;

        let func_addr = match func {
            FunctionBody::Wasm(code) => {
                let body = gc.new_function_body(&code.locals, &code.expr);
                gc.new_func(&FunctionInstanceData {
                    instance_addr: inst_addr,
                    function_flags: FunctionInstanceData::create_wasm_flags(&code.locals),
                    body,
                    funcidx,
                })
            }
            FunctionBody::Host(fp) => {
                let align = align_of::<HostFunction>();
                let align64 = align == 8;
                let header = if align64 {
                    Header::new(ObjectType::Raw, word_size::<usize>())
                        .align64()
                        .initialized()
                } else {
                    Header::new(ObjectType::Raw, word_size::<usize>()).initialized()
                };
                let body = gc.allocate(header);
                let ptr = unsafe { gc.get_value_mut::<usize>(body, 0) };
                unsafe { std::ptr::write(ptr, fp as usize) };
                gc.new_func(&FunctionInstanceData {
                    instance_addr: inst_addr,
                    function_flags: FunctionInstanceData::create_host_flags(),
                    body,
                    funcidx,
                })
            }
        };

        funcs.push(func_addr);

        tracing::trace!("linking: {funcidx} => {func_addr:?}");
    }

    for init in &global_init {
        globals.push(vm_try!(init_global(gc, init, &globals, &funcs)));
    }
    let mut table_instances: Vec<GcRef> = m_tables.iter().map(|v| gc.new_table(*v)).collect();
    tables.append(&mut table_instances);

    tracing::trace!("funcs: {funcs:?}");

    let res = (|| {
        tracing::trace!("funcs2: {funcs:?}");
        for (idx, elem) in (0u32..).zip(m_elems.0.into_iter()) {
            tracing::trace!("funcs3: {funcs:?}");
            match &elem.mode {
                ElemMode::Active(idx, offset) => match &elem.init {
                    ElemInit::FuncIdx(idxs) => {
                        let offset =
                            vm_try!(execute_offset_const_expr(gc, &globals, offset)) as usize;
                        let table_addr = tables[idx.0 as usize];

                        let instance = unsafe { gc.get_table(table_addr) };

                        if instance.0.reftype != elem.kind {
                            panic!("reftype mismatch")
                        }
                        if offset + idxs.len() > instance.1.len() {
                            return VMResult::TableIndexOutOfRange;
                        }
                        for (idx, funcidx) in idxs.iter().enumerate() {
                            instance.1[offset + idx] = funcs[*funcidx as usize].get();
                            tracing::trace!(
                                "table[{}] = {:?}",
                                offset + idx,
                                funcs[*funcidx as usize]
                            );
                        }
                    }
                    ElemInit::ConstExpr(idxs) => {
                        let offset =
                            vm_try!(execute_offset_const_expr(gc, &globals, offset)) as usize;
                        let table_addr = tables[idx.0 as usize];
                        let Store { .. } = store;
                        let instance = unsafe { gc.get_table(table_addr) };
                        if offset + idxs.len() > instance.1.len() {
                            return VMResult::TableIndexOutOfRange;
                        }
                        let rt = instance.0.reftype;

                        tracing::trace!("funcs4: {funcs:?}");

                        for (idx, idx_expr) in idxs.iter().enumerate() {
                            let addr = vm_try!(execute_elem_init_const_expr(
                                gc, &globals, &funcs, idx_expr, rt
                            ));
                            let instance = unsafe { gc.get_table(table_addr) };
                            instance.1[offset + idx] = addr.get();
                            tracing::trace!("table[{}] = {:?}", offset + idx, addr);
                        }
                    }
                },
                ElemMode::Passive => {
                    store.elems.insert((instance_id, idx), elem);
                }
                ElemMode::Declarative => {
                    //do nothing
                }
            }
        }
        VMResult::Success(())
    })();

    tracing::trace!("instance funcs: {funcs:?}");
    let mod_addr = gc.new_module(ModuleInstance {
        function_types: fts.0,
        functions,
        exports: exs,
        tables: m_tables,
        globals: m_globals,
        mems,
    });

    let instance = Instance {
        module_addr: mod_addr,
        instance_id,
        memory: memory.into_iter().collect::<Vec<_>>(),
        tables,
        globals,
        funcs,
    };

    if let Some(start) = start {
        let mut stack = Stack::new(128 * 1024);

        let funcaddr = instance.funcs[start.0 as usize];
        unsafe {
            gc.place_instance_unchecked(inst_addr, &instance);
        }
        vm_try!(res);

        let funcinst = unsafe { gc.get_func(funcaddr) };
        if funcinst.is_host_func() {
            let fp = funcinst.host_code_pointer(gc);
            let local_reference = vm_try!(stack.function_call(
                0,
                0,
                funcaddr,
                LocalReference {
                    local_size: 0,
                    local_top: 0
                },
                &vm::VM_END
            ));

            let mut ctx = ExecuteContext {
                stack: &mut stack,
                store,
                local_reference,
                gc,
            };
            let return_addr = vm_try!(fp(&mut ctx));
            vm_try!(unsafe { vm::call_next(return_addr, 0, &mut ctx) });
        } else {
            let (locals, offset) = funcinst.locals_and_code_offset(gc);
            let local_reference = vm_try!(stack.function_call(
                0,
                locals.byte_size(),
                funcaddr,
                LocalReference {
                    local_size: 0,
                    local_top: 0
                },
                &vm::VM_END
            ));
            let ptr = unsafe { gc.get_value::<Instr>(funcinst.body, offset) };
            let mut ctx = ExecuteContext {
                stack: &mut stack,
                store,
                local_reference,
                gc,
            };
            vm_try!(unsafe { vm::call_next(ptr, 0, &mut ctx) });
        }
    } else {
        unsafe {
            gc.place_instance_unchecked(inst_addr, &instance);
        }
        vm_try!(res);
    }
    let addr = InstanceHandle(Rc::new(GcRootHandle::new_with_ref(
        inst_addr,
        gc,
        store.gc.clone(),
    )));
    VMResult::Success(addr)
}
// TODO:
#[allow(dead_code)]
pub fn aliasing(
    registry: &Registry,
    triplets: &[(String, String, String)],
    store: &mut Store,
) -> VMResult<InstanceHandle> {
    let inst_id = store.new_instance_id();
    let mut gc = store.gc.borrow_mut();
    let gc = &mut gc;
    let mut functions = vec![];
    let mut function_types = vec![];
    let mut globals = vec![];
    let mut memories = vec![];
    let mut tables = vec![];
    let mut function_addrs = vec![];
    let mut global_addrs = vec![];
    let mut mem_addr = None;
    let mut table_addrs = vec![];
    let mut exports = vec![];
    for (modname, importname, exportname) in triplets {
        let instance_addr = vm_try!(VMResult::from_option(registry.get(modname), || {
            VMResult::Unlinkable
        }));

        let gc_ref = instance_addr.get_gc_ref_with_pool(gc);
        let ext_instance = unsafe { &*gc.get_instance_unchecked(gc_ref) };
        let ext_module = unsafe { gc.get_module(ext_instance.module_addr) };
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
                let addr = ext_instance.funcs.as_slice(&store.gc.borrow())[idx.0 as usize];
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
                let addr = ext_instance.globals.as_slice(&store.gc.borrow())[idx.0 as usize];
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
                mem_addr = ext_instance
                    .mems
                    .as_slice(&store.gc.borrow())
                    .first()
                    .copied();
                exports.push(Export(
                    exportname,
                    ExportDesc::Mem(MemIdx(new_memidx as u32)),
                ));
            }
            ExportDesc::Table(idx) => {
                let tt = ext_module.tables[idx.0 as usize];
                let new_tableidx = tables.len();
                tables.push(tt);
                table_addrs.push(ext_instance.tables.as_slice(&store.gc.borrow())[idx.0 as usize]);
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
    let inst_addr = gc.new_instance(&Instance {
        module_addr: mod_addr,
        memory: mem_addr.into_iter().collect::<Vec<_>>(),
        globals: global_addrs,
        funcs: function_addrs,
        tables: table_addrs,
        instance_id: inst_id,
    });

    VMResult::Success(InstanceHandle(Rc::new(GcRootHandle::new(
        inst_addr,
        store.gc.clone(),
    ))))
}
pub fn link_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFunction,
    store: &mut Store,
) {
    let mut gc = store.gc.borrow_mut();
    let gc_ref = addr.get_gc_ref_with_pool(&gc);
    let instance = unsafe { &*gc.get_instance_unchecked(gc_ref) };
    let funcaddr = instance.funcs.as_slice(&gc)[funcidx as usize];
    let func = unsafe { gc.get_func_mut(funcaddr) };
    func.function_flags = FunctionInstanceData::create_host_flags();
    let funcbody = func.body;
    unsafe { *gc.get_value_mut::<HostFunction>(funcbody, 0) = f };
}
pub fn link_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: HostFunction,
    store: &mut Store,
) {
    let mut gc = store.gc.borrow_mut();
    let instance = unsafe { &*gc.get_instance_unchecked(addr.get_gc_ref_with_pool(&gc)) };
    let module = unsafe { gc.get_module(instance.module_addr) };
    let export = &module.exports.find(name).unwrap();
    let func_idx = if let ExportDesc::Func(v) = export {
        v
    } else {
        unreachable!()
    };
    let funcaddr = instance.funcs.as_slice(&gc)[func_idx.0 as usize];
    let func = unsafe { gc.get_func_mut(funcaddr) };
    func.function_flags = FunctionInstanceData::create_host_flags();
    let funcbody = func.body;

    unsafe { *gc.get_value_mut::<HostFunction>(funcbody, 0) = f };
}
