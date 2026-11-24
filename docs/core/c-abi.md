# C ABI scope

Status: decision record for
[#146](https://github.com/yieldspace/telomere/issues/146). This document is
the contract for how hosts written in languages other than Rust integrate
telomere in v1, and for the platform assumptions that decision rests on.

## Decision

**V1 integration is Rust-only. Telomere ships no C ABI in v1.**

There is no `cdylib`/`staticlib` artifact, no C header, and no supported way
for a C/C++ host to link the runtime directly. A C API is not rejected
permanently: it is deferred until one of the re-entry conditions at the end
of this document is met, and the minimal surface recorded there is the
starting point when it is.

## Measured current state

Audited on `origin/main` at `3922f93` (2026-11-24):

- **Zero public C-ABI exports.** `pub extern "C" fn` occurs 0 times in the
  workspace. The 53 `extern "C"` occurrences are all internal: `pub(crate)`
  stub functions in `crates/telomere/src/runtime/jit/stubs.rs` that
  JIT-generated code calls, `extern "C" { }` blocks in
  `crates/telomere-jit-codegen/src/code_memory.rs` that *import* OS entry
  points (`mach_vm_*`, `__clear_cache`, `sys_icache_invalidate`), and
  function-pointer type aliases in tests.
- **The three `#[unsafe(no_mangle)]` items are not a C API.** They are the
  interpreter dispatch handlers `op_select4/8/16`
  (`crates/telomere/src/runtime/vm/locals.rs`), whose signatures use Rust
  types (`&mut ExecuteContext`, `VMResult<()>`). The unmangled names exist
  for the dispatch/codegen machinery, not for foreign callers. The three
  `#[repr(C)]` structs (`DirectCallTarget`, `CallRecipeRef`, the JIT ABI
  struct) are likewise `pub(crate)` instruction-layout types.
- **Nothing to link against.** No `Cargo.toml` in the workspace declares
  `crate-type = ["cdylib"]` or `["staticlib"]`; there are no `.h` files and
  no cbindgen configuration. The only supported integration artifact is the
  Rust crate, as used by
  [`examples/minimal-embedder/`](../../examples/minimal-embedder/README.md),
  whose configurations are what the footprint gate measures.
- **The public Rust surface is pre-stable and still moving.** The workspace
  version is `0.1.0-alpha.1`, every package is `publish = false`, and the
  README states that public APIs are implementation-driven until a stable
  release. Within the current development cycle alone, the surface a C ABI
  would have to mirror was incompatibly redesigned three times: the
  `VMResult` error redesign (#143, PR #256), the host-linking API redesign
  that removed `telomere::host_abi` (#216, PR #269), and the
  embedder-configurable limits added to `RuntimeConfig` (#126, PR #289).
- **No C consumer exists.** No issue, document, or integration request from
  a C/C++ host is recorded in the repository. The three documents that
  mention a C ABI (`docs/core/public-api.md`, `docs/core/trap-reporting.md`,
  `docs/support-matrix.md`) only assign its ownership to #146; they record
  no demand.

## Platform assumptions (part of this decision)

These are measured properties of the current tree, stated here so the
footprint and product-surface tracks can rely on them:

- **Every crate is `std`.** No crate declares `no_std`, and the runtime
  allocates through `std` throughout. A host without a Rust `std` target
  cannot run telomere, in any language.
- **The interpreter requires Unix `mmap`.** Linear memory reservation in
  `crates/telomere/src/common/memory.rs` calls `libc::mmap` / `mprotect` /
  `munmap` with no platform `cfg` gate and no fallback backend
  (`mmap_region`, line 579). `libc` is a dependency of three crates
  (`telomere`, `telomere-jit-codegen`, `telomere-component-wasi`), with
  call sites in four files.
- **The JIT is Unix-only.** `crates/telomere-jit-codegen/src/code_memory.rs`
  is `cfg(unix)`-gated throughout, split between Linux (`memfd_create`) and
  macOS (`mach_vm_remap`) executable-memory backends; the supported backend
  list is macOS AArch64, macOS/Linux x86_64, and Linux riscv64.
- **Consequence for the FFI question:** the hosts a C header alone cannot
  serve are exactly the deeply embedded ones. Bare-metal and RTOS firmware
  without `std` and `mmap` is blocked by platform support, not by the
  absence of a header; adding a C ABI would not change its situation. The
  hosts a C ABI *would* newly serve are C/C++ programs on Unix-class
  systems (Linux appliance firmware with an OS), which can already run the
  runtime — they lack only the language bridge.

## Options considered

### (a) Ship a minimal C surface in v1

A new `telomere-capi` crate (`cdylib` + `staticlib`) exposing opaque-handle
load / instantiate / call / limits / error-read functions.

- Technically feasible on Unix-class hosts today: a Rust `staticlib` links
  into a C/C++ program, and the runtime's platform requirements are already
  met there.
- Rejected for v1 because it freezes a mirror of an alpha API. A C ABI
  carries a stronger stability expectation than a Rust API — C consumers
  cannot be updated by a compiler, and ABI drift fails silently. Each of
  the three redesigns above would have forced either a C ABI break or a
  constraint on the Rust change. The gated embedder footprint series would
  not move (the capi crate is outside their dependency graph), but the
  *surface* the footprint track is still shrinking would acquire a
  stability obligation it does not currently have.
- It also adds a test matrix that does not exist: a C compile/link smoke
  test and header-drift checks in CI, maintained for a surface with no
  consumer.

### (b) Rust-only for v1, C API deferred with named re-entry conditions

- Matches the project's declared state (alpha, `publish = false`, APIs
  implementation-driven until stable). Nothing that exists today is taken
  away; no consumer is broken, because there is none.
- The cost is explicit and accepted: C/C++ hosts, including Linux-class
  appliance firmware, cannot embed v1. Per the platform analysis above,
  the deeper-embedded hosts are unreachable in v1 regardless, so the
  marginal loss of deferring the header is limited to Unix-class C hosts
  until a re-entry condition fires.
- This is the decision taken.

### (c) Phased header generation with cbindgen

- Rejected as a v1 step. cbindgen generates a header from an existing Rust
  FFI layer; it does not design the ownership rules, the error contract, or
  the versioning policy that make an ABI supportable. Generating a header
  over today's surface would create a de-facto ABI commitment without the
  stability decision, which is the worst combination. cbindgen (or a
  hand-written header) remains the likely tooling *when* a C surface is
  built under the re-entry conditions.

## Recorded for the future C surface (out of v1 scope)

When a re-entry condition fires, the scoped starting point — recorded now
so the decision does not have to be re-derived — is the minimal surface
from #146, as follow-up implementation work in its own issue:

- opaque handles for engine/store/instance; `load` (compile from bytes),
  `instantiate`, `call` (typed scalar arguments/results only), `limits`
  (the `LimitsConfig` fields of #126), and `error_read` (a stable numeric
  error code plus a caller-buffer message, mirroring the #143 error kinds);
- ownership rules: telomere allocates, the caller releases through the
  matching `*_free` function; no Rust-allocated buffer is freed by the
  caller directly; no guest/host callback crosses the boundary in the
  first surface;
- testing approach: a C11 smoke test compiled and run in CI against the
  shipped header, plus a header-drift check so the header cannot silently
  diverge from the FFI layer.

## Re-entry conditions

Revisit this decision when either holds:

1. The Rust public API is declared stable for a release — the boundary
   `RELEASING.md` already uses (crates.io publishing decision / stable 1.0).
   At that point the mirror cost of a C surface is bounded and the
   stability obligation is one the project has already accepted for Rust.
2. A concrete C/C++ consumer on a Unix-class host appears with a named
   surface need. A named consumer converts the maintenance cost from
   speculative to paid-for, and its requirements replace the guesses in
   the recorded surface above.

Until then, documents that deferred to #146 for "the C ABI mirror"
(`docs/core/public-api.md`, `docs/core/trap-reporting.md`,
`docs/support-matrix.md`) should be read through this record: there is no
C mirror in v1, and Rust-side diagnostics and error shapes have no C
counterpart to stay compatible with.
