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
- [x] Unsupported functions fail closed with `VMResult::Unimplemented` during
      native compilation/linking instead of silently falling back.
- [x] Tail-call-threading integration through the existing direct call and
      return-call paths.
- [x] Minimal runtime exits for done, trap, direct call, and unimplemented
      fallback conditions.
- [x] Native emission for the current i32 baseline subset: `i32.const`,
      `i32.add`, `i32.sub`, `i32.mul`, `i32.eqz`, `i32.eq`, signed/unsigned
      `i32.lt`, 32-bit local get/set/tee, selected local/fused superinstructions,
      `br`, `br_if`, `return`, `end`, direct `call`, and direct `return_call`.
- [x] Runtime stub exits for supported i32 memory operations, including
      8/16/32-bit load/store widths and out-of-bounds memory traps.
- [ ] Non-macOS and non-AArch64 native backends.
- [ ] Full Wasm numeric coverage (`i64`, `f32`, `f64`, `v128`) in native code.
- [ ] Native emission for globals, tables, `call_indirect`, references,
      exceptions, atomics, SIMD, bulk memory, and multi-memory-specific fast
      paths beyond the current supported memory helpers.
- [ ] Native import/host-call stubs; imported calls still rely on the existing
      runtime call machinery rather than a dedicated native ABI path.
- [ ] Register allocation beyond the current small fixed stack-register pool.
- [ ] Hotness counters, tier-up policy, and an optimizing compiler.
- [ ] Full trap maps, GC maps, source debug info, profiling metadata, and
      deoptimization metadata.
- [ ] Cross-platform executable memory policy and cache tuning.
