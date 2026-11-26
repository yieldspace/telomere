# Unsafe Code Policy

This document is the written unsafe policy required by #132. It states
which layers may use `unsafe`, what justification every site must carry,
and how the lint ratchet enforces both over time. The companion pieces
of #132 are the per-site audit of the parser/optimizer layer (tracked
separately on the issue) and the miri CI subset (defined under
"Dynamic checking" below); this document is the policy they operate
under.

Telomere is a WebAssembly runtime: it parses unvalidated input and then
executes it on an interpreter core that is deliberately built on raw
pointer arithmetic. Rewriting that core into safe Rust is out of scope
(#132). What this policy governs is that every unsafe site is *placed*
deliberately, *scoped* tightly, and *justified* in writing at the site.

## Where unsafe may live

Measured on `main` at the time of writing (token counts are
`grep -o '\bunsafe\b'` over `src/`; see "Current measured state" below):

| Layer | Unsafe today | Rule |
| --- | --- | --- |
| Parser / optimizer (`crates/telomere/src/parser/`) and binary reader (`src/binary/`) | 107 + 0 tokens | Permitted only where a safe API cannot express the operation. These layers touch input before validation, so the burden of proof is highest here. Deny-stage lint (below). |
| Common layer (`crates/telomere/src/common/`, `src/common.rs`) | foundation of all layers | Same rule as the parser layer. Deny-stage lint. |
| Interpreter runtime (`crates/telomere/src/runtime/`) | the bulk of the workspace's sites | The unsafe core is the design (stack, linear memory, dispatch). Sites are permitted, must follow the site contract below, and are being ratcheted subtree by subtree. |
| JIT (`crates/telomere-jit-codegen/`, `src/runtime/jit*`) | executable memory, native ABI | Permitted for code-memory management and generated-code invariants. The codegen crate has opted into the workspace lints. |
| Component model (`telomere-component`, `-wasi`, `-bindgen`, `telomere-macros`) | 11 tokens total; `telomere-component` itself has **zero** | Keep it that way: new unsafe in this layer needs the adding-unsafe procedure below and is expected to stay confined to FFI and proc-macro edges. |
| CLI and examples (`src/`, `examples/`) | 9 tokens | No new unsafe without the adding-unsafe procedure. |

## The site contract

Every unsafe site, in every layer, must satisfy all of the following:

1. **Scoped.** Inside an `unsafe fn`, every unsafe operation sits in an
   explicit inner `unsafe { }` block, as small as the operation allows.
   An `unsafe fn` body is never one implicit unsafe region. This is the
   `unsafe_op_in_unsafe_fn` lint.
2. **Justified at the site.** Every inner `unsafe` block (and every
   `unsafe` block elsewhere that is not self-evident from the callee's
   documented contract) carries a `// SAFETY:` comment stating the
   invariant that makes the operation sound *and why it holds here* —
   the check, allocation bound, or caller obligation that establishes
   it. Restating what the operation does is not a justification.
3. **Contract on the function.** Every `unsafe fn` documents its
   `# Safety` obligations: what the caller must guarantee. Every
   `unsafe impl` (e.g. `Send`/`Sync`) states the invariant that makes
   the impl sound, at the impl site.
4. **Minimal surface.** Prefer one small `unsafe fn` with a documented
   contract over unsafe operations spread across callers. New unsafe
   goes behind the narrowest wrapper that expresses the invariant.

## Enforcement: the lint ratchet

Enforcement is staged, because the interpreter runtime still has a
measured four-digit count of unscoped sites and a blanket `deny` would
either break the build or invite mass rubber-stamp edits. The stages:

- `[workspace.lints.rust]` in the root `Cargo.toml` declares
  `unsafe_op_in_unsafe_fn = "warn"`. The lint workflow runs clippy with
  `-D warnings`, so for every crate that has opted in with
  `[lints] workspace = true` the lint is a hard CI error. Opted in
  today: the root CLI package, `telomere-jit-codegen`,
  `telomere-component`, `telomere-component-wasi`,
  `telomere-component-bindgen`, `telomere-macros`, and
  `examples/minimal-embedder` — all measured at zero sites.
- Inside `crates/telomere`, the parser, common, and binary subtrees
  carry `#![deny(unsafe_op_in_unsafe_fn)]` at their module roots
  (`src/parser.rs`, `src/common.rs`, `src/binary/mod.rs`). They
  measured zero sites after two common-layer sites were scoped and
  justified for this change.
- The `telomere` crate as a whole does **not** inherit the workspace
  lints yet; `src/runtime/` is the remaining open subtree. Its count
  may only go down: a change that increases the measured open-site
  count of a not-yet-deny subtree does not land. When a subtree
  reaches zero it gains the module-root deny in the same change. When
  `src/runtime/` reaches zero, the crate switches to
  `[lints] workspace = true` and the per-subtree attributes come out.

To re-measure the open sites of any subtree:

```shell
RUSTFLAGS="-W unsafe_op_in_unsafe_fn" \
  cargo check -p telomere --all-features --all-targets \
  --message-format short 2>&1 | grep 'warning\[E0133\]'
```

## Adding new unsafe

1. First try to express the operation with a safe API or an existing
   wrapper. If that fails, keep the new surface as small as possible.
2. Write the site to the contract above: scoped block, `// SAFETY:`
   justification naming the establishing check or obligation, and a
   `# Safety` contract on any new `unsafe fn`.
3. In a deny-stage subtree or an opted-in crate, the lint enforces
   scoping mechanically. Elsewhere, the PR description must call out
   the new site explicitly so review can check the justification.

## Dynamic checking: the Miri subset

The lint ratchet above is static. Its dynamic counterpart is a Miri run
over the layers that touch input before validation — the parser and
optimizer, the binary reader, and the common layer — executed by
`.github/workflows/miri.yaml` on a weekly schedule and on manual
dispatch. Like the toolchain watch, the job **reports without
gating**: it is not a required status check and never runs on pull
requests. A red run is the report: either Miri found undefined
behaviour in a covered layer, or a covered test started reaching a
platform facility Miri cannot emulate, which means the subset
definition below needs revisiting rather than the code having
regressed.

The subset is the `telomere` lib-test suite filtered to the `parser::`,
`binary::`, and `common::` module trees, minus the exclusions below:
187 of the 267 lib tests (parser 131 of 132, binary 9 of 9, common 47
of 68) at the time of writing. It covers the layers with the highest
unsafe burden of proof — the parser/optimizer subtree (107 unsafe
tokens, including the pipeline whose uninitialized-operand read was
#292) and the common layer everything stands on (operand stack, store,
metering, the precompiled codec) — while staying inside what Miri can
execute: no covered test allocates JIT code memory or runs generated
native code.

Pass criterion: a Miri **error** (undefined behaviour) fails the run.
Miri **warnings** do not; in particular the integer-to-pointer
provenance warning in `common::StablePc::resolve`, first recorded by
the #292 investigation, is expected noise until that cast is revisited.

The run uses `MIRIFLAGS="-Zmiri-disable-isolation"` (the configuration
the #292 investigation established) and the dated nightly
`nightly-2026-11-17`, whose rustc and Miri are the exact builds the
subset was measured with (rustc 1.102.0-nightly `5df8c02af`, Miri
`5df8c02afd`). Bump the pin deliberately, in its own change, after
re-measuring the subset.

### Exclusions (each verified individually under Miri)

1. **Tests that construct a real mmap-backed linear memory** — 20
   tests, 13 in `common::memory` and 7 in `common::store`. The memory
   implementation reserves its address space with a `PROT_NONE` `mmap`
   (`crates/telomere/src/common/memory.rs`), and Miri does not emulate
   `mmap`/`mprotect` with protections other than
   `PROT_READ|PROT_WRITE`; it stops with "unsupported operation" and
   notes that this is likely not a bug in the program. Every test in
   this group was run individually and stops at exactly that operation
   (no UB, no other failure):
   `common::memory`: `failed_grow_keeps_pages_and_commit_length_unchanged`,
   `initial_mprotect_failure_is_returned_from_local_memory_construction`,
   `interruptible_wait_is_woken_by_a_metering_interrupt`,
   `interruptible_wait_keeps_notify_and_timeout_result_codes`,
   `local_memory_reserves_lazily_and_commits_only_grown_pages`,
   `memory_write_copy_fill_and_grow_match_linear_model_bytes`,
   `resolved_long_waits_release_their_timer_threads`,
   `shared_memory_reserves_lazily_and_commits_only_grown_pages`,
   `shared_wait64_queue_tracks_notify_and_timeout_cleanup`,
   `shared_wait_queue_internal_state_tracks_notify_and_timeout_cleanup`,
   `shared_wait_queue_rejects_mismatch_and_notifies_fifo_up_to_count`,
   `shared_wait_timeout_completes_on_non_tokio_executor`,
   `zero_sized_memory_keeps_a_valid_uncommitted_reservation`.
   `common::store`: `cross_memory_copy_checkpoint_interrupts_before_the_later_chunk`,
   `store_atomic_cmpxchg_dispatch_preserves_old_value_and_alignment_errors`,
   `store_memory_creation_returns_initial_mprotect_failure`,
   `store_memory_dispatch_matches_handle_kind_and_cross_copy_paths`,
   `total_budget_adoption_refuses_over_cap_without_residue`,
   `total_budget_never_exceeds_cap_under_concurrent_shared_growth`,
   `total_budget_reservation_is_consumed_by_adoptions_and_released`.
   Five further `common::memory` tests that reject or account before
   any mapping exists were initially excluded by module and are **not**
   excluded: the individual probes showed they pass, and they run in
   the subset.
2. **One wall-clock complexity guard**:
   `parser::core::optimizer::pipeline::analysis::tests::analyze_stays_fast_on_a_deeply_nested_chain`
   asserts `analyze` finishes a 4,096-block chain in under 10 seconds
   of host time (the #170 complexity regression guard). Its input is
   sized for native speed; under interpretation it did not finish
   within 7 minutes, and a host-time bound is not a property Miri can
   judge. The analysis code it exercises is covered by the other
   analysis tests in the subset.

Everything else in the three trees runs. The interpreter runtime tree
(`runtime::`, the remaining 58 lib tests, plus the integration suites)
is outside the subset for now: its tests instantiate and execute
modules, reaching the JIT code-memory allocation (`memfd_create`, a
foreign call Miri cannot emulate) on jit-enabled paths and the
mmap-backed memory above wherever a test creates a memory. That is the
recorded boundary of the subset, not a silent gap; extending coverage
there is follow-up work.

## Current measured state

Baseline measurement for this policy (2026-11-27, `main` = `cc6550b`,
Rust 1.96.0, distinct `warning[E0133]` sites under
`-W unsafe_op_in_unsafe_fn`, `cargo check --workspace --all-targets
--features jit`, confirmed identical under
`cargo check -p telomere --all-features --all-targets`):

- **2,306 open sites, all in `crates/telomere/src/runtime/`** except the
  two common-layer sites fixed by the change that introduced this
  document. Parser, binary, common, and every other crate: **zero**.
- Concentration: `runtime/vm/memory.rs` 1,315;
  `runtime/vm/superinstructions.rs` 286; `runtime/vm/simd.rs` 182;
  `runtime/vm/numeric.rs` 182; `runtime/vm/atomics.rs` 94; the
  remaining 245 spread over the rest of `runtime/vm/` and
  `runtime/jit.rs`.
- By kind, the emitted warnings were 1,737 calls to unsafe functions,
  650 union-field accesses, and 634 raw-pointer dereferences (3,021
  emissions; the same site is reported once per target that compiles
  it, which is why emissions exceed the 2,306 distinct sites).
- For scale: `unsafe` token counts are 1,727 in `crates/telomere/src`
  (987 blocks, 728 `unsafe fn`, 6 `unsafe impl`, 2 `unsafe extern`),
  61 in `telomere-jit-codegen`, 11 across the component crates and
  macros, 9 in the root `src/`. Only 55 `SAFETY:` comments existed in
  `crates/telomere/src` before this policy — the sparsity #132 calls
  out, and the gap the ratchet exists to close.

Miri subset measurement (2026-11-27, nightly-2026-11-17 as above,
`MIRIFLAGS=-Zmiri-disable-isolation`, on fleur-du-lapin):

- **187 tests run, 187 pass, 0 fail**, running the workflow's exact
  commands with the pinned nightly from a clean target directory.
  Parser: 131 tests, test phase 853–1,000 s across two runs at the
  default parallelism of a 72-thread host and 940 s at
  `--test-threads=4` — the wall is set by
  the slowest individual tests, not by aggregate throughput, which is
  why the parser job's timeout (120 min) carries large headroom over
  both. Common: 47 tests, 118 s. Binary: 9 tests, 3 s. One further
  subset test (`common::debug_names` measurement probe) is ignored by
  default in the source and stays ignored under Miri.
- The first subset run found four remaining sites of the #292 class —
  `Operand { local_addr }.encoded`, which constructs the union's raw
  eight bytes from a four-byte field and leaves the high half
  uninitialized — in the optimizer pipeline: one in production code
  (`pipeline/transform.rs`, the temp-local materialisation used by the
  GVN/fusion transforms) and three in pipeline test helpers
  (`pipeline/analysis.rs`, `pipeline/encode.rs`, `optimizer/mod.rs`).
  All four now build the operand through `Operand::from_local_addr`,
  the canonical constructor #292 added; the subset is the standing
  regression guard for the class, and a static sweep confirms no other
  narrow-field union literal remains in the parser, common, or binary
  trees (the runtime tree's literals are the separate remainder the
  audit plan already tracks: they are only ever read back through the
  field that wrote them and never reach the optimizer).
