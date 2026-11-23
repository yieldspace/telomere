# Diagnostics and observability

> Status: current embedder contract. The normative target list lives in the
> [`telomere::diagnostics`](../../crates/telomere/src/diagnostics.rs) module
> documentation; this guide explains how to consume it.

A plugin host in the field must be able to answer "what did plugin X just
do" without recompiling. Telomere answers through `tracing` events on
documented targets, plus the owned trap record described in
[trap reporting](trap-reporting.md).

## The release-tracing fix

`crates/telomere` previously depended on `tracing` with the
`release_max_level_off` feature. Cargo feature unification applies that
feature to the whole dependency graph, so every release build containing
Telomere had **all** tracing statically compiled out — including the
embedder's own application logging, silently. `telomere-component` did not
set the feature, so the workspace also disagreed with itself.

The feature is removed. Release builds now keep every event compiled in,
and the embedder's subscriber and filter decide what is recorded.

### Measured cost of the alternative

Removing the feature means `tracing` callsites in the runtime evaluate a
cached interest check (an atomic load and a branch when no subscriber is
interested) instead of being erased at compile time. Measured on
fleur-du-lapin (criterion, 30 samples, interleaved A/B against the
unmodified tree, with the second unmodified run as the A/A noise control;
no subscriber installed):

| Benchmark | Unmodified (run 1) | Unmodified (run 2, A/A) | This change | Delta vs A/A band |
| --- | --- | --- | --- | --- |
| fib | 1.4316 ms | 1.3967 ms (−2.4%) | 1.2423 / 1.2720 ms | faster in both rounds — a layout/host-variance artefact, not a semantic effect: the fib path contains no changed code |
| return_call_chain | 2.4822 µs | 2.4696 µs (−0.5%) | 2.4897 / 2.4941 µs | +0.3–0.5%, inside the A/A band |
| scalar_local_loop | 10.094 µs | 10.233 µs (+1.4%) | 10.280 / 10.404 µs | +1.7% against the unmodified mean in the A/B rounds, but covered by the control spread below — no attributable regression |
| nested_blocks_at_depth_limit (parse) | 8.0128 ms | 8.0482 ms (+0.4%) | 8.0028 / 7.9244 ms | −0.1% / −1.1%, inside the A/A band — notable because the parser's per-byte LEB trace callsites are the most exposed path in this change |

A control build with **only** the `Cargo.toml` feature removal (none of
this change's code) was measured the same way: fib 1.3982 / 1.3275 ms and
scalar_local_loop 10.103 / 10.772 µs. Both land inside the envelope spanned
by the unmodified and changed builds, and scalar_local_loop alone swings
±3.5% between runs of near-identical binaries on this shared host. The
verdict is therefore: **no performance regression is attributable to
removing `release_max_level_off`.** That is also the semantic expectation —
the change adds no work to any interpreter hot path; the newly live
callsites sit in parsing, instantiation, and cold diagnostic paths, and
each costs one cached interest check when no subscriber is listening.

The trap, JIT, and stack events added by the same change sit on cold paths
(trap recording, compile rejection, fallback exit, stack-overflow trace)
and are gated by the same interest check, so they do not add measurable
hot-path work.

### Stack cost, and the one place it had to be paid back

Live callsites also cost stack in recursive code. Measured with a
minimum-viable-thread-stack probe (64 KiB scan granularity) over the
nesting fixtures of `component_nesting_depth.rs`:

| Fixture | Unmodified | Feature removed, before mitigation | After mitigation |
| --- | --- | --- | --- |
| 100 component sections + 512 core blocks | ≤ 512 KiB | 576 KiB | ≤ 512 KiB |
| 512 core blocks alone | 384 KiB | 448 KiB | 384 KiB |
| 100 component sections + empty module | ≤ 256 KiB | ≤ 320 KiB | ≤ 256 KiB |

The growth was ~128 B per control-nesting level in the instruction
parser: `parse_inst` / `parse_instrs` recurse once per level inside the
documented 512 KiB budget ([parser limits](parser-limits.md)), and that
function carries ~200 trace callsites. The mitigation follows the
runtime's existing convention (`runtime/trace.rs`): the instruction
parser's developer traces are debug-only, so its release frames return to
exactly their previous size — every fixture above measures identical to
the unmodified tree. The documented event targets are unaffected;
per-opcode parser traces were never part of the contract.

## Events

| Target | Level | Meaning | Fields |
| --- | --- | --- | --- |
| `telomere::trap` | WARN | A guest trap was recorded for `Store::take_last_trap`. | `kind` (stable `TrapKind` label), `total_frames`, `truncated`, innermost `funcidx`, `pc_index` |
| `telomere::jit` | DEBUG | A JIT compilation was rejected. | `reason`, or `pc` / `op` / `stack_depth` / `op_addr` for an emission rejection |
| `telomere::jit` | DEBUG | Native execution fell back to the interpreter. | `kind`, `funcidx`, `pc` |
| `telomere::jit` | INFO | One line of the `TELOMERE_JIT_PROFILE` report (only while that facility is enabled). | message only |
| `telomere::stack` | DEBUG | The interpreter stack-overflow trace fired. | message only |

The trap event is emitted at the single recording choke point
(`StoreInner::set_last_trap`), so the sync, async-driver, component
re-entrant, and instantiate-with-start boundaries all report identically.
It fires in every profile and feature combination — no `vm-diagnostics`
build is needed. The full record (frames, resolved names) is still
retrieved synchronously with `Store::take_last_trap`; the event is the
notification that a record exists.

Telomere additionally emits unstructured `trace` events under the emitting
module's own path (instantiation and section-parser detail). Those are
developer traces and may change without notice; the table above is the
stable contract. The instruction parser's per-opcode traces are the one
exception: they are debug-build-only, for the stack-budget reason measured
above.

### Subscribing

Any `tracing` subscriber works. With `tracing-subscriber`, for example:

```toml
[dependencies]
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

```rust
tracing_subscriber::fmt()
    .with_env_filter("telomere::trap=warn,telomere::jit=debug")
    .init();
```

## Environment variables

The environment switches predate the event contract and remain as
command-line conveniences. Event diagnostics print the same information to
stderr when their variable is set, **and** emit the events above whether
or not it is set (the stderr copies keep their historical filters and
count caps; the events are left for the subscriber to filter):

| Variable | Gates | Event twin |
| --- | --- | --- |
| `TELOMERE_JIT_TRACE_COMPILE` (+ `_MAX`) | JIT compile-reject stderr trace | `telomere::jit` DEBUG |
| `TELOMERE_JIT_TRACE_FALLBACK` (+ `_FUNC` / `_KIND` / `_MAX`) | JIT fallback stderr trace | `telomere::jit` DEBUG |
| `TELOMERE_STACK_TRACE_OVERFLOW` | Interpreter stack-overflow stderr trace | `telomere::stack` DEBUG |
| `TELOMERE_JIT_PROFILE` (+ `_TOP`) | JIT counter collection; report printed at thread exit | report lines also emitted as `telomere::jit` INFO events |

The remaining facilities are developer profiling/dump tools whose output
is bulk text rather than events. They stay environment-driven and are
compiled only when requested:

| Variable | Feature | Facility |
| --- | --- | --- |
| `TELOMERE_VM_PROFILE` (+ `_TOP`) | `vm-profile` | Per-op dispatch profile printed when a call finishes |
| `TELOMERE_VM_DUMP_FUNC` (+ `_PC_START` / `_PC_END`) | `vm-diagnostics` | Materialized-body dump at instantiation |
| `TELOMERE_VM_INSTR_BUDGET` (+ `_LOG_EVERY`) | `vm-diagnostics` | Dispatch instruction budget with periodic stderr logging |

CI compiles the `vm-profile` and `vm-diagnostics` features both together
(`--all-features`) and each on its own, so these builds cannot rot
unnoticed.
