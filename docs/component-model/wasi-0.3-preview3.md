# WASI 0.3 / Preview 3 Tracking

This page records Telomere's WASI 0.3 / Preview 3 target snapshot and support
policy. The user-facing term "WASI 3.0" is treated here as WASI 0.3 / WASI P3.

## Snapshot Pin

Checked on 2026-05-19:

- WASI 0.3 is still described by WASI.dev as a forthcoming release.
- WASI.dev lists the proposed WASI 0.3 draft interfaces as clocks, random,
  filesystem, sockets, CLI, and HTTP.
- The pinned WIT source for implementation work is the official
  `WebAssembly/WASI` repository, branch `main`, under
  `proposals/{cli,clocks,filesystem,random,sockets}/wit-0.3.0-draft`.
  Telomere vendors that snapshot under
  `crates/telomere-component-wasi/wit-preview3/`. The existing
  `crates/telomere-component-wasi/wit/` directory remains the WASI 0.2.6
  compatibility input for provider bindgen.
- The package version observed for those P3 proposal packages is
  `0.3.0-rc-2026-03-15`.
- `wasi:io` is not present as a `proposals/io/wit-0.3.0-draft` package in the
  official `WebAssembly/WASI` repository. Telomere's current `wasi:io/*@0.2.8`
  registration is therefore documented as a compatibility bridge for existing
  pollable/stream resource tests, not as a claim of a separate WASI P3 I/O
  package.

Reference URLs checked for this pin:

- <https://wasi.dev/interfaces>
- <https://wasi.dev/roadmap>
- <https://github.com/WebAssembly/WASI/tree/main/proposals>
- <https://raw.githubusercontent.com/WebAssembly/WASI/main/proposals/cli/wit-0.3.0-draft/command.wit>
- <https://raw.githubusercontent.com/WebAssembly/WASI/main/proposals/filesystem/wit-0.3.0-draft/world.wit>
- <https://raw.githubusercontent.com/WebAssembly/WASI/main/proposals/sockets/wit-0.3.0-draft/world.wit>
- <https://github.com/WebAssembly/component-model>

Until WASI 0.3 reaches a final release, Telomere treats this as an RC snapshot
pin, not as a stable compatibility promise. Any later WIT refresh must update
this page with the source repository, commit or release tag, package versions,
and migration notes.

## Initial Scope

The first implementation target is the relation-driven component runtime plus
WASI provider support for:

| Interface family | Target package | Initial policy |
| --- | --- | --- |
| CLI | `wasi:cli@0.3.0-rc-2026-03-15` | Implement |
| Clocks | `wasi:clocks@0.3.0-rc-2026-03-15` | Implement |
| Random | `wasi:random@0.3.0-rc-2026-03-15` | Implement |
| Filesystem | `wasi:filesystem@0.3.0-rc-2026-03-15` | Implement preopens, read paths, and scoped read-write preopen mutations |
| Sockets | `wasi:sockets@0.3.0-rc-2026-03-15` | Implement official `wasi:sockets/types` TCP/UDP resource allocation, `get-address-family`, local literal-IP lookup through `wasi:sockets/ip-name-lookup`, and explicit `not-supported`/`Unsupported` paths for operations that would require connected I/O, DNS, or readiness |
| I/O compatibility | `wasi:io@0.2.8` | Compatibility bridge only; P3 uses Component Model `stream<T>` / `future<T>` handles |
| HTTP | `wasi:http@0.3.0-rc-2026-03-15` | Not in the initial implementation unless explicitly requested; imports must fail closed |

Current implementation status:

- `telomere_component_wasi::preview3::add_to_linker_async` registers the
  pinned 0.3 RC package names for `wasi:cli/environment`,
  `wasi:cli/exit`, `wasi:cli/stdin`, `wasi:cli/stdout`, `wasi:cli/stderr`,
  `wasi:filesystem/preopens`,
  `wasi:clocks/wall-clock`, `wasi:clocks/monotonic-clock`,
  `wasi:random/random`, `wasi:random/insecure`,
  `wasi:random/insecure-seed`, `wasi:sockets/types`, and
  `wasi:sockets/ip-name-lookup`.
- `wasi:io/error`, `wasi:io/poll`, and `wasi:io/streams` are registered at
  `0.2.8` as a compatibility bridge while the P3 stream/future canonical ABI is
  still being implemented.
- `wasi:cli/stdin.read-via-stream` now returns the official P3
  `tuple<stream<u8>, future<result<_, error-code>>>` shape for buffered/local
  stdin streams. `wasi:cli/stdout.write-via-stream` and
  `wasi:cli/stderr.write-via-stream` accept P3 `stream<u8>` handles and return
  `future<result<_, error-code>>` handles after draining currently available
  local input-stream data into the selected stdio buffer. The returned future
  handle can be consumed by Telomere's local Component Model `future.read` /
  waitable event path when it is ready.
- `wasi:cli/exit.exit` records the payloadless `result` status in `WasiState`
  and traps to terminate the command path, matching the existing 0.2.x provider
  policy.
- `wasi:io/poll` covers the pollable resource method path used by monotonic
  clock subscriptions and local stream readiness. Async P2/P3 poll and block
  calls now use the shared WASI substrate and return a real pending Rust future
  for non-ready monotonic deadlines. When reached from a core guest through an
  async canonical lower trampoline, that future is submitted to Telomere's core
  scheduler as an async host-call pending operation and resumes the original
  task after the Tokio timer completes. Sync calls remain ready-only and return
  `Unsupported` for non-ready pollables.
- `wasi:clocks/monotonic-clock` supports the pinned P3 `get-resolution`,
  `wait-for`, and `wait-until` names. Future deadlines suspend on the caller
  task using `tokio::time::sleep` and then resume through the same component
  async call graph.
- `wasi:io/streams` currently covers the same local stream model as the 0.2.6
  provider: buffered stdin reads, stdout/stderr writes, flush/check-write,
  stream subscriptions, write-zeroes, and splice. It does not yet provide a
  full socket/file reactor; inherited host stdin readiness is handled through a
  non-store-capturing Tokio blocking readiness wait on Unix.
- `wasi:filesystem/preopens` exposes the configured preopened directories, and
  the `wasi:filesystem/types` descriptor path currently covers `get-type`,
  `get-flags`, `open-at`, `read`, `write`, `read-via-stream`, and
  `read-directory` / `directory-entry-stream.read-directory-entry` operations.
  The default `WasiState::builder().preopen_dir(...)` remains read-only.
  `preopen_dir_read_write(...)` opts a preopen into `write` and
  `mutate-directory`, enabling scoped `open-at` creation/truncation,
  `write`, `set-size`, `create-directory-at`, `rename-at`, `unlink-file-at`,
  and `remove-directory-at` within the preopen. Path resolution still rejects
  absolute paths and lexical escapes.
- `wasi:sockets/types` implements the official P3 resource shape:
  `[static]tcp-socket.create`, `[static]udp-socket.create`,
  `[method]tcp-socket.get-address-family`, and
  `[method]udp-socket.get-address-family` allocate and report local socket
  handle state. Option setters currently return `error-code::not-supported`;
  operations that have no immediate result channel, such as TCP send/receive,
  return `Unsupported` until connected I/O is implemented.
- `wasi:sockets/ip-name-lookup.resolve-addresses` resolves literal IPv4/IPv6
  strings locally and fails closed for real DNS lookup.
- The Preview 3 provider remains a distinct public entrypoint, but P2 and P3
  now share an internal WASI substrate for pollable readiness, stream tables,
  stdio handles, filesystem descriptors, and monotonic timer readiness. The
  existing 0.2.6 `add_to_linker_sync` / `add_to_linker_async` exports are kept
  as compatibility adapters over that substrate.
- Remaining filesystem operations such as hard links, symlinks, timestamp
  updates, stream-backed writes/appends, connected socket I/O, blocking
  wait/cancellation semantics, and subtask handles are still in-progress.
  Components that require those P3 interfaces must fail closed until their
  provider bindings and runtime tests are added.

HTTP is listed by WASI.dev as a proposed WASI 0.3 interface, but Telomere's
first P3 pass keeps it out of the default provider scope to avoid turning the
component runtime work into a full HTTP runtime. Components importing
`wasi:http/*@0.3.0-rc-2026-03-15` must receive a clear unsupported import or
validation error until HTTP support is deliberately added.

## Detailed Availability Checklist

This checklist is the source of truth for what "WASI P2/P3 support" currently
means in Telomere. Checked items are implemented and covered by local tests or
existing provider tests. Unchecked items must fail closed with `Unsupported`,
`not-supported`, validation errors, or unresolved imports rather than silently
falling back to another WASI version.

### WASI P2 / 0.2.x compatibility

- [x] `wasi:*@0.2.6` provider entrypoints remain available through
  `telomere_component_wasi::add_to_linker_sync` and
  `telomere_component_wasi::add_to_linker_async`.
- [x] Stable `0.2.x` imports resolve with Wasmtime-style semver fallback after
  exact-name lookup fails, for example a `@0.2.0` component import can resolve
  against the `@0.2.6` provider registration when the type surface matches.
- [x] CLI environment and arguments are available through
  `wasi:cli/environment@0.2.x`.
- [x] CLI exit records the exit status in `WasiState` and traps to terminate the
  command path.
- [x] CLI stdio resources are available for local buffered stdin and captured
  stdout/stderr.
- [x] Random and insecure random providers are available.
- [x] Wall and monotonic clock now/resolution calls are available.
- [x] Monotonic clock subscriptions create pollable handles on the shared WASI
  substrate.
- [x] Async P2 `wasi:io/poll.poll` and `pollable.block` can suspend on
  non-ready monotonic timer pollables and resume through the existing component
  async host-call path.
- [x] Sync P2 poll/block remains ready-only and rejects non-ready pollables
  instead of blocking a local `Store`.
- [x] Local stream operations cover buffered/file stdin reads, stdout/stderr
  writes, check-write, flush, stream subscriptions, write-zeroes, and splice.
- [x] Filesystem preopens, descriptor metadata, read paths, directory reads, and
  scoped read-write preopen mutations are available through the existing P2
  provider surface.
- [x] Socket resource interfaces are registered for P2 compatibility where WIT
  bindgen provides them.
- [ ] P2 connected socket I/O is not implemented.
- [ ] P2 full DNS lookup is not implemented.
- [ ] P2 full file/socket reactor readiness is not implemented.

### WASI P3 / 0.3.0 RC provider

- [x] P3 provider entrypoint is available as
  `telomere_component_wasi::preview3::add_to_linker_async`.
- [x] P3 target WIT is pinned to `0.3.0-rc-2026-03-15` for CLI, clocks,
  random, filesystem, and sockets proposal packages.
- [x] P3 prerelease package names require exact-name lookup; the stable semver
  fallback intentionally does not apply to `0.3.0-rc-*`.
- [x] `wasi:cli/environment` supports `get-environment`, `get-arguments`, and
  `initial-cwd`.
- [x] `wasi:cli/exit.exit` records payloadless result status in `WasiState` and
  traps to terminate the command path.
- [x] `wasi:cli/stdin.get-stdin`, `wasi:cli/stdout.get-stdout`, and
  `wasi:cli/stderr.get-stderr` return local stream resources.
- [x] `wasi:cli/stdin.read-via-stream` returns the P3
  `tuple<stream<u8>, future<result<_, error-code>>>` shape for local stdin.
- [x] `wasi:cli/stdout.write-via-stream` and
  `wasi:cli/stderr.write-via-stream` accept P3 `stream<u8>` handles and return
  local future handles after draining currently available stream data.
- [x] `wasi:clocks/wall-clock.now` and `resolution` are available.
- [x] `wasi:clocks/monotonic-clock.now`, `resolution`, and `get-resolution` are
  available.
- [x] P3 `monotonic-clock.wait-for` and `wait-until` can suspend on future
  deadlines using `tokio::time::sleep` and resume on the caller task.
- [x] P3 `monotonic-clock.subscribe-instant` and `subscribe-duration` create
  pollable handles on the shared WASI substrate.
- [x] `wasi:io/error`, `wasi:io/poll`, and `wasi:io/streams` are registered at
  `0.2.8` as the current compatibility bridge for P3 local stream/poll tests.
- [x] P3 async `wasi:io/poll.poll` and `pollable.block` can suspend on
  non-ready monotonic timer pollables and resume through the existing component
  async host-call path.
- [x] P3 `wasi:io/streams` local stream methods cover read, blocking-read,
  skip, blocking-skip, subscribe, check-write, write, blocking-write, flush,
  blocking-flush, write-zeroes, blocking-write-zeroes, splice, and
  blocking-splice for the same local stream model as P2.
- [x] `wasi:random/random` and `wasi:random/insecure` are available.
- [x] `wasi:random/insecure-seed` is registered.
- [x] `wasi:filesystem/preopens.get-directories` exposes configured preopens.
- [x] `wasi:filesystem/types` covers descriptor `get-type`, `get-flags`,
  `open-at`, `read`, `write`, `read-via-stream`, `read-directory`, and
  `directory-entry-stream.read-directory-entry`.
- [x] `preopen_dir(...)` remains read-only by default.
- [x] `preopen_dir_read_write(...)` enables scoped create/truncate/write,
  `set-size`, `create-directory-at`, `rename-at`, `unlink-file-at`, and
  `remove-directory-at` inside the preopen.
- [x] Filesystem path resolution rejects absolute paths and lexical escapes.
- [x] `wasi:sockets/types` supports P3 TCP/UDP socket resource allocation.
- [x] `wasi:sockets/types` supports TCP/UDP `get-address-family`.
- [x] Socket option setters that have a WASI error result return
  `error-code::not-supported`.
- [x] `wasi:sockets/ip-name-lookup.resolve-addresses` resolves literal IPv4 and
  IPv6 strings locally.
- [ ] P3 HTTP is not implemented or registered by default.
- [ ] P3 real DNS lookup is not implemented.
- [ ] P3 connected TCP/UDP send, receive, bind, connect, listen, accept, and
  shutdown I/O is not implemented.
- [ ] P3 full socket readiness is not implemented.
- [ ] P3 stream-backed filesystem writes/appends are not complete.
- [ ] P3 hard links, symlinks, and timestamp updates are not implemented.
- [ ] P3 blocking waitable delivery without an already-ready local event is not
  implemented.
- [ ] P3 subtask handles and subtask cancellation runtime behavior are not
  implemented.
- [ ] P3 cancellation semantics that require scheduler-blocking cancellation
  state are not implemented.
- [ ] P3 `core type` / `gc` canonical runtime execution is not implemented.

### Shared WASI substrate

- [x] P2 and P3 share a local substrate for pollable readiness, local stream
  tables, stdio handles, filesystem descriptors, and monotonic timer readiness.
- [x] The substrate does not introduce a separate execution engine or event
  loop; async readiness is exposed as Rust futures and uses Telomere's existing
  component/core async call path.
- [x] Non-ready monotonic timer pollables produce real Pending/Resume behavior
  in P2 and P3 regression tests.
- [x] Inherited Unix stdin readiness can wait through a non-store-capturing
  Tokio blocking task.
- [x] Sync APIs remain ready-only and fail closed for non-ready pollables.
- [ ] The substrate does not yet provide a full file reactor.
- [ ] The substrate does not yet provide connected socket reactor integration.
- [ ] The substrate does not yet model DNS request lifecycle resources.

## Component Model Features

| Feature | Policy |
| --- | --- |
| Async WIT functions | Implement as local futures without requiring `Send` |
| `future<T>` | Add parser, IR, typed API, runtime handles, canonical `future.new` / `future.drop-{readable,writable}`, ready local `future.read` / `future.write` payload transfer, and `future.cancel-{read,write}` for local pending/queued state. `async` read returns `BLOCKED` when the future is not ready; sync not-ready reads trap |
| `stream<T>` | Add parser, IR, typed API, runtime handles, canonical `stream.new` / `stream.drop-{readable,writable}`, ready local `stream.read` / `stream.write` queue transfer with `CopyResult` progress, and `stream.cancel-{read,write}` for local pending/queued state. `async` reads return `BLOCKED` when no element is ready; sync not-ready reads trap |
| `error-context` | Implement parser, value representation, canonical ABI `new` / `debug-message` / `drop`, runtime handle table, and bindgen type |
| Waitable sets | Implement `waitable-set.new` / `waitable-set.drop`, `waitable.join`, and ready event delivery through `waitable-set.poll` for local stream/future read completion. `waitable-set.wait` can return an already-pending event but still fails closed instead of blocking when no event is ready. The cancellable flag is accepted for the same local ready-event path; cancellation while blocked remains unsupported because blocking wait is still fail-closed |
| Canonical option `async` | Feature-gated decode and validation; `canon lower ... async` runs through the core scheduler and can resume a pending host future; `canon lift ... async` supports stackful `task.return` for local guest exports; subtask cancellation and blocking waitable delivery remain fail-closed |
| Canonical option `callback` | Feature-gated decode and runtime execution for stackless async lifts. `EXIT` completes through `task.return`, `YIELD` immediately re-enters the callback with an empty event, and `WAIT` can deliver an already-ready local waitable event. Blocking `WAIT` remains fail-closed |
| Canonical option `core type` / `gc` | Decode function core-type references and reject invalid shapes; runtime execution currently fails closed with `Unsupported` |
| Cancellation | Local stream/future cancel built-ins clear pending read or queued write state. `task.cancel` is implemented as a local acknowledgement when no cancellation request state is tracked. `subtask.cancel` / `subtask.drop` decode and validate with the canonical signatures, but runtime execution fails closed until async lower creates real subtask handles. Cancellation that would require scheduler blocking remains fail-closed |
| Threads / `Send` store | Non-goal; `Store` remains local / `!Send` |

The runtime must continue to use `ComponentProgram` relation snapshots as the
only resolution source. Instantiation and calls must not rescan component
binary bytes.

## Compatibility

The existing WASI 0.2.6 provider remains the compatibility baseline while P3 is
added. Existing `wasi:cli/command@0.2.6` component commands and synchronous
component parity tests must keep passing.

The P2 provider is now an adapter over the same local WASI substrate used by
P3. Async P2 poll/block paths can suspend on shared substrate timers. Sync P2
poll/block paths intentionally stay ready-only; components that need non-ready
pollables must use `add_to_linker_async`.

Component import lookup follows Wasmtime-style semver compatibility for stable
component package names. A provider registration such as
`wasi:cli/environment@0.2.6` can satisfy imports in the same `0.2` track after
exact-name lookup fails. Stable `1.x` package names share a major-version
track. `0.0.x`, prerelease names such as `0.3.0-rc-2026-03-15`, unversioned
names, and invalid semver strings do not participate in fallback lookup.
Fallback only resolves the name; the existing type validation still decides
whether the resolved provider actually satisfies the component import surface.

If a P3 API cannot share the current `telomere-component-wasi` public surface
without ambiguity, it should be introduced behind a distinct module or feature
boundary and documented here before the 0.2.6 API is changed.

## Evidence Required Before Claiming Support

Ready-only futures are not enough evidence for WASI P3 support. At minimum, the
test suite must demonstrate a host future that returns `Poll::Pending` before
resuming, a guest async export, async canonical lift/lower across a host import,
`future<T>` / `stream<T>` decode and validation, `error-context` canonical
built-in execution, P2 and P3 WASI pollable paths that suspend and resume on the
shared substrate, and non-regression for WASI 0.2.6 plus synchronous component
parity.
