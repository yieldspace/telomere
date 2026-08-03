#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;
use telomere::WasmValue;
use telomere_component::{
    fuzz_canonical_lift_args, ComponentError, ComponentValue, FuzzCanonicalLiftInput,
    FuzzStringEncoding, MAX_FUZZ_MEMORY_IMAGE_BYTES,
};

const FIXTURE_BYTES: &[u8] = include_bytes!("../fixtures/canon_abi.wasm");
const HEADER_BYTES: usize = 7;
const FLAT_ARG_BYTES: usize = 17;
const MAX_FLAT_ARGS: usize = 32;
const MAX_FLAT_ARG_BYTES: usize = MAX_FLAT_ARGS * FLAT_ARG_BYTES;
const MAX_PAYLOAD_BYTES: usize = MAX_FUZZ_MEMORY_IMAGE_BYTES + MAX_FLAT_ARG_BYTES;

#[derive(Debug)]
struct CanonLiftInput {
    export_selector: u8,
    string_encoding: u8,
    memory_offset: u32,
    memory_image: Vec<u8>,
    flat_args: Vec<WasmValue>,
}

/// Decodes every byte sequence without an `Arbitrary` underflow path.
///
/// The first seven bytes are a fixed header: export selector, encoding, memory
/// offset, and a memory/flat split selector. The bounded payload is then split
/// deterministically between the memory image and fixed-width `WasmValue`
/// records. This keeps short corpus entries on the adapter path.
fn decode_input(bytes: &[u8]) -> CanonLiftInput {
    let header_byte = |index| bytes.get(index).copied().unwrap_or_default();
    let payload = bytes.get(HEADER_BYTES..).unwrap_or_default();
    let payload = &payload[..payload.len().min(MAX_PAYLOAD_BYTES)];
    let split_selector = usize::from(header_byte(6));
    let selected_memory_len = payload.len().saturating_mul(split_selector) / usize::from(u8::MAX);
    let minimum_memory_len = payload.len().saturating_sub(MAX_FLAT_ARG_BYTES);
    let memory_len = selected_memory_len
        .max(minimum_memory_len)
        .min(MAX_FUZZ_MEMORY_IMAGE_BYTES);
    let (memory_image, flat_bytes) = payload.split_at(memory_len);

    CanonLiftInput {
        export_selector: header_byte(0),
        string_encoding: header_byte(1),
        memory_offset: u32::from_le_bytes([
            header_byte(2),
            header_byte(3),
            header_byte(4),
            header_byte(5),
        ]),
        memory_image: memory_image.to_vec(),
        flat_args: flat_bytes
            .chunks_exact(FLAT_ARG_BYTES)
            .map(flat_arg)
            .collect(),
    }
}

fn flat_arg(chunk: &[u8]) -> WasmValue {
    debug_assert_eq!(chunk.len(), FLAT_ARG_BYTES);
    let payload: [u8; 16] = chunk[1..].try_into().expect("chunk has 16 payload bytes");
    let low_u32 = u32::from_le_bytes(payload[..4].try_into().expect("u32 payload"));
    let low_i32 = i32::from_le_bytes(payload[..4].try_into().expect("i32 payload"));
    let low_i64 = i64::from_le_bytes(payload[..8].try_into().expect("i64 payload"));
    let low_f32 = f32::from_le_bytes(payload[..4].try_into().expect("f32 payload"));
    let low_f64 = f64::from_le_bytes(payload[..8].try_into().expect("f64 payload"));

    match chunk[0] % 7 {
        0 => WasmValue::I32(low_i32),
        1 => WasmValue::I64(low_i64),
        2 => WasmValue::F32(low_f32),
        3 => WasmValue::F64(low_f64),
        4 => WasmValue::V128(u128::from_le_bytes(payload)),
        5 => WasmValue::FuncRef(low_u32),
        _ => WasmValue::ExternRef(low_u32),
    }
}

fn string_encoding(selector: u8) -> FuzzStringEncoding {
    match selector % 4 {
        0 => FuzzStringEncoding::None,
        1 => FuzzStringEncoding::Utf8,
        2 => FuzzStringEncoding::Utf16,
        _ => FuzzStringEncoding::CompactUtf16,
    }
}

fn invoke(bytes: &[u8]) -> Result<Vec<ComponentValue>, ComponentError> {
    let input = decode_input(bytes);
    let adapter_input = FuzzCanonicalLiftInput {
        fixture_bytes: FIXTURE_BYTES,
        export_selector: input.export_selector,
        string_encoding: string_encoding(input.string_encoding),
        memory_offset: input.memory_offset,
        memory_image: &input.memory_image,
        flat_args: &input.flat_args,
    };
    fuzz_canonical_lift_args(&adapter_input)
}

#[cfg(not(test))]
fuzz_target!(|input: &[u8]| {
    let _ = invoke(input);
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_inputs_decode_without_rejection() {
        for bytes in [b"".as_slice(), b"\x04", b"\x04\x01\x10"] {
            let decoded = decode_input(bytes);
            assert!(decoded.memory_image.len() <= MAX_FUZZ_MEMORY_IMAGE_BYTES);
            assert!(decoded.flat_args.len() <= MAX_FLAT_ARGS);
        }
    }

    #[test]
    fn short_input_reaches_the_canonical_adapter() {
        let error = invoke(&[]).expect_err("empty input still invokes the adapter");
        assert!(
            error.to_string().contains("canonical ABI value underflow"),
            "unexpected adapter error: {error}"
        );
    }

    #[test]
    fn payload_split_is_deterministic_and_bounded() {
        let bytes = [
            4, 0, 0, 0, 0, 0, 128, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', b'j',
        ];
        let first = decode_input(&bytes);
        let second = decode_input(&bytes);
        assert_eq!(first.memory_image, second.memory_image);
        assert_eq!(first.flat_args, second.flat_args);
        assert!(first.memory_image.len() <= MAX_FUZZ_MEMORY_IMAGE_BYTES);
        assert!(first.flat_args.len() <= MAX_FLAT_ARGS);
    }
}
