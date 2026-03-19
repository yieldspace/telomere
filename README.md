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

- `threads`
- `tail-call`
- `multi-memory`

`threads` and `tail-call` are enabled by default for the
library crate. `multi-memory` is opt-in on the library crate and is
included by the `full` feature.

The root `telomere-cli` package builds with `full` by default, so ordinary
workspace commands such as `cargo run` and `cargo test --workspace --release`
exercise the CLI with all currently supported proposal features enabled.
