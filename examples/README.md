# Examples

Small, runnable fixtures for the three entry points of `telomere-cli`. Every
command in this file was executed against the committed `.wasm` files before it
was written down.

| File | Entry point | Source |
| --- | --- | --- |
| `add.wasm` | core Wasm export call | Compiled from Rust; no source kept in-tree. |
| `wasi-preview1-hello.wasm` | WASI preview1 command module | `wasi-preview1-hello.wat` |
| `wasi-component-args.wasm` | WASI 0.2 component command | `wasi-component-args.wat` |

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

This companion fixture calls `wasi:cli/stdout@0.2.6.get-stdout` and then
`wasi:io/streams@0.2.6.[method]output-stream.blocking-write-and-flush` through
the canonical ABI. It deliberately does not call `canon resource.drop`, because
dropping a host-provided resource is still unsupported (see below).

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
wasm-tools parse examples/wasi-preview1-hello.wat -o examples/wasi-preview1-hello.wasm
wasm-tools parse examples/wasi-component-args.wat -o examples/wasi-component-args.wasm
wasm-tools validate --features all examples/wasi-component-args.wasm
wasm-tools parse examples/wasi-component-stdout.wat -o examples/wasi-component-stdout.wasm
wasm-tools validate --features all examples/wasi-component-stdout.wasm
```

They are committed as `.wat` sources rather than generated from Rust because of
the two toolchain gaps described next.

## Why these fixtures use WAT sources

### The preview1 sample cannot come from `wasm32-wasip1`

A plain `fn main()` built for `wasm32-wasip1` imports `environ_get` and
`environ_sizes_get`, which the CLI's preview1 runner does not implement:

```shell
cargo build --release --target wasm32-wasip1     # in a scratch crate
target/release/telomere-cli path/to/guest.wasm
```

```text
unsupported `wasi_snapshot_preview1` imports: environ_get, environ_sizes_get
```

The implemented host functions are `args_sizes_get`, `args_get`,
`clock_time_get`, `fd_close`, `fd_fdstat_get`, `fd_seek`, `fd_write`, and
`proc_exit` (see `src/core_wasi_preview1.rs`). `wasi-preview1-hello.wat` stays
inside that set.

### The component sample cannot come from `wasm32-wasip2`

Two separate issues:

1. **Export version.** A `wasm32-wasip2` build from current Rust imports the
   `@0.2.6` WASI interfaces but exports `wasi:cli/run@0.2.0`. The CLI looks up
   `wasi:cli/run@0.2.6` exactly, so such a component is rejected:

   ```text
   failed to invoke `wasi:cli/run.run`

   Caused by:
       export not found: wasi:cli/run@0.2.6
   ```

   Telomere does not currently do semver-compatible interface matching on
   component exports.

2. **Host-provided resource destruction is still unavailable.** Components can
   lower a WASI import whose result is an owned resource handle - for example
   `wasi:cli/stdout@0.2.6.get-stdout`, which returns
   `own<wasi:io/streams.output-stream>` - and use that handle for stream
   operations, as `wasi-component-stdout.wat` demonstrates. However, calling
   `canon resource.drop` for a host-provided resource is not supported yet.
   Keep fixtures that exercise this path free of an explicit resource drop until
   lifecycle support lands.

   Imports with non-resource results also continue to work, including ones that
   need linear memory and `realloc` such as
   `wasi:cli/environment@0.2.6.get-arguments`, which is what
   `wasi-component-args.wat` uses. Unresolved imports are reported cleanly
   (`export not found: ...`).

Both points are reproducible from a clean checkout with the commands above.
