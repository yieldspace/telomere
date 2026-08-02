# Core JIT

The core JIT is an experimental function-local lazy baseline JIT for core Wasm.
It is not a component-model JIT, not an optimizing tier, and not a replacement
for the direct-threaded interpreter.

Build it with the `jit` Cargo feature and enable it at runtime with `--jit` or
`RuntimeConfig { jit: JitConfig { enabled: true, .. } }`.

```shell
cargo run --features jit -- --jit examples/add.wasm main 1 2
```

## Supported Targets

The active native backends are:

- macOS AArch64
- macOS/Linux x86_64
- Linux GNU riscv64 targets with standard riscv64gc F/D floating-point ISA
  assumptions

Runtime use is additionally gated by `telomere::jit_supported()`. The CLI fails
closed if `--jit` is requested on an unsupported build or target.

## Architecture Boundary

- The parser and optimizer still produce `LoweredFunction`; it is the canonical
  optimized artifact consumed by the JIT.
- Compilation is lazy and function-local. The first JIT-enabled call can compile
  the callee and install a store-local cache entry.
- The emitter is a single-pass Rust native emitter. Telomere does not use LLVM
  or an external code generator.
- Executable memory is W^X through `mmap-rs`; each compiled function owns an
  executable mapping that is not made writable again.
- Unsupported direct native op shapes fail closed into JIT ABI helpers or
  continuation bridges. Opcode coverage alone does not decide whole-function
  acceptance.
- Accepted JIT frames return through explicit runtime exits; they do not
  side-exit into the interpreter in the middle of the same function.
- Store-local native code cache entries are bounded by
  `--jit-code-cache-mib` and evicted by least-recently-used policy.

The component runtime remains relation-driven and interpreter-backed at the
component layer. Embedded core modules can still use the core runtime beneath
component canonical ABI calls.

## Current Coverage

Implemented baseline pieces include:

- lazy compilation on first JIT-enabled core Wasm call;
- direct native backends for the supported targets listed above;
- direct native paths for scalar integer and floating-point arithmetic,
  comparisons, conversions, locals, globals, select, references, direct and
  indirect calls, control flow, `memory.size`, `memory.grow`, and common
  default-memory load/store helpers;
- tail-call-threading integration through existing direct call and return-call
  paths;
- runtime exits for done, trap, direct call, pending async work, continuation
  bridges, and callee-level interpreter execution when a called function is not
  JIT-accepted;
- baseline acceptance for enabled SIMD and threads/atomics through direct native
  paths where implemented and JIT ABI helper/continuation bridges for the
  remaining handlers.

Known gaps:

- full direct-native SIMD and atomics coverage;
- remaining branch-heavy guarded memory shapes that still need continuation
  bridges;
- native import/host-call stubs;
- register allocation beyond the current fixed stack-register pool;
- hotness counters, tier-up policy, and an optimizing compiler;
- trap maps, GC maps, source debug info, profiling metadata, and deoptimization
  metadata;
- broader executable-memory policy and cache tuning outside the supported
  Unix-like targets.

Benchmark-specific whole-function rewrites are intentionally not enabled. JIT
work should improve reusable Wasm instruction patterns rather than recognizing a
single benchmark.

## Diagnostics

Useful environment variables:

| Variable | Purpose |
| --- | --- |
| `TELOMERE_JIT_PROFILE=1` | Print or collect JIT profile counters in tests and profile runs. |
| `TELOMERE_JIT_PROFILE_TOP=N` | Limit the number of profile counters shown. |
| `TELOMERE_JIT_TRACE_COMPILE=1` | Trace native compilation decisions. More detail is available with `vm-diagnostics`. |
| `TELOMERE_JIT_TRACE_COMPILE_MAX=N` | Limit compile trace output. |
| `TELOMERE_JIT_TRACE_FALLBACK=1` | Trace fallback and bridge behavior. |
| `TELOMERE_JIT_TRACE_FALLBACK_FUNC=N` | Restrict fallback tracing to one function index. |
| `TELOMERE_JIT_TRACE_FALLBACK_KIND=K` | Restrict fallback tracing to one fallback kind. |
| `TELOMERE_JIT_TRACE_FALLBACK_MAX=N` | Limit fallback trace output. |
| `TELOMERE_WAST_JIT=1` | Run WAST harness cases with runtime JIT enabled. |
| `TELOMERE_WAST_JIT_CACHE_MAX_BYTES=N` | Override the WAST harness JIT cache cap. |
| `TELOMERE_WAST_JIT_REQUIRE_ACCEPT=1` | Require JIT acceptance in WAST harness cases intended to prove coverage. |

Focused checks:

```shell
cargo test -p telomere --release --features jit --test jit -- --nocapture
TELOMERE_WAST_JIT=1 TELOMERE_WAST_JIT_REQUIRE_ACCEPT=1 cargo test -p telomere --release --features jit --test wast -- --nocapture
```

CI runs the full WAST suite with strict JIT acceptance on the three supported
desktop targets (Linux x86_64, macOS x86_64, and macOS AArch64), and separately
executes the riscv64 Linux GNU JIT and WAST suites under QEMU.

See [jit-coverage-audit.md](jit-coverage-audit.md) for the handler-level audit.

## CoreMark Note

See [coremark-benchmark.md](coremark-benchmark.md) for the current local
CoreMark comparison against Telomere interpreter/JIT and other Wasm runtimes.
CoreMark remains a sanity benchmark, not a target for benchmark-specific
recognizers.
