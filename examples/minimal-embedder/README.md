# Minimal embedder

This crate contains four standalone embedding samples. Each row is built and
run through its own Cargo invocation so that Cargo feature unification cannot
pull a higher layer into a lower-layer measurement.

"Minimal" means the minimal supported dependency topology, not the minimum
achievable byte count. The headline configurations retain SIMD because it is a
supported default; core-nosimd is included to make the byte-cost difference
visible.

Run all commands from the repository root. The binaries accept the fixture path
as their only argument and fail with a non-zero status if parsing,
instantiation, execution, or the expected result fails.

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
