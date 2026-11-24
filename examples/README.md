# Examples

Small, runnable fixtures for the three entry points of `telomere-cli`. Every
command in this file was executed against the committed `.wasm` files before it
was written down.

| File | Entry point | Source |
| --- | --- | --- |
| `add.wasm` | core Wasm export call | Compiled from Rust; no source kept in-tree. |
| `component-add.wasm` | WASI-free Component Model export call | `component-add.wat` |
| `wasi-preview1-hello.wasm` | WASI preview1 command module | `wasi-preview1-hello.wat` |
| `wasi-preview1-stock-hello.wasm` | WASI preview1 command module | `wasi-preview1-stock-hello.rs` (stock `wasm32-wasip1`) |
| `wasi-component-args.wasm` | WASI 0.2 component command | `wasi-component-args.wat` |

## Standalone minimal embedder

The `telomere-minimal-embedder` crate in
[`minimal-embedder/`](minimal-embedder/README.md) deliberately excludes the CLI
dependency topology. Its standalone ladder uses `add.wasm` for the baseline and
core rows, `component-add.wasm` for the WASI-free Component Model row, and
`wasi-component-args.wasm` for the WASI 0.2 rows. Each configuration builds and
runs with its own feature invocation; see that crate's README for commands and
[the footprint note](../docs/benchmarks/footprint.md) for the measured boundary.

## Running

Core module export call:

```shell
cargo run -- examples/add.wasm main 1 2
```

```text
3
```

WASI preview1 command module. Guest argv goes after `--`; `argv[0]` is the
module file name:

```shell
cargo run -- examples/wasi-preview1-hello.wasm -- one two
```

```text
hello from telomere (wasi preview1)
wasi-preview1-hello.wasm
one
two
```

Any number of arguments works, and so does argv larger than the module's one
declared page of linear memory - the module reads `argc` and `argv_buf_size`
from `args_sizes_get` and grows memory when it has to:

```shell
cargo run -- examples/wasi-preview1-hello.wasm -- $(python3 -c "print(' '.join('y'*100 for _ in range(1000)))") | wc -l
```

```text
    1002
```

Stock `wasm32-wasip1` command module - a plain Rust binary, not WAT. It
prints its argv, reports one environment variable, seeds a `HashMap`, and
reports how many bytes it read from stdin:

```shell
printf abc | cargo run --release -- examples/wasi-preview1-stock-hello.wasm -- one
```

```text
stock wasip1 hello
argc=2
arg=wasi-preview1-stock-hello.wasm
arg=one
fixture-env=<unset>
hashmap=42
stdin-len=3
```

The guest environment is the host process environment, as with the
`component` subcommand; `--env KEY=VALUE` and `--no-inherit-env` apply to
the preview1 path too:

```shell
printf abc | TELOMERE_PREVIEW1_FIXTURE=present cargo run --release -- examples/wasi-preview1-stock-hello.wasm -- one
```

```text
stock wasip1 hello
argc=2
arg=wasi-preview1-stock-hello.wasm
arg=one
fixture-env=present
hashmap=42
stdin-len=3
```

WASI 0.2 component command. This one reports its result through the process
exit status so that the fixture stays focused on argument passing:

```shell
cargo run -- component examples/wasi-component-args.wasm -- one ; echo $?
```

```text
0
```

```shell
cargo run -- component examples/wasi-component-args.wasm ; echo $?
```

```text
1
```

### WASI 0.2 stdout component command

This companion fixture calls `wasi:cli/stdout@0.2.6.get-stdout`, writes
through `wasi:io/streams@0.2.6.[method]output-stream.blocking-write-and-flush`,
and then releases the stream handle with `canon resource.drop`, exercising
the full resource lifecycle through the canonical ABI.

```shell
wasm-tools parse examples/wasi-component-stdout.wat -o examples/wasi-component-stdout.wasm
cargo run --release -- component examples/wasi-component-stdout.wasm
```

Expected output:

```text
hello from telomere (wasi 0.2 component)
```

## Rebuilding the `.wat` sources

The WASI fixtures are committed as WebAssembly text and are rebuilt with
[`wasm-tools`](https://github.com/bytecodealliance/wasm-tools):

```shell
cargo install wasm-tools
wasm-tools parse examples/component-add.wat -o examples/component-add.wasm
wasm-tools validate --features all examples/component-add.wasm
wasm-tools parse examples/wasi-preview1-hello.wat -o examples/wasi-preview1-hello.wasm
wasm-tools parse examples/wasi-component-args.wat -o examples/wasi-component-args.wasm
wasm-tools validate --features all examples/wasi-component-args.wasm
wasm-tools parse examples/wasi-component-stdout.wat -o examples/wasi-component-stdout.wasm
wasm-tools validate --features all examples/wasi-component-stdout.wasm
```

The stock fixture is the one exception: it is a real `wasm32-wasip1`
binary, rebuilt with the workspace toolchain once the target is installed:

```shell
rustup target add wasm32-wasip1
rustc --edition 2021 --target wasm32-wasip1 -O -C strip=symbols examples/wasi-preview1-stock-hello.rs -o examples/wasi-preview1-stock-hello.wasm
```

The remaining fixtures are committed as `.wat` sources rather than generated
from Rust because of the toolchain gap described next.

## Why these fixtures use WAT sources

### The preview1 WAT sample predates stock `wasm32-wasip1` support

A plain `fn main()` built for `wasm32-wasip1` imports `environ_get` and
`environ_sizes_get`, which the CLI's preview1 runner originally did not
implement, so the first preview1 fixture had to be hand-written WAT. The
runner now implements those plus `fd_read` and `random_get` (#137), and
`wasi-preview1-stock-hello.wasm` is the stock-binary fixture that proves
it. The implemented host functions are `args_sizes_get`, `args_get`,
`clock_time_get`, `environ_sizes_get`, `environ_get`, `fd_close`,
`fd_fdstat_get`, `fd_read`, `fd_seek`, `fd_write`, `proc_exit`, and
`random_get` (see `src/core_wasi_preview1.rs` and
`docs/core/preview1-scope.md`). `wasi-preview1-hello.wat` stays inside the
original subset.

The preview1 filesystem functions remain unimplemented by decision. A
stock program that opens files - for example one calling
`std::fs::read_to_string` - imports `fd_prestat_get`,
`fd_prestat_dir_name`, `path_open`, and `fd_filestat_get` and fails closed
before it runs:

```shell
cargo build --release --target wasm32-wasip1     # in a scratch crate
target/release/telomere-cli path/to/guest.wasm
```

```text
unsupported `wasi_snapshot_preview1` imports: fd_filestat_get, fd_prestat_dir_name, fd_prestat_get, path_open (the preview1 runner implements a scoped subset of preview1; see docs/core/preview1-scope.md)
```

### The component sample could not come from `wasm32-wasip2`

Two issues blocked it; both are fixed:

1. **Export version (fixed).** A `wasm32-wasip2` build from current Rust
   imports the `@0.2.6` WASI interfaces but may export `wasi:cli/run` at an
   older patch version such as `@0.2.0`. Component export lookup now applies
   the ecosystem's semver rules instead of requiring an exact version match:
   versions on the same track (the same major, or the same minor within
   `0.x`) resolve to each other, so such a component runs. Genuinely
   incompatible versions are still rejected, with an error that names both
   the version that was found and the version that was expected:

   ```text
   failed to invoke `wasi:cli/run.run`

   Caused by:
       export not found: wasi:cli/run@0.2.6 (found incompatible version wasi:cli/run@0.3.0)
   ```

2. **Host-provided resource destruction (fixed).** Components can lower a
   WASI import whose result is an owned resource handle - for example
   `wasi:cli/stdout@0.2.6.get-stdout`, which returns
   `own<wasi:io/streams.output-stream>` - use that handle for stream
   operations, and release it with `canon resource.drop`, as
   `wasi-component-stdout.wat` demonstrates. The drop runs the host's
   destructor for the resource type, so the provider state behind the
   handle is released. Dropping the same handle twice, or a handle that
   was never issued, traps, as the canonical ABI requires.

   Imports with non-resource results also continue to work, including ones that
   need linear memory and `realloc` such as
   `wasi:cli/environment@0.2.6.get-arguments`, which is what
   `wasi-component-args.wat` uses. Unresolved imports are reported cleanly
   (`export not found: ...`).

With both fixed, a stock `wasm32-wasip2` hello-world (built with the
current stable Rust toolchain) now runs end to end under the CLI, printing
its output and exiting 0. The WAT fixtures above remain as the committed
regression fixtures for these paths.
