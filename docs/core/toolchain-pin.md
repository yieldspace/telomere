# Toolchain pin: the tail-call threading contract

Status: decision record for
[#114](https://github.com/yieldspace/telomere/issues/114). This document is
the contract for why the workspace toolchain is pinned, what would have to
change for the pin to come off, and what already exists so that correctness
does not depend on the pin.

## Decision

**The Rust 1.96.0 pin stays. Unpinning is not possible on current
evidence.**

- Every measured release after the pin — 1.97.0, 1.98.0, 1.99.1, and
  1.100.0 (the current stable at decision time) — breaks the tail-call
  threading contract in release builds, at roughly 42–168 bytes of host
  stack per call cycle. Known good remains 1.86.0 (MSRV) through 1.96.0.
- The 2026-08-04 design input recorded in #114 makes unpinning depend on
  two things together: the loop-dispatch fallback existing, *and* the CI
  gate being green. The fallback now exists (PR #387). The gate side
  cannot be green while every current toolchain fails the sensitivity
  tripwire (below), so the second half is unmet and the pin stays.
- Correctness on a post-pin toolchain is available today, but only
  through the fallback build mode. The pin protects the default
  (direct-threaded) build, which is the build embedders get.

## The contract

The interpreter's dispatch is direct-threaded and chained: every handler
ends in a tail call through `Instr.op` (`call_next` / `call_code`) into the
next handler, rather than returning to a dispatch loop. Under the
contract, executing a guest call loop consumes constant host stack,
independent of the number of guest calls.

Stable Rust does not guarantee tail calls (`become` / RFC 3407 is still
unstable), so the contract currently holds only where LLVM applies
sibling-call optimisation to the dispatch. That dependence is the
fragility #114 is about.

The maintainer constraint recorded in #114 (2026-08-04) is part of this
decision: **the tail-call threading contract stays**. The chained dispatch
shape is the design; the loop-dispatch fallback is a safety net for the
pinned-toolchain problem, not a migration target, and no change in this
area may trade the tail-call form away on the main path.

## Measured evidence

All measurements are from 2026-11-27 on x86_64 Linux (the issue's original
reproduction was aarch64 macOS plus x86_64 CI, so this is not a
platform-specific effect). The test body throughout is the
`call_threading` call loop: 200,000 Wasm-level calls.

### The shipped tripwire's blind spot

The issue-era test ran on a default-sized thread and caught the 1.97.0
break. PR #250 (#154) later moved the test onto a 64 MiB thread so it
could also execute in dev builds. That move silently desensitised the
release side: the leak totals only 8.4–33.6 MiB over the test's 200,000
cycles, which a 64 MiB thread absorbs. Measured on current main, the
shipped (64 MiB) shape **passes on 1.97.0, 1.98.0, 1.99.1, and 1.100.0** —
the apparent recovery on 1.99+ was this artifact, not a codegen recovery.

### Small-stack re-measurement

The same body on explicit small stacks separates the toolchains cleanly:

| toolchain | 2 MiB | 8 MiB | 32 MiB |
| --- | --- | --- | --- |
| 1.96.0 (pin) | pass | pass | — |
| 1.97.0 | stack overflow | stack overflow | pass |
| 1.98.0 | stack overflow | stack overflow | pass |
| 1.99.1 | stack overflow | stack overflow | pass |
| 1.100.0 | stack overflow | stack overflow | pass |

1.96.0 completes in 2 MiB, i.e. flat (under ~10 B/cycle). The boundary is
the pin itself: known good ends at 1.96.0, and every later release
measured so far is broken.

### The broken link

Disassembly of the release test binary localises the leak to one link:
`op_call`'s dispatch into the callee (the `call_next` after
`internal_op_call` returns `CallOutcome::Immediate`).

- 1.96.0: the frame is torn down and the callee is entered with
  `jmp *%rax` — a sibling call.
- 1.97.0: the same source compiles to `call *(%rax)`, leaving `op_call`'s
  88-byte frame live until the segment unwinds — ~88 B per guest call,
  inside the 42–168 B/cycle bracket above, and consistent with the
  2 MiB / 8 MiB overflows and the 32 MiB pass.

The return side (`special_function_return`) and the leaf handlers keep
their tail form on 1.97.0; the call-side dispatch is the only broken link
in the cycle.

## What landed

### Sensitivity tripwire (PR #386)

`call_loop_keeps_direct_threading_on_a_small_stack` in
`crates/telomere/tests/call_threading.rs` runs the same 200,000-cycle body
on a 4 MiB stack, sized with the discipline of the #277/#282 sibling
tests: flat usage on the pin completes in 2 MiB (≥2× headroom), and the
smallest measured leak total is 8.4 MiB (>2× margin on the leak side).
The 64 MiB test remains as the both-profiles execution tripwire; its
comment records that it is not the sensitive one.

The sensitive test is ignored by default and run explicitly, for a
measured reason: in dev builds the same body needs 32 MiB even on the pin
(the dev codegen configuration leaves a per-cycle host cost), and
`cfg!(debug_assertions)` cannot separate the profiles here — the
workspace dev-profile contract compiles the telomere crate, integration
tests included, with debug-assertions off. No single stack size is both
dev-passing and release-sensitive at 200,000 iterations. CI therefore
runs the test with `-- --ignored` in the release and release-size jobs,
and the toolchain watch runs the file with `--include-ignored`.

Measured: the small-stack tripwire passes on 1.96.0 (release and
release-size) and aborts with a stack overflow on 1.97.0, 1.98.0, 1.99.1,
and 1.100.0 (release; 1.97.0 also in release-size).

### Loop-dispatch fallback (PR #387)

The cargo feature `loop-dispatch-fallback` on the `telomere` crate
(default off) is the design input's fallback mode: in fallback builds,
`call_code` records the next instruction in `ctx.cont` and returns to the
trampoline driver `vm::drive_loop_dispatch`, which dispatches one handler
per iteration, so host-stack depth is independent of both instruction
count and guest call depth. All interpreter segment entries route through
the driver; the lazy-call continuation entry is the one deliberate
exception — it is already dispatched inside a driver, so it returns the
bounce to that driver instead of starting a nested one (the initial
version nested a driver per call cycle there: measured nesting depth
20,000 over 20,000 cycles before the fix, depth 1 and zero stack descent
after). Design detail and the entry inventory live in the fallback
section of [optimizer.md](optimizer.md).

Measured cost (criterion, pin 1.96.0, medians; threaded = 1.00):
fib ≈ 1.19, return_call_chain ≈ 1.10, tail_recursive_accumulate ≈ 1.15,
scalar_local_loop ≈ 1.59–1.73, memory_load_store_loop ≈ 1.78–1.98. The
denser the dispatch, the more the trampoline costs — the price of the
safety net, tracked separately from main-path performance goals.

Verification: on 1.97.0 with the fallback enabled, the call loop
completes at 2, 4, 8, and 32 MiB stacks (the threaded build overflows at
2 and 8 MiB and aborts the 4 MiB sensitivity shape). On the pin, the full
workspace suite passes in both modes and the vendored component suite
numbers are unchanged in both modes. With the feature off, the main
path's code generation is unchanged (`op_call` keeps the teardown +
`jmp *%rax` form), and a feature-off A/B against unmodified main showed
no systematic delta outside the ±5% noise band.

## The unpin condition, read against the design input

The design input's second half says "the CI gate being green". What #148
actually delivered (PR #379) is the toolchain watch: a non-gating,
weekly-plus-dispatch workflow that builds with latest stable and runs the
tripwire — a reporting signal, not the disassembly-asserting required
check the design input originally routed to #148.

**Decision: the behavioural form of the gate is accepted.** With the
sensitivity tripwire restored, the watch measures the contract's actual
consequence — bounded host stack over 200,000 guest calls — directly,
instead of asserting a codegen shape that would have to be maintained per
target and per codegen-unit layout. The gate, for unpin purposes, is:
*the sensitivity tripwire green on the candidate toolchain* — in CI on
the pin, and in the watch on latest stable.

This reading does not unpin anything today: the tripwire fails on the
current stable (measured above), so the watch reports red on current
stable — by design, that red is the standing signal that the pin still
matters. Both halves of the design input's condition must hold at once,
and the green half does not.

## Re-evaluation triggers

Revisit this decision when any of the following happens:

1. **The toolchain watch goes green on a new stable** (the
   `--include-ignored` tripwire passes end to end). Then verify locally
   before unpinning: the small-stack measurement table above for the
   candidate release, plus an `op_call` disassembly spot check showing
   the tail form. Unpin to that release, not to a floating `stable`.
2. **Guaranteed tail calls stabilise** (`become`, RFC 3407). Then the
   contract can be made unconditional in the source; revisit both the
   pin and the fallback's role at that point.
3. **A maintainer decision to change the default build** to the fallback.
   That is a design change against the 2026-08-04 constraint, not a
   consequence of this record; it would need its own decision and its
   own measurement of the cost table above.

## Out of scope

- **Making `loop-dispatch-fallback` the default.** Rejected for now: the
  maintainer constraint keeps chained dispatch as the design, and the
  measured 1.10–1.98× cost on dispatch-dense shapes is a safety-net
  price, not a main-path cost to adopt while the pin holds.
- **A disassembly-based CI gate.** Not built; the behavioural gate above
  is accepted in its place, for the reasons stated there.
- **Reporting the codegen change upstream.** Left open from the issue's
  follow-up list. Whether the 1.97.0 change is deliberate or an
  unintended regression remains unverified — it has persisted across
  four releases (1.97.0–1.100.0), which does not fit a quickly reverted
  regression, but no upstream changelog or issue was audited for this
  decision. Reporting is not a condition of the pin decision either way.
- **Removing the fallback if a future toolchain restores the tail
  form.** The fallback stays as the standing safety net regardless; its
  existence is one half of the unpin condition, and the next codegen
  change is not predictable from the last one.

## References

- `rust-toolchain.toml` — the pin and its measured known-good /
  known-broken ranges.
- `crates/telomere/tests/call_threading.rs` — the execution tripwire
  (64 MiB), the sensitivity tripwire (4 MiB), and the fallback completion
  tests, with the sizing history in their comments.
- `.github/workflows/toolchain-watch.yaml` — the non-gating watch whose
  red/green on latest stable is the re-evaluation signal.
- [optimizer.md](optimizer.md), "Loop-dispatch fallback (#114)" — the
  fallback's design, entry inventory, and cost record.
- [parser-limits.md](parser-limits.md) — the debug-profile side of the
  same mechanism family (#154).
