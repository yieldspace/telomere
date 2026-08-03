# Contributing

Thanks for working on Telomere. This repository is pre-release, so correctness,
clear boundaries, and reproducible verification matter more than API churn
avoidance. Keep changes small enough to review and document any user-visible
runtime, parser, CLI, or feature-gate behavior.

## Setup

Install Rust 1.86 or newer, then initialize the pinned fixture submodules:

```shell
git submodule update --init --recursive
```

The workspace is intentionally unpublished (`publish = false`), so local
development uses path dependencies inside the repository.

## Workspace Layout

- `src/` contains `telomere-cli`.
- `crates/telomere` contains the core Wasm parser, optimizer, runtime, host
  linking APIs, and optional JIT.
- `crates/telomere-component` contains the Component Model decoder, validator,
  IR, linker, and runtime.
- `crates/telomere-component-wasi` contains the WASI 0.2.6 component provider.
- `crates/telomere-component-bindgen` contains the WIT bindgen proc macro.
- `docs/` contains architecture notes and audits.
- `examples/` contains runnable fixtures.

## Required Checks

Run the CI-equivalent checks before opening a PR:

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --tests --features jit -- -D warnings
cargo test --verbose --release --workspace
```

When iterating, use scoped commands first:

```shell
cargo test -p telomere --release
cargo test -p telomere-component --release
cargo test -p telomere-component-wasi --release
cargo test -p telomere --test optimizer_runtime
cargo test -p telomere-component --test component_model_wast -- --nocapture
```

For JIT work, include focused JIT evidence:

```shell
cargo test -p telomere --release --features jit --test jit -- --nocapture
TELOMERE_WAST_JIT=1 TELOMERE_WAST_JIT_REQUIRE_ACCEPT=1 cargo test -p telomere --release --features jit --test wast -- --nocapture
```

## Fuzzing

The fuzz harnesses live in the independent `fuzz/` workspace and require
`nightly-2026-07-31` plus `cargo-fuzz`. Run `cargo fuzz` only from that
directory. Before a fuzz campaign, generate the stable core and component seed
corpora with an absolute output path:

```shell
cd fuzz
TELOMERE_FUZZ_CORPUS_OUT="$(pwd -P)/corpus" \
  cargo +1.96.0 test --manifest-path ../Cargo.toml -p telomere \
  --test fuzz_corpus_replay --release -- --nocapture
TELOMERE_FUZZ_CORPUS_OUT="$(pwd -P)/corpus" \
  cargo +1.96.0 test --manifest-path ../Cargo.toml -p telomere-component \
  --test fuzz_corpus_replay --release -- --nocapture
```

Keep mutable corpus and artifact files out of commits. Promote reviewed inputs
only to `fuzz/seeds/<target>/` or `fuzz/regressions/<target>/`, then run the
corresponding replay test. See [docs/fuzzing.md](docs/fuzzing.md) for the three
targets, corpus-directory ordering, triage, current CI cadence, and known
limits.

## Documentation Expectations

Update docs with every behavior change that affects users, contributors, or
future maintainers.

- CLI behavior belongs in `README.md`.
- Developer workflow belongs in this file.
- Architecture and implementation boundaries belong in `docs/`.
- Historical notes should be labeled as historical when they no longer describe
  the current runtime contract.
- Feature support should distinguish core Wasm, Component Model, WASI preview1,
  WASI 0.2 components, and the experimental core JIT.

## Coding Style

Use standard Rust 2021 style. Let `rustfmt` own formatting, keep modules focused,
and treat clippy warnings as bugs. Prefer existing parser/runtime/component
boundaries over catch-all modules.

The core optimizer contract is fail-closed: malformed or unverifiable optimized
output must not replace the original materialized instruction stream. The JIT
contract is also fail-closed and uses `LoweredFunction` as the canonical input
artifact.

## Pull Requests

Use a short Conventional Commit-style title where it fits:

- `feat:`
- `fix:`
- `refactor:`
- `test:`
- `doc:`
- `chore:`

PR descriptions should include:

- what changed;
- why it changed;
- user-visible behavior or compatibility impact;
- tests and commands run;
- links to issues or follow-up docs when relevant.

Call out parser/runtime semantics, fixtures, component model coverage, and JIT
acceptance changes explicitly.
