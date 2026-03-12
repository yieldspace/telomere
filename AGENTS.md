# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the `telomere-cli` entrypoint. The workspace crates live under `crates/`: `crates/telomere` is the core Wasm parser/runtime, `crates/telomere-component` covers component-model decoding and runtime support, `crates/telomere-macros` holds proc macros, and `crates/union-find` provides shared utilities. Keep design notes in `docs/`, runnable samples in `examples/`, and benchmark code in each crate's `benches/` directory. Large upstream fixtures live in `crates/telomere/tests/wasm-testsuite`, and the local component-model compile/validate suite lives in `crates/telomere-component/tests/component_model_testsuite`.

## Build, Test, and Development Commands
Use the same commands that CI runs:

- `cargo test --workspace --release` builds and runs the full workspace test suite.
- `cargo clippy --workspace --all-targets --tests -- -D warnings` enforces a warning-free build.
- `cargo fmt --all -- --check` verifies formatting.
- `cargo run -- examples/add.wasm main 1 2` runs the CLI against the sample module and should print `3`.

When iterating on one crate, prefer scoped commands such as `cargo test -p telomere-component --release`.

## Coding Style & Naming Conventions
Follow Rust 2021 defaults: 4-space indentation, `snake_case` for functions/modules, `CamelCase` for types, and small, focused modules. Let `rustfmt` handle layout instead of manual alignment, and treat every `clippy` warning as a bug to fix. Keep parser/runtime code close to the existing boundaries (`parser`, `runtime`, `decoder`, `ir`) rather than creating catch-all files.

## Testing Guidelines
Add regression tests with every behavior change. Integration tests sit in `crates/*/tests` and typically use descriptive `snake_case` file names such as `host_function_link.rs` or `component_runtime_e2e.rs`. Add `.wast` fixtures only when the scenario cannot be expressed in Rust alone, and place them next to the suite they extend. No numeric coverage gate is enforced, so rely on targeted tests plus the full release-mode workspace run before opening a PR.

## Commit & Pull Request Guidelines
Recent history favors short Conventional Commit prefixes like `feat:`, `fix:`, `refactor:`, `test:`, `doc:`, `chore:`, and `format`. Keep subjects imperative and specific to the touched crate or subsystem. PRs should explain the behavioral change, list the commands you ran, and link the relevant issue when one exists. If parser/runtime semantics or fixtures change, call that out explicitly in the PR body.
