# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- A standalone minimal-embedder configuration ladder, size-oriented release
  profile, and footprint harness/CI coverage for measured file size, text, RSS,
  and whole-process cold start.
- Opt-in Store-scoped interpreter metering with finite or unlimited fuel,
  cumulative fuel accounting, and cross-thread cancellation through
  `MeteringHandle`, including per-checkpoint cancellation observation and
  proportional precharges for native bulk operations. Fuel exhaustion and
  cancellation are exposed as payload-free `VMResult` variants with
  `InterruptReason` conversion helpers.
- Owned core trap diagnostics through `TrapInfo`, `TrapFrame`, `TrapKind`, and
  `Store::take_last_trap`, including optional retained producer names and
  function-granularity JIT reporting. Existing `VMResult` call signatures are
  unchanged.
- `VMResult` and `TrapKind` now implement `Display` (with the WebAssembly
  spec's trap messages where the spec defines one) and `std::error::Error`,
  and `TrapInfo` implements `std::error::Error`, so a failed guest call can
  flow through ordinary error handling. The faulting function for a trap is
  available from `Store::take_last_trap` as the first `TrapInfo` frame
  (function index, plus the producer name when a name section exists).
- `ComponentError::GuestTrap` preserves the core `TrapKind` for guest traps
  crossing the component boundary, so component-level callers can match on
  the trap cause instead of parsing a message string.
- The preview1 runner now implements `environ_sizes_get`, `environ_get`,
  `fd_read` (stdin), and `random_get`, so stock `wasm32-wasip1` command
  modules that use argv, the environment, stdio, clocks, and random run end
  to end (#137). The guest environment is inherited from the host process
  by default, matching the `component` subcommand; the core command path
  gains `--env KEY=VALUE` and `--no-inherit-env` for it. The preview1
  filesystem family remains unimplemented and fail-closed; the scope
  decision is recorded in `docs/core/preview1-scope.md`.
- `HostContext::with_memory` and `HostContext::with_caller_memory` closure
  accessors on the public host boundary (#216). Unlike `memory` /
  `caller_memory`, which return `None` when the default memory is shared,
  the closure form dispatches to a shared memory under its lock, restoring
  the reach the raw pre-#216 callback surface had.

### Changed

- **Breaking:** replaced the raw host-linking ABI with a designed public
  boundary (#216). Host callbacks are now `HostFn`/`AsyncHostFn` functions
  receiving a `HostContext` (decoded `WasmValue` arguments, the owning
  `Store`, guest-memory access) and returning `VMResult<Vec<WasmValue>>`;
  native modules are `HostModule`/`AsyncHostModule` built from
  `HostFnDefinition`/`AsyncHostFnDefinition`. The `telomere::host_abi`
  module is removed, `Module::codes` is crate-private, and the driver
  surface exposes opaque `HostCallFuture`/`HostCallOutcome` wrappers
  instead of interpreter continuations. The raw ABI (raw `ExecuteContext`
  callbacks, `NativeModule`, `link_raw_*` / `instantiate_raw_*` entry
  points) remains available behind the opt-in `unstable-internals`
  feature. The component-model host trampoline and the telomere test
  suite's ordinary host functions migrated to the new boundary; the CLI's
  Preview1 support stays on the raw opt-in surface pending #137.
- **Breaking:** split the spec trap causes out of `VMResult::InvalidOperand`.
  Integer division/remainder by zero now returns `VMResult::DivideByZero`,
  signed division overflow returns `VMResult::IntegerOverflow`, and a failed
  float-to-integer truncation returns `VMResult::InvalidConversion`, in both
  the interpreter and the JIT; `TrapKind` gained the matching variants.
  Exhaustive matches on `VMResult` or `TrapKind` must add the three arms.
  `InvalidOperand` remains for runtime-internal invariant violations, which
  validated guest execution cannot produce. Migration: replace any arm that
  relied on `InvalidOperand` covering arithmetic traps with the three new
  variants (or a wildcard), and prefer matching the new variants or reading
  `TrapInfo::kind` over comparing message strings. Trap causes raised inside
  host functions keep their identity across the host-call boundary through
  these variants, and the captured `TrapInfo` attributes the trap to the
  host frame; host functions still cannot attach their own error payload,
  because the public host ABI is a raw function pointer whose only failure
  channel is `VMResult` (that ABI's redesign is tracked by #216).
- Consolidated internal ad-hoc reentrancy guards around consumption of
  `lock_runtime`'s typed `Result`; existing fail-closed return values and the
  public API surface remain unchanged.
- Defined the Cargo feature policy and simplified feature wiring: the CLI keeps
  its `full` default while forwarding `simd` and `threads` directly, and
  `telomere` no longer has its redundant `full` alias.
- Added a `tools/check-feature-wiring.py` lint for bare optional-dependency
  keys that leak implicit Cargo features.
- Curated the default `telomere` embedder surface: module AST representation
  (except the `host_abi::CodeSection` compatibility exception) and raw
  `Op`/`Operand` are internal, while the default host-linking compatibility
  carve-out is documented and frozen through `host_abi` plus the public-surface
  snapshot. Auxiliary interpreter representations and helpers are opt-in through
  `unstable-internals`; reduction and replacement of the default-public raw host
  ABI remain tracked by #216. The documented JIT cache statistics surface stays
  public with `jit` so the minimal-embedder JIT configuration remains stable.
- Extended the public-surface guard to enumerate `#[macro_export]` macros
  across all of `src/` and freeze the discovered root `vm_try!` and
  `with_count!` macros in the committed snapshot.
- Defined the component host-function execution contract (#147): the
  component runtime is synchronous, so futures handed to the `*_async`
  linker registrations (`register_import_async`, `register_func_async`,
  `register_import_resource_drop_async`, and the generated `HostAsync` /
  `ImportsAsync` bindings) are polled exactly once and must complete on
  that first poll. The contract is now stated on every async registration
  method, on `ComponentFuture`, and in the generated binding docs, and the
  error returned for a yielding host future names the contract and the
  remedy. No signatures changed.

### Fixed

- Fixed baseline JIT trap emission to continue scanning and emit labels for enclosing live
  paths with a normalized cached-register stack depth.
- A submitted asynchronous host call's future no longer writes its result
  values into the suspended task's return slot when it is polled (#216
  review). The future is owned by the embedder's driver and could still be
  polled after the submitting execution was dropped — freeing the scheduler
  and the task stack — which wrote through a dangling slot pointer. The
  future now resolves to the callback's values, and the scheduler performs
  the validated return-slot write when it applies the completion.
- Completions from an embedder driver are now bound to the execution that
  submitted them (#216 re-review). Root tasks take a fresh, process-unique
  task identity instead of the constant every run used to share, so if a
  cancelled execution's driver is reused without being drained, its
  retained completion names no task of the later execution and the
  scheduler discards the stale outcome before any return-slot write or
  resume — including when the two runs' host calls have different
  signatures.
- Task identities are now 64-bit throughout the driver surface
  (`PendingOp::task_id`, the pending structs, and `Completion::task_id`),
  widened from `u32` (#216 re-review). A 32-bit process-wide counter
  would wrap after 2^32 executions in a long-lived process and reissue
  an identity, letting a completion retained by a driver name a task of
  a later execution again; 2^64 identities cannot be exhausted in
  practice, so identities are never reused.
- The raw code-pointer accessor behind `unstable-internals` no longer
  panics on callbacks registered through the public host boundary (#216
  re-review). `function_host_code_pointer` gated on "host and not async",
  which admits public synchronous callbacks, and then reached an
  `unreachable!` because a public callback's signature cannot be
  represented by the raw `HostFunction` type. The accessor now matches
  `FunctionBody::Host` explicitly and returns `None` for public
  callbacks.

### Removed

- Removed the implicit `wide` Cargo feature, which existed because `simd`
  referenced its optional dependency by bare name. Builds using `--features wide`
  must use `--features simd` instead.

### Security

- Metered Stores disable the experimental JIT, preventing a fuel-configured
  Store from executing an unmetered native JIT path.

## [0.1.0-alpha.1] - 2026-08-03

This is the planned, provisional tag date. If the release PR's merge or tag
date moves, update this heading before merging the release PR. This first entry
is a snapshot of the project's current state, not a difference from an earlier
release.

### Added

- The `telomere` core WebAssembly parser, validator-facing optimizer,
  interpreter, host-linking APIs, and optional experimental baseline JIT.
- Component Model decoding, validation, linking, and runtime support through
  `telomere-component`, plus WIT binding generation through
  `telomere-component-bindgen`.
- A partial WASI 0.2.6 component provider in `telomere-component-wasi`, and
  core WASI preview1 and component command paths in `telomere-cli`.
- Runnable core, preview1, and component fixtures, together with the upstream
  core Wasm testsuite submodule and local Component Model test suite.

### Changed

- The eight crates use one workspace version and remain intentionally
  unpublished to crates.io (`publish = false`).
- Minimal `telomere` and `telomere-component` normal dependency graphs exclude
  Tokio; threaded and async support remains feature-gated.

### Fixed

- In-place return-frame sizing for host and async-host calls that return values.

### Removed

- No removals are recorded in this initial snapshot.

### Security

- There is no supported version line or backport branch. Pre-release tags carry
  no support commitment; fixes land on `main`.
