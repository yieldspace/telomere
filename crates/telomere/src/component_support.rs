/// Binary-reader traits and implementations shared with Component Model decoders.
pub mod binary {
    /// Reads bytes from a core WebAssembly or Component Model binary.
    pub use crate::binary::BinaryReader;
    /// Adapts a standard I/O reader to the binary-reader interface.
    pub use crate::binary::IoReadBinaryReader;
    /// A limited view produced by [`BinaryReader::take`].
    pub use crate::binary::LimitingBinaryReader;
}

/// Parsing helpers that Component Model crates reuse from the core parser.
pub mod parser {
    /// Core parser helpers with stable component-support signatures.
    pub mod core {
        use crate::binary::BinaryReader;
        use crate::WasmParserError;

        /// Reads one signed LEB128 `i32` and its encoded byte length.
        pub fn parse_i32<R: BinaryReader>(reader: &mut R) -> Result<(usize, i32), WasmParserError> {
            crate::parser::core::parse_i32(reader)
        }

        /// Reads a length-prefixed UTF-8 name and its encoded byte length.
        pub fn parse_name<R: BinaryReader>(
            reader: &mut R,
        ) -> Result<(usize, String), WasmParserError> {
            crate::parser::core::parse_name(reader)
        }

        /// Reads one unsigned LEB128 `u32` and its encoded byte length.
        pub fn parse_u32<R: BinaryReader>(reader: &mut R) -> Result<(usize, u32), WasmParserError> {
            crate::parser::core::parse_u32(reader)
        }

        /// Parses a vector whose reader and element parser borrow one shared environment.
        ///
        /// This wrapper preserves the core parser's byte-count convention while
        /// allowing Component Model decoders to attach their own error type.
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

    /// Const-evaluable LEB128 helpers used by generated component code.
    pub mod leb128 {
        /// Decodes a signed LEB128 `i32` from a compile-time byte array.
        pub const fn compile_i32<const N: usize>(bytes: [u8; N]) -> i32 {
            crate::parser::leb128::compile_i32(bytes)
        }
    }
}

/// Store, memory, and instance helpers shared with Component Model integrations.
pub mod common {
    /// Named core types used by component adapters without exposing parser or runtime modules.
    pub use crate::common::{
        Export, ExportDesc, FuncIdx, FuncType, GlobalIdx, GlobalType, Import, ImportDesc,
        InstanceHandle, Limits, MemArg, MemIdx, MemType, MemoryHandle, Module, Mut, RefType,
        ResultType, ResultValue, Store, StoreState, TableIdx, TableType, TypeIdx, VMResult,
        ValType, WasmValue,
    };

    /// A memory handle that remains valid only for the store that produced it.
    pub type CoreMemoryHandle = crate::common::MemoryHandle;

    /// Returns the function signatures declared by `module`.
    pub fn module_function_types(module: &crate::common::Module) -> &[crate::common::FuncType] {
        &module.fts.0
    }

    /// Returns the imports declared by `module`.
    pub fn module_imports(module: &crate::common::Module) -> &[crate::common::Import] {
        &module.imports.0
    }

    /// Returns the exports declared by `module`.
    pub fn module_exports(module: &crate::common::Module) -> &[crate::common::Export] {
        &module.exs.0
    }

    /// Returns the complete function index space of `module`.
    pub fn module_functions(module: &crate::common::Module) -> &[crate::common::TypeIdx] {
        &module.functions
    }

    /// Returns the complete table index space of `module`.
    pub fn module_tables(module: &crate::common::Module) -> &[crate::common::TableType] {
        &module.tables
    }

    /// Returns the complete memory index space of `module`.
    pub fn module_memories(module: &crate::common::Module) -> &[crate::common::MemType] {
        &module.mems
    }

    /// Returns the complete global index space of `module`.
    pub fn module_globals(module: &crate::common::Module) -> &[crate::common::GlobalType] {
        &module.globals
    }

    /// Returns the numeric ID of a handle when it belongs to `store`.
    pub fn instance_id(
        handle: &crate::common::InstanceHandle,
        store: &crate::common::Store,
    ) -> Option<u32> {
        handle.matches_store(store).then_some(handle.instance_id())
    }

    /// Looks up an exported memory and returns a handle for component adapters.
    ///
    /// The handle is store-bound. The error string distinguishes a foreign
    /// instance, a missing export, a non-memory export, and an invalid index.
    /// The `String` error is retained pending [issue #143](https://github.com/yieldspace/telomere/issues/143).
    pub fn memory_export(
        instance: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        export_name: &str,
    ) -> Result<CoreMemoryHandle, String> {
        let object_ref = instance
            .object_ref_for_store(store)
            .ok_or_else(|| "instance handle belongs to another store".to_owned())?;
        store.with_active_or_locked_runtime(|runtime| {
            let instance = runtime.get_instance(object_ref);
            let module = runtime.get_module(instance.module_addr);
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
                .map(|addr| runtime.memory_handle(addr))
                .ok_or_else(|| "memory index is out of bounds".to_owned())
        })
    }

    /// Copies a byte range from a store-owned memory.
    ///
    /// Returns `None` when the address range overflows or lies outside memory.
    pub fn read_memory(
        store: &crate::common::Store,
        memory: &CoreMemoryHandle,
        ptr: u32,
        len: usize,
    ) -> Option<Vec<u8>> {
        let len = u32::try_from(len).ok()?;
        let end = ptr.checked_add(len)? as usize;
        store.with_active_or_locked_runtime(|runtime| match *memory {
            crate::common::MemoryHandle::Local(id) => runtime
                .local_memory(id)
                .memory()
                .get(ptr as usize..end)
                .map(|bytes| bytes.to_vec()),
            crate::common::MemoryHandle::Shared(id) => runtime
                .shared_memory(id)
                .with_memory(|memory| memory.get(ptr as usize..end).map(|bytes| bytes.to_vec())),
        })
    }

    /// Copies exactly `N` bytes from a store-owned memory.
    ///
    /// Returns `None` when the requested range overflows or lies outside memory.
    pub fn read_memory_array<const N: usize>(
        store: &crate::common::Store,
        memory: &CoreMemoryHandle,
        ptr: u32,
    ) -> Option<[u8; N]> {
        let len = u32::try_from(N).ok()?;
        let end = ptr.checked_add(len)? as usize;
        let mut out = [0u8; N];
        store.with_active_or_locked_runtime(|runtime| match *memory {
            crate::common::MemoryHandle::Local(id) => {
                let bytes = runtime.local_memory(id).memory().get(ptr as usize..end)?;
                out.copy_from_slice(bytes);
                Some(out)
            }
            crate::common::MemoryHandle::Shared(id) => {
                runtime.shared_memory(id).with_memory(|memory| {
                    let bytes = memory.get(ptr as usize..end)?;
                    out.copy_from_slice(bytes);
                    Some(out)
                })
            }
        })
    }

    /// Writes `bytes` into a store-owned memory.
    ///
    /// Returns `false` when the range overflows or lies outside memory.
    pub fn write_memory(
        store: &crate::common::Store,
        memory: &CoreMemoryHandle,
        ptr: u32,
        bytes: &[u8],
    ) -> bool {
        let Some(len) = u32::try_from(bytes.len()).ok() else {
            return false;
        };
        let Some(end) = ptr.checked_add(len).map(|it| it as usize) else {
            return false;
        };
        store.with_active_or_locked_runtime(|runtime| match *memory {
            crate::common::MemoryHandle::Local(id) => {
                let Some(slot) = runtime
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
                runtime.shared_memory(id).with_memory(|memory| {
                    let Some(slot) = memory.get_mut(ptr as usize..end) else {
                        return false;
                    };
                    slot.copy_from_slice(bytes);
                    true
                })
            }
        })
    }
}

/// Runtime entry points reused by Component Model adapters.
pub mod runtime {
    /// Builds a module that aliases imports from registered instances.
    pub use crate::runtime::aliasing;
    /// Instantiates a parsed core module.
    pub use crate::runtime::instantiate;
    /// Calls a named exported core function with the default driver.
    pub use crate::runtime::run_module_function;
    /// Ordered values supplied to or returned from core function calls.
    pub use crate::runtime::ResultValue;

    /// Calls an exported core function while the same store is already active.
    ///
    /// Component adapters use this narrow synchronous bridge only during an
    /// explicitly authorized reentrant host callback. Ordinary embeddings should
    /// use [`run_module_function`] instead.
    /// The `String` error is retained pending [issue #143](https://github.com/yieldspace/telomere/issues/143).
    pub fn run_core_export_sync_reentrant(
        instance: &crate::common::InstanceHandle,
        store: &crate::common::Store,
        name: &str,
        args: &crate::runtime::ResultValue,
    ) -> Result<crate::common::VMResult<crate::runtime::ResultValue>, String> {
        store.with_active_or_locked_runtime(|store_runtime| {
            crate::runtime::vm::run_module_function_sync_with_runtime(
                instance,
                store,
                store_runtime,
                name,
                args,
            )
            .map_err(|error| format!("{error:?}"))
        })
    }
}
