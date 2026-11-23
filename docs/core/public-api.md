# Public API Surface

## Status and scope

Current. This document records the source-level public paths of the core
telomere crate. The default embedding surface is intentionally narrower than
the interpreter implementation. Host-function linking is part of that
surface through a designed public boundary (#216): host callbacks receive a
`HostContext` with decoded arguments and return `WasmValue`s, so no
interpreter-internal type appears in a default public signature. The former
raw host-linking ABI remains available only behind the opt-in
`unstable-internals` feature (tier S3).

## Surface tiers

| Tier | Path | Purpose and stability |
| --- | --- | --- |
| S1 | telomere::* | Curated core embedding API: parsing, stores, instantiation, execution, and supporting values. |
| H1 | Selected root telomere::* paths | Designed host-function boundary (#216): `HostContext`, the `HostFn`/`AsyncHostFn` callback types, native-module construction, and the link/instantiate entry points. |
| H2 | Selected root telomere::* paths | Execution-driver surface: pending operations, completions, and the opaque `HostCallFuture`/`HostCallOutcome` wrappers (#216). |
| S2 | telomere::component_support::* | Explicit documented boundary used by component-model crates. It is not a glob of the core implementation. |
| S3 | telomere::unstable_internals::* behind unstable-internals | Opt-in raw interpreter helpers and representations for in-repository or downstream integrations. No stability promise applies. |

The S1 root also retains `JitCacheStats` behind `jit`. `Store::jit_cache_stats`
and the minimal-embedder JIT configuration use this documented observability
surface to verify that an enabled workload compiled; it is not part of the
host-linking API.

S1 also exposes the cold core-trap diagnostics capability:
`TrapInfo`, `TrapFrame`, `TrapFrameKind`, `TrapKind`, and
`Store::take_last_trap`. The record is owned, its retrieval is consuming and
best-effort on the calling thread, and it leaves `VMResult` plus the existing
execution signatures unchanged. It is therefore a diagnostic bridge rather
than the public error redesign: [#143](https://github.com/yieldspace/telomere/issues/143)
owns that redesign, while [#146](https://github.com/yieldspace/telomere/issues/146)
owns the corresponding C ABI surface. See [trap reporting](trap-reporting.md)
for the exact retrieval, formatting, name-retention, and JIT boundaries.

### H1: designed host-function API

Host callbacks are written against public vocabulary only. A synchronous
host function has the type

```
HostFn = fn(&mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>>
```

and an asynchronous one returns a `HostFuture` resolving to the same value
shape. `HostContext` carries the decoded arguments, the owning `Store`
(including embedder state), the calling instance/function identity, and
guest-memory access through the public `Memory` type. The runtime adapter
decodes the arguments from the frame's local area, validates and writes the
returned values into the frame's return slot, and completes the frame
return, so embedders never complete a frame through raw stack state.

Native modules are built from `HostFnDefinition`/`AsyncHostFnDefinition`
collected in `HostModule`/`AsyncHostModule`. Six root entry points cover
linking and instantiation:

- instantiate_native_module
- instantiate_native_async_module
- link_host_function_with_export_name
- link_host_function_with_function_idx
- link_async_host_function_with_export_name
- link_async_host_function_with_function_idx

The internal calling convention is unchanged: the adapter returns the same
`VMResult<*const Instr>` to the dispatcher, so the return-type
classification guarded by the #178 size assertions is unaffected.

The raw ABI that preceded this boundary (raw `ExecuteContext` callbacks,
`NativeModule`, frame completion through `Stack`/`StoreInner`, the decoded
`Module::codes` field) is closed in default builds. `Module::codes` is
crate-private, and the raw types and entry points are nameable only through
S3: `unstable_internals` re-exports the raw types and provides
`link_raw_*` / `instantiate_raw_*` counterparts. The public-surface guard
forbids the raw final segments from ever reappearing ungated.

### H2: execution-driver surface

Eight root paths cover asynchronous driver and continuation handling:

- run_module_function_with_driver
- Completion
- CompletionPayload
- ExecutionDriver
- HostCallPending
- MemoryWaitPending (threads only)
- PendingOp
- TokioDriver

A driver never names an interpreter continuation: `HostCallPending.future`
is the opaque `HostCallFuture`, whose completion result is the opaque
`HostCallOutcome` carried by `CompletionPayload::HostCall`. With threads,
`MemoryWaitPending` additionally carries the root driver types
`SharedMemoryObject` and `SharedWaitRegistration`, whose wait operations a
driver is expected to call.

unstable-internals remains an explicit maintenance boundary. It is enabled by
the CLI (whose Preview1 support manipulates raw instruction sequences; #137
tracks Preview1's fate), component-model consumers, and the crate's self
dev-dependency where needed. The component-model crate's host trampoline
itself uses the designed H1 boundary. New consumers should prefer S1/S2/H1/H2
and opt in to S3 only when they need the raw integration contract.

`special_function_return` and `WasmAsyncPending` are S3 items under
`unstable-internals`, despite retaining cfg-gated crate-root spellings for
existing opt-in consumers. The former requires construction of a raw `Instr`
return sequence, and the default surface exposes no `Instr` construction API.
The latter has no runtime producer, while `TokioDriver` rejects it as reserved
future guest-async support. Neither is part of the default host-linking or
driver surface. `CompletionPayload::WasmAsync` remains a type-free reserved
completion marker, not a supported default driver capability.

## Public-surface snapshot

crates/telomere/tests/public_surface.rs has two source passes. Its ordinary
public-path pass parses the reviewed source closure with syn: src/lib.rs,
src/component_support.rs, src/unstable_internals.rs, and the root-re-exported
`Module` and `Store` definitions in src/common.rs and src/common/store.rs.
The latter two sources are parsed solely to record the reviewed
`Store::take_last_trap` method and to assert that `Module::codes` stays
crate-private. The pass
expands every explicit public use tree and public item into a telomere::... path,
carries textual #[cfg(...)] predicates through nested public modules, sorts the
result, and compares it byte-for-byte with the single committed
crates/telomere/tests/public_surface.snapshot file.

A separate macro-only pass scans all of src/ for #[macro_export] macros and
records them at the telomere crate root: macro exports hoist there regardless of
their containing module's visibility. It walks the module tree to preserve
module cfg predicates, so a macro-bearing file that is not reachable from
src/lib.rs fails loudly rather than escaping the snapshot. A macro export
outside module scope also fails loudly instead of guessing the cfg inherited
from an enclosing item.

The test deliberately reads source rather than the active Cargo feature set, so
default, no-default, and SIMD-only invocations all compare against the same
snapshot. In addition to the full-path equality check, it compares the
snapshot's ungated partition and unstable-internals partition with the
corresponding source-derived sets. The snapshot, rather than a second predicted
allow-list in the test, is therefore the source of truth for the H1/H2 and S3
carve-outs.

The guard rejects ungated Operand, Op, the ten non-N6 names above, and every
decoded *Section name except CodeSection. It separately requires the reviewed
`telomere::JitCacheStats` path with its `jit` cfg predicate, plus the four S1
trap-reporting type paths and the one reviewed `Store::take_last_trap` method.
It also rejects a new root public module unless it is deliberately added to the
reviewed parser closure. This catches a broad accidental reopening of the
interpreter surface while leaving the exact path decision to the snapshot
review.

When an intentional public-path change is ready for review, regenerate and
then verify the snapshot without the update environment variable:

    TELOMERE_UPDATE_PUBLIC_SURFACE_SNAPSHOT=1 \
      cargo test -p telomere --release --test public_surface
    cargo test -p telomere --release --test public_surface
    cargo test -p telomere --release --no-default-features --test public_surface
    cargo test -p telomere --release --no-default-features --features simd \
      --test public_surface
    cargo test -p telomere --doc --release
    cargo test -p telomere --doc --release --no-default-features
    cargo test --workspace --release
    cargo fmt --all -- --check
    git diff --check

### What this does and does not prove

The snapshot is a path-level guard. It has one deliberately reviewed method
exception, `Store::take_last_trap`, and asserts the formerly public
`Module::codes` field stays crate-private, but does not detect arbitrary new
public methods, fields, or changed signatures on an already exported type.
warn(unnameable_types), private_interfaces, compilation, and the warning-free
Clippy job provide complementary checks; cargo public-api would be the
appropriate future signature-level compatibility tool.

The doctests on the root link and instantiate functions establish the
synchronous link route, the native-module route, and the async
native-module/custom-driver route against the designed H1 boundary. They do
not by themselves replace the snapshot's full path audit.

Release lib-only builds establish the public-surface closure for a concrete
feature set. --all-targets builds establish that workspace consumers still
compile, but cannot by themselves establish that closure: the CLI and test
dev-dependencies opt into unstable-internals, and Cargo feature unification can
otherwise hide an unwanted default exposure. Type-name greps and consumer
counts are useful discovery aids, but not proof either: aliases, inferred
types, generic bounds, cfg paths, and indirect public fields evade those
counts.

The review reduction was recorded as iteration 0 = 17, iteration 1 = 1, and
iteration 2 = 0 remaining unintended release-lib-only exposures. Those numbers
are closure-review checkpoints, not a claim about the number of textual type
matches or the number of consumers.

That review achieved the reviewed path closure and the N6 default
host-linking compatibility set without replacing the raw ABI. Issue #216 has
since replaced the raw ABI with the designed H1 boundary described above;
the raw types and entry points survive only in S3, and the guard now forbids
their ungated return.

## Measurement boundary

AC5 is not measured here. A source-surface refactor does not itself yield a
valid performance or footprint result, and this work records no numerical
claim. Issue #184's retained three committed artifacts -- the busy-machine
attempt, environment characterization, and finite-schedule bias audit in
[measurement attempts](measurement-attempts/README.md) -- are explicitly
non-baseline evidence. The required controlled method and publication criteria
are in [Interpreter Baseline Methodology](interpreter-baseline.md).

The existing call_threading and tail_call_threading regressions, together with
the release-size profile checks, are structural execution evidence only: they
establish call-frame behavior and a build-profile boundary, not a timing,
footprint, or performance result for this change. A future AC5 claim needs a
fresh eligible raw record under docs/core/baseline/, not an extrapolation from
this API audit or from those structural checks.
