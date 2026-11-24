# vm-profile frequency reports (#188)

Raw `vm-profile` dispatch reports behind the superinstruction-selection
record in [`docs/core/superinstruction-selection.md`](../../core/superinstruction-selection.md).
Each `.txt` file is the verbatim stderr of one `telomere-cli` run built with
`--features vm-profile` and `TELOMERE_VM_PROFILE=1` on 2026-11-24
(host: fleur-du-lapin, x86_64, Rust 1.96.0).

Counting scope, as implemented in `runtime/vm.rs`: only handlers that call
`dispatch_profile_count` are counted — the fused superinstruction families,
the call family, and the local-get family. Plain stack ops (for example
`op_local_binop32`, `op_i32_const_binop`, the f64 stack ops) do not count
themselves, so pair/triple frequencies are frequencies over the counted
subsequence, not over raw adjacent instructions, and shares are shares of
counted dispatches. The f64 kernel report is the extreme case: its loop body
contains no counted handler, so it records a single counted dispatch.

| Report | Workload | Invocation | Counted dispatches |
| --- | --- | --- | --- |
| `fib.txt` | `tools/baseline/repeat-fib32.wat` (sha256 `d8687e6c…`) | `run 4` | 42,294,938 |
| `loop-linear.txt` | `tools/baseline/loop-linear.wat` (sha256 `ee4d27e2…`) | `run 50000000` | 50,000,002 |
| `f64-kernel.txt` | `tools/baseline/f64-kernel.wat` (sha256 `ae9561c4…`) | `run 50000000` | 1 (see counting scope) |
| `coremark.txt` | CoreMark 1.0, `coremark.wasm` (sha256 `5acb1c0e…`) | default `_start` | 145,885,206 |
| `coremark-recognizers-on.txt` | same module, `--benchmark-recognizers` | default `_start` | 145,885,232 |
| `json-agg.txt` | `tools/vm-profile/json_agg.rs` (sha256 of wasm `404714f0…`) | `run 3000` | 39,579,842 |
| `text-index.txt` | `tools/vm-profile/text_index.rs` (sha256 of wasm `f0a9644e…`) | `run 3000` | 44,749,225 |

The two Rust modules are small but real compiled workloads (a JSON parser
aggregating into hash maps; a text tokenizer building an inverted index),
built with `rustc -O --crate-type cdylib --target wasm32-unknown-unknown`
(Rust 1.92.0). Their sources are committed under `tools/vm-profile/` so the reports
can be regenerated; the wasm binaries are not committed.

CoreMark note: the artifact above is the file served from
`https://wasm3.github.io/wasm-coremark/coremark.wasm`. Its sha256
(`5acb1c0e…`) does not match the pin in `tools/baseline/artifacts.json`
(`e71e3582…`); that mismatch predates this change and is recorded in the
baseline methodology document.
