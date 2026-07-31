# Footprint and Cold Start

This note records measured binary size, peak resident memory, and process cold
start for the `telomere-cli` host binary. Every number below was produced by the
commands in this document on the machine described in
[Environment](#environment). Nothing here is estimated, and no number for another
runtime is reported unless it was measured here or quoted with a source.

## Scope and honesty boundary

- This measures the **CLI host binary**, not a minimal embedder. `telomere-cli`
  links `clap` and a multi-thread `tokio` runtime on top of the runtime crates,
  so these numbers are an upper bound on what an embedded host would carry.
- Telomere's stated goal is an embedded-class Component Model runtime. The
  numbers below are the current starting point, **not** evidence that the goal
  has been reached. A ~4 MiB host binary is roughly two orders of magnitude
  larger than the smallest published WAMR interpreter footprint (see
  [Other runtimes](#other-runtimes)).
- `riscv64` and Linux numbers are **not yet measured**. See
  [Not yet measured](#not-yet-measured).

## Environment

| Item | Value |
| --- | --- |
| Date | 2026-08-01 |
| Host | macOS 26.5.2 (Darwin 25.5.0), arm64 |
| CPU | Apple M2 Pro |
| RAM | 32 GiB |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)`, host `aarch64-apple-darwin` |
| Telomere commit | `60b45c9` plus the example fixtures added on this branch |
| Release profile | workspace default: `codegen-units = 1`, `lto = "thin"` |
| `strip` | Apple `strip` from the active Xcode command line tools |

`hyperfine` was not available on this machine, so cold start was measured with a
repeated-run median instead. Both methods used are given below.

## Binary size

```shell
cargo build --release                          # default features (= full)
cargo build --release --no-default-features
cargo build --release --features jit
# after each build:
ls -l target/release/telomere-cli
cp target/release/telomere-cli /tmp/telomere-cli-stripped
strip /tmp/telomere-cli-stripped
ls -l /tmp/telomere-cli-stripped
```

| Build | Unstripped | Stripped |
| --- | ---: | ---: |
| `--release` (default features: `full`) | 4,174,816 B (3.98 MiB) | 3,435,280 B (3.28 MiB) |
| `--release --no-default-features` | 4,174,848 B (3.98 MiB) | 3,435,272 B (3.28 MiB) |
| `--release --features jit` | 4,693,968 B (4.48 MiB) | 3,870,344 B (3.69 MiB) |

Observations:

- Disabling the default `full` feature (core Wasm `simd` and `threads`) does not
  measurably change the binary size on this target. The 32-byte difference is
  below any meaningful resolution.
- The experimental JIT costs about 519 KB unstripped / 435 KB stripped.

## Peak resident memory

Peak RSS is the `maximum resident set size` line from `/usr/bin/time -l`, taken
as the median of 5 runs. On Linux use `/usr/bin/time -v` and read
`Maximum resident set size`. `tools/measure-cold-start.py` handles both platforms
and reports this alongside cold start.

```shell
/usr/bin/time -l target/release/telomere-cli examples/add.wasm main 1 2
/usr/bin/time -l target/release/telomere-cli --jit examples/add.wasm main 1 2      # jit build
/usr/bin/time -l target/release/telomere-cli examples/wasi-preview1-hello.wasm
/usr/bin/time -l target/release/telomere-cli component examples/wasi-component-args.wasm -- one
```

| Workload | Peak RSS (median of 5) |
| --- | ---: |
| `examples/add.wasm` core call, interpreter | 3,473,408 B (3,392 KiB) |
| `examples/add.wasm` core call, `--jit` | 3,620,864 B (3,536 KiB) |
| `examples/wasi-preview1-hello.wasm` preview1 command | 4,014,080 B (3,920 KiB) |
| `examples/wasi-component-args.wasm` WASI 0.2 component | 4,997,120 B (4,880 KiB) |

`examples/add.wasm` declares a 16-page (1 MiB) linear memory, so part of the core
row is the guest memory rather than runtime overhead.

## Cold start

Cold start here is whole-process wall time: `exec` through exit, including
parsing, instantiation, and the guest call. Two independent methods were used
and agree.

Method 1, median of 30 runs after 3 warm-up runs, driven by
`tools/measure-cold-start.py`:

```shell
python3 tools/measure-cold-start.py
```

| Workload | Median | Min | Max |
| --- | ---: | ---: | ---: |
| `examples/add.wasm` core call, interpreter | 3.41 ms | 3.15 ms | 4.96 ms |
| `examples/add.wasm` core call, `--jit` | 3.42 ms | 3.09 ms | 3.86 ms |
| `examples/wasi-preview1-hello.wasm` preview1 command | 3.71 ms | 3.44 ms | 4.14 ms |
| `examples/wasi-component-args.wasm` WASI 0.2 component | 3.90 ms | 3.65 ms | 4.14 ms |

Method 2, 100 sequential runs timed as a block and divided by 100, repeated
three times:

```shell
/usr/bin/time -p sh -c 'for i in $(seq 1 100); do \
  target/release/telomere-cli examples/add.wasm main 1 2 >/dev/null; done'
```

| Workload | Observed totals for 100 runs | Per run |
| --- | --- | ---: |
| `examples/add.wasm` core call | 0.36 s, 0.35 s, 0.32 s | ~3.2-3.6 ms |
| `examples/wasi-component-args.wasm` component | 0.38 s, 0.38 s, 0.38 s | ~3.8 ms |

Both methods include the operating system's process creation cost, so they are
an upper bound on the runtime's own start-up cost. They are not an in-process
instantiate-only measurement.

## Other runtimes

No other runtime was built or measured for this note. Two facts are quoted from
upstream sources so the size comparison in the repository README has something
checkable behind it:

- WAMR publishes, for its core `vmlib`: "Small runtime binary size (core vmlib on
  cortex-m4f with tail-call/bulk memory/shared memory support, text size from
  bloaty) * ~58.9K for fast interpreter * ~56.3K for classic interpreter *
  ~29.4K for aot runtime * ~21.4K for libc-wasi library * ~3.7K for
  libc-builtin library". Source:
  <https://github.com/bytecodealliance/wasm-micro-runtime> (README, retrieved
  2026-08-01). That is a library text size on Cortex-M4F and is **not**
  comparable like-for-like with the macOS arm64 CLI binary measured above.
- Wasmtime lists `component-model` and the WASI 0.2 interface proposals
  (`wasi-io`, `wasi-clocks`, `wasi-filesystem`, `wasi-random`, `wasi-sockets`,
  `wasi-http`) plus `wasi_snapshot_preview1` in its Tier 1 support table.
  Source: <https://docs.wasmtime.dev/stability-tiers.html> (retrieved
  2026-08-01).

A wasmtime or WAMR binary size figure is deliberately **not** given here. A fair
comparison requires building each runtime from source with matched features,
target, and optimization settings, which this note does not do.

The repository already has a separate, previously recorded execution-speed
comparison in [../core/coremark-benchmark.md](../core/coremark-benchmark.md).
That note was measured on a different date and is not re-measured here.

## Not yet measured

The following are open and should not be inferred from the numbers above:

- `riscv64gc-unknown-linux-gnu` binary size, RSS, and cold start. This is the
  target that matters most for the embedded plugin-host use case, and it is
  currently only cross-checked for compilation in CI, never measured for
  footprint. Measuring it needs either hardware or a QEMU user-mode setup.
- Linux `x86_64` and `aarch64` numbers.
- Minimum-embedder footprint: a host binary that links only `telomere` and
  `telomere-component` without `clap` and `tokio`. This is the number that would
  actually be comparable with WAMR's `vmlib` text size.
- Static memory ceiling under a constrained RAM budget (the 256 MiB class device
  target).
- Instantiation-only latency, separated from process start-up.

## Reproducing

```shell
git submodule update --init --recursive
cargo build --release
python3 tools/measure-cold-start.py
```

The script lives in `tools/` rather than in this directory because `docs/` holds
prose, and because `cargo bench` only picks up `.rs` targets in a crate's
`benches/`, so a Python harness there would never run. It prints JSON and takes
no arguments. It expects to be run from the repository root with
`target/release/telomere-cli` already built, and it aborts rather than reporting
a number if any invocation exits non-zero. It includes the `--jit` row only when
`TELOMERE_JIT_BIN` points at a binary built with `--features jit`:

```shell
cargo build --release --features jit
cp target/release/telomere-cli /tmp/telomere-cli-jit
cargo build --release
TELOMERE_JIT_BIN=/tmp/telomere-cli-jit python3 tools/measure-cold-start.py
```

The tables above were filled in from a single run of that sequence.
