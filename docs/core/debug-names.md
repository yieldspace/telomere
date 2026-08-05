# Debug-name retention

Telomere retains producer-supplied WebAssembly module and function names after
instantiation for future diagnostic reporting. This document specifies the
retained representation, its error boundaries, and the logical-accounting
measurement used for this implementation.

## Retained representation and lookup contract

[`ModuleNames`](../../crates/telomere/src/common/debug_names.rs) is an internal,
compact representation held by
[`ModuleInstance`](../../crates/telomere/src/common/store.rs) as
`Option<Arc<ModuleNames>>`. It contains:

- an optional boxed module-name string;
- a sorted boxed table of `(funcidx, byte_offset)` entries; and
- one boxed, concatenated function-name string blob.

`ModuleNames::function_name(funcidx)` binary-searches the table and slices the
blob; `ModuleNames::module_name()` returns the optional module name. This keeps
the retained function data to one function-entry table allocation and one string allocation,
rather than retaining a `String` allocation for every function. The `Arc` that
owns `ModuleNames` and a non-empty module name can add allocations, so the
observed allocation count is reported with every byte total below.

Function indices use the core module index space: imported functions come
first, followed by defined functions. Instantiation assigns `funcidx` from the
same ordered function vector, so there is no import-count offset or other
translation between a `name` subsection index and a
`FunctionInstanceData.funcidx`. A name at index 1 therefore names the first
defined function in a module with one imported function. This imports-first
contract is intentionally tested because a refactor could otherwise retain
names while looking them up in the wrong index space.

Local names are parsed but are never retained in `ModuleNames`. A module that
has only local names consequently creates no retained name set. This is
intentional: the diagnostics consumers planned in #207 and #210 require module
and function names, not local names.

The hand-off seam for #207 and #210 is exactly
`ModuleNames::module_name()` and `ModuleNames::function_name(funcidx)`. It now
has its shipped consumer: [`Store::take_last_trap`](trap-reporting.md) resolves
a captured frame's code address through the diagnostics-safe store lookup,
uses the defining function's module, and calls these accessors while building
the owned `TrapInfo`. It does not follow a stack-derived instance id, introduce
a parallel name index, or invoke either accessor on the dispatch or capture
path. That keeps capture index-only and makes symbolization a cold, checked
read at retrieval time.

## Retention configuration

[`DiagnosticsConfig`](../../crates/telomere/src/common/store.rs) is part of
`RuntimeConfig` and defaults to `retain_function_names: true`. With the default
configuration, [`instantiate`](../../crates/telomere/src/runtime/instantiate.rs)
compacts a parsed name section and stores `Some(Arc<ModuleNames>)` when the
section contains a module or function name.

An embedder can opt out per store:

```rust
let mut config = RuntimeConfig::default();
config.diagnostics.retain_function_names = false;
let store = Store::new_with_runtime_config(config);
```

With `retain_function_names: false`, instantiation does not create
`ModuleNames`; the parsed name data is dropped and `ModuleInstance.names` is
`None`. The option is a retention gate, not a parser gate: the parser still
reads custom sections and applies the same malformed-name recovery described
below. Apart from that common parser work, the disabled path performs only the
configuration branch and retains no diagnostic name allocation. A later
`Store::take_last_trap()` still reports the function index, frame kind, and
available program counter, but its `module_name` and `func_name` are `None`.

## Custom-section recovery and reader bounds

WebAssembly custom sections are non-semantic ([core specification,
custom sections](https://webassembly.github.io/spec/core/appendix/custom.html)).
Accordingly, a malformed *body* of a custom section named `name` is non-fatal.
[`parse_namedata`](../../crates/telomere/src/parser/core/parser.rs) limits the
inner body, records how many bytes it consumed, skips the remaining bytes in
the declared outer section, and then drops the malformed name data. The parser
therefore re-synchronizes at the outer custom-section boundary and can parse
and instantiate the following core sections.

The recovery boundary is deliberately narrow. Parsing the custom section's own
name happens before the non-fatal branch, so a malformed or truncated custom
section name is still a fatal parse error. Likewise, an outer custom-section
size that extends beyond the enclosing module is a truncated module and remains
a hard `UnexpectedEof` error. The accounting used to re-synchronize employs
checked subtraction; it does not turn an invariant violation into a saturated
zero.

The outer-boundary rule depends on the nested
[`LimitingBinaryReader::take`](../../crates/telomere/src/binary/reader.rs)
clamp. A child limit is the smaller of its requested end and its parent's end
(with an overflowing requested end clamped to the parent), so a nested reader
cannot consume bytes that belong to a following section. This is production
reachable in the component decoder, not merely a reader-unit-test case:
[`telomere-component`'s component decoder](../../crates/telomere-component/src/decoder/component.rs)
wraps every embedded core module in `ctx.reader.take(section_size)` before
passing it to `WasmParser`. The parser then takes the name-section body inside
that limiter. Without the parent clamp, an oversized inner name section could
read past the embedded core module; with it, recovery reaches the core-module
boundary and the oversized outer declaration fails without consuming following
component bytes.

## Logical-accounting boundary

The figures in this document are logical accounting for the parser
representation. They are not RSS, allocator telemetry, or an exact statement
of process memory. Allocator bucket rounding and allocation headers are
excluded.

For a compact name set, `compact payload bytes` is the sum of the bytes owned
by `ModuleNames`: module-name string length, `function_entries.len() *
size_of::<(u32, u32)>()`, and function-name-blob length.
`compact total logical bytes` adds the current probe's
`size_of::<ModuleNames>()`, the `Option<Arc<ModuleNames>>` slot in
`ModuleInstance`, and an assumed `Arc` control block of two `usize` counters.
The last term is an explicit 64-bit assumption, not stable public API layout.

The probe reports `size_of::<ModuleNames>()` and
`size_of::<Option<Arc<ModuleNames>>>()` directly. Its `Vec-as-is` comparison
does not normalize capacity to length: it counts the parser's actual module
`String` capacity, function `Vec<(u32, String)>` capacity times its element
size, and every function-name `String` capacity. Its allocation count similarly
counts the non-empty module string, the function vector, and non-empty function
strings. This makes the comparison an as-is retained parser representation,
with the same exclusion of allocator rounding and headers.

`name payload bytes` means the sum of the declared payload sizes of every
custom section whose custom-section name is exactly `name`. It includes that
custom section's name-length LEB and `name` bytes as part of its payload, plus
all name subsections; it excludes the section ID and the outer section-size
LEB. `module bytes` is the complete input module length.

`DWARF section bytes` is the sum of the full encoded bytes of every custom
section whose custom-section name starts with `.debug_`. Each such total
includes the section ID, outer section-size LEB, and payload. `module bytes
excluding DWARF` is exactly `module bytes - DWARF section bytes`.

## Reproducing the logical-accounting probe

Run the driver from the repository root:

```shell
python3 tools/measure-debug-name-retention.py
```

The driver creates all generated WAT, Wasm, manifest, and Rust inputs in a
temporary directory and deletes them when it exits. It measures the committed
fixtures `crates/telomere/benches/telomere-benchmark.wasm`, `examples/add.wasm`,
and `examples/wasi-preview1-hello.wasm`; four synthetic modules with function
counts `F = 10`, `100`, `1000`, and `5000`; and a temporary Rust 2021 debug
hello-world crate. The temporary crate uses the debug profile and is built for
`wasm32-unknown-unknown`.

The driver invokes these commands for the generated inputs and probe:

```text
wasm-tools parse -o <synthetic>.wasm <synthetic>.wat
cargo build --manifest-path <temporary>/rust-hello/Cargo.toml --target wasm32-unknown-unknown --target-dir <temporary>/rust-hello/target
cargo test -p telomere --lib common::debug_names::tests::measurement_probe -- --ignored --exact --nocapture
```

The ignored `measurement_probe` runs in a test-worker thread with a 64 MiB
stack so that the `F = 5000` synthetic input can be measured. This is a
measurement-harness detail only; it does not change the production parser or
runtime stack configuration.

### Recorded toolchain and constants

The following are the complete toolchain and accounting constants printed by
the recorded driver run:

- Python: `3.9.6`
- cargo: `cargo 1.96.0 (30a34c682 2026-05-25)`
- rustc: `rustc 1.96.0 (ac68faa20 2026-05-25)`
- wasm-tools: `wasm-tools 1.255.0`
- Pointer width: `64` bits
- `size_of::<ModuleNames>()`: `48` bytes
- `size_of::<Option<Arc<ModuleNames>>>()`: `8` bytes
- `Arc` control-block assumption (two `usize` counters): `16` bytes

### Observed results

`fixture-benchmark` is kept in the table as the required
`telomere-benchmark.wasm` measurement. It has no `name` custom section, so all
retention totals remain `0`; the name-bearing `add` and WASI preview1 fixtures
are reported beside it rather than substituted for it.

| input | module bytes | DWARF section bytes | module bytes excluding DWARF | name payload bytes | compact payload bytes | compact total logical bytes | compact allocations | Vec-as-is logical bytes | Vec allocations |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fixture-benchmark | 115 | 0 | 115 | 0 | 0 | 0 | 0 | 0 | 0 |
| fixture-add | 335 | 0 | 335 | 69 | 44 | 116 | 4 | 168 | 3 |
| fixture-wasi-preview1-hello | 842 | 0 | 842 | 232 | 125 | 197 | 3 | 336 | 9 |
| synthetic-f10 | 709 | 0 | 709 | 646 | 694 | 766 | 4 | 1168 | 12 |
| synthetic-f100 | 6651 | 0 | 6651 | 6227 | 6815 | 6887 | 4 | 10512 | 102 |
| synthetic-f1000 | 66930 | 0 | 66930 | 62902 | 68016 | 68088 | 4 | 96784 | 1002 |
| synthetic-f5000 | 334930 | 0 | 334930 | 314902 | 340016 | 340088 | 4 | 582160 | 5002 |
| rust-2021-debug-hello | 1541690 | 1518593 | 23097 | 5868 | 6256 | 6328 | 4 | 11912 | 75 |

The `rust-2021-debug-hello` row needs a separate interpretation. Of its
`1,541,690` module bytes, `1,518,593` bytes (about `98.5%`) are DWARF custom
sections outside this issue's scope. Against the `23,097` bytes remaining after
that exclusion, the `5,868` name-payload bytes are about `25.4%`, comparable to
the name-payload ratios in the `add` and WASI rows. Both percentages are derived
from the probe artifact columns (`module bytes`, `DWARF section bytes`, and
`module bytes excluding DWARF`), not from a performance measurement. The length
of the temporary build path can change the full-module and DWARF byte counts;
it does not change the name payload for this generated source.

DWARF symbolication is an explicit non-goal. An embedder that strips DWARF
before shipping should budget from the roughly `25%` post-strip comparison, not
the roughly `0.4%` comparison against the full debug module. This is a retained
byte-budget interpretation only; it does not add a performance number.

## Interpreter benchmark disposition

The issue's interpreter-benchmark result is **failed-with-reason**. No timing
or throughput number is published here. The committed
[`measurement-attempts`](measurement-attempts/README.md) evidence records why
this host cannot pass the load gate, why the environment characterization is
not a timing result, and why the finite schedule's A/A evidence cannot bound
build-specific effects. It is therefore not valid to fabricate or infer an
interpreter result for this change.

**Structural argument, not a measurement:** name retention runs once during
instantiation. This change adds no name lookup, hashing, or name access to a
dispatch or capture path, and the planned diagnostic accessors currently have
no such caller. That code-path statement explains why the retained state is
not expected to alter interpreter-loop work, but it is not a performance
measurement and does not replace the failed benchmark.
