use crate::ir::types::{DefValType, PrimValType, Type, ValType};
use crate::ir::{ComponentExport, ComponentImport};
use crate::runtime;
use crate::support::binary::IoReadBinaryReader;
use crate::support::Store;
use crate::validate::{ParseState, Validator};
use crate::{
    ComponentError, ComponentInstance, ComponentLinker, ComponentOp, ComponentProgram,
    ComponentTypeInfo,
};

/// Compiles Component Model binaries and instantiates their exported functions.
///
/// A `ComponentEngine` is stateless and cheap to create. Compilation validates a
/// component and creates a reusable [`ComponentProgram`]; instantiation binds
/// that program to a [`Store`] and [`ComponentLinker`].
///
/// # Examples
///
/// The executor below uses only `std` and mirrors the complete standalone
/// example in `examples/minimal-embedder/`; do not use a nested
/// `futures::executor::block_on` for component calls.
///
/// ```
/// use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue, Store};
/// # use std::{future::Future, sync::Arc, task::{Context, Poll, Wake, Waker}};
/// # struct ThreadWaker(std::thread::Thread);
/// # impl Wake for ThreadWaker {
/// #     fn wake(self: Arc<Self>) { self.0.unpark(); }
/// #     fn wake_by_ref(self: &Arc<Self>) { self.0.unpark(); }
/// # }
/// # fn block_on<F: Future>(future: F) -> F::Output {
/// #     let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
/// #     let mut context = Context::from_waker(&waker);
/// #     let mut future = std::pin::pin!(future);
/// #     loop {
/// #         match future.as_mut().poll(&mut context) {
/// #             Poll::Ready(output) => return output,
/// #             Poll::Pending => std::thread::park(),
/// #         }
/// #     }
/// # }
/// # fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let bytes = std::fs::read(concat!(
///     env!("CARGO_MANIFEST_DIR"),
///     "/../../examples/component-add.wasm",
/// ))?;
/// let engine = ComponentEngine::new();
/// let program = engine.compile(&bytes)?;
/// let store = Store::new();
/// let linker = ComponentLinker::new();
/// let instance = block_on(engine.instantiate(&program, &store, &linker))?;
/// let result = block_on(instance.call(
///     &store,
///     "add",
///     &[ComponentValue::S32(20), ComponentValue::S32(22)],
/// ))?;
/// assert_eq!(result, vec![ComponentValue::S32(42)]);
/// # Ok(())
/// # }
/// # run().unwrap();
/// ```
#[derive(Default, Debug, Clone, Copy)]
pub struct ComponentEngine;

impl ComponentEngine {
    /// Creates a stateless engine ready to compile Component Model binaries.
    pub fn new() -> Self {
        Self
    }

    /// Validates `bytes` and returns the reusable program representation.
    ///
    /// The returned program owns a copy of the component bytes and its decoded
    /// type metadata. Use [`Self::instantiate`] to bind it to a store and host
    /// imports. Invalid binaries return decode or validation variants of
    /// [`ComponentError`].
    pub fn compile(&self, bytes: &[u8]) -> Result<ComponentProgram, ComponentError> {
        let mut reader = IoReadBinaryReader::from(bytes);
        let state_arena = typed_arena::Arena::new();
        let mut state = ParseState::new(&state_arena);
        let validator_arena = typed_arena::Arena::new();
        let mut validator = Validator::new(&validator_arena);

        crate::decoder::parse_component(&mut reader, &mut state, &mut validator)?;

        let scope = state.scope();
        let root = scope.make_component();

        let mut imports = Vec::with_capacity(scope.imports.len());
        let mut callable_imports = Vec::new();
        for (name, import) in &scope.imports {
            imports.push(name.clone());
            if matches!(import, ComponentImport::Func(_)) {
                callable_imports.push(name.clone());
            }
        }

        let mut exports = Vec::with_capacity(scope.exports.len());
        let mut callable_exports = Vec::new();
        let mut ops = Vec::with_capacity(scope.exports.len());

        for (name, export) in &scope.exports {
            exports.push(name.clone());
            match export {
                ComponentExport::Func { .. } => {
                    callable_exports.push(name.clone());
                    ops.push(ComponentOp::CanonLift { func_idx: 0 });
                }
                _ => {
                    ops.push(ComponentOp::Export { name: name.clone() });
                }
            }
        }
        imports.shrink_to_fit();
        callable_imports.shrink_to_fit();
        exports.shrink_to_fit();
        callable_exports.shrink_to_fit();
        ops.shrink_to_fit();

        let types = validator.snapshot_types();
        let mut type_infos = build_type_infos(types.as_ref())?;
        type_infos.shrink_to_fit();

        Ok(ComponentProgram {
            type_infos,
            imports,
            callable_imports,
            exports,
            callable_exports,
            ops,
            bytes: bytes.to_vec(),
            root,
            types,
            component_store: state.component_store.snapshot(),
            instance_store: state.instance_store.snapshot(),
            func_store: state.func_store.snapshot(),
            core_module_store: state.core_module_store.snapshot(),
            core_type_store: state.core_type_store.snapshot(),
            core_instance_store: state.core_instance_store.snapshot(),
            core_func_store: state.core_func_store.snapshot(),
            core_memory_store: state.core_memory_store.snapshot(),
            core_global_store: state.core_global_store.snapshot(),
            core_table_store: state.core_table_store.snapshot(),
        })
    }

    /// Instantiates a compiled component with the supplied store and linker.
    ///
    /// Import resolution happens while the returned future is polled. The
    /// resulting [`ComponentInstance`] keeps the program alive and can call its
    /// root exports or traverse nested export instances.
    pub async fn instantiate(
        &self,
        program: &ComponentProgram,
        store: &Store,
        linker: &ComponentLinker,
    ) -> Result<ComponentInstance, ComponentError> {
        runtime::instantiate(program.clone(), store, linker).await
    }
}

fn build_type_infos(types: &[Type]) -> Result<Vec<ComponentTypeInfo>, ComponentError> {
    fn align_to(value: u32, align: u32) -> u32 {
        if align <= 1 {
            value
        } else {
            let rem = value % align;
            if rem == 0 {
                value
            } else {
                value + (align - rem)
            }
        }
    }

    fn variant_discriminant_size(case_count: usize) -> Result<u32, ComponentError> {
        if case_count <= (1 << 8) {
            Ok(1)
        } else if case_count <= (1 << 16) {
            Ok(2)
        } else if (case_count as u64) <= (1 << 32) {
            Ok(4)
        } else {
            Err(ComponentError::Unsupported(
                "variants with more than 2^32 cases are not supported".to_owned(),
            ))
        }
    }

    fn flags_info(count: usize) -> (usize, u32, u32) {
        if count == 0 {
            (0, 0, 1)
        } else if count <= 8 {
            (1, 1, 1)
        } else if count <= 16 {
            (1, 2, 2)
        } else {
            let words = count.div_ceil(32);
            (words, 4 * words as u32, 4)
        }
    }

    fn primitive_info(prim: &PrimValType) -> ComponentTypeInfo {
        match prim {
            PrimValType::Bool | PrimValType::S8 | PrimValType::U8 => ComponentTypeInfo {
                id: 0,
                flat_len: 1,
                indirect_size: 1,
                indirect_align: 1,
                fixed_length: None,
            },
            PrimValType::S16 | PrimValType::U16 => ComponentTypeInfo {
                id: 0,
                flat_len: 1,
                indirect_size: 2,
                indirect_align: 2,
                fixed_length: None,
            },
            PrimValType::S32 | PrimValType::U32 | PrimValType::Char | PrimValType::F32 => {
                ComponentTypeInfo {
                    id: 0,
                    flat_len: 1,
                    indirect_size: 4,
                    indirect_align: 4,
                    fixed_length: None,
                }
            }
            PrimValType::S64 | PrimValType::U64 | PrimValType::F64 => ComponentTypeInfo {
                id: 0,
                flat_len: 1,
                indirect_size: 8,
                indirect_align: 8,
                fixed_length: None,
            },
            PrimValType::String => ComponentTypeInfo {
                id: 0,
                flat_len: 2,
                indirect_size: 8,
                indirect_align: 4,
                fixed_length: None,
            },
            #[cfg(feature = "component-gated-feature-async")]
            PrimValType::ErrorContext => ComponentTypeInfo {
                id: 0,
                flat_len: 1,
                indirect_size: 4,
                indirect_align: 4,
                fixed_length: None,
            },
        }
    }

    fn valtype_info(
        ty: &ValType,
        types: &[Type],
        memo: &mut [Option<ComponentTypeInfo>],
        visiting: &mut [bool],
    ) -> Result<ComponentTypeInfo, ComponentError> {
        match ty {
            ValType::Primitive(prim) => Ok(primitive_info(prim)),
            ValType::Type(type_id) => type_info(*type_id, types, memo, visiting),
        }
    }

    fn type_info(
        type_id: crate::ir::TypeId,
        types: &[Type],
        memo: &mut [Option<ComponentTypeInfo>],
        visiting: &mut [bool],
    ) -> Result<ComponentTypeInfo, ComponentError> {
        let index = type_id.index() as usize;
        if let Some(info) = memo.get(index).and_then(|slot| slot.clone()) {
            return Ok(info);
        }
        if *visiting
            .get(index)
            .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
        {
            return Err(ComponentError::Unsupported(
                "recursive canonical ABI metadata is not implemented".to_owned(),
            ));
        }
        visiting[index] = true;
        let info = match types
            .get(index)
            .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
        {
            Type::DefVal(DefValType::Primitive(prim)) => primitive_info(prim),
            Type::DefVal(DefValType::Record(fields)) => {
                let mut flat_len = 0usize;
                let mut size = 0u32;
                let mut align = 1u32;
                for field in fields {
                    let info = valtype_info(&field.ty, types, memo, visiting)?;
                    flat_len += info.flat_len;
                    align = align.max(info.indirect_align);
                    size = align_to(size, info.indirect_align);
                    size = size.saturating_add(info.indirect_size);
                }
                ComponentTypeInfo {
                    id: type_id.index(),
                    flat_len,
                    indirect_size: align_to(size, align.max(1)),
                    indirect_align: align.max(1),
                    fixed_length: None,
                }
            }
            Type::DefVal(DefValType::Variant(cases)) => {
                let discriminant_size = variant_discriminant_size(cases.len())?;
                let mut flat_len = 1usize;
                let mut payload_size = 0u32;
                let mut payload_align = discriminant_size;
                let mut payload_flat = 0usize;
                for case in cases {
                    if let Some(ty) = &case.ty {
                        let info = valtype_info(ty, types, memo, visiting)?;
                        payload_flat = payload_flat.max(info.flat_len);
                        payload_size = payload_size.max(info.indirect_size);
                        payload_align = payload_align.max(info.indirect_align);
                    }
                }
                flat_len += payload_flat;
                let align = payload_align.max(1);
                let payload_offset = align_to(discriminant_size, align);
                ComponentTypeInfo {
                    id: type_id.index(),
                    flat_len,
                    indirect_size: align_to(payload_offset.saturating_add(payload_size), align),
                    indirect_align: align,
                    fixed_length: None,
                }
            }
            Type::DefVal(DefValType::Flags(labels)) => {
                let (flat_len, indirect_size, indirect_align) = flags_info(labels.len());
                ComponentTypeInfo {
                    id: type_id.index(),
                    flat_len,
                    indirect_size,
                    indirect_align,
                    fixed_length: Some(labels.len() as u32),
                }
            }
            Type::DefVal(DefValType::List(elem, maybe_len)) => {
                let elem_info = valtype_info(elem, types, memo, visiting)?;
                let _stride = align_to(elem_info.indirect_size, elem_info.indirect_align.max(1));
                let fixed = maybe_len.map(|len| len as u32);
                ComponentTypeInfo {
                    id: type_id.index(),
                    flat_len: 2,
                    indirect_size: 8,
                    indirect_align: 4,
                    fixed_length: fixed,
                }
            }
            Type::DefVal(DefValType::Own(_))
            | Type::DefVal(DefValType::Borrow(_))
            | Type::Resource(_)
            | Type::Generic(_) => ComponentTypeInfo {
                id: type_id.index(),
                flat_len: 1,
                indirect_size: 4,
                indirect_align: 4,
                fixed_length: None,
            },
            Type::Func(_) | Type::Component(_) | Type::Instance(_) => ComponentTypeInfo {
                id: type_id.index(),
                flat_len: 0,
                indirect_size: 0,
                indirect_align: 1,
                fixed_length: None,
            },
        };
        visiting[index] = false;
        memo[index] = Some(info.clone());
        Ok(info)
    }

    let mut memo = vec![None; types.len()];
    let mut visiting = vec![false; types.len()];
    (0..types.len())
        .map(|index| {
            let type_id = crate::ir::TypeId::from_index(index as u32);
            type_info(type_id, types, &mut memo, &mut visiting)
        })
        .collect()
}

#[cfg(all(test, feature = "component-gated-feature-async"))]
mod tests {
    use super::*;

    #[test]
    fn error_context_uses_i32_handle_layout() {
        let infos = build_type_infos(&[Type::DefVal(DefValType::Primitive(
            PrimValType::ErrorContext,
        ))])
        .expect("error-context metadata should be representable");
        let info = &infos[0];

        assert_eq!(info.flat_len, 1);
        assert_eq!(info.indirect_size, 4);
        assert_eq!(info.indirect_align, 4);
        assert_eq!(info.fixed_length, None);
    }
}
