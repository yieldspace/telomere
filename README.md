# telomere

Telomere is a WebAssembly runtime aimed at hosts that need the **Component Model
and WASI 0.2 without a large engine underneath them**. The core Wasm parser,
optimizer, interpreter, baseline JIT, Component Model decoder and runtime, and
the WASI 0.2 provider are all implemented from scratch in Rust: there is no
Cranelift and there are no `wasmtime`/`wasmparser` crates in the runtime path.
(`wit-parser` runs at macro-expansion time in the bindgen crate, and
`wat`/`wast` appear only in dev-dependencies.)

It exists because of a gap between the two runtimes usually reached for. WAMR is
the established choice for deeply embedded targets, and its upstream README
documents an interpreter core in the tens of kilobytes - but it documents no
Component Model or WASI 0.2 support. Wasmtime implements both at its highest
support tier, but it is engineered for server- and desktop-class hosts. Telomere
targets the space between: Component Model plus WASI 0.2, in a runtime intended
to be small enough to embed. See
[Where telomere sits](#where-telomere-sits) for what is measured and what is
still only an intention.

The concrete motivation is a plugin execution engine for appliance-class
hardware - RISC-V, 256 MiB-class devices - where third-party plugins ship as
WASI 0.2 components instead of native code.

## Status

The project is still pre-release and the workspace packages are not published
to crates.io (`publish = false`). Treat the public APIs as implementation
driven until the project reaches a stable release. `publish = false` is
intentional, and the CI packaging guard keeps that boundary in place. See
[RELEASING.md](RELEASING.md) for the tag, version, changelog, and future
publishing policy.

The JIT is experimental. The WASI surfaces are partial, and two gaps are worth
knowing before you evaluate the Component Model path: components produced by
current Rust `wasm32-wasip2` export `wasi:cli/run@0.2.0` while the CLI looks up
`wasi:cli/run@0.2.6` exactly, and canonical `resource.drop` is not yet available
for host-provided resources. Components can call host imports that return owned
resource handles, including `wasi:cli/stdout.get-stdout`, and call
`output-stream.blocking-write-and-flush`; see the runnable
[`wasi-component-stdout` fixture](examples/wasi-component-stdout.wat). The
remaining resource-lifecycle constraint is documented in
[examples/README.md](examples/README.md). The bundled arguments sample reports
its result through the exit status so that it stays focused on argument passing.

This is a personal project developed with heavy use of AI agents, and it is
maintained alongside other work. Issues and pull requests are welcome, but
responses may be slow or, for changes outside the current direction, may not
come at all. Please do not read silence as a judgement of the contribution.

## Where telomere sits

| | Component Model | WASI 0.2 | JIT | Binary size (CLI / minimal; not an overhead ratio) | Peak RSS (CLI / minimal embedder) |
| --- | --- | --- | --- | --- | --- |
| **telomere** | Yes (own decoder, linker, and runtime) | Yes, partial (0.2.6 provider) | Yes, experimental baseline JIT | 3.98 MiB CLI; 1.10 MiB minimal WASI (measured, see note 1) | 3.3-4.8 MiB CLI; 2.94 MiB minimal WASI (measured, see note 1) |
| wasmtime | Yes (Tier 1, note 2) | Yes (Tier 1, note 2) | Yes (Cranelift) | not measured here | not measured here |
| WAMR | No (note 3) | No - preview1 only (note 3) | Yes | see note 3 | not measured here |

Notes:

1. The retained 3.98 MiB / 3.3-4.8 MiB figures are historical
   `telomere-cli` host measurements on macOS arm64 with default features
   (2026-08-01). The added minimal WASI figures are #139's macOS arm64
   `release-size` `embed-wasi` measurement (1154832 B file; 3080192 B peak RSS,
   2026-08-03). The CLI links `clap` and a multi-thread `tokio` runtime; the
   minimal sample deliberately does not. Here "minimal" means the minimal
   supported dependency topology, not the fewest possible bytes. Full method,
   environments, and configuration ladder:
   [docs/benchmarks/footprint.md](docs/benchmarks/footprint.md).
   The paired CLI/minimal figures use different profiles and are shown for
   orientation, not as a ratio or CLI-overhead comparison.
2. From the wasmtime stability tiers page,
   <https://docs.wasmtime.dev/stability-tiers.html> (retrieved 2026-08-01).
3. From the WAMR README, <https://github.com/bytecodealliance/wasm-micro-runtime>
   (retrieved 2026-08-01). It documents no Component Model or WASI 0.2 support,
   and reports "~58.9K for fast interpreter" as the core `vmlib` text size on
   Cortex-M4F measured with bloaty.

**These columns are not a size benchmark.** The only numbers measured for this
table are telomere's, and a Cortex-M4F library text size is not comparable with
a macOS arm64 CLI binary, the macOS minimal-embedder artifact, or the measured
runtime text deltas (about 729 KiB on macOS and 1015 KiB on Linux).
Telomere is not WAMR-class; closing that distance is the goal, not a result.
The footprint note explains the unmatched target, libc, and standard-library
conditions without deriving a WAMR ratio. Cross-runtime footprint measurement
with matched targets and feature sets is listed as open work there. For
execution speed there is a separate, earlier local comparison in
[docs/core/coremark-benchmark.md](docs/core/coremark-benchmark.md).

## What is in this repository?

| Path | Purpose |
| --- | --- |
| `src/` | `telomere-cli`, including core module, preview1, and component runners. |
| `crates/telomere` | Core Wasm binary parser, validator-facing optimizer, runtime, host linking, and optional JIT. |
| `crates/telomere-component` | Component Model decoder, validator, IR, linker, and runtime. |
| `crates/telomere-component-wasi` | WASI 0.2.6 component host provider used by the CLI and tests. |
| `crates/telomere-component-bindgen` | Proc macro that generates component bindings from WIT. |
| `crates/telomere-jit-codegen` | Low-level executable memory and target code-emission helpers for the core JIT. |
| `crates/telomere-macros` | Internal support crate. |
| `docs/` | Architecture notes, audits, and design boundaries. |
| `examples/` | Small runnable Wasm fixtures with `.wat` sources; see [examples/README.md](examples/README.md). |
| `examples/minimal-embedder/` | Standalone core, Component Model, and WASI embedding ladder with no CLI dependency topology. |
| `tools/` | Out-of-process measurement scripts, such as the cold-start harness used by [docs/benchmarks/footprint.md](docs/benchmarks/footprint.md). |

Start with [docs/README.md](docs/README.md) for the documentation map.

## Requirements

- Rust 1.86.0 through 1.96.0. The repository pins 1.96.0 via
  `rust-toolchain.toml`, so `cargo` picks the right toolchain automatically.
  Rust 1.97.0 miscompiles the interpreter's dispatch loop — call-heavy modules
  overflow the host stack in release builds — so it is not supported yet.
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

Run the bundled WASI preview1 command module, passing guest argv after `--`.
`argv[0]` is the module file name:

```shell
cargo run -- examples/wasi-preview1-hello.wasm -- one two
```

Expected output:

```text
hello from telomere (wasi preview1)
wasi-preview1-hello.wasm
one
two
```

Run the bundled WASI 0.2 component command. It reports through the exit status
whether it received a guest argument:

```shell
cargo run -- component examples/wasi-component-args.wasm -- one ; echo $?
```

Expected output:

```text
0
```

```shell
cargo run -- component examples/wasi-component-args.wasm ; echo $?
```

Expected output:

```text
1
```

The arguments component reports through its exit status to keep that fixture
focused on argument passing. The companion stdout component writes a line via
`get-stdout` and `blocking-write-and-flush`. See
[examples/README.md](examples/README.md) for both fixtures, how to rebuild them
from the committed `.wat` sources, and the remaining WASI constraints.

## Embedding telomere

For a host that does not need the CLI dependency topology, start with
[`examples/minimal-embedder/`](examples/minimal-embedder/README.md). Its
standalone configuration ladder shows core, Component Model, and WASI 0.2
embedding code using the committed `add.wasm`, `component-add.wasm`, and
`wasi-component-args.wasm` fixtures. "Minimal" means the supported dependency
topology, not a promise of the fewest bytes; the headline rows keep SIMD on and
`core-nosimd` is the comparison row.

The core, component, and WASI samples use a visible std-only local executor
(`Wake` + `thread::park`), not Tokio or `futures::executor::block_on`. The
latter nests incompatibly with the executor used inside `telomere-component`;
that Component Model follow-up is [#167](https://github.com/yieldspace/telomere/issues/167).
The sample does not change component-runtime internals. See the measured
footprint and its boundaries in
[docs/benchmarks/footprint.md](docs/benchmarks/footprint.md).

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

## Support matrix

The [support matrix](docs/support-matrix.md) records the conformance-fixture
meaning of "supported", feature-dependent evidence, named unsupported core
proposal errors, Component Model canonical ABI coverage, and per-interface WASI
0.2.6 function coverage. It is the authoritative map for these boundaries;
this feature list remains the source of truth for Cargo feature selection.

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

Known gaps on these paths, with reproductions, are collected in
[examples/README.md](examples/README.md): the preview1 runner does not implement
`environ_get`/`environ_sizes_get` (so a stock `wasm32-wasip1` Rust binary will
not run), component export lookup is exact-version rather than
semver-compatible, and host-provided Component Model resources cannot yet be
released through `resource.drop`.

## Security

Vulnerability reporting is described in [SECURITY.md](SECURITY.md).

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
- [docs/benchmarks/footprint.md](docs/benchmarks/footprint.md) - measured binary size, peak RSS, and cold start.
- [docs/component-model/relation-driven-runtime.md](docs/component-model/relation-driven-runtime.md) - component runtime architecture.
- [docs/memory-reduction-audit.md](docs/memory-reduction-audit.md) - memory reduction audit and tradeoffs.

## License

This repository is licensed under the terms in [LICENSE](LICENSE).
