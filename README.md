# telomere

## CLI

```shell
> telomere-cli examples/add.wasm main 1 2
3
```

## Development

After cloning, initialize the pinned test fixtures submodule before running the
workspace tests:

```shell
git submodule update --init --recursive
```

This repository's CI runs the following commands:

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --tests -- -D warnings
cargo test --workspace --release
```

## Proposal Features

The `telomere` library crate exposes proposal-specific build features:

- `simd`
- `threads`

`simd` and `threads` are enabled by default for the library crate.

Multi-memory, tail-call support, and the async runtime are always enabled; they
are no longer separate Cargo features.

The root `telomere-cli` package builds with `full` by default, so ordinary
workspace commands such as `cargo run` and `cargo test --workspace --release`
exercise the CLI with both optional proposal features enabled.

## Core JIT Status

The core JIT is an experimental function-local lazy baseline JIT. Build it with
the `jit` Cargo feature and enable it at runtime with `--jit` or
`RuntimeConfig { jit: JitConfig { enabled: true, .. } }`.

- [x] Function-level lazy compilation on first JIT-enabled wasm call.
- [x] Single-pass direct native emitter written in Rust; LLVM and external code
      generators are not used.
- [x] macOS AArch64 backend for local development on Apple Silicon.
- [x] Store-local native code cache with a configurable byte cap
      (`--jit-code-cache-mib`, default 4 MiB) and least-recently-used eviction.
- [x] Tier-aware cache structure prepared for a future optimizing tier; the
      active tier is currently always baseline.
- [x] W^X executable memory through `mmap-rs`; each compiled function owns an
      independent executable mapping that is never made writable again.
- [x] Unsupported direct native op shapes fail closed at function compile time;
      accepted JIT frames do not side-exit into the interpreter.
- [x] Tail-call-threading integration through the existing direct call and
      return-call paths.
- [x] Minimal runtime exits for done, trap, direct call, pending async work, and
      callee-level interpreter execution when a called function is not JIT
      accepted.
- [x] Native emission for the scalar baseline subset: i32/i64 integer
      arithmetic and comparisons, f32/f64 arithmetic/comparisons/rounding,
      numeric conversions, globals, locals, select, references, direct and
      indirect calls, control flow, memory.size/grow, and default-memory
      8/16/32/64-bit load/store helpers.
- [x] Native helper calls remain for supported JIT ABI operations, while
      complex VM operations that would require interpreter continuation are
      rejected for whole-function interpreter execution.
- [ ] Non-macOS and non-AArch64 native backends.
- [ ] Full Wasm SIMD (`v128`) and threads/atomics coverage in direct native
      code; `i8x16.extract_lane_s`, `v128.bitselect`, and atomic wait/notify
      currently use native runtime stubs.
- [ ] Direct native emission for remaining shape-general branch-heavy guarded
      memory patterns that currently use continuation runtime stubs.
- [x] Benchmark-specific whole-function rewrites are intentionally not enabled;
      JIT improvements should be reusable Wasm instruction patterns rather than
      CoreMark-only recognizers.
- [ ] Native import/host-call stubs; imported calls still rely on the existing
      runtime call machinery rather than a dedicated native ABI path.
- [ ] Register allocation beyond the current small fixed stack-register pool.
- [ ] Hotness counters, tier-up policy, and an optimizing compiler.
- [ ] Full trap maps, GC maps, source debug info, profiling metadata, and
      deoptimization metadata.
- [ ] Cross-platform executable memory policy and cache tuning.

Local Apple Silicon CoreMark check using
`/Users/sizumita/Workspace/wasm3/wasm-coremark/coremark.wasm`
(`ITERATIONS=30000`, Clang 11 `-O3`, `STATIC` memory), with CoreMark-specific
function recognizers disabled: `--features jit` without runtime JIT measured
`2010.184129` iterations/sec, and `--features jit -- --jit` measured
`2027.199476` iterations/sec.
