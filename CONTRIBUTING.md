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
python3 tools/check-packaging.py
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

With `TELOMERE_WAST_JIT_REQUIRE_ACCEPT=1`, WAST testing fails closed if the
binary lacks the `jit` feature, the host is unsupported, runtime JIT is not
enabled, or the completed WAST run compiles zero functions.

## Releases

Git tags, not crates.io packages, are the current release artifacts. The
workspace intentionally keeps `publish = false`; do not change it as part of
ordinary release preparation or a feature change. Publishing is a separate
maintainer decision. See [RELEASING.md](RELEASING.md) for the versioning policy,
release gate, and release checklist.

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
