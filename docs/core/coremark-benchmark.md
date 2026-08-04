# CoreMark Runtime Comparison

> Status: **Historical (superseded)**. The 2026-05-19 values below are retained
> unchanged as provenance; they are not the current interpreter baseline.
> Current measurements use the [Interpreter Baseline Methodology](interpreter-baseline.md).

This note preserves a local CoreMark comparison for Telomere and several other
Wasm runtimes. It follows the [WAMR performance reporting convention](https://github.com/bytecodealliance/wasm-micro-runtime/wiki/Performance):
CoreMark is compared by the score reported by CoreMark itself
(`Iterations/Sec`), not by shell wall-clock time.

## Current method and pending record

The current method defines L1 CoreMark, L2 workload slopes, and L3
microbenchmarks; its full build/run matrix, noise floor, tail-call witnesses,
and raw-record lifecycle live in [Interpreter Baseline Methodology](interpreter-baseline.md).

No current CoreMark or optimizer-pipeline result has been measured or committed
yet. The first valid record will be JSON under [`baseline/`](baseline/), from
which a current-results table can be derived. Until then, this page intentionally
contains no current score, delta, or ratio.

Future Telomere rows have these cell meanings:

| Future row | Cell meaning |
| --- | --- |
| Default interpreter control | `default` build, used as the A1/A2 control for measurement-feature inertness and noise. |
| JIT-feature interpreter control | `jit` build without `--jit`; it exposes the JIT feature's interpreter tax. |
| Optimizer pipeline on | `opt-on`: `measure-switches` build with `TELOMERE_OPTIMIZER` unset. |
| Optimizer pipeline off | `opt-off`: same build with exactly `TELOMERE_OPTIMIZER=off`; an upper bound on the whole optimizer pipeline's contribution, not a recognizer-only result. |
| JIT-feature optimizer on/off | `jit,opt-on` / `jit,opt-off`, analogous cells with both features compiled and no `--jit`. |

The harness records the feature-gated `measure-switches-probe` JSON in a
separate process, so the artifact records the state the library resolved rather
than only the value the harness requested.

## Why the historical record is superseded

The historical numbers remain useful provenance, but they are not a current
baseline for three independent reasons:

1. Telomere's interpreter row came from a binary built with `--features jit`.
   It ran without `--jit`, but still carried JIT-feature interpreter checks; it
   is not the normal default-build interpreter control.
2. The record is tied to its older commit and local host/toolchain, rather than
   the current four-build/six-cell methodology and raw artifact contract.
3. The five instantiate-time CoreMark-shaped rewrites assumed to affect the
   result did not all run in production, as documented below.

The new `opt-on`/`opt-off` delta is explicitly an upper bound on the whole
optimizer pipeline's contribution. It must not be renamed or interpreted as the
cost of CoreMark-specific recognizers.

### Reachability correction for the five rewrites

The correction changes what the historical number means; it does not delete the
record.

- Four rewrites — `rewrite_list_crc_summary_function`,
  `rewrite_matrix_i16_crc_summary_function`,
  `rewrite_core_state_benchmark_function`, and
  `rewrite_list_crc_pair_loops` — never had production call sites.
- `rewrite_crc16_update_masked_wrapper` was the one live rewrite at `e292f72`.
  Commit `f56abfc` removed its production call site and placed it behind
  `#[cfg(test)]`.
- `f56abfc` is an ancestor of the historical-record commit `60b45c9`.
  Therefore the historical CoreMark result excludes **all five** rewrites.

The lesson is narrow but important: ancestry proves code was present, not that
it executed. These historical data must not be used to claim that a recognizer
or one-off rewrite was measured.

## Historical (superseded) record — 2026-05-19

### Scope

This was a local sanity benchmark, not a portable performance claim. It remains
useful only as historical provenance and a reproducible record of its command
line on that machine.

CoreMark is a single benchmark. Do not use this document to justify
CoreMark-only recognizers or one-off rewrites. Telomere JIT and optimizer work
should improve reusable Wasm instruction patterns.

### Environment

| Item | Value |
| --- | --- |
| Date | 2026-05-19 |
| Host | macOS Darwin 25.2.0, arm64 |
| CPU | Apple M2 Pro |
| Telomere commit | `60b45c929e861a36abb8452b863257689282e1f1` (the commit that recorded this run) |
| CoreMark artifact | `https://wasm3.github.io/wasm-coremark/coremark.wasm` |
| CoreMark compile info | Clang 11.0.0, `-O3`, `STATIC` memory |

Runtime versions:

| Runtime | Version |
| --- | --- |
| Telomere | `telomere-cli 0.1.0`, release build with `--features jit` |
| WAMR / `iwasm` | 2.4.4 Homebrew bottle |
| wasm3 | 0.5.0 Homebrew bottle |
| WasmEdge | 0.17.0 Homebrew bottle |
| Wasmtime | 44.0.1 Homebrew bottle |

Wasmtime is listed for environment completeness, but the local `wasmtime run -C
cache=n` CoreMark command did not produce a result within a 45 second cap in
this run. It is excluded from the score table rather than mixed with partial
data.

### Historical commands

The artifact was downloaded into `/private/tmp`:

```shell
mkdir -p /private/tmp/telomere-coremark
curl -L --fail \
  -o /private/tmp/telomere-coremark/coremark.wasm \
  https://wasm3.github.io/wasm-coremark/coremark.wasm
```

Telomere was built once before measurement:

```shell
cargo build --release --features jit
```

The valid measurement run was executed serially. Earlier exploratory parallel
runs were discarded because they contend for CPU and are not valid benchmark
data.

```shell
target/release/telomere-cli /private/tmp/telomere-coremark/coremark.wasm
target/release/telomere-cli --jit /private/tmp/telomere-coremark/coremark.wasm
iwasm --interp /private/tmp/telomere-coremark/coremark.wasm
wasm3 /private/tmp/telomere-coremark/coremark.wasm
wasmedge --log-level off --run-mode interpreter /private/tmp/telomere-coremark/coremark.wasm
wasmedge --log-level off --run-mode jit /private/tmp/telomere-coremark/coremark.wasm
```

Wasmtime was checked separately:

```shell
perl -e 'alarm 45; exec @ARGV' \
  wasmtime run -C cache=n /private/tmp/telomere-coremark/coremark.wasm
```

That command was killed by the timeout without a CoreMark report.

### Historical row meanings

| Historical result row | Exact cell or runtime mode |
| --- | --- |
| WasmEdge JIT | External WasmEdge JIT invocation from the historical command block. |
| Telomere JIT | Historical `jit`-feature Telomere build, invoked with `--jit`; includes lazy compilation and cache behavior. |
| wasm3 | External wasm3 interpreter invocation. |
| WAMR `iwasm --interp` | External WAMR interpreter invocation. |
| Telomere interpreter | Historical `jit`-feature Telomere build invoked **without** `--jit`; not equivalent to a current `default` or `opt-on` control cell. |
| WasmEdge interpreter | External WasmEdge interpreter invocation. |

### Historical results

All rows below printed `Correct operation validated`.

| Runtime mode | CoreMark score (`Iterations/Sec`) | CoreMark iterations | Total time reported by CoreMark |
| --- | ---: | ---: | ---: |
| WasmEdge JIT | 31249.333510 | 400000 | 12.800273 s |
| Telomere JIT | 5473.572770 | 110000 | 20.096563 s |
| wasm3 | 2844.023175 | 40000 | 14.064583 s |
| WAMR `iwasm --interp` | 1409.507239 | 20000 | 14.189356 s |
| Telomere interpreter | 1189.914735 | 20000 | 16.807927 s |
| WasmEdge interpreter | 315.450049 | 4000 | 12.680296 s |

Relative to Telomere interpreter:

| Runtime mode | Relative score |
| --- | ---: |
| WasmEdge JIT | 26.26x |
| Telomere JIT | 4.60x |
| wasm3 | 2.39x |
| WAMR `iwasm --interp` | 1.18x |
| Telomere interpreter | 1.00x |
| WasmEdge interpreter | 0.27x |

Relative to WAMR `iwasm --interp`:

| Runtime mode | Relative score |
| --- | ---: |
| WasmEdge JIT | 22.17x |
| Telomere JIT | 3.88x |
| wasm3 | 2.02x |
| WAMR `iwasm --interp` | 1.00x |
| Telomere interpreter | 0.84x |
| WasmEdge interpreter | 0.22x |

### Historical notes

- The Homebrew WAMR package exposes `iwasm`, but no `wamrc` command was present
  in `PATH`, so this run did not include WAMR AOT.
- CoreMark's reported `Iterations` varied by runtime. That is why the table
  uses `Iterations/Sec`, matching WAMR's documented convention for CoreMark.
- Telomere JIT was measured through the CLI `--jit` flag, so the run includes
  lazy compilation and cache behavior in the reported score.
- WasmEdge JIT prints compilation logs unless `--log-level off` is supplied; the
  score above used `--log-level off`.
- Wasmtime should be rechecked with a longer cap or a precompiled setup if it
  becomes important for this comparison.
