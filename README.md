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
