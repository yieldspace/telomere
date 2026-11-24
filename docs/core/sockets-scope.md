# Sockets scope

Status: decision record for
[#135](https://github.com/yieldspace/telomere/issues/135). This document is
the contract for how the default WASI provider world treats the
`wasi:sockets` interfaces, and records the capability model and scope a
future sockets implementation must start from.

## Decision

**`wasi:sockets` is excluded from the default provider world. The provider
does not register the sockets interfaces, and no sockets implementation
ships until a re-entry condition at the end of this document is met.**

Concretely: `add_to_linker_sync` / `add_to_linker_async` in
`telomere-component-wasi` register `cli`, `io`, `clocks`, `random`, and
`filesystem` only. A component that imports a `wasi:sockets` interface and
wires it into an export fails when the runtime resolves that import during
instantiation, with an error naming the interface — for example
`instance import 'wasi:sockets/instance-network@0.2.6' is not supported by
linker` — before any guest code runs. The generated bindings still contain
the sockets traits and registration functions, and the bundled WIT is
unchanged, so an embedder can implement and register sockets itself; only
the default provider world excludes them.

## Measured current state

Audited on `origin/main` at `5e3f100` (2026-11-24), before this change:

- **All seven sockets interfaces were registered with empty
  implementations.** `provider/sockets.rs` (44 lines) called the generated
  `add_to_linker` for `instance-network`, `network`, `udp`,
  `udp-create-socket`, `tcp`, `tcp-create-socket`, and `ip-name-lookup`,
  and implemented each `Host`/`HostAsync` trait with an empty `impl`. The
  bundled WIT declares **53 functions** across those interfaces (tcp 28,
  udp 18, ip-name-lookup 3, the other four 1 each) and **6 resource types**
  (`network`, `tcp-socket`, `udp-socket`, `incoming-datagram-stream`,
  `outgoing-datagram-stream`, `resolve-address-stream`). Every call fell
  through to the bindgen default and trapped with
  `host function ... is not implemented`
  (`telomere-component-bindgen/src/generator.rs`), mid-execution, after
  the component had linked successfully.
- **No consumer exists.** Nothing in the repository uses sockets: no test,
  example, or downstream crate references the sockets interfaces outside
  the provider registration itself, the support matrix, and the preview1
  scope record (which notes the preview1 adapter would need them, as one
  of several reasons that path stays rejected). There is no recorded
  request from a plugin host for TCP or UDP through this provider.
- **No network capability grant exists to implement against.**
  `WasiState`'s posture is deny-by-default: the builder starts with no
  arguments, no environment, no preopened directories, and buffered
  standard input, and the only capability grants it can express are
  read-only filesystem preopens. There is no builder setting that grants
  network access, at any granularity. A sockets implementation therefore
  has only two shapes available today: grant every guest full network
  access unconditionally — contradicting the provider's capability
  posture — or design the grant model first. The grant model is a product
  decision, not an implementation detail.
- **The implementation is not 53 leaf functions.** TCP and UDP socket
  methods return `wasi:io` stream resources (`tcp-socket.stream`,
  datagram streams, pollable subscriptions), so sockets state must
  integrate with the io provider's stream and pollable tables; name
  lookup is a streaming resource of its own; and host I/O errors must map
  onto the `wasi:sockets/network` error-code enum. Both the sync and
  async generated trait surfaces need every method. This is a subsystem,
  comparable in shape to the filesystem provider, not a stub-filling
  exercise.
- **Exclusion has an in-tree precedent.** `wasi:clocks/timezone` is
  already simply not registered by the provider, and the support matrix
  records it as such. The default world has never been "everything the
  bundled WIT declares"; it is the set the provider actually implements.
  Sockets were the anomaly: the one unimplemented family that was
  registered anyway.

One nuance of the runtime's link model, recorded for accuracy: function
imports are resolved lazily in this runtime (see the comment in
`crates/telomere-component/src/runtime/env.rs`), so the failure surfaces
when an import is resolved — during instantiation for any export path
that uses it, which is the only way a guest can call it. A component that
imports a sockets interface but never wires it into an export holds an
inert import, exactly as with any other unprovided import; it cannot
reach a sockets call.

## Options considered

### (a) Implement sockets now, scoped to outbound TCP

The smallest honest implementation: `instance-network`,
`tcp-create-socket`, the connect/stream subset of `tcp`, and
`ip-name-lookup`, with UDP and listen/accept deferred.

- Rejected for now, on two grounds. First, the capability model does not
  exist (above): this option forces the grant-model design into an
  implementation change, where it would get the least scrutiny, or ships
  unconditional network access for every guest, which breaks the
  provider's deny-by-default contract. Second, there is no consumer:
  the scope above is a guess at what a hypothetical plugin host needs,
  and the last several scope decisions in this repository (#137, #146)
  deferred exactly this shape of speculative surface until a named need
  or a stability boundary made the cost paid-for rather than guessed.
- The recorded capability model and scope below are the starting point
  when this option is taken under a re-entry condition, so the analysis
  is not lost.

### (b) Exclude sockets from the default world

- The advertised world matches the implementation, which is the bar this
  track sets (#134 states the same principle for filesystem). The failure
  shape changes from the worst one — link succeeds, first use traps
  mid-execution, guest state lost — to a resolution error at
  instantiation that names the unsupported interface.
- Nothing working is taken away: the previous behaviour was a trap on
  every call, so no functioning consumer can exist. Embedders who need
  sockets today keep a path: implement the generated traits and register
  them in their own linker.
- This is the decision taken.

### (c) Keep the stubs registered and document them as unimplemented

- Rejected. Documentation does not change the failure shape the issue
  identifies as the problem: the component still links and still traps on
  first use. The support matrix already documented the stubs accurately
  before this change; the defect is the registration, not the wording.

## Recorded for the future implementation (out of scope here)

When a re-entry condition fires, the starting point — recorded now so the
decision does not have to be re-derived — is:

- **Capability model first.** Network access is a `WasiState` grant,
  deny-by-default, consistent with preopens: no grant, no sockets. The
  grant design must answer, at minimum: granularity (whole-network
  versus per-family versus address allowlist), whether name lookup is
  part of the same grant or a separate one (the WIT separates
  `ip-name-lookup` for a reason), and whether listen/accept (inbound)
  requires a distinct grant from connect (outbound). With no grant, the
  denial must surface as the WASI `access-denied` error code from the
  call, not as a trap.
- **Scope.** First subset: `instance-network`, `tcp-create-socket`, the
  outbound connect/stream path of `tcp`, and `ip-name-lookup`. UDP,
  listen/accept, and socket options beyond the connect path are separate
  follow-ups, each behind the same grant.
- **Testing approach.** Loopback-based tests in the provider style:
  connect happy path against a local peer, lookup happy path,
  permission-denied paths with no grant (and with a grant that does not
  cover the target, if the model is finer than whole-network), and
  resource-drop coverage for every socket resource type.

## Re-entry conditions

Revisit this decision when any of these holds:

1. A concrete consumer — a named plugin host or component — needs TCP or
   UDP through this provider. A named consumer converts the
   implementation and maintenance cost from speculative to paid-for, and
   its requirements replace the guessed scope above.
2. The network capability grant model is designed and recorded as its
   own decision. Implementation must not precede it: a sockets
   implementation that lands first would set the de-facto model by
   accident.
3. A world-consistency audit (in the line of #134/#135) reopens the
   provider surface — for example if the filesystem decision lands write
   support and the provider's capability model is generalised, the
   network grant should be designed as part of that generalisation
   rather than sockets-specifically.

Until then, documents that describe the provider world should be read
through this record: the default world is `cli`, `io`, `clocks`,
`random`, and `filesystem`; `wasi:sockets` is not part of it.
