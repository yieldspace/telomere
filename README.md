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
