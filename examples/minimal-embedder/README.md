# Minimal embedder

This crate contains four standalone binary entry points arranged into seven
embedding configurations. Each row is built and run through its own Cargo
invocation so that Cargo feature unification cannot pull a higher layer into a
lower-layer measurement.

"Minimal" means the minimal supported dependency topology, not the minimum
achievable byte count. The headline configurations retain SIMD because it is a
supported default; core-nosimd is included to make the byte-cost difference
visible.

## Rustdoc walkthrough

For a self-contained inline-WAT core embedding example, start with the
[`telomere` crate rustdoc source](../../crates/telomere/src/lib.rs). It uses a
Tokio current-thread runtime to show the public asynchronous API; this example
crate is the complementary standalone program with the feature configurations
and executable commands below.

Run all commands from the repository root. The binaries accept the fixture path
as their only argument and fail with a non-zero status if parsing,
instantiation, execution, or the expected result fails.

Measured file size, text, RSS, and whole-process cold-start results, including
their environment and comparison limits, are in
[`docs/benchmarks/footprint.md`](../../docs/benchmarks/footprint.md).

## `release-size` panic boundary

The size-oriented profile uses `panic = "abort"` only after the `baseline`,
`core`, `component`, and `wasi` binaries all ran with their correct expected
outputs. A host Rust panic therefore aborts the process. This does not make a
guest- or input-reachable panic safe; that remaining boundary is tracked by
[#128](https://github.com/yieldspace/telomere/issues/128). The profile gate and
unmeasured `build-std` alternatives are documented in the footprint note.

## Configuration ladder

### baseline

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --bin embed-baseline
cargo run -p telomere-minimal-embedder --release --no-default-features --bin embed-baseline -- examples/add.wasm
~~~

The run prints the fixture's byte length.

### core

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --features simd --bin embed-core
cargo run -p telomere-minimal-embedder --release --no-default-features --features simd --bin embed-core -- examples/add.wasm
~~~

The run prints:

~~~text
3
~~~

### core-nosimd

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --bin embed-core
cargo run -p telomere-minimal-embedder --release --no-default-features --bin embed-core -- examples/add.wasm
~~~

The run prints 3.

### component

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --features simd,component --bin embed-component
cargo run -p telomere-minimal-embedder --release --no-default-features --features simd,component --bin embed-component -- examples/component-add.wasm
~~~

The run prints 42.

### wasi

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --features simd,wasi --bin embed-wasi
cargo run -p telomere-minimal-embedder --release --no-default-features --features simd,wasi --bin embed-wasi -- examples/wasi-component-args.wasm
~~~

The run prints 0 after successfully calling wasi:cli/run.

### core-jit

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --features simd,jit --bin embed-core
cargo run -p telomere-minimal-embedder --release --no-default-features --features simd,jit --bin embed-core -- examples/add.wasm
~~~

On a supported target, this configuration enables the JIT in RuntimeConfig and
verifies that it compiled the core workload. The run prints 3.

### wasi-threads

~~~shell
cargo build -p telomere-minimal-embedder --release --no-default-features --features simd,threads,wasi --bin embed-wasi
cargo run -p telomere-minimal-embedder --release --no-default-features --features simd,threads,wasi --bin embed-wasi -- examples/wasi-component-args.wasm
~~~

The run prints 0.

## Executor boundary

The `core`, `component`, and `wasi` binaries visibly drive their asynchronous
work with a small local `block_on` built only from `std::task::Wake` and
`std::thread::park`. They intentionally do not add Tokio or
`futures::executor::block_on`: the nesting failure occurs in the Component
Model layer, where it conflicts with the executor already entered by
`telomere-component`. That design follow-up is
[#167](https://github.com/yieldspace/telomere/issues/167); this sample does not
modify component-runtime internals.
