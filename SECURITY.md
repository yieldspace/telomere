# Security Policy

## Supported versions

None. Telomere is pre-release, has no stable release, and its workspace packages
are not published to crates.io (`publish = false`). Pre-release tags carry no
support commitment. There is no supported version line and no backport branch.
Fixes land on `main` only.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's private
vulnerability reporting on this repository:

<https://github.com/yieldspace/telomere/security/advisories/new>

Do not open a public issue for a suspected vulnerability first.

A useful report includes:

- the affected crate and, if you have it, the commit hash;
- a minimal `.wasm` or `.wat` reproducer, or the exact CLI invocation;
- what an attacker gains - sandbox escape, host memory disclosure, host memory
  corruption, or a denial of service that survives the guest.

## What to expect

This is a personal, pre-release project maintained alongside other work. **There
is no response-time guarantee and no guarantee that a report will be answered at
all.** If a report matters to you and you have had no reply, a follow-up is
reasonable. If you need a commitment on timelines, telomere is not a suitable
dependency yet.

## Scope

Telomere is a WebAssembly runtime, so the interesting boundary is guest-to-host:
a guest module or component escaping the sandbox, reading or corrupting host
memory, or reaching host resources that `WasiState` did not grant.

Please note that the following are already known and documented, and are better
filed as ordinary issues than as security reports:

- the experimental JIT is not hardened and is not recommended for untrusted
  guests. Metering and JIT are mutually exclusive: enabling metering on a Store
  disables its JIT rather than offering an unmetered JIT execution path;
- crashes reachable from a malformed or hostile component, including the
  resource-handle abort described in [examples/README.md](examples/README.md);
- resource exhaustion by a guest whose embedder intentionally disables
  metering or chooses unlimited fuel. The library metering API bounds only its
  documented checkpointed interpreter paths and precharges native bulk work by
  its extent before it begins. It is not a memory ceiling, does not impose a
  wall-clock deadline, does not interrupt a blocked host call, and does not
  interrupt an already started native bulk operation. It is not yet exposed by
  the CLI.

A report that turns one of these into a host-memory or sandbox-escape primitive
is in scope and worth sending privately.
