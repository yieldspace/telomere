# CoreMark Runtime Comparison

This note records a local CoreMark comparison for Telomere and several other
Wasm runtimes. It follows the WAMR performance page's reporting convention:
CoreMark is compared by the score reported by CoreMark itself
(`Iterations/Sec`), not by shell wall-clock time.

Reference:
https://github.com/bytecodealliance/wasm-micro-runtime/wiki/Performance

## Scope

This is a local sanity benchmark, not a portable performance claim. It is useful
for tracking Telomere's current position on this machine and for keeping the
measurement command line reproducible.

CoreMark is a single benchmark. Do not use this document to justify
CoreMark-only recognizers or one-off rewrites. Telomere JIT and optimizer work
should improve reusable Wasm instruction patterns.

## Environment

| Item | Value |
| --- | --- |
| Date | 2026-05-19 |
| Host | macOS Darwin 25.2.0, arm64 |
| CPU | Apple M2 Pro |
| Telomere branch | `docs/oss-documentation` |
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

## Commands

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

## Results

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

## Notes

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
