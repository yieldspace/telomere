# Parser / Optimizer Unsafe Audit

This document is the per-site audit of the parser/optimizer layer
required by #132 acceptance condition 3, carried out under the rules of
[unsafe-policy.md](unsafe-policy.md). It covers every `unsafe` site in
`crates/telomere/src/parser/` — 107 token sites at the audit base
(`main` = `e1be4a9`, Rust 1.96.0) — and records, for each class of
site, the invariant that makes it sound and the evidence for that
invariant. One production defect of the #292 class was found and fixed
by this audit; see "Defect found" below.

## What the sites are

Every one of the 107 sites is a field access on one of two unions in
`crates/telomere/src/common.rs`. There are no raw-pointer dereferences,
no `unsafe fn`, and no `unsafe impl` in the subtree.

- `Operand` — an eight-byte union. Its arms are `i32`, `u32`, `i64`,
  `u64`, `f32`, `f64`, `jump_addr` (`u32`), `drop_size` (`u32`),
  `local_addr` (`u32`), `select` (`u32`), `call_recipe_ref`
  (`CallRecipeRef`: two `u32` fields, `#[repr(C)]`),
  `direct_call_target` (two `u32` fields), `memarg` (`MemArg`:
  `align: u32, offset: u32`), `block_return` and `loop_param` (two
  `u32` fields each), `encoded` (`[u8; 8]`), and `start_host_function`.
  **Every arm the parser reads is plain data**: integers, floats, and
  small `u32`-pair structs with no validity invariants beyond
  initialization. No arm read in this subtree is an enum, a reference,
  a `bool`, or a nonzero type, so an initialized bit pattern can never
  be an invalid value of the arm's type.
- `Instr` — a union of `op: Op` (a function pointer) and
  `operand: Operand`. Reading the `op` arm of a slot that actually
  holds an operand *would* materialize an invalid function pointer,
  so the stream-layout invariant below is load-bearing for the `op`
  reads.

Two facts therefore decide the soundness of every site:

1. **Initialization.** Reading an arm observes the bytes of that arm.
   Reading `encoded` (or any eight-byte arm) observes all eight
   bytes, so it is sound only if the operand is *fully* initialized.
   A union literal that writes a narrow arm (`Operand { u32: v }`)
   leaves the remaining bytes uninitialized; materializing them as a
   value is undefined behaviour — this is exactly the bug Miri found
   in #292, and the reason the canonical constructors
   `Operand::from_i32` / `from_u32` / `from_f32` / `from_jump_addr` /
   `from_drop_size` / `from_local_addr` / `from_select` exist (they
   write the value bytes plus a zeroed tail, documented on
   `Operand::from_i32`).
2. **Layout.** In a parser-built instruction stream, each instruction
   occupies one head slot holding the `Op` followed by its operand
   slots. `InstructionGenerator::push_with_operand` writes exactly
   that shape, direct `push` calls in the parser write one arm per
   slot, and `op_lens` / `InstructionMeta` record the widths. The
   materialized streams produced by the pipeline's encoder follow the
   same head-plus-operands layout.

The initialization invariant for parser-built streams was established
by exhausting the construction sites in `src/parser/`: every operand
written into a stream is built either by a canonical `from_*`
constructor (43 `from_u32`, 19 `from_i32`, and a handful of the other
`from_*` forms at the audit base), by an eight-byte arm literal
(`memarg`, `block_return`, `loop_param`, `call_recipe_ref`, `f64`),
or as an `encoded` `[u8; 8]` value. No narrow-arm literal writes a
stream operand. The #292 regression test
`narrow_operands_are_fully_initialized_with_zero_tails` pins the same
fact dynamically for parsed modules.

## Classification

Line numbers are those of the audit base; the change that lands this
audit inserts one `// SAFETY:` line per site, shifting later lines.

### Class A — Raw-payload reinterpretation (35 sites): sound

`Operand { encoded: <[u8; 8]> }.<arm>` where the array was destructured
out of a `LoweredOperand::Raw`. A `[u8; 8]` is a value: all eight bytes
are initialized by the type itself, so no uninitialized byte can be
observed, and the arm read is a plain-data reinterpretation.
Sites: `pipeline/encode.rs` (5 production: the `local_get4_run`
count read, `lowered_op_is_noop`, `raw_local_addr`, and the two
local-kind reads; 6 test reads), `pipeline/analysis.rs`
(`raw_local_get_key`, `raw_i32`, `raw_memarg`, `raw_local_addr`),
`pipeline/select.rs` (`raw_i32`, `raw_select`, `raw_memarg`),
`pipeline/transform.rs` (`raw_local_addr`, `raw_i32`, `raw_i64`),
`pipeline/versioning.rs` (`raw_local_addr`, `raw_i32`, `raw_memarg`,
`raw_u32`), and 10 test reads in `optimizer/mod.rs`.

### Class B — Canonical-constructor reads (24 sites): sound

`Operand::from_*(...).encoded`. The constructors fully initialize the
union by contract, so reading the raw arm observes no uninitialized
bytes. Sites: the `raw_*_operand` helpers in `encode.rs`,
`analysis.rs` (tests), `transform.rs`, `versioning.rs`, `select.rs`,
and 13 inline constructions in `optimizer/mod.rs` tests.

### Class C — Stream and record reads (31 sites): sound, on the layout and initialization invariants

- `optimizer/cfg.rs`: the nine `DecodedInstr` operand accessors and
  the two `build_program` reads. `DecodedInstr` operands are copied
  out of the parser-built stream by `build_program` itself
  (`instrs[meta.start].op` for the head slot — `meta` records the
  layout and the bounds are checked immediately above — and
  `instrs[meta.start + operand_idx].operand` for the operand slots).
  These accessors are the one small wrapper the rest of the pipeline
  reads through.
- `parser/core/instruction.rs`: `infer_instruction_meta_shape` reads
  the head slot of the instruction the parser is currently emitting,
  plus the funcidx operand at `start + 1` for call ops and the
  typeidx operand at `start + 2` for `call_indirect` — the slots the
  same parse step just wrote for those ops. Five further reads are in
  test helpers walking parsed streams by `op_lens`.
- `pipeline/ir.rs`: `lower_operand`, one of the two `.encoded` read
  sites on the external-input path singled out by the audit plan. The
  `call_recipe_ref` read is gated on a direct-call op identity check,
  and the parser builds operand 0 of those ops as a `CallRecipeRef`
  (the call parsing in `instruction.rs`); the fallthrough `encoded`
  read observes a fully initialized operand copied from the stream
  (facts 1–2). The twin site in the common layer,
  `lower_materialized_operand` in `common.rs`, has the same shape and
  the same justification — its operands are materialized from
  `LoweredOperand` values, which are fully initialized by
  construction — but it belongs to the common layer and is recorded
  here as an observation for the follow-up audit lane, not changed by
  this one.
- Test reads of materialized streams in `pipeline/encode.rs` (6) and
  parsed streams in `optimizer/mod.rs` (4): the reads follow the
  `op_lens` layout of streams the test itself produced through the
  parser or the encoder under test.

### Class D — Eight-byte arm literals (13 sites): sound

`Operand { memarg }.encoded`, `Operand { memarg: MemArg { .. } }`,
and `Operand { i64: value }.encoded`. The written arm spans all eight
bytes of the union and is plain data (`MemArg` has no padding), so
the literal fully initializes the union and the `encoded` read
observes no uninitialized bytes. Sites: `fold_const_base_memarg` in
`analysis.rs` and `versioning.rs`, the three `folded.encoded` reads in
`select.rs` (whose `fold_const_base_memarg` returns the `MemArg`
operand), `raw_i64_operand` in `transform.rs`, the `raw_memarg` test
helper in `encode.rs`, and six test constructions in
`optimizer/mod.rs`.

### Class E — Narrow-arm literals (4 sites): defect, fixed

`Operand { local_addr }.encoded` reads the raw eight bytes of a union
whose high four bytes the `u32`-typed `local_addr` arm never wrote.
See the next section.

## Defect found: an uninitialized-tail read in the transform pipeline

`raw_local_operand` in `pipeline/transform.rs` built
`LoweredOperand::Raw(unsafe { Operand { local_addr }.encoded })`.
`local_addr` is a `u32` arm, so the literal left the high four bytes
uninitialized and the `.encoded` read materialized them — the #292
undefined behaviour, live in production code: the helper is called by
`make_temp_local_get` / `make_temp_local_tee` / `make_temp_local_set`,
which the common-subexpression transform (`cache_redundant_local_exprs`)
and the hoisting transform use to synthesize temp-local instructions
for ordinary optimized functions.

Reproduction (before the fix), with the rolling nightly's Miri:

```text
$ MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri test \
    -p telomere --lib optimizer_caches_redundant_local_expression_into_temp_local
error: Undefined Behavior: constructing invalid value of type [u8; 8]:
       at [4], encountered uninitialized memory, but expected an integer
  --> crates/telomere/src/parser/core/optimizer/pipeline/transform.rs:656
  0: transform::raw_local_operand
  1: transform::make_temp_local_tee            (transform.rs:334)
  2: transform::cache_redundant_local_exprs    (transform.rs:233)
  3: transform::run_transforms                 (transform.rs:58)
```

Byte `[4]` is the first uninitialized tail byte — the same signature
as #292. The defect survived the #292 sweep because that sweep (and
the audit plan's count) matched narrow literals by the pattern
`Operand { <field>:` with an explicit colon; the field-init shorthand
`Operand { local_addr }` has no colon and was invisible to it. The
same shorthand form existed in three test helpers
(`raw_local` in `pipeline/encode.rs` tests and `optimizer/mod.rs`
tests, `raw_local_operand` in `pipeline/analysis.rs` tests). Test
helpers matter here: they feed `Raw` payloads into the same equality
comparisons as pipeline output, so a garbage tail in a helper is a
latent flake as well as UB under Miri.

Fix: all four sites now build the operand through the canonical
constructor, `Operand::from_local_addr(local_addr).encoded`, which
zeroes the tail — the same form the sibling helper in
`versioning.rs` already used. No behaviour change beyond making the
tail deterministic zero, which is the canonical form the rest of the
pipeline already produces and compares against.

After the fix, the same Miri command passes (2 tests under the
`optimizer_caches_redundant` filter, 0 failed), as do the
`narrow_operands_are_fully_initialized_with_zero_tails` and
`miri_repro_module_operands_are_fully_initialized` tests under Miri.
The full release workspace suite is unchanged at 1,431 passed /
0 failed, and the vendored component suite passes unchanged
(files=79, ran=22, excluded=57, checked=836 — the current-main
values since #384 removed the external-visibility exclusion).

A note for the follow-up lanes: the runtime-side count of narrow
`Operand` literals in the audit plan (25) was taken with the same
colon pattern, so it undercounts shorthand forms. The runtime and
common layers should be re-counted with a pattern that also matches
`Operand { <field> }` shorthand before their audit is planned.

## What this audit changed

- The four Class E sites now use `Operand::from_local_addr`
  (one production helper in `transform.rs`, three test helpers).
- Every one of the 107 sites carries a `// SAFETY:` comment at the
  site stating its class invariant and the fact that establishes it,
  per the site contract in [unsafe-policy.md](unsafe-policy.md).
- No `allow` attributes were added anywhere, and no site needed one:
  every site is either sound under the invariants above or was the
  Class E fix.
- Individual allows: 0. Safe rewrites: 0 (the Class E change is a
  constructor swap, counted as the defect fix). SAFETY comments: 107.

## Verification

- `cargo test --release --workspace`: 1,431 passed / 0 failed before
  and after (same target directory).
- `cargo clippy --workspace --all-targets --tests --features jit --
  -D warnings` (the lint workflow's command): exit 0; the parser
  subtree's `#![deny(unsafe_op_in_unsafe_fn)]` remains in force.
- `cargo fmt --all -- --check`: clean.
- Miri (rolling nightly, `-Zmiri-disable-isolation`): the defect
  reproduction above fails before the fix and passes after; the
  two `optimizer_caches_redundant*` tests and the two
  full-initialization regression tests
  (`narrow_operands_are_fully_initialized_with_zero_tails`,
  `miri_repro_module_operands_are_fully_initialized`) all pass
  under Miri after the fix.
- Vendored component suite: unchanged (see above).

## Out of scope, recorded for the follow-up lanes

- The runtime subtree's unsafe sites (2,304 open
  `unsafe_op_in_unsafe_fn` sites, all in `src/runtime/`), the common
  layer's sites, and the runtime-side narrow `Operand` literals —
  follow-up lanes 132-D through 132-G in the #132 plan. The
  shorthand-literal blind spot described above applies to their
  counts.
- The Miri CI subset (acceptance condition 2) is lane 132-C; this
  audit used Miri locally as dynamic backing for the parser layer
  only.
