# Preview1 runner scope

Status: decision record for
[#137](https://github.com/yieldspace/telomere/issues/137), implemented in the
same change. This document is the contract for what the CLI's core WASI
preview1 runner (`src/core_wasi_preview1.rs`) does and does not provide.

## Decision

The preview1 runner covers the **non-filesystem** surface that stock
`wasm32-wasip1` command modules use, and nothing else. It implements twelve
host functions:

| Function | Source |
| --- | --- |
| `args_sizes_get`, `args_get` | original set |
| `clock_time_get` | original set |
| `fd_close`, `fd_fdstat_get`, `fd_seek`, `fd_write` | original set |
| `proc_exit` | original set |
| `environ_sizes_get`, `environ_get` | added by #137 |
| `fd_read` | added by #137 (fd 0 only) |
| `random_get` | added by #137 |

The filesystem family — `fd_prestat_get`, `fd_prestat_dir_name`, `path_open`
and the rest of the `path_*` functions, `fd_filestat_get`, and file-descriptor
operations beyond stdio — is **out of scope** for this runner, as are the
remaining preview1 functions (`poll_oneoff`, `sched_yield`, `clock_res_get`,
`proc_raise`, the `sock_*` family, and the other `fd_*` operations). A module
importing any of them fails closed at validation time, with every unsupported
import named:

```text
unsupported `wasi_snapshot_preview1` imports: fd_filestat_get, fd_prestat_dir_name, fd_prestat_get, path_open (the preview1 runner implements a scoped subset of preview1; see docs/core/preview1-scope.md)
```

Full preview1 compatibility is explicitly not the goal.

## Contract

- **Environment.** The guest environment is the host process environment,
  inherited by default — the same default as the `component` subcommand.
  `--env KEY=VALUE` (repeatable) adds or replaces entries, and
  `--no-inherit-env` suppresses inheritance.
- **`fd_read`** reads fd 0 (stdin) only. Fds 1 and 2 are write-only and any
  other descriptor is unknown; both cases return `ERRNO_BADF`, as does fd 0
  after `fd_close`. End of input is success with `nread = 0`.
- **`random_get`** fills from OS entropy via `getrandom`, the same source
  the component WASI random provider uses.
- **Everything else** about the original runner is unchanged: argv handling,
  the four clocks, stdio `fd_write`/`fd_seek`/`fd_fdstat_get` semantics, and
  `proc_exit` exit-code mapping.

## Evidence

Measured on fleur-du-lapin (x86_64 Linux) with stock Rust 1.99.1
`wasm32-wasip1` release binaries, before the change:

| Program shape | preview1 imports | Missing under the old 8-function set |
| --- | --- | --- |
| `println!` hello-world | `environ_get`, `environ_sizes_get`, `fd_write`, `proc_exit` | `environ_*` |
| argv + `env::var` | above + `args_get`, `args_sizes_get` | `environ_*` |
| stdin echo (`read_to_string`) | above + `fd_read` | `environ_*`, `fd_read` |
| `HashMap` insert/lookup | hello-world set + `random_get` | `environ_*`, `random_get` |
| `fs::read_to_string` | above + `fd_close`, `fd_fdstat_get`, `fd_filestat_get`, `fd_prestat_dir_name`, `fd_prestat_get`, `fd_read`, `path_open` | `environ_*`, `fd_read`, `fd_filestat_get`, `fd_prestat_*`, `path_open` |

So the non-filesystem stock surface needs exactly the four added functions,
and the filesystem family is a disjoint, larger set. After the change, the
first four program shapes run end to end under `telomere-cli` (exit 0,
expected stdout); the filesystem shape still fails closed with the message
above. The committed fixture `examples/wasi-preview1-stock-hello.wasm`
(source: `examples/wasi-preview1-stock-hello.rs`) exercises argv, environ,
`fd_read`, and `random_get` in one stock binary and is run by the CLI test
`preview1_stock_wasip1_fixture_runs_end_to_end`.

## Why the filesystem family stays out

- **There is nothing to extend incrementally.** The runner's descriptor
  space is a three-bit stdio mask: `fd_bit` maps only fds 0–2 and every
  other descriptor is `ERRNO_BADF` by construction. There is no fd table,
  no preopen concept, and no path-resolution code in the runner. Filesystem
  support means designing all three, including sandboxed path resolution
  (canonicalisation, `..` and symlink-escape prevention) — the capability
  class whose escape is the highest-severity outcome in `SECURITY.md`.
- **The capability already has a home.** The component stack is building
  filesystem access as `wasi:filesystem`
  ([#134](https://github.com/yieldspace/telomere/issues/134)) with preopen
  grants (`--dir` on the `component` subcommand). A second, divergent
  sandbox inside the preview1 runner would double that audit surface for a
  legacy ABI.
- **The cost asymmetry is measured, not assumed.** The four added functions
  mirror existing code (`environ_*` share the argv string-table helpers;
  `fd_read` mirrors `fd_write`'s iovec walk; `random_get` reuses the
  workspace's `getrandom`). The filesystem family mirrors nothing.

## Alternatives considered

- **(b) Components only.** Rejected. The preview1 runner is a documented,
  measured entry point (README WASI status, `examples/`, the footprint
  ladder's preview1 row, the support matrix), and its failure mode was
  already explicit — it names every unsupported import. Option (b)'s only
  gain over the status quo was a recorded scope statement, which this
  document provides without deleting a working path and stranding the
  runner's existing users (including CoreMark-style preview1 guests that
  use the process-CPU clock).
- **(c) Adapter path.** Rejected for now. The canonical preview1 adapter
  component itself imports the WASI 0.2 filesystem and sockets interfaces;
  telomere's filesystem provider is partial and read-only (#134) and its
  sockets are trapping stubs
  ([#135](https://github.com/yieldspace/telomere/issues/135)), so the
  adapter would move the gap rather than close it, at the cost of
  vendoring an external binary artifact with its own provenance. Revisit
  if #134/#135 reach full coverage.
- **Full (a), including a scoped `path_*` list.** Rejected for the reasons
  above: any honest `path_open` needs the fd table and the sandbox, at
  which point the list is no longer scoped.

## Known gaps this decision leaves

- Stock programs that sleep (`std::thread::sleep`) import `poll_oneoff`
  and still fail closed. A one-shot poll needs an event model (clock and
  fd subscriptions) the runner does not have; a stub that returns
  immediately would busy-spin guests, so this stays rejected until a real
  poll exists.
- Stock programs that open files fail closed, as documented above. Guests
  needing filesystem access should target `wasm32-wasip2` and the
  `component` subcommand, where preopens are explicit grants.
