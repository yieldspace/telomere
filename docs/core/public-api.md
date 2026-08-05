# Public API Surface

## Status and scope

Current. This document records the source-level public paths of the core
telomere crate. The default embedding surface is intentionally narrower than
the interpreter implementation, with one compatibility carve-out: existing
synchronous and asynchronous host-function linking must continue to work
without requiring an opt-in feature. The replacement and eventual closure of
that raw ABI are tracked by #216.

## Surface tiers

| Tier | Path | Purpose and stability |
| --- | --- | --- |
| S1 | telomere::* | Curated core embedding API: parsing, stores, instantiation, execution, and supporting values. |
| H1 | telomere::host_abi::* | Default compatibility carve-out for raw host callbacks and native-module construction. It is retained for existing host linking, not a newly designed stable API; #216 owns its replacement. |
| H2 | Selected root telomere::* paths | Default compatibility carve-out for replacing host functions and driving asynchronous continuations. #216 owns its replacement. |
| S2 | telomere::component_support::* | Explicit documented boundary used by component-model crates. It is not a glob of the core implementation. |
| S3 | telomere::unstable_internals::* behind unstable-internals | Opt-in raw interpreter helpers and representations for in-repository or downstream integrations. No stability promise applies. |

The S1 root also retains `JitCacheStats` behind `jit`. `Store::jit_cache_stats`
and the minimal-embedder JIT configuration use this documented observability
surface to verify that an enabled workload compiled; it is not part of the raw
host-linking ABI and therefore does not belong in `host_abi`.

S1 also exposes the cold core-trap diagnostics capability:
`TrapInfo`, `TrapFrame`, `TrapFrameKind`, `TrapKind`, and
`Store::take_last_trap`. The record is owned, its retrieval is consuming and
best-effort on the calling thread, and it leaves `VMResult` plus the existing
execution signatures unchanged. It is therefore a diagnostic bridge rather
than the public error redesign: [#143](https://github.com/yieldspace/telomere/issues/143)
owns that redesign, while [#146](https://github.com/yieldspace/telomere/issues/146)
owns the corresponding C ABI surface. See [trap reporting](trap-reporting.md)
for the exact retrieval, formatting, name-retention, and JIT boundaries.

### H1: default host ABI

host_abi has nineteen always-present paths:

AsyncHostFunction, AsyncHostFunctionDefinition, AsyncHostFuture,
AsyncNativeModule, CodeSection, ExecuteContext, Func, FunctionBody,
HostFunction, HostFunctionDefinition, Instr, Memory, NativeModule, ObjectRef,
ReturnSlot, Stack, LocalReference, StoreInner, and instantiate_native_module.

With threads, it additionally has SharedMemoryObject and
SharedWaitRegistration. Thus the default configuration exposes 21 H1 paths;
--no-default-features exposes 19, and
--no-default-features --features simd also exposes 19.

The 16-to-6 nameability classification is **N6** in the default build: the
always-present Memory, Stack, LocalReference, and StoreInner, plus
SharedMemoryObject and SharedWaitRegistration with threads. It is N4 for both
--no-default-features and --no-default-features --features simd. The latter two
are necessary because a custom ExecutionDriver receives a public
MemoryWaitPending { shared, wait, .. }. The former stack/frame values remain
necessary because no new finish API was introduced: a host callback continues
to complete its frame through ExecuteContext's raw stack and store fields.

The other ten candidates are not default ABI names: LocalsData, AtomicRmwOp,
AtomicWaitResult, MemoryMappingOperation, MemoryInitError, CallFrameCache,
IntoCallFrameCache, InstanceData, FunctionInstanceData, and EffectSupplier.
They are represented by S3 helpers where a supported opt-in consumer needs an
operation, rather than by a default export of the underlying interpreter type.

### H2: root compatibility carve-out

Five root paths preserve synchronous or asynchronous host-function replacement:

- instantiate_native_async_module
- link_host_function_with_export_name
- link_host_function_with_function_idx
- link_async_host_function_with_export_name
- link_async_host_function_with_function_idx

Eight root paths preserve asynchronous driver and continuation handling:

- run_module_function_with_driver
- Completion
- CompletionPayload
- ExecutionDriver
- HostCallPending
- MemoryWaitPending (threads only)
- PendingOp
- TokioDriver

Module::codes is also public for the existing decoded-AST compatibility
boundary. Its type is host_abi::CodeSection; the other decoded section types
remain internal. H1 and H2 are intentionally small compatibility boundaries,
not an endorsement of raw interpreter representation as the general embedding
API.

unstable-internals remains an explicit maintenance boundary. It is enabled by
the CLI, component-model consumers, and the crate's self dev-dependency where
needed. New consumers should prefer S1/S2/H1/H2 and opt in to S3 only when they
need the raw integration contract.

`special_function_return` and `WasmAsyncPending` are S3 items under
`unstable-internals`, despite retaining cfg-gated crate-root spellings for
existing opt-in consumers. The former requires construction of a raw `Instr`
return sequence, and the default surface exposes no `Instr` construction API.
The latter has no runtime producer, while `TokioDriver` rejects it as reserved
future guest-async support. Neither is part of the default host-linking or
driver carve-out. `CompletionPayload::WasmAsync` remains a type-free reserved
completion marker, not a supported default driver capability.

## Public-surface snapshot

crates/telomere/tests/public_surface.rs parses src/lib.rs, src/host_abi.rs,
src/component_support.rs, and src/unstable_internals.rs with syn. It also
parses the root-re-exported `Module` definition in src/common.rs and the
`Store` definition in src/common/store.rs solely to record the reviewed
`Module::codes` field and `Store::take_last_trap` method. It expands every
explicit public use tree and public item into a telomere::... path, carries
textual #[cfg(...)] predicates through nested public modules, sorts the result,
and compares it byte-for-byte with the single committed
crates/telomere/tests/public_surface.snapshot file.

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
    cargo fmt --all -- --check
    git diff --check

### What this does and does not prove

The snapshot is a path-level guard. It has one deliberately reviewed field
exception, `Module::codes`, and one deliberately reviewed method exception,
`Store::take_last_trap`, but does not detect arbitrary new public methods,
fields, or changed signatures on an already exported type.
warn(unnameable_types), private_interfaces, compilation, and the warning-free
Clippy job provide complementary checks; cargo public-api would be the
appropriate future signature-level compatibility tool.

The `host_abi` doctests establish the synchronous index-link/frame-return route
and the async native-module/custom-driver route. They do not by themselves
replace the snapshot's full path audit.

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

This change achieves the reviewed path closure and the N6 default host-linking
compatibility set. It does not replace the raw ABI; that work remains #216.
An existing export-name host-linking failure is tracked by #220. It is
unrelated to the surface boundary and remains deliberately unmodified here.

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
