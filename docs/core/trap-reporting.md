# Trap reporting

> Status: current core embedding contract.

Telomere exposes an owned, cold diagnostic record for a failed core guest call.
The S1 API is [`Store::take_last_trap`], which returns a [`TrapInfo`] after the
call has completed. This is a diagnostic bridge: it does not change
`VMResult`, the result type of `run_module_function`, or the dispatch ABI.

[`Store::take_last_trap`]: ../../crates/telomere/src/common/store.rs
[`TrapInfo`]: ../../crates/telomere/src/common/trap_info.rs

## Retrieval contract

> `take_last_trap()` returns the trap that failed the most recent **outermost** guest call
> (`run_module_function`, `run_module_function_with_driver`,
> `component_support::runtime::run_core_export_sync_reentrant`, or `instantiate` including a
> trapping `start` function) **on the calling thread**, and consumes it. It returns `None` when
> that call succeeded, when the trap was already taken, when the trap belongs to another thread,
> and while a guest call is active on the calling thread.
>
> **It is best-effort, not a guarantee that a trap is retrievable.** A `Store` holds one trap slot.
> When guest calls run concurrently on the same `Store` from several threads, another thread's call
> can clear or overwrite the slot between your call trapping and your `take_last_trap()`, so a call
> that genuinely trapped can still yield `None`. What is guaranteed is the *direction* of the
> failure: you may lose your trap, but you are never handed someone else's. Code that must not miss
> a trap should call `take_last_trap()` immediately after the call returns, or serialise guest calls
> per `Store`.

The method is consuming: a second call after `Some` returns `None`. A successful
outermost call also clears any earlier trap record. Code that needs diagnostics
should therefore call it immediately after the guest-call result is available.

Nested re-entry is deliberately outermost-only. If a guest-to-host-to-guest
call traps and the host recovers, the successful outer call leaves no trap to
take. If the host propagates the failure, the current report contains the outer
chain only (including its host frame); the re-entrant inner guest frames are
absent because that call used its own `Stack`. Issue
[#211](https://github.com/yieldspace/telomere/issues/211) owns any future
inner-chain stitching. Calling `take_last_trap()` from an active host callback
also returns `None`, so an older same-thread record cannot be mistaken for the
active call's result.

## Owned record and names

`TrapInfo`, `TrapFrame`, `TrapFrameKind`, and `TrapKind` are
`#[non_exhaustive]` root types. `TrapInfo` owns its frame records and optional
names, so an embedder may keep a report after later calls on the `Store`.

Capture remains index-only. Symbolization happens only when
`take_last_trap()` consumes the cold record, using checked store lookups and
the retained [`ModuleNames`] accessors. This avoids name work on traps that no
caller reads and makes a damaged captured frame degrade to absent names rather
than a diagnostic panic. At take time, retained module and function names are
cloned into owned strings; this can allocate up to two strings per captured
frame, with the capture's bounded frame limit. It is intentionally not a
dispatch-path allocation.

[`ModuleNames`]: debug-names.md

With the default `DiagnosticsConfig`, retained producer names appear as
`TrapFrame::module_name` and `TrapFrame::func_name` when the module supplied
them. With `retain_function_names: false`, or when the module has no retained
name section, both are `None`; the frame's function index, kind, and available
program counter remain useful. The imports-first function-index rule is shared
with [debug-name retention](debug-names.md).

## Trap kinds and formatted output

`TrapKind::as_str()` returns these Telomere diagnostic labels, in enum order:
`unreachable`, `stack overflow`, `memory index out of range`, `unaligned
atomic`, `table index out of range`, `call indirect invalid type`, `table
uninitialized`, `unlinkable`, `memory allocation failed`, `invalid operand`,
`unimplemented`, `fuel exhausted`, and `cancelled`.

These are Telomere's diagnostic labels; they are **not** WebAssembly spec-test
trap-message strings. Issue [#131](https://github.com/yieldspace/telomere/issues/131)
owns spec-test messages, and callers must not use these labels as replacements
for them.

`Display` for `TrapInfo` is a stable diagnostic format:

```text
trap: <kind>
  <depth>: <name> (<location>)<pc><tag>
  ...
```

The first line is exactly `trap: ` followed by `TrapKind::as_str()`; it has no
`error:` prefix. Each frame line has two leading spaces. `<name>` is the
function name, optionally prefixed by a non-empty module name and `::`;
`<unnamed>` when no function name is retained; or `<unknown>` for an
unresolved frame. `<location>` is `(func N)` for a known index and `(func ?)`
otherwise. `<pc>` is ` @ pc N` only when a program counter is available.
`<tag>` is empty for Wasm frames and is ` [host]`, ` [async host]`, or
` [unresolved]` for the other frame kinds.

When the displayed frame depths have a positive gap, the formatter inserts
`  ... N frames elided ...`. When `truncated` is true and no elision line was
emitted (including the corrupt-stack walk-limit case), it appends
`  ... frame walk truncated ...`. An empty frame list has only the first line,
and formatted output never has a trailing newline. Formatting is total even if
an embedder mutates public fields into an inconsistent shape, such as
non-monotonic depths.

For example, with names retained and a host function in the chain:

```text
trap: unreachable
  0: demo::inner (func 3) @ pc 0
  1: demo::bounce (func 2) [host]
  2: <unnamed> (func 1) @ pc 7
  3: demo::run (func 0) @ pc 2
```

## Program counters and JIT

For a normal captured chain, depth zero's `pc_index` identifies the faulting
instruction. Every caller frame instead identifies its return address: the
instruction after its call site. A fused interpreter superinstruction reports
the fused unit's first instruction. Tail calls intentionally elide frames, so
a tail-called-away frame is absent under the existing
[#114](https://github.com/yieldspace/telomere/issues/114) contract.

With the optional JIT, a native fault has no justified instruction attribution,
so frame zero has `pc_index: None` and its formatted line has no ` @ pc`
suffix. The callers remain present and retain their decoded-stream return
addresses, so frames after zero may have `pc_index: Some(_)`. The report still
has function-level identity and names for the JIT frame. Issue
[#209](https://github.com/yieldspace/telomere/issues/209) owns additive JIT
trap-site tables; it may fill the missing frame-zero pc without changing this
record or its format.

## Scope and performance boundary

Issue [#143](https://github.com/yieldspace/telomere/issues/143) owns the
overall public error redesign. It may carry this owned `TrapInfo` payload, but
this API deliberately leaves `VMResult` and existing call signatures intact.
Issue [#146](https://github.com/yieldspace/telomere/issues/146) owns the C ABI
mirror; ownership here is the compatibility boundary it can rely on. Component
surface decisions remain with #211 rather than adding this report to
`component_support` now.

The interpreter-regression acceptance criterion is **failed-with-reason**, not
a performance result: this host cannot currently publish a trusted timing
record under the established gates. No unmeasured numeric result is claimed
here. A future valid measurement must use the
[interpreter-baseline method](interpreter-baseline.md) and preserve its raw
record; the blocked environment and fail-closed rule are documented in
[measurement attempts](measurement-attempts/README.md).

The structural argument is limited to the implementation boundary: `VMResult`
and its size guards are unchanged; no dispatch path is edited; the owner stamp
reads the current thread only on cold trap publication; capture adds diagnostic
metadata to its bounded cold frame vector; and name strings are created only at
`take_last_trap()` time. These facts justify where a future measurement should
look, but do not substitute for one.
