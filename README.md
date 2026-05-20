# telomere

Telomere is a lightweight WebAssembly runtime written in Rust. The workspace
contains a core Wasm parser/runtime, a WebAssembly Component Model decoder and
runtime, a WASI component provider, a component bindgen macro, and the
`telomere-cli` command-line runner.

The project is still pre-release and the workspace packages are not published
to crates.io (`publish = false`). Treat the public APIs as implementation
driven until the project reaches a tagged release.

## What is in this repository?

| Path | Purpose |
| --- | --- |
| `src/` | `telomere-cli`, including core module, preview1, and component runners. |
| `crates/telomere` | Core Wasm binary parser, validator-facing optimizer, runtime, host linking, and optional JIT. |
| `crates/telomere-component` | Component Model decoder, validator, IR, linker, and runtime. |
| `crates/telomere-component-wasi` | WASI 0.2.6 component host provider plus the experimental WASI 0.3 / Preview 3 async provider. |
| `crates/telomere-component-bindgen` | Proc macro that generates component bindings from WIT. |
| `crates/telomere-jit-codegen` | Low-level executable memory and target code-emission helpers for the core JIT. |
| `crates/telomere-macros`, `crates/union-find` | Internal support crates. |
| `docs/` | Architecture notes, audits, and design boundaries. |
| `examples/` | Small runnable Wasm fixtures. |

Start with [docs/README.md](docs/README.md) for the documentation map.

## Requirements

- Rust 1.86 or newer.
- A Unix-like development environment for the current JIT executable-memory
  backend tests. The non-JIT interpreter and component tests are the portable
  baseline.
- The upstream Wasm testsuite submodule when running the full workspace test
  set.

Initialize fixtures after cloning:

```shell
git submodule update --init --recursive
```

## Quick start

Run the sample core Wasm module:

```shell
cargo run -- examples/add.wasm main 1 2
```

Expected output:

```text
3
```

Run the same module through the experimental JIT on a supported target:

```shell
cargo run --features jit -- --jit examples/add.wasm main 1 2
```

Run a WASI preview1 command module by passing guest argv after `--`:

```shell
cargo run -- path/to/command.wasm -- arg1 arg2
```

Run a WASI 0.2 component command:

```shell
cargo run -- component path/to/component.wasm --env KEY=VALUE --dir host:guest -- arg1 arg2
```

## CLI usage

The legacy core invocation form calls an exported function and parses remaining
arguments as `i32` values:

```shell
telomere-cli [--jit] [--jit-code-cache-mib N] <module.wasm> <export> [i32...]
```

If a core module imports `wasi_snapshot_preview1` and exports `_start`, the CLI
can run it as a command module. Use `--` before guest argv:

```shell
telomere-cli <command.wasm> -- [argv...]
```

The component subcommand runs components that export
`wasi:cli/run@0.2.6.run`:

```shell
telomere-cli component <component.wasm> [--dir HOST[:GUEST]] [--env KEY=VALUE] [--no-inherit-env] -- [argv...]
```

## Cargo features

The root CLI package enables `full` by default, which enables the core Wasm
`simd` and `threads` proposal features in `crates/telomere`.

| Feature | Scope | Notes |
| --- | --- | --- |
| `simd` | `telomere` | Enables core Wasm SIMD parsing/runtime support through `wide`. Enabled by default. |
| `threads` | `telomere` | Enables shared-memory and atomic instruction support. Enabled by default. |
| `jit` | root CLI and `telomere` | Builds the experimental function-local lazy baseline JIT. Runtime use still requires `--jit` or `RuntimeConfig`. |
| `vm-profile` | root CLI and `telomere` | Enables VM profile counters used by runtime/JIT diagnostics. |
| `vm-diagnostics` | root CLI and `telomere` | Enables additional runtime diagnostics. |

Multi-memory, tail calls, and async runtime support are always enabled; they are
not separate Cargo features.

The component crate also contains gated Component Model proposal features such
as `component-gated-feature-async` and
`component-gated-feature-fixed-length-lists`. These are intentionally explicit
because Component Model proposal stability differs from core Wasm proposal
coverage.

## Library entry points

The core crate re-exports the common runtime entry points from `telomere`:

```rust
use telomere::{
    instantiate, run_module_function, IoReadBinaryReader, Registry, ResultValue,
    Store, WasmParser, WasmValue,
};
```

The component crate exposes a compile/instantiate/call split:

```rust
use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

let engine = ComponentEngine::new();
let program = engine.compile(bytes)?;
let linker = ComponentLinker::new();
let instance = engine.instantiate(&program, &store, &linker).await?;
let results = instance.call(&store, "export-name", &[ComponentValue::S32(1)]).await?;
```

For WASI components, register the provider from `telomere-component-wasi`:

```rust
use telomere_component_wasi::{add_to_linker_sync, WasiState};
```

## JIT status

The core JIT is experimental. It is a function-local lazy baseline JIT that is
compiled with `--features jit` and enabled at runtime with `--jit` or
`RuntimeConfig { jit: JitConfig { enabled: true, .. } }`.

Supported native backends are:

- macOS AArch64
- macOS/Linux x86_64
- Linux GNU riscv64 targets with standard riscv64gc F/D floating-point ISA
  assumptions

See [docs/core/jit.md](docs/core/jit.md) for the current status, boundaries,
diagnostic environment variables, and known gaps.

## WASI status

Telomere has two WASI entry points:

- core WASI preview1 command modules through `telomere-cli <command.wasm> -- ...`;
- WASI 0.2.6 component commands through
  `telomere-cli component <component.wasm> -- ...`.

The preview1 runner is intentionally small. It currently supports command-style
modules that import `wasi_snapshot_preview1`, export `_start`, and use the
implemented host functions for argv, clocks, fd write/seek/stat/close, and
`proc_exit`. The component WASI provider registers the generated WIT bindings
for cli, io, clocks, random, filesystem, and sockets, with capability supplied
through `WasiState`.

WASI 0.3 / Preview 3 work tracks the official `WebAssembly/WASI`
`wit-0.3.0-draft` snapshot vendored under
`crates/telomere-component-wasi/wit-preview3/` and pinned in
[docs/component-model/wasi-0.3-preview3.md](docs/component-model/wasi-0.3-preview3.md).
The initial `telomere_component_wasi::preview3::add_to_linker_async` surface
registers the 0.3 RC CLI environment/exit/stdio, random, clocks, and
filesystem/sockets paths, including the official P3 `wasi:sockets/types`
resource shape and literal-IP `ip-name-lookup`. P2 and P3 share an internal
WASI substrate for pollables, local streams, stdio handles, filesystem
descriptors, and monotonic timer readiness; async poll/timer calls can now
suspend on the caller task and resume through Telomere's existing async host
call scheduler path. P3 stdio includes the official `read-via-stream` /
`write-via-stream` stream/future handle shape for local stdio buffers.
`wasi:io/{error,poll,streams}` is currently a `0.2.8` compatibility bridge
because official P3 WIT uses Component Model `stream<T>` / `future<T>` handles
instead of a separate `wasi:io@0.3` package.
Remaining WASI 0.3 imports, unsupported mutating filesystem operations, and
connected socket I/O are expected to fail closed rather than fall back to the
0.2.6 provider.

## Development

CI runs the following commands:

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --tests --features jit -- -D warnings
cargo test --verbose --release --workspace
```

Useful scoped commands while iterating:

```shell
cargo test -p telomere --release
cargo test -p telomere-component --release
cargo test -p telomere-component-wasi --release
cargo test -p telomere --release --features jit --test jit -- --nocapture
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow and PR
checklist.

## Documentation

- [docs/README.md](docs/README.md) - documentation map.
- [docs/core/optimizer.md](docs/core/optimizer.md) - current core optimizer.
- [docs/core/runtime-memory.md](docs/core/runtime-memory.md) - runtime memory model.
- [docs/core/jit.md](docs/core/jit.md) - experimental core JIT.
- [docs/core/coremark-benchmark.md](docs/core/coremark-benchmark.md) - local CoreMark comparison.
- [docs/component-model/relation-driven-runtime.md](docs/component-model/relation-driven-runtime.md) - component runtime architecture.
- [docs/component-model/wasi-0.3-preview3.md](docs/component-model/wasi-0.3-preview3.md) - WASI 0.3 / Preview 3 snapshot pin and support matrix.
- [docs/memory-reduction-audit.md](docs/memory-reduction-audit.md) - memory reduction audit and tradeoffs.

## License

This repository is licensed under the terms in [LICENSE](LICENSE).
