# Precompiled codec: cold-start measurements (#196)

Measurements for the precompiled-module codec
([contract](../core/precompiled-modules.md)). Two boundaries are
reported: whole-process cold start through the CLI, and aggregate
in-process phase times over the spec corpus. Both were measured on
2026-11-24 on fleur-du-lapin (Ryzen 9 5900X, Linux x86_64, release
profile, Rust 1.96.0 toolchain) against the implementing branch. These
are local measurements, not CI-gate numbers.

## Whole-process cold start (CLI)

Method: `tools/measure-cold-start.py`, median of 30 runs after 3
warm-ups, exactly as documented in that script. The pair runs the same
preview1 command module (`examples/wasi-preview1-stock-hello.wasm`,
76,005 bytes) once from the wasm and once from its precompiled artifact
(`telomere-cli compile` output, 330,227 bytes):

| Case | Wall median | Peak RSS median |
| --- | ---: | ---: |
| `wasi-preview1-stock-hello.wasm` preview1 command | 162.66 ms | 13,564 KiB |
| `wasi-preview1-stock-hello.tpre` precompiled preview1 command | 15.32 ms | 11,872 KiB |

The precompiled run is **~10.6× faster** end to end for this module and
uses slightly less memory. For scale, the same script measures the
842-byte `wasi-preview1-hello.wasm` at 10.04 ms: at that size the
process floor (~8–10 ms of exec, runtime init, and instantiate)
dominates and there is almost no parse time left to save. The codec's
win grows with module size because what it removes — parse, validate,
and optimize — is the part that scales with the module.

## Aggregate phase times (spec corpus)

Method: `crates/telomere/examples/precompiled_cold_start.rs` over the
fuzz corpus directory generated from the wast suites
(`TELOMERE_FUZZ_CORPUS_OUT` + `cargo test -p telomere --test
fuzz_corpus_replay --release`; 5,167 unique module payloads). For every
payload that parses (1,346 modules; the rest are the suites'
intentionally malformed/invalid modules), the example times
`WasmParser::parse_module`, then `serialize_module_timed`, then
`deserialize_module_timed`, in-process, single-threaded, release
profile:

| Phase | Total over 1,346 modules |
| --- | ---: |
| parse | 213.1 ms |
| serialize (for reference) | 95.1 ms |
| deserialize | 90.0 ms |
| — of which header + fingerprint | 0.8 ms |
| — of which module sections | 1.9 ms |
| — of which function bodies (incl. `materialize`) | 86.9 ms |

Per module that is 0.158 ms parse vs 0.067 ms deserialize:
**deserialize is ~2.4× faster than parse in aggregate**, meeting the
#196 target (`deserialize < parse` on the corpus). The deserialize time
is dominated by the function phase, which is mostly `materialize`
rebuilding the instruction streams — work that instantiate would do
from the lowered form regardless of how the module arrived. The pure
decode share (header + sections + non-materialize function decode) is
small; further large wins would have to come from skipping
materialize, which is a different design (and out of #196's scope).

Caveat: corpus modules are small (293,619 wasm bytes total across the
1,346 modules), so fixed per-module costs weigh more here than they
would for production-size modules; the CLI pair above is the
large-module data point.

## Artifact size

The lowered form is verbose — one entry per op with its operands, plus
the const pool, jump table, and block map:

| Input | Bytes | Artifact | Ratio |
| --- | ---: | ---: | ---: |
| corpus aggregate (1,346 modules) | 293,619 | 1,223,410 | 4.17× |
| `wasi-preview1-stock-hello.wasm` | 76,005 | 330,227 | 4.34× |

Artifacts are a load-time cache, not a distribution format; the size
ratio is the accepted cost, recorded here so it is a decision rather
than a surprise.

## Reproduce

```sh
cargo build --release
python3 tools/measure-cold-start.py

TELOMERE_FUZZ_CORPUS_OUT=/tmp/fuzz-corpus \
  cargo test -p telomere --test fuzz_corpus_replay --release
cargo run --release -p telomere --example precompiled_cold_start -- \
  /tmp/fuzz-corpus/parse_core_module
```
