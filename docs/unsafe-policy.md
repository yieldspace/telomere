# Unsafe Code Policy

This document is the written unsafe policy required by #132. It states
which layers may use `unsafe`, what justification every site must carry,
and how the lint ratchet enforces both over time. The companion pieces of
#132 — the per-site audit of the parser/optimizer layer and the miri CI
subset — are tracked separately on the issue; this document is the
policy they operate under.

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
