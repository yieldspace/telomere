# AoT artifacts

Status: contract for [#198](https://github.com/yieldspace/telomere/issues/198),
implemented in the same change. This document is the contract for the
AoT artifact container: what an artifact is, what it pins to, what
loading one guarantees, and what it explicitly does not guarantee.
The module codec it embeds is specified in
[precompiled-modules.md](precompiled-modules.md) (#196); the
relocation-capable emitter it relies on is #197.

## What it is

An AoT artifact is a signed native-code bundle for one parsed core
module:

- the module itself, as a #196 precompiled serialization;
- one native blob per defined function the baseline emitter accepts,
  emitted offline with placeholder immediates, plus the relocation
  table (stub sites and direct-call funcaddr sites), the #209
  trap-site table, and a per-function layout checksum;
- an exact-build fingerprint (below);
- a signature block.

Functions the JIT emitter would reject have no blob and run
interpreted after load — per `docs/core/jit-coverage-audit.md` that
is the normal case for bridge-heavy code, not an error.

The public API is `build_aot_artifact` (producer, `jit` feature),
`open_aot_artifact` (loader), and `install_aot_artifact` (cache
installation), all in the `telomere` crate root.

## CLI surface (#199)

`telomere-cli` (built with `--features jit`) is the offline
producer and the reference loader:

- `telomere-cli compile --aot MODULE [-o OUTPUT]` writes an AoT
  artifact (default output: the module path with `.taot`
  appended). Compilation only ever parses the module — it never
  instantiates it, so guest code (including the start function)
  never runs at compile time; a test pins this with a module whose
  start function traps.
- `--sign-key <path>` signs the artifact; the environment variable
  `TELOMERE_SIGN_KEY` names the same kind of file when the flag is
  absent. Key handling stays in the CLI layer: the file's bytes are
  an HMAC-SHA256 key (the same file verifies, so this is a
  developer/CI scheme, not a distribution scheme), and the stored
  key id is derived from the key material. Without a key the
  artifact is **unsigned — an explicitly opted-in developer mode**:
  the producer prints that it did so, and loaders refuse the
  artifact by default.
- Running an artifact (`telomere-cli MODULE.taot ...`, in both the
  export-call and preview1-command forms) opens it under the
  fail-closed loader: a signed artifact requires
  `--verify-key <path>` naming its key, and an unsigned one
  requires `--trust-unverified-artifact`. Loading an artifact
  enables native execution for that run, since installed blobs are
  only entered under it.
- `telomere-cli compile --check ARTIFACT` validates an existing
  artifact against the running build (fingerprint, envelope, and —
  with `--verify-key` — signature) and reports the typed rejection
  reason for a stale or corrupt one. Checking never installs or
  executes anything.
- `compile` rejects component binaries with a pointer to the core
  workflow; component bundles are a separate issue.

## Trust boundary

A native artifact is arbitrary code execution once its bytes are
mapped executable. In the terms of the sandboxing track (#119): the
artifact loader is a *host-side* trust boundary, not a guest-side
one. The boundary is cryptographic — who produced this — not
structural — what does it look like. Structural checks in the loader
are defense in depth only.

Accordingly:

- The default posture is **refuse unsigned artifacts**. Accepting an
  unsigned artifact requires the embedder to opt in explicitly
  (`AotLoadPolicy::allow_unsigned`; the honest name for a CLI flag is
  `--trust-unverified-artifact`).
- Signature verification is embedder-pluggable through the
  `AotSigner`/`AotVerifier` traits (ed25519 is the suggested
  default). The container carries an opaque signature plus a key id;
  the workspace gains no new non-dev crypto dependency.
- **Not a validation substitute:** structural checks are defense in
  depth; wasm-sandbox guarantees hold only for artifacts produced by
  a trusted builder from validated wasm. A signature from an
  untrusted key is worth nothing.

## Load path (strict order, fail-closed)

1. Parse only the outer envelope (magic, format version, payload
   length, signature-block lengths).
2. Verify the signature over the whole container. Opening an
   artifact takes no store and allocates no executable memory, so
   demonstrably no executable mapping exists before this step
   passes.
3. Check the exact-build fingerprint; any field difference is a
   typed `AotError::FingerprintMismatch` naming both fingerprints.
4. Check the module binding: the SHA-256 of the embedded
   precompiled module must equal the binding recorded in the signed
   payload, so native blobs can never be paired with a different
   module.
5. Deserialize the precompiled module and validate the function
   table: funcidx range, strictly increasing entries, per-function
   layout checksum against the deserialized module's materialized
   form, reloc offsets inside the blob, stub indices inside the
   stub table, funcaddr targets inside the module's function space.

Installation is a separate, explicit step. `install_aot_artifact`
first binds the artifact to the instance's complete module
identity: the module facts recorded at instantiation (function
types for the whole index space, table/global/memory types, and
exports) must equal the artifact module's, and every blob's target
slot must name a function the instance itself defines at that
index — never an imported slot holding another instance's function
object. It then re-checks every layout checksum against the
materialized form of the *instance's actual functions*, patches a
private copy of each blob (stub addresses from this process,
funcaddrs from this instance), and installs the whole batch into
the store's JIT cache atomically — any failure installs nothing.
Entries the start function already cached through lazy compilation
are accepted as-is rather than failing the batch. It refuses metered
stores (AoT code is emitted unmetered; #127/#171 gaps apply to AoT
code identically). There is no silent fallback to parsing wasm
anywhere in this path; falling back is the embedder's explicit
decision. Every failure is a typed `AotError` (#143 alignment); the
decoder never panics on malformed input (#128 posture), and the
container deserializer has a dedicated fuzz target
(`decode_aot_artifact`, wired into `fuzz.yaml`).

Diagnostics are carried, not dropped: each blob ships its #209
trap-site table, and the embedded module retains its name section
through the #196 codec, so traps in AoT-loaded code attribute to a
pc and a function name exactly as JIT-compiled code does.

## The contract: exact-build pinning

The format makes **no cross-version stability promise** — and
unlike the codec, not even a same-version one. Emitted code bakes
`repr(Rust)` layout constants (ExecuteContext offsets, the JIT
layout structs, `size_of::<Instr>` operand scaling) that Rust
guarantees across no two compilations, so only "same build" is
compatible. The fingerprint covers: format version, crate version,
rustc version, target triple, cargo feature set, the #196
source-tree hash (any change to a layout constant's source changes
it mechanically, with no developer discipline involved), the op-ID
table hash, the stub-table hash, a layout digest over the
`size_of`/`align_of` facts and every field offset emission bakes
into native code (execute-context, local-reference, global-value,
and memory layouts), and the emission mode
string (`baseline-unmetered-v1`). There is no cross-target artifact
production; riscv64 is excluded until #161 resolves.

## Position independence (v1 tradeoff, stated openly)

Funcaddr immediates are store-level, so patched code is
per-instance, like today's per-funcaddr JIT cache. Patching also
dirties essentially all text pages (stub calls are inline and
dense), so AoT text is anonymous-equivalent memory, the same as JIT
today: **the artifact saves time, not resident code memory, in
v1.** A future "clean text" mode (stub table + funcaddr table
reached via ctx, file-backed shared mappings) is explicitly
deferred and should be revisited after #106 replaces ObjectRef
with typed handles.

## Patching availability

Relocation patch encodings live in the emit-gated backends, so
installation is fully functional in `jit` builds (which include
`aot-load`). In an `aot-load`-only build, opening and verifying
works, but `install_aot_artifact` fails closed with
`AotError::PatchingUnavailable`. Lifting this means moving the
patch encodings into the load-gated codegen surface; it is recorded
here rather than silently assumed.

## Enforcement

- `TELOMERE_WAST_AOT=1 TELOMERE_WAST_AOT_REQUIRE_ACCEPT=1` routes
  every wast-corpus module through build → open → instantiate →
  install in the test harness, with native execution enabled so
  the installed code is actually entered; CI runs it fail-closed
  in the debug-wast job and in the `jit-native-test` matrix
  (macOS arm64 + Linux x86_64, same pass list as the JIT lane).
  The harness also asserts, via the thread-local front-end phase
  counters (`telomere::frontend_phase_counters`, #199), that
  opening an artifact deserializes exactly one module and moves
  neither the parse nor the optimize counter, and that installing
  one emits nothing on demand — so the lane cannot silently pass
  by re-running the front-end.
- `tests/cli_compile.rs` in the CLI package covers the producer
  and load path end to end (examples-corpus equivalence, signing
  and trust refusals, `--check`, the never-executes-guest-code
  probe) and runs in the same CI matrix.
- `crates/telomere/tests/aot_artifact.rs` covers the tamper matrix,
  fingerprint mismatch, unsigned refusal, two-instance patching,
  and cache atomicity.
- Measurements: [benchmarks/aot-artifact.md](../benchmarks/aot-artifact.md).
