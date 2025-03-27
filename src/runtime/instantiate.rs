use std::{cell::RefCell, rc::Rc};

use crate::{
    common::{
        ConstExpr, DataMode, ElemMode, ExportDesc, ImportDesc, Limits, Memory, TableInstance,
        PAGE_SIZE_MAX,
    },
    Instance, Module, Registry, Store, VMResult,
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
fn execute_const_expr(store: &mut Store, globals: &[u32], exprs: &[ConstExpr]) -> VMResult<u32> {
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
    return VMResult::Unlinkable;
}
pub fn instantiate(m: &Module, store: &mut Store, registry: &Registry) -> VMResult<Instance> {
    let mut memory: Option<Rc<RefCell<Memory>>> = None;

    let mut globals = vec![];
    for import in &m.imports.0 {
        tracing::trace!("{import:?}");
        let (ext_module, ext_inst) =
            vm_try!(VMResult::from_option(registry.get(&import.module), || {
                VMResult::Unlinkable
            }));
        let export = vm_try!(VMResult::from_option(
            ext_module.exs.find(&import.name),
            || { VMResult::Unlinkable }
        ));
        match (&import.desc, export) {
            (ImportDesc::TypeIdx(tidx), ExportDesc::Func(funcidx)) => {
                let import_ft = m.fts.get(*tidx).unwrap();
                let export_ft_idx = ext_module.functions[funcidx.0 as usize];
                let export_ft = ext_module.fts.get(export_ft_idx).unwrap();
                if import_ft != export_ft {
                    tracing::trace!("import function type");

                    return VMResult::Unlinkable;
                }
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
                vm_try!(validate_limit(
                    import_tt.limits,
                    /*FIXME:*/ export_tt.limits.min,
                    export_tt.limits
                ))
            }
            (ImportDesc::MemType(mt), ExportDesc::Mem(_idx)) => {
                memory = ext_inst.memory.clone();
                if let Some(memory) = &memory {
                    vm_try!(validate_limit(
                        mt.0,
                        memory.borrow().page_size(),
                        ext_module.mems[0].0
                    ))
                } else {
                    tracing::trace!("invalid instance memory");
                    return VMResult::Unlinkable;
                }
            }
            // TODO: import other type objects
            _ => {
                tracing::trace!("import other type objects");

                return VMResult::Unlinkable;
            }
        }
    }
    if memory.is_none() {
        if let Some(mem) = m.mems.first() {
            memory = Some(Rc::new(RefCell::new(Memory::new(
                mem.0.min,
                (mem.0.max).unwrap_or(PAGE_SIZE_MAX as u32),
            ))))
        }
    }

    for d in &m.data.0 {
        match &d.mode {
            DataMode::Active(mem, offset) => {
                assert_eq!(mem.0, 0);
                let offset = vm_try!(execute_const_expr(store, &globals, &offset)) as usize;
                if let Some(memory) = &memory {
                    if let Some(slice) = memory.borrow_mut().get_mut(offset..offset + d.init.len())
                    {
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
    let mut table: Vec<TableInstance> = m
        .tables
        .iter()
        .map(|v| TableInstance(*v, vec![TABLE_UNINITIALIZED; v.limits.min as usize]))
        .collect();
    for elem in &m.elems.0 {
        match &elem.mode {
            ElemMode::Active(idx, offset) => {
                let offset = vm_try!(execute_const_expr(store, &globals, &offset)) as usize;
                let instance = table.get_mut(idx.0 as usize).unwrap();
                if instance.0.reftype != elem.kind {
                    panic!("reftype mismatch")
                }
                let expected_len = offset + elem.init.len();
                if instance.1.len() < expected_len {
                    instance.1.resize(expected_len, TABLE_UNINITIALIZED);
                }

                for (idx, e) in elem.init.iter().enumerate() {
                    instance.1[offset + idx] = *e;
                }
            }
            _ => {
                // do nothing
            }
        }
    }
    for init in &m.global_init {
        globals.push(vm_try!(store.globals.init(init)));
    }
    let instance = Instance {
        memory,
        table,
        globals,
    };
    VMResult::Success(instance)
}
