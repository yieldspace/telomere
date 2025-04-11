use crate::{
    common::{
        execute_elem_init_const_expr, ConstExpr, DataMode, ElemInit, ElemMode, ExecuteContext,
        Export, ExportDesc, ExportSection, FuncIdx, FunctionInstance, Import, ImportDesc,
        InstanceAddr, JumpTable, Limits, LocalState, Memory, ModuleInstance, TableInstance,
        TypeIdx, PAGE_SIZE_MAX,
    },
    runtime::vm,
    Instance, Module, Registry, Stack, Store, VMResult,
};

use super::TABLE_UNINITIALIZED;

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
    store: &mut Store,
    globals: &[u32],
    exprs: &[ConstExpr],
) -> VMResult<u32> {
    for expr in exprs {
        return VMResult::Success(match expr {
            ConstExpr::I32(v) => *v as u32,
            ConstExpr::GlobalGet(idx) => {
                let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                    VMResult::Unlinkable
                })) as usize;
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&store.globals.0[addr..addr + 4]);
                u32::from_le_bytes(buf)
            }
            _ => {
                todo!()
            }
        });
    }
    VMResult::Unlinkable
}

pub fn instantiate(m: Module, store: &mut Store, registry: &Registry) -> VMResult<InstanceAddr> {
    let mod_addr = store.modules.len() as u32;
    let inst_addr = store.instances.len() as u32;
    // -> addr
    let mut memory: Option<u32> = None;
    let mut globals = vec![];
    let mut funcs: Vec<u32> = vec![];
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
    } = m;
    for import in &imports.0 {
        tracing::trace!("{import:?}");
        let ext_inst_addr = vm_try!(VMResult::from_option(registry.get(&import.module), || {
            VMResult::Unlinkable
        }));
        let ext_inst = &store.instances[ext_inst_addr.0 as usize];
        let ext_module = &store.modules[ext_inst.module_addr as usize];
        let export = vm_try!(VMResult::from_option(
            ext_module.exports.find(&import.name),
            || { VMResult::Unlinkable }
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
                let funcaddr = ext_inst.funcs[funcidx.0 as usize];
                let funcidx = funcs.len();
                funcs.push(funcaddr);
                tracing::trace!("linking: {mod_addr} {funcidx} => {funcaddr}")
            }
            (ImportDesc::GlobalType(import_gt), ExportDesc::Global(global_idx)) => {
                let export_gt = ext_module.globals.get(global_idx.0 as usize).unwrap();
                if import_gt != export_gt {
                    tracing::trace!("import global type");

                    return VMResult::Unlinkable;
                }
                globals.push(ext_inst.globals[global_idx.0 as usize]);
            }
            (ImportDesc::TableType(import_tt), ExportDesc::Table(idx)) => {
                let export_tt = ext_module.tables[idx.0 as usize];
                tracing::trace!("{export_tt:?}");

                if import_tt.reftype != export_tt.reftype {
                    tracing::trace!("import table type");

                    return VMResult::Unlinkable;
                }
                let addr = ext_inst.tables[idx.0 as usize];
                vm_try!(validate_limit(
                    import_tt.limits,
                    store.tables[addr as usize].1.len() as u32,
                    export_tt.limits
                ));
                tables.push(ext_inst.tables[idx.0 as usize]);
            }
            (ImportDesc::MemType(mt), ExportDesc::Mem(_idx)) => {
                memory = ext_inst.memory;
                if let Some(memory_addr) = &memory {
                    let memory = &store.memory[*memory_addr as usize];
                    vm_try!(validate_limit(
                        mt.0,
                        memory.page_size(),
                        ext_module.mems[0].0
                    ))
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
    if memory.is_none() {
        if let Some(mem) = mems.first() {
            memory = Some(store.memory.len() as u32);
            store.memory.push(Memory::new(
                mem.0.min,
                (mem.0.max).unwrap_or(PAGE_SIZE_MAX as u32),
            ));
        }
    }

    for d in &data.0 {
        match &d.mode {
            DataMode::Active(mem, offset) => {
                assert_eq!(mem.0, 0);
                let offset = vm_try!(execute_offset_const_expr(store, &globals, offset)) as usize;
                if let Some(memory) = &memory {
                    let memory = &mut store.memory[*memory as usize];
                    if let Some(slice) = memory.get_mut(offset..offset + d.init.len()) {
                        slice.copy_from_slice(&d.init);
                    } else {
                        return VMResult::MemoryIndexOutOfRange;
                    }
                } else {
                    return VMResult::MemoryIndexOutOfRange;
                }
            }
            _ => {
                // do nothing
            }
        }
    }

    let mut funcaddr = store.funcs.0.len();
    let mut s_funcs = vec![];
    for func in codes.0.into_iter() {
        let funcidx = funcs.len() as u32;
        funcs.push(funcaddr as u32);
        s_funcs.push(FunctionInstance {
            instance_addr: inst_addr,
            funcidx,
            body: func,
        });

        tracing::trace!("linking: {mod_addr} {funcidx} => {funcaddr}");
        funcaddr += 1;
    }
    store.funcs.0.append(&mut s_funcs);
    for init in &global_init {
        globals.push(vm_try!(store
            .globals
            .init(init, &globals, &funcs, &m_globals)));
    }
    let table_instances: Vec<TableInstance> = m_tables
        .iter()
        .map(|v| TableInstance(*v, vec![TABLE_UNINITIALIZED; v.limits.min as usize]))
        .collect();
    let mut table_addr = store.tables.len() as u32;
    let mut s_tables = vec![];

    for table in table_instances {
        tables.push(table_addr);
        s_tables.push(table);
        table_addr += 1;
    }
    store.tables.append(&mut s_tables);
    tracing::trace!("funcs: {funcs:?}");

    let res = (|| {
        tracing::trace!("funcs2: {funcs:?}");
        for (idx, elem) in (0u32..).zip(m_elems.0.into_iter()) {
            tracing::trace!("funcs3: {funcs:?}");
            match &elem.mode {
                ElemMode::Active(idx, offset) => match &elem.init {
                    ElemInit::FuncIdx(idxs) => {
                        let offset =
                            vm_try!(execute_offset_const_expr(store, &globals, offset)) as usize;
                        let table_addr = tables[idx.0 as usize] as usize;
                        let instance = &mut store.tables[table_addr];

                        if instance.0.reftype != elem.kind {
                            panic!("reftype mismatch")
                        }
                        if offset + idxs.len() > instance.1.len() {
                            return VMResult::TableIndexOutOfRange;
                        }
                        for (idx, funcidx) in idxs.iter().enumerate() {
                            instance.1[offset + idx] = funcs[*funcidx as usize];
                            tracing::trace!(
                                "table[{}] = {}",
                                offset + idx,
                                funcs[*funcidx as usize]
                            );
                        }
                    }
                    ElemInit::ConstExpr(idxs) => {
                        let offset =
                            vm_try!(execute_offset_const_expr(store, &globals, offset)) as usize;
                        let table_addr = tables[idx.0 as usize] as usize;
                        let Store {
                            globals: global_store,
                            tables,
                            ..
                        } = store;
                        let instance = &mut tables[table_addr];
                        if offset + idxs.len() > instance.1.len() {
                            return VMResult::TableIndexOutOfRange;
                        }
                        tracing::trace!("funcs4: {funcs:?}");

                        for (idx, idx_expr) in idxs.iter().enumerate() {
                            let addr = vm_try!(execute_elem_init_const_expr(
                                global_store,
                                &globals,
                                &funcs,
                                idx_expr,
                                instance.0.reftype,
                            ));
                            instance.1[offset + idx] = addr;
                            tracing::trace!("table[{}] = {}", offset + idx, addr);
                        }
                    }
                },
                ElemMode::Passive => {
                    store.elems.insert((inst_addr, idx), elem);
                }
                ElemMode::Declarative => {
                    //do nothing
                }
            }
        }
        VMResult::Success(())
    })();

    tracing::trace!("instance funcs: {funcs:?}");

    store.modules.push(ModuleInstance {
        function_types: fts.0,
        functions,
        data: data.0,
        exports: exs,
        tables: m_tables,
        globals: m_globals,
        mems,
    });
    let instance = Instance {
        module_addr: mod_addr,
        memory,
        tables,
        globals,
        funcs,
    };
    let addr = InstanceAddr(inst_addr);
    if let Some(start) = start {
        let mut stack = Stack::new(128 * 1024);

        let funcaddr = instance.funcs[start.0 as usize];
        store.instances.push(instance);
        vm_try!(res);

        let funcinst = &store.funcs.0[funcaddr as usize];
        let code = &funcinst.body;
        let mut jump_table = JumpTable::new();
        jump_table.push((code.expr.len() - 2) as u32);
        let mut local_size = 0usize;
        for local in &code.locals {
            local_size += local.n as usize * local.t.stack_size().usize();
        }
        let local_reference = vm_try!(stack.function_call(0, local_size, &vm::VM_END));
        let ptr = code.expr.as_ptr();

        let mut ctx = ExecuteContext {
            stack: &mut stack,
            local_state: vec![LocalState {
                jump_table,
                local_reference,
                code_addr: funcaddr,
                instance_addr: funcinst.instance_addr,
            }],
            store,
        };
        vm_try!(unsafe { vm::call_next(ptr, 0, &mut ctx) });
    } else {
        store.instances.push(instance);
        vm_try!(res);
    }
    VMResult::Success(addr)
}
// TODO:
#[allow(dead_code)]
pub fn aliasing(
    registry: &Registry,
    triplets: &[(&str, &str, &str)],
    store: &mut Store,
) -> VMResult<InstanceAddr> {
    let mod_addr = store.modules.len() as u32;
    let inst_addr: u32 = store.instances.len() as u32;
    let mut functions = vec![];
    let mut function_types = vec![];
    let mut function_addrs = vec![];
    let mut exports = vec![];
    for (modname, importname, exportname) in triplets {
        let instance_addr = vm_try!(VMResult::from_option(registry.get(modname), || {
            VMResult::Unlinkable
        }));
        let Store {
            instances: s_instances,
            modules: s_modules,
            ..
        } = store;
        let ext_instance = &s_instances[instance_addr.0 as usize];
        let ext_module = &s_modules[ext_instance.module_addr as usize];
        let export_desc = vm_try!(VMResult::from_option(
            ext_module.exports.find(importname),
            || { VMResult::Unlinkable }
        ));
        match export_desc {
            ExportDesc::Func(idx) => {
                let tidx = ext_module.functions[idx.0 as usize];
                let ft = &ext_module.function_types[tidx.0 as usize];
                let new_tidx = function_types.len();
                let new_funcidx = functions.len();
                function_types.push(ft.clone());
                functions.push(TypeIdx(new_tidx as u32));
                let addr = ext_instance.funcs[idx.0 as usize];
                function_addrs.push(addr);
                exports.push(Export(
                    (*exportname).to_owned(),
                    ExportDesc::Func(FuncIdx(new_funcidx as u32)),
                ));
            }
            _ => {}
        }
    }
    store.modules.push(ModuleInstance {
        exports: ExportSection(exports),
        tables: vec![],
        globals: vec![],
        functions,
        function_types,
        data: vec![],
        mems: vec![],
    });
    store.instances.push(Instance {
        module_addr: mod_addr,
        memory: None,
        globals: vec![],
        funcs: function_addrs,
        tables: vec![],
    });
    VMResult::Success(InstanceAddr(inst_addr))
}
