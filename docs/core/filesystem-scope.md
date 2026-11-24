# Filesystem scope

Status: decision record for
[#134](https://github.com/yieldspace/telomere/issues/134), implemented in the
same change. This document is the contract for what the component WASI
provider's `wasi:filesystem` implementation does and does not provide.

## Decision

**Read-only is the contract.** The provider implements every non-mutating
`wasi:filesystem/types` operation and does not register the mutating ones
with the component linker, so a guest that imports a mutating function
fails at instantiation time with a link error naming the function, instead
of trapping mid-call. The withheld functions are excluded from the
generated bindings' linker registration (the `exclude` option of
`telomere-component-bindgen`), in
`crates/telomere-component-wasi/src/lib.rs`:

| Withheld (mutating) | Implemented (non-mutating) |
| --- | --- |
| `write-via-stream`, `append-via-stream` | `read-via-stream` |
| `write`, `set-size` | `read`, `read-directory` |
| `set-times`, `set-times-at` | `stat`, `stat-at` |
| `create-directory-at`, `remove-directory-at` | `get-flags`, `get-type` |
| `rename-at`, `unlink-file-at` | `open-at` (read opens only, see below) |
| `link-at`, `symlink-at` | `readlink-at`, `is-same-object` |
|  | `metadata-hash`, `metadata-hash-at` |
|  | `advise`, `sync`, `sync-data` |
|  | `directory-entry-stream.read-directory-entry` |
|  | `filesystem-error-code` |

`wasi:filesystem/preopens` (`get-directories`) is fully implemented.
The support matrix records the resulting count (17 of 29 `types`
functions).

## Contract

- **Preopens are read-only grants.** `WasiStateBuilder::preopen_dir` grants
  a directory the guest may read. There is no writable-preopen grant, and
  `get-flags` never reports `write` or `mutate-directory` on any descriptor
  this provider issues.
- **`open-at` rejects write intent with an error code, not a trap.** Asking
  for `create`, `exclusive`, or `truncate`, or for the `write` or
  `mutate-directory` descriptor flags, fails with `error-code::read-only`,
  as the WIT contract specifies for a base descriptor without those rights.
- **The sync/advice operations are honest no-ops.** `sync` and `sync-data`
  are specified to succeed with no effect on a descriptor not opened for
  writing, and no descriptor here is ever opened for writing; `advise` is a
  hint a host may ignore. All three still validate that the descriptor is
  live and fail with `bad-descriptor` otherwise.
- **Withheld functions fail at link time.** A guest whose core module
  imports, for example, `[method]descriptor.write` fails instantiation with
  an error naming that function. A guest that imports the
  `wasi:filesystem/types` instance but only uses the read surface links and
  runs unchanged, because every previously implemented function is still
  registered. A guest that imports a withheld function fails even if it
  would never have called it (see Known gaps).
- **The exclusion travels with the provider's bindings.** Downstream
  worlds that adopt the provider's generated bindings
  (`telomere_component_wasi::bindgen!`) share this registration surface; a
  host that wants to offer the mutating functions generates its own
  bindings from the WIT and implements them itself.

## Evidence

Measured on the pre-change tree (origin/main `5e3f100`):

- 10 of 29 `types` functions were implemented; the other 19 — every write
  and mutation path, plus `readlink-at`, `is-same-object`,
  `metadata-hash(-at)`, `advise`, and the sync pair — fell through to the
  generated trait defaults and trapped mid-call with
  ``host function `[method]descriptor.write` is not implemented``.
- Preopens were already read-only by construction
  (`read_only_descriptor_flags` in `state.rs`): the missing piece was the
  advertised surface, not the grant.

After the change, the runtime tests in
`crates/telomere-component-wasi/tests/wasi_runtime.rs` pin both sides of
the contract: a guest that reads `hello.txt` through
preopens → `open-at` → `read` runs end to end, and a guest importing
`[method]descriptor.write` fails instantiation under both the sync and
async linkers with the withheld function named. Provider tests pin the
completed read surface (readlink targets and escapes, same-object,
metadata-hash stability and change, the no-op sync/advice validation, and
the `open-at` mutate-directory rejection).

## Why not write support

Option (a) in #134 — implement the write methods a plugin host needs —
was rejected for now, on measured rather than assumed grounds:

- **There is no write grant to extend.** The builder offers exactly one
  filesystem grant, the read-only preopen. Write support first needs a
  grant-model design (an opt-in writable preopen in `WasiStateBuilder`,
  descriptor-flag plumbing, and a CLI surface for it). That is a product
  decision, and nothing in the repository scopes it: no example, test, or
  CLI path persists anything through `wasi:filesystem`, so "the methods a
  plugin host needs" has no concrete referent to design against yet.
- **The methods are the small part.** `write-via-stream` and
  `append-via-stream` need file-backed output streams, which the `io`
  provider does not have (its output streams are stdout/stderr only);
  `set-times(-at)` needs timestamp mutation the standard library cannot
  perform; and `symlink-at`/`link-at` create new sandbox-escape vectors
  (guest-created links inside a preopen) on the surface `SECURITY.md`
  treats as highest-severity — while TOCTOU hardening of the existing
  path checks is already a noted follow-up. Landing the easy 80% of that
  surface without the grant model and the stream work would recreate this
  issue in a worse shape.
- **It coheres with the preview1 decision.** #137's scope record puts
  filesystem access in the component stack with preopen grants and keeps
  the preview1 runner filesystem-free. The grants that exist are read
  grants; this record completes that position instead of leaving the
  component provider advertising a full read/write interface it only
  partially implements.

Revisit write support when a concrete plugin-host consumer names the
methods it needs; the re-entry work is then a writable-preopen grant
(defaulting to read-only) plus per-method implementations with
path-sandboxing tests proving preopen escapes fail on the new paths.

## Alternatives considered

- **Document the status quo.** Rejected: the mid-call trap is the failure
  shape the issue prohibits, and documentation alone leaves the default
  world advertising functions that do not exist.
- **Withhold everything unimplemented, without completing the read
  surface.** Rejected: `readlink-at`, `is-same-object`, and
  `metadata-hash(-at)` are non-mutating, and read-only guests reach them
  through ordinary library calls (for example `std::fs::canonicalize`
  lowers to `readlink-at`). Withholding them would keep read-only guests
  failing on functions a read-only filesystem can honestly provide.

## Known gaps this decision leaves

- TOCTOU hardening of the existing path checks (`resolve_guest_path` /
  `ensure_within_preopen`) remains a follow-up, as #134 notes; it is
  unchanged by this decision and applies to the read surface as before.
- Guests are linked by what their core modules import, so a guest that
  imports a withheld function fails at instantiation even if that call
  would never execute. That is the intended link-time contract: the
  failure is early, named, and identical for every embedder of the
  default provider.
