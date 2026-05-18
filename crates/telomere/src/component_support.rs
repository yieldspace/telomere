pub mod binary {
    pub use crate::binary::{BinaryReader, IoReadBinaryReader};
}

pub mod parser {
    pub mod core {
        use crate::binary::BinaryReader;
        use crate::WasmParserError;

        pub fn parse_i32<R: BinaryReader>(reader: &mut R) -> Result<(usize, i32), WasmParserError> {
            crate::parser::core::parse_i32(reader)
        }

        pub fn parse_name<R: BinaryReader>(
            reader: &mut R,
        ) -> Result<(usize, String), WasmParserError> {
            crate::parser::core::parse_name(reader)
        }

        pub fn parse_u32<R: BinaryReader>(reader: &mut R) -> Result<(usize, u32), WasmParserError> {
            crate::parser::core::parse_u32(reader)
        }

        pub fn parse_vec<A, R: BinaryReader, F, G, V, E>(
            env: &mut A,
            reader: F,
            f: G,
        ) -> Result<(usize, Vec<V>), E>
        where
            E: From<WasmParserError>,
            F: for<'b> FnOnce(&'b mut A) -> &'b mut R,
            G: for<'b> FnMut(&'b mut A) -> Result<(usize, V), E>,
        {
            crate::parser::core::parse_vec(env, reader, f)
        }
    }

    pub mod leb128 {
        pub const fn compile_i32<const N: usize>(bytes: [u8; N]) -> i32 {
            crate::parser::leb128::compile_i32(bytes)
        }
    }
}

pub mod common {
    pub use crate::common::*;

    pub type CoreMemoryHandle = crate::common::MemoryHandle;

    pub fn instance_id(
        handle: &crate::common::InstanceHandle,
        store: &crate::common::Store,
    ) -> Option<u32> {
        handle.matches_store(store).then_some(handle.instance_id())
    }

    pub fn memory_export(
        instance: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        export_name: &str,
    ) -> Result<CoreMemoryHandle, String> {
        let object_ref = instance
            .object_ref_for_store(store)
            .ok_or_else(|| "instance handle belongs to another store".to_owned())?;
        store
            .with_active_runtime(|gc| {
                let instance = gc.get_instance(object_ref);
                let module = gc.get_module(instance.module_addr);
                let crate::common::ExportDesc::Mem(idx) = module
                    .exports
                    .find(export_name)
                    .ok_or_else(|| format!("memory export '{export_name}' is missing"))?
                else {
                    return Err(format!("export '{export_name}' is not a memory"));
                };
                instance
                    .mems
                    .get(idx.0 as usize)
                    .copied()
                    .map(|addr| gc.memory_handle(addr))
                    .ok_or_else(|| "memory index is out of bounds".to_owned())
            })
            .unwrap_or_else(|| {
                let gc = store.lock_gc();
                let instance = gc.get_instance(object_ref);
                let module = gc.get_module(instance.module_addr);
                let crate::common::ExportDesc::Mem(idx) = module
                    .exports
                    .find(export_name)
                    .ok_or_else(|| format!("memory export '{export_name}' is missing"))?
                else {
                    return Err(format!("export '{export_name}' is not a memory"));
                };
                instance
                    .mems
                    .get(idx.0 as usize)
                    .copied()
                    .map(|addr| gc.memory_handle(addr))
                    .ok_or_else(|| "memory index is out of bounds".to_owned())
            })
    }

    pub fn read_memory(
        store: &crate::common::Store,
        memory: &CoreMemoryHandle,
        ptr: u32,
        len: usize,
    ) -> Option<Vec<u8>> {
        let end = ptr.checked_add(len as u32)? as usize;
        store
            .with_active_runtime(|gc| match *memory {
                crate::common::MemoryHandle::Local(id) => gc
                    .local_memory(id)
                    .memory()
                    .get(ptr as usize..end)
                    .map(|bytes| bytes.to_vec()),
                crate::common::MemoryHandle::Shared(id) => {
                    gc.shared_memory(id).with_memory(|memory| {
                        memory.get(ptr as usize..end).map(|bytes| bytes.to_vec())
                    })
                }
            })
            .unwrap_or_else(|| {
                let gc = store.lock_gc();
                match *memory {
                    crate::common::MemoryHandle::Local(id) => gc
                        .local_memory(id)
                        .memory()
                        .get(ptr as usize..end)
                        .map(|bytes| bytes.to_vec()),
                    crate::common::MemoryHandle::Shared(id) => {
                        gc.shared_memory(id).with_memory(|memory| {
                            memory.get(ptr as usize..end).map(|bytes| bytes.to_vec())
                        })
                    }
                }
            })
    }

    pub fn read_memory_array<const N: usize>(
        store: &crate::common::Store,
        memory: &CoreMemoryHandle,
        ptr: u32,
    ) -> Option<[u8; N]> {
        let len = u32::try_from(N).ok()?;
        let end = ptr.checked_add(len)? as usize;
        let mut out = [0u8; N];
        store
            .with_active_runtime(|gc| match *memory {
                crate::common::MemoryHandle::Local(id) => {
                    let bytes = gc.local_memory(id).memory().get(ptr as usize..end)?;
                    out.copy_from_slice(bytes);
                    Some(out)
                }
                crate::common::MemoryHandle::Shared(id) => {
                    gc.shared_memory(id).with_memory(|memory| {
                        let bytes = memory.get(ptr as usize..end)?;
                        out.copy_from_slice(bytes);
                        Some(out)
                    })
                }
            })
            .unwrap_or_else(|| {
                let gc = store.lock_gc();
                match *memory {
                    crate::common::MemoryHandle::Local(id) => {
                        let bytes = gc.local_memory(id).memory().get(ptr as usize..end)?;
                        out.copy_from_slice(bytes);
                        Some(out)
                    }
                    crate::common::MemoryHandle::Shared(id) => {
                        gc.shared_memory(id).with_memory(|memory| {
                            let bytes = memory.get(ptr as usize..end)?;
                            out.copy_from_slice(bytes);
                            Some(out)
                        })
                    }
                }
            })
    }

    pub fn write_memory(
        store: &crate::common::Store,
        memory: &CoreMemoryHandle,
        ptr: u32,
        bytes: &[u8],
    ) -> bool {
        let Some(end) = ptr.checked_add(bytes.len() as u32).map(|it| it as usize) else {
            return false;
        };
        store
            .with_active_runtime(|gc| match *memory {
                crate::common::MemoryHandle::Local(id) => {
                    let Some(slot) = gc
                        .local_memory_mut(id)
                        .memory_mut()
                        .get_mut(ptr as usize..end)
                    else {
                        return false;
                    };
                    slot.copy_from_slice(bytes);
                    true
                }
                crate::common::MemoryHandle::Shared(id) => {
                    gc.shared_memory(id).with_memory(|memory| {
                        let Some(slot) = memory.get_mut(ptr as usize..end) else {
                            return false;
                        };
                        slot.copy_from_slice(bytes);
                        true
                    })
                }
            })
            .unwrap_or_else(|| {
                let mut gc = store.lock_gc();
                match *memory {
                    crate::common::MemoryHandle::Local(id) => {
                        let Some(slot) = gc
                            .local_memory_mut(id)
                            .memory_mut()
                            .get_mut(ptr as usize..end)
                        else {
                            return false;
                        };
                        slot.copy_from_slice(bytes);
                        true
                    }
                    crate::common::MemoryHandle::Shared(id) => {
                        gc.shared_memory(id).with_memory(|memory| {
                            let Some(slot) = memory.get_mut(ptr as usize..end) else {
                                return false;
                            };
                            slot.copy_from_slice(bytes);
                            true
                        })
                    }
                }
            })
    }
}

pub mod runtime {
    pub use crate::runtime::{
        aliasing, instantiate, instantiate_native_async_module, instantiate_native_module,
        link_async_host_function_with_export_name, link_async_host_function_with_function_idx,
        run_module_function, ResultValue,
    };

    pub fn run_core_export_sync_reentrant(
        instance: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        name: &str,
        args: &crate::runtime::ResultValue,
    ) -> Result<crate::common::VMResult<crate::runtime::ResultValue>, String> {
        store
            .with_active_runtime(|gc| {
                crate::runtime::vm::run_module_function_sync_with_gc(
                    instance, store, gc, name, args,
                )
                .map_err(|error| format!("{error:?}"))
            })
            .unwrap_or_else(|| {
                let mut gc = store.lock_gc();
                crate::runtime::vm::run_module_function_sync_with_gc(
                    instance, store, &mut gc, name, args,
                )
                .map_err(|error| format!("{error:?}"))
            })
    }
}
