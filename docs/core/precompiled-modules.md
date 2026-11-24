# Precompiled modules

Status: contract for [#196](https://github.com/yieldspace/telomere/issues/196),
implemented in the same change. This document is the contract for the
precompiled-module codec: what an artifact is, what it pins to, and what
loading one guarantees.

## What it is

A parsed core module — including each function's optimizer output
(`LoweredFunction`) — can be serialized into a precompiled artifact with
`telomere::serialize_module` and restored with
`telomere::deserialize_module` without re-running the wasm parser or the
optimizer. The CLI exposes both directions:

- `telomere-cli compile MODULE [-o OUTPUT]` writes the artifact
  (default output: the module path with `.tpre` appended) and prints the
  per-phase serialize timings.
- Every CLI entry point that loads a core module detects artifacts by
  magic and restores them transparently.

Loading an artifact still runs `instantiate` in full: linking, memory
and table initialization, and `materialize` of the lowered instruction
streams all happen store-side, exactly as for a parsed module. What is
skipped is the parse + validate + optimize pipeline only.

## The contract: exact-build pinning

The format makes **no cross-version stability promise**. An artifact is
a cache for one exact build, nothing more. Every artifact header carries
a build fingerprint:

- the format version,
- the crate version,
- the rustc version that compiled the build,
- the target triple,
- the enabled feature set (the subset that changes handler sets or
  value layouts),
- a hash of the crate's source tree as compiled into the build (so a
  handler whose implementation or operand interpretation changed —
  without any rename — still produces a different fingerprint), and
- a hash of the op-ID table compiled into the build.

`deserialize_module` compares every field against the running build and
rejects any mismatch with a typed `PrecompiledError::FingerprintMismatch`
naming the field. There is no migration and no silent fallback to
parsing: a caller that wants a fallback implements it itself, in the
open. The same typed-error posture covers the rest of the failure
space — bad magic, unsupported format version, truncation, corruption,
unknown op ids, and trailing bytes are all distinct
`PrecompiledError` variants.

## Format sketch

```
header:  magic "TELOPRE1" | format version (LEB u32)
         crate version | rustc version | target | feature set
         (each a length-prefixed string) | op-table hash (u64 LE)
body:    length-prefixed sections in fixed order:
         types, functions, imports, mems, globals, exports, tables,
         elems, data, start, name, codes
```

Counts and indices are canonical (minimal) unsigned LEB128 — a
non-minimal encoding is corruption, not an alias. Fixed-size numeric
payloads (floats by bit pattern, `V128` constants, const-pool entries,
locals counters, op ids) are little-endian. Strings are length-prefixed
and must be valid UTF-8.

Function bodies carry the optimizer's lowered form: locals counters,
the op stream (stable op id plus symbolic operands), the const pool,
call recipes, the jump table, and the block map.
`materialized_preview` is deliberately not stored; the restored
function re-materializes from the lowered form at instantiate time,
through the same code path the optimizer's fallback uses.

## Op ids

Handlers are serialized as stable `u16` ids from a generated registry
(`crates/telomere/src/common/op_registry.rs`, produced by
`tools/gen-op-registry.py`). Ids are assigned in sorted-name order over
every handler the generator can find, so an id names the same handler
in every feature configuration; feature-gated handlers live in
cfg-gated sub-tables.

Serialization resolves a handler by fn-pointer identity against the
registry and fails closed with `PrecompiledError::UnregisteredOp` for a
handler with no entry: a module using a brand-new handler cannot be
serialized until the registry is regenerated (`python3
tools/gen-op-registry.py`; `--check` verifies freshness). The wast
round-trip lane (below) catches a stale registry over the spec corpus
before it can ship.

Two registry subtleties are load-bearing:

- Handler identity is fn-pointer identity, per the #240 contract. ICF
  may fold semantically identical handlers to one address; the first
  matching registry entry wins, which is safe because folded twins are
  interchangeable on the untyped bit stack.
- Aliases (`pub(crate) use` re-exports of one handler under several
  names) collapse to the same pointer and therefore to one id; the
  alias name is preserved only as an additional registry entry.

## Decoder safety posture

The decoder is **total**: no input can make it panic. Every read is
bounds-checked, every element count is checked against the remaining
input before any allocation sized by it, and decoded cross-references
(function/type/table/memory/global indices, const-pool references,
jump-target labels, block-map entries, call-recipe targets) are
range-validated against the module being built, mirroring the
invariants the optimizer's own `verify` checks. This is the #128
no-abort posture applied to a new input surface, and it is exercised by
the `decode_precompiled_module` fuzz target and by the rejection test
matrix in `crates/telomere/tests/precompiled_codec.rs` (every prefix
truncation, every header byte flip, seeded random corruption).

## Conformance enforcement

`tests/common/mod.rs` routes every module the wast harness instantiates
through a codec round-trip when `TELOMERE_WAST_PRECOMPILED=1` is set.
With `TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT=1` (the CI lane), any
serialize or deserialize failure is a hard failure, and the lane
asserts at the end that at least one module actually round-tripped —
the same fail-closed shape as the JIT acceptance lane. The recovered
modules are what the suite then instantiates and runs, so the pass list
is the round-trip conformance evidence.

## Scope limits

- **Interpreter tier only.** No native code is serialized or produced;
  JIT-compiled code is never part of an artifact.
- **Core modules only.** The component model is not serialized.
- **Host function bodies** have no lowered form; serializing a module
  that contains one fails with `PrecompiledError::HostFunctionBody`.
- Artifact size is not a goal: the lowered form is verbose, and
  artifacts are typically several times larger than the wasm they were
  compiled from (see `docs/benchmarks/precompiled-codec.md` for the
  measured ratio). The trade is load time, not bytes.
