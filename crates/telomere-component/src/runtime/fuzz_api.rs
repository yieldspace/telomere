//! Feature-gated canonical ABI fuzzing adapter.
//!
//! The adapter owns a per-thread core memory and delegates lifting to the
//! production canonical ABI implementation. It compiles the supplied component
//! fixture for type metadata only; it never instantiates the component fixture
//! or executes guest code.

use super::canonical::{lift_component_args, program_func_type};
use super::{CoreExportRef, RuntimeCanonicalOptions};
use crate::ir::CanonicalStringEncoding;
use crate::support::binary::IoReadBinaryReader;
use crate::support::{instantiate, Registry, Store, VMResult};
use crate::{ComponentEngine, ComponentError, ComponentProgram, ComponentValue};
use futures::executor::block_on;
use std::cell::RefCell;
use telomere::{WasmParser, WasmValue};

const MEMORY_BYTES: usize = 64 * 1024;

// `(module (memory 1) (export "mem" (memory 0)))`
//
// There are no code, start, data, or element sections in this module.
const MEMORY_MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic and version
    0x05, 0x03, 0x01, 0x00, 0x01, // one minimum-one-page memory
    0x07, 0x07, 0x01, 0x03, b'm', b'e', b'm', 0x02, 0x00, // memory export
];

static ZERO_MEMORY: [u8; MEMORY_BYTES] = [0; MEMORY_BYTES];

/// The maximum number of input bytes written into the thread-local core memory
/// for one fuzz iteration.
pub const MAX_FUZZ_MEMORY_IMAGE_BYTES: usize = 4 * 1024;

/// The string-encoding option supplied to the canonical ABI adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzStringEncoding {
    None,
    Utf8,
    Utf16,
    CompactUtf16,
}

impl FuzzStringEncoding {
    fn canonical(self) -> Option<CanonicalStringEncoding> {
        match self {
            Self::None => None,
            Self::Utf8 => Some(CanonicalStringEncoding::Utf8),
            Self::Utf16 => Some(CanonicalStringEncoding::Utf16),
            Self::CompactUtf16 => Some(CanonicalStringEncoding::CompactUtf16),
        }
    }
}

/// Structured input accepted by [`fuzz_canonical_lift_args`].
///
/// The first call on a worker thread supplies the fixed component fixture. Its
/// bytes are retained by that worker, and later calls with different bytes fail
/// rather than accidentally mixing type metadata across fuzz iterations.
/// `flat_args` is forwarded unchanged to production canonical lifting.
#[derive(Clone, Debug)]
pub struct FuzzCanonicalLiftInput<'a> {
    pub fixture_bytes: &'a [u8],
    pub export_selector: u8,
    pub string_encoding: FuzzStringEncoding,
    pub memory_offset: u32,
    pub memory_image: &'a [u8],
    pub flat_args: &'a [WasmValue],
}

struct FuzzState {
    fixture_bytes: Vec<u8>,
    program: ComponentProgram,
    store: Store,
    memory: CoreExportRef,
}

thread_local! {
    static FUZZ_STATE: RefCell<Option<FuzzState>> = const { RefCell::new(None) };
}

/// Runs production canonical argument lifting against a normalized callable
/// export from the fixed fixture.
///
/// Every call resets all 64 KiB of core memory before attempting the bounded
/// image write. An out-of-bounds image write is an error; argument pointers are
/// never clamped or otherwise rewritten.
pub fn fuzz_canonical_lift_args(
    input: &FuzzCanonicalLiftInput<'_>,
) -> Result<Vec<ComponentValue>, ComponentError> {
    FUZZ_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.is_none() {
            *state = Some(build_state(input.fixture_bytes)?);
        }
        let state = state
            .as_mut()
            .expect("fuzz state is initialized before use");
        if state.fixture_bytes.as_slice() != input.fixture_bytes {
            return Err(ComponentError::InvalidArgument(
                "canonical fuzz fixture changes within one worker thread".to_owned(),
            ));
        }

        reset_and_write_memory(state, input.memory_offset, input.memory_image)?;

        let mut exports = state.program.callable_exports.clone();
        exports.sort_unstable();
        let export = exports
            .get(usize::from(input.export_selector) % exports.len().max(1))
            .ok_or_else(|| ComponentError::Runtime("fixture has no callable exports".to_owned()))?;
        let type_id = state.program.get_root_func_type_id(export).ok_or_else(|| {
            ComponentError::Runtime(format!(
                "canonical fuzz fixture export `{export}` is missing"
            ))
        })?;
        let func_type = program_func_type(&state.program, type_id)?;
        let options = RuntimeCanonicalOptions {
            string_encoding: input.string_encoding.canonical(),
            memory: Some(state.memory.clone()),
            realloc: None,
            post_return: None,
        };

        lift_component_args(
            &func_type,
            input.flat_args,
            &options,
            &state.program,
            &state.store,
        )
    })
}

fn build_state(fixture_bytes: &[u8]) -> Result<FuzzState, ComponentError> {
    let program = ComponentEngine::new().compile(fixture_bytes)?;
    let mut reader = IoReadBinaryReader::from(MEMORY_MODULE);
    let memory_module = WasmParser::new(&mut reader)
        .parse_module()
        .map_err(|error| ComponentError::Runtime(format!("parse fuzz memory module: {error}")))?;
    let store = Store::new();
    let registry = Registry::new();
    let instance = match block_on(instantiate(memory_module, &store, &registry)) {
        VMResult::Success(instance) => instance,
        error => {
            return Err(ComponentError::Runtime(format!(
                "instantiate fuzz memory module: {error:?}"
            )))
        }
    };

    Ok(FuzzState {
        fixture_bytes: fixture_bytes.to_vec(),
        program,
        store,
        memory: CoreExportRef {
            instance,
            export_name: "mem".to_owned(),
        },
    })
}

fn reset_and_write_memory(
    state: &FuzzState,
    offset: u32,
    memory_image: &[u8],
) -> Result<(), ComponentError> {
    write_memory(&state.store, &state.memory, 0, &ZERO_MEMORY)
        .map_err(|_| ComponentError::Runtime("reset fuzz memory: out of bounds".to_owned()))?;

    if memory_image.len() > MAX_FUZZ_MEMORY_IMAGE_BYTES {
        return Err(ComponentError::InvalidArgument(format!(
            "fuzz memory image exceeds {MAX_FUZZ_MEMORY_IMAGE_BYTES} bytes"
        )));
    }

    write_memory(&state.store, &state.memory, offset, memory_image)
        .map_err(|_| ComponentError::Trap("fuzz memory image write is out of bounds".to_owned()))
}

fn write_memory(store: &Store, memory: &CoreExportRef, ptr: u32, bytes: &[u8]) -> Result<(), ()> {
    let memory =
        crate::support::common::memory_export(&memory.instance, store, &memory.export_name)
            .map_err(|_| ())?;
    crate::support::common::write_memory(store, &memory, ptr, bytes)
        .then_some(())
        .ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_BYTES: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fuzz/fixtures/canon_abi.wasm"
    ));

    fn selector(export: &str) -> u8 {
        let mut exports = ComponentEngine::new()
            .compile(FIXTURE_BYTES)
            .expect("fixture must compile")
            .callable_exports;
        exports.sort_unstable();
        exports
            .iter()
            .position(|candidate| candidate == export)
            .expect("fixture export must exist") as u8
    }

    fn input<'a>(
        export: &str,
        memory_offset: u32,
        memory_image: &'a [u8],
        flat_args: &'a [WasmValue],
    ) -> FuzzCanonicalLiftInput<'a> {
        FuzzCanonicalLiftInput {
            fixture_bytes: FIXTURE_BYTES,
            export_selector: selector(export),
            string_encoding: FuzzStringEncoding::None,
            memory_offset,
            memory_image,
            flat_args,
        }
    }

    #[test]
    fn fixture_exposes_all_canonical_lift_shapes() {
        let program = ComponentEngine::new()
            .compile(FIXTURE_BYTES)
            .expect("fixture must compile");
        for export in [
            "string",
            "list",
            "nested-record",
            "variant",
            "flags",
            "indirect",
        ] {
            assert!(
                program.get_root_func_type_id(export).is_some(),
                "fixture is missing `{export}`"
            );
        }

        let indirect = program_func_type(
            &program,
            program
                .get_root_func_type_id("indirect")
                .expect("indirect export must exist"),
        )
        .expect("indirect export must have a function type");
        assert_eq!(indirect.params.len(), 17);
    }

    #[test]
    fn same_input_is_stable_across_an_interleaved_input() {
        let args = [WasmValue::I32(12), WasmValue::I32(1)];
        let original = input("string", 12, b"A", &args);
        let interleaved = input("string", 12, b"B", &args);

        let first = fuzz_canonical_lift_args(&original).expect("first lift must succeed");
        let second = fuzz_canonical_lift_args(&original).expect("second lift must succeed");
        let different =
            fuzz_canonical_lift_args(&interleaved).expect("interleaved lift must succeed");
        let after_interleave =
            fuzz_canonical_lift_args(&original).expect("replayed lift must succeed");

        assert_eq!(first, second);
        assert_ne!(first, different);
        assert_eq!(first, after_interleave);
    }

    #[test]
    fn zero_reset_removes_the_previous_iteration_image() {
        let args = [WasmValue::I32(20), WasmValue::I32(1)];
        fuzz_canonical_lift_args(&input("string", 20, b"A", &args))
            .expect("priming lift must succeed");

        let reset = fuzz_canonical_lift_args(&input("string", 0, &[], &args))
            .expect("zeroed lift must succeed");
        assert_eq!(reset, vec![ComponentValue::String("\0".to_owned())]);
    }

    #[test]
    fn image_write_oob_is_an_error() {
        let error = fuzz_canonical_lift_args(&input(
            "string",
            MEMORY_BYTES as u32,
            &[1],
            &[WasmValue::I32(0), WasmValue::I32(0)],
        ))
        .expect_err("image write must not be clamped");
        assert!(error.to_string().contains("image write is out of bounds"));
    }

    #[test]
    fn indirect_fixture_export_lifts_from_memory() {
        let values = (0_u32..17).flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let result =
            fuzz_canonical_lift_args(&input("indirect", 64, &values, &[WasmValue::I32(64)]))
                .expect("indirect canonical lift must succeed");
        assert_eq!(result.len(), 17);
        assert_eq!(result.first(), Some(&ComponentValue::U32(0)));
        assert_eq!(result.last(), Some(&ComponentValue::U32(16)));
    }
}
