# Documentation Map

This directory contains implementation documentation for Telomere. The docs are
split by audience:

- users and embedders should start with the repository [README](../README.md);
- contributors should read [CONTRIBUTING](../CONTRIBUTING.md);
- maintainers should use the architecture notes and audits below as the
  current design record.

Some files are historical design notes. Those files are useful context, but
their current-status banner decides whether they describe the implemented
runtime or an older proposal.

Several design notes are written wholly or partly in Japanese. Those documents
carry a `## Summary (English)` section at the top so that the decision recorded
in them is readable without Japanese. The summaries are not translations; read
the body for the details.

## Core Wasm Runtime

| Document | Status | Use it for |
| --- | --- | --- |
| [core/runtime-memory.md](core/runtime-memory.md) | Current | Store ownership, linear memory, execution lease, and compact runtime representation. |
| [support-matrix.md](support-matrix.md) | Current | Core proposal, Component Model canonical ABI, and WASI 0.2.6 support boundaries with source-backed coverage counts. |
| [core/optimizer.md](core/optimizer.md) | Current | The canonical optimizer pipeline and runtime boundary. |
| [core/metering.md](core/metering.md) | Current | Store-scoped interpreter fuel, accounting, cancellation, and its JIT boundary. |
| [core/current-optimizer.md](core/current-optimizer.md) | Current snapshot | As-built optimizer behavior, research basis, and remaining fallback families. |
| [core/jit.md](core/jit.md) | Current | Experimental function-local lazy baseline JIT status, supported targets, diagnostics, and gaps. |
| [core/coremark-benchmark.md](core/coremark-benchmark.md) | Local benchmark note | Serial CoreMark comparison against WAMR, wasm3, and WasmEdge. |
| [core/jit-coverage-audit.md](core/jit-coverage-audit.md) | Audit | Baseline JIT acceptance and lowering coverage at the audit point. |
| [core/optimizer-family-budgets.md](core/optimizer-family-budgets.md) | Design note | Family selection and specialization budgets. |
| [core/jump-address-resolution.md](core/jump-address-resolution.md) | Design note | Jump target resolution details. |
| [core/type-checking.md](core/type-checking.md) | Background note | Core Wasm stack type checking background. |
| [core/stack.md](core/stack.md) | Historical note | Earlier unified-stack design context. Check current runtime code before using it as a contract. |
| [core/function-local-readdressing.md](core/function-local-readdressing.md) | Historical note | Earlier local-addressing and compact frame ideas. Check current runtime code before using it as a contract. |

## Component Model

| Document | Status | Use it for |
| --- | --- | --- |
| [component-model/relation-driven-runtime.md](component-model/relation-driven-runtime.md) | Current | Relation-driven component compile/instantiate/call architecture. |
| [component-model/new-component-runtime.md](component-model/new-component-runtime.md) | Current implementation plan | Public API, parity boundary, coverage, and verification commands. |
| [component-model/type-system.md](component-model/type-system.md) | Background note | Existential resource type reasoning and the current dense-arena strategy. |
| [component-model/wasmtime-critical-analysis.md](component-model/wasmtime-critical-analysis.md) | Design rationale | Why Telomere keeps the component layer lightweight instead of adopting a Wasmtime-like engine shape. |

The component runtime intentionally does not JIT or AOT component execution. It
uses the core Wasm runtime for embedded core modules and keeps component-level
resolution in `ComponentProgram`.

## Benchmarks

| Document | Status | Use it for |
| --- | --- | --- |
| [benchmarks/footprint.md](benchmarks/footprint.md) | Local measurement | Measured `telomere-cli` binary size, peak RSS, and cold start, plus the reproduction script and the list of unmeasured targets. |
| [core/coremark-benchmark.md](core/coremark-benchmark.md) | Local benchmark note | Serial CoreMark comparison against WAMR, wasm3, and WasmEdge. |

## Audits

| Document | Status | Use it for |
| --- | --- | --- |
| [memory-reduction-audit.md](memory-reduction-audit.md) | Audit | Implemented memory reductions, measurements, and tradeoffs left for later. |

## Verification Commands

Full CI-equivalent checks:

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --tests --features jit -- -D warnings
cargo test --verbose --release --workspace
```

Focused checks used by architecture work:

```shell
cargo test -p telomere parser::core::optimizer --lib
cargo test -p telomere --test optimizer_runtime
cargo test -p telomere-component --test component_model_wast -- --nocapture
cargo test -p telomere-component --test component_runtime_e2e -- --nocapture
cargo test -p telomere-component --test component_wasmtime_sync_parity -- --nocapture
cargo test -p telomere --release --features jit --test jit -- --nocapture
```
