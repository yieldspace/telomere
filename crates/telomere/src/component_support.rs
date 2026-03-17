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

    pub mod gc {
        pub use crate::common::gc::{GcRef, MemoryPool};
    }

    pub fn instance_gc_ref(
        handle: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        pool: &crate::common::gc::MemoryPool,
    ) -> Option<crate::common::gc::GcRef> {
        handle.get_gc_ref_with_pool(store, pool)
    }

    pub fn instance_id(
        handle: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        pool: &crate::common::gc::MemoryPool,
    ) -> Option<u32> {
        let gc_ref = handle.get_gc_ref_with_pool(store, pool)?;
        Some(unsafe { (*pool.get_instance_unchecked(gc_ref)).instance_id })
    }

    pub fn memory_export_addr(
        instance: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        export_name: &str,
        gc: &mut crate::common::gc::MemoryPool,
    ) -> Result<crate::common::gc::GcRef, String> {
        let gc_ref = instance
            .get_gc_ref_with_pool(store, gc)
            .ok_or_else(|| "instance handle belongs to another store".to_owned())?;
        let instance = unsafe { &*gc.get_instance_unchecked(gc_ref) };
        let module = unsafe { gc.get_module(instance.module_addr) };
        let crate::common::ExportDesc::Mem(idx) = module
            .exports
            .find(export_name)
            .ok_or_else(|| format!("memory export '{export_name}' is missing"))?
        else {
            return Err(format!("export '{export_name}' is not a memory"));
        };
        instance
            .mems
            .as_slice(gc)
            .get(idx.0 as usize)
            .copied()
            .ok_or_else(|| "memory index is out of bounds".to_owned())
    }

    pub fn read_memory(
        gc: &mut crate::common::gc::MemoryPool,
        addr: crate::common::gc::GcRef,
        ptr: u32,
        len: usize,
    ) -> Option<Vec<u8>> {
        let memory = unsafe { gc.get_memory(addr) };
        let end = ptr.checked_add(len as u32)? as usize;
        memory.get(ptr as usize..end).map(|bytes| bytes.to_vec())
    }

    pub fn write_memory(
        gc: &mut crate::common::gc::MemoryPool,
        addr: crate::common::gc::GcRef,
        ptr: u32,
        bytes: &[u8],
    ) -> bool {
        let memory = unsafe { gc.get_memory(addr) };
        let Some(end) = ptr.checked_add(bytes.len() as u32).map(|it| it as usize) else {
            return false;
        };
        let Some(slot) = memory.get_mut(ptr as usize..end) else {
            return false;
        };
        slot.copy_from_slice(bytes);
        true
    }
}

pub mod runtime {
    pub use crate::runtime::{
        aliasing, instantiate, instantiate_native_module, run_module_function, ResultValue,
    };

    pub fn run_module_function_sync_with_gc(
        instance: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        gc: &mut crate::common::gc::MemoryPool,
        name: &str,
        args: &crate::runtime::ResultValue,
    ) -> Result<crate::common::VMResult<crate::runtime::ResultValue>, String> {
        crate::runtime::vm::run_module_function_sync_with_gc(instance, store, gc, name, args)
            .map_err(|error| format!("{error:?}"))
    }
}
