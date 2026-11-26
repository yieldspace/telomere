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
/// # Executor requirements
///
/// The futures returned by this API may be driven by any executor: a
/// tokio runtime, `futures::executor::block_on`, or a minimal
/// hand-written executor. The executor below uses only `std` and mirrors
/// the complete standalone example in `examples/minimal-embedder/`.
/// Instantiation drives core modules on the current thread without
/// entering the caller's executor, so nested executors never panic with
/// an executor re-entry error.
///
/// One requirement remains: a host function reached through synchronous
/// execution must complete on its first poll. If one returns
/// `Poll::Pending`, the call fails with a returned [`ComponentError`]
/// instead of panicking.
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

        self.build_program(&state, &mut validator, bytes)
    }

    /// Decodes `bytes` with a bundle substitution session (#201)
    /// installed on the parse state, returning the program and the
    /// consumed session so the caller can key the opened artifacts to
    /// the program's core module indices.
    #[cfg(feature = "aot-bundle")]
    pub(crate) fn compile_with_substitution(
        &self,
        bytes: &[u8],
        substitution: crate::bundle::CoreModuleSubstitution,
    ) -> Result<(ComponentProgram, crate::bundle::CoreModuleSubstitution), ComponentError> {
        let mut reader = IoReadBinaryReader::from(bytes);
        let state_arena = typed_arena::Arena::new();
        let mut state = ParseState::new(&state_arena);
        state.set_core_module_substitution(substitution);
        let validator_arena = typed_arena::Arena::new();
        let mut validator = Validator::new(&validator_arena);

        crate::decoder::parse_component(&mut reader, &mut state, &mut validator)?;

        let program = self.build_program(&state, &mut validator, bytes)?;
        let substitution = state
            .take_core_module_substitution()
            .expect("substitution session installed above");
        Ok((program, substitution))
    }

    /// Loads a component bundle (#201): verifies the bundle
    /// signature, pre-checks the keyed artifact set against the
    /// embedded component's core module sections, opens the
    /// artifacts, and decodes the component with per-section
    /// `parse_module` calls substituted by the artifacts'
    /// deserialized modules. See [`crate::bundle`] for the format
    /// and the fail-closed load order.
    #[cfg(feature = "aot-bundle")]
    pub fn compile_bundle(
        &self,
        bytes: &[u8],
        verifier: Option<&dyn telomere::AotVerifier>,
        policy: telomere::AotLoadPolicy,
    ) -> Result<crate::bundle::LoadedComponentBundle, crate::bundle::BundleError> {
        let envelope = crate::bundle::parse_bundle_envelope(bytes)?;
        crate::bundle::verify_bundle_signature(bytes, &envelope, verifier, policy)?;
        let opened = crate::bundle::open_bundle_artifacts(&envelope)?;
        let sections = crate::bundle::enumerate_core_module_sections(envelope.component)?;
        let substitution = crate::bundle::CoreModuleSubstitution::new(opened);
        let (program, substitution) =
            self.compile_with_substitution(envelope.component, substitution)?;
        let (used, substituted_ordinals) = substitution.into_outcome();
        let parsed_ordinals = sections
            .iter()
            .map(|section| section.ordinal)
            .filter(|ordinal| !substituted_ordinals.contains(ordinal))
            .collect();
        let report = crate::bundle::BundleLoadReport {
            core_module_sections: sections.len() as u32,
            substituted_ordinals,
            parsed_ordinals,
        };
        Ok(crate::bundle::LoadedComponentBundle::new(
            program,
            crate::bundle::ComponentBundleArtifacts::new(used),
            report,
            envelope.key_id.clone(),
            envelope.signature.is_empty(),
        ))
    }

    /// Instantiates a component loaded from a bundle (#201) with the
    /// supplied store and linker. Defined core instances created from
    /// substituted modules get their bundle artifacts installed into
    /// the store's JIT cache; the reports accumulate on
    /// [`crate::bundle::LoadedComponentBundle::artifacts`].
    #[cfg(feature = "aot-bundle")]
    pub async fn instantiate_bundle(
        &self,
        bundle: &crate::bundle::LoadedComponentBundle,
        store: &Store,
        linker: &ComponentLinker,
    ) -> Result<ComponentInstance, ComponentError> {
        runtime::instantiate_with_artifacts(
            bundle.program().clone(),
            bundle.artifacts(),
            store,
            linker,
        )
        .await
    }

    fn build_program(
        &self,
        state: &ParseState,
        validator: &mut Validator,
        bytes: &[u8],
    ) -> Result<ComponentProgram, ComponentError> {
        let scope = state.scope();
        let root = scope.make_component(&state.core_instance_store, &state.instance_store);

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
            value_store: state.value_store.snapshot(),
        })
    }

    /// Instantiates a compiled component with the supplied store and linker.
    ///
    /// Import resolution happens while the returned future is polled. The
    /// resulting [`ComponentInstance`] keeps the program alive and can call its
    /// root exports or traverse nested export instances.
    ///
    /// The returned future may be driven by any executor, including
    /// `futures::executor::block_on` (see the executor requirements on
    /// [`ComponentEngine`]); instantiation never enters the caller's
    /// executor, so driving it from inside another executor cannot panic
    /// with an executor re-entry error.
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
