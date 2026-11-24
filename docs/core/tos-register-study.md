# TOS-in-register dispatch study (#189): measured prototype, NO-GO for migration

This document is the landing record for #189, a design study asking whether
threading the top-of-stack (TOS) value through the dispatch chain in a
register — wasm3-style — instead of round-tripping every value through the
byte stack is worth migrating the `Op` handler signature
(`unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>`,
common.rs:974) to a three-argument form.

**Decision: NO-GO.** Do not migrate the dispatch signature. The prototype
below shows the mechanism works and is fast on *unfused* produce–consume
code (1.85–1.95× on a handler-only model), but the interpreter that exists
today is not that model: superinstruction fusion already captures a larger
share of the same traffic (2.5× on the same loop shape, measured), the
remaining per-dispatch costs are ones TOS cannot remove, and the migration
surface is the largest in the interpreter (376 handlers, 522 dispatch
sites, 509 JIT-decode specs, plus every suspension/resume entry point).
The null hypothesis in the issue — fusion may already capture most of the
win — is what the data supports.

This record follows the performance-issue landing rule: the candidate was
measured, and the no-go rests on the measured evidence in §§2–4, not on an
expectation that it would not help.

## 0. Contract compliance

- The tail-call threading contract stays. Both the current dispatch and
  the prototype dispatch end in a sibling jump, shown by disassembly in
  §4 — not asserted in prose. Nothing here proposes a switch/computed-goto
  loop, a return-to-loop step, or migration toward the #114 loop fallback.
- Return classification is untouched: the study only ever *adds an
  argument*. `VMResult` and its register-pair static asserts
  (runtime/vm.rs, the #127/#178 bound) are unchanged in shape; the
  prototype's three-argument handlers return the same register-classified
  word as the two-argument ones.
- Cross-check from the issue: the stack-trace track's public-API work is
  designed *without* touching the dispatch ABI and relies on the ABI
  staying put. This study's outcome keeps the ABI stable, so that
  constraint wins by virtue of the no-go; had the answer been go, the
  signature change would have had to be coordinated against it first.
- No change to default builds. There is no cfg: the prototype is a
  standalone file (`tools/tos-register-prototype.rs`, not part of any
  crate target — `tools/` is where this repository keeps standalone
  measurement harnesses, per the root `AGENTS.md`, which prohibits
  executable measurement code under `docs/`) and may be deleted now that
  the decision is recorded.

## 1. Method

Three measurement layers, all on fleur-du-lapin (x86_64, 72 threads) with
the repository-pinned Rust 1.96.0, 2026-11-25, host shared with other
work — so every contrast below is a *paired, same-binary or same-process*
comparison (the L3 local-investigation layer of
[`interpreter-baseline.md`](interpreter-baseline.md); none of these
numbers is a published baseline):

1. **Real interpreter, fused vs unfused.** The existing Criterion benches
   (`fib`, `scalar_local_loop`) in the `measure-switches` build, run with
   the optimizer pipeline on (default) and off
   (`TELOMERE_OPTIMIZER=off`, the materialized unfused fallback). Per the
   baseline methodology, on/off is an upper bound on the *whole pipeline's*
   contribution, not on any single recognizer.
2. **Prototype A/B.** Two miniature direct-threaded interpreters over the
   closed subset the issue specifies (i32 const, local get/set/tee,
   i32 add/sub/mul, comparisons, br/br_if, drop, end), with handler bodies
   mirroring telomere's shapes (byte stack with unaligned LE u32
   load/store, `reduce_top`-style dyadic ops, `#[inline(never)]`
   handlers, dispatch as the last expression):
   - **A** — the current contract: `fn(*const Instr, &mut Ctx) -> u32`,
     every value round-trips through the byte stack.
   - **B** — TOS threaded as a third integer argument (`u64`); the byte
     stack holds only the values below TOS. Whether TOS is live at a
     program point is a static lowering fact, so producers exist in two
     variants (dead-TOS: value simply becomes TOS; live-TOS: spill first)
     — exactly the variant split a real lowering would need.
   - **Shim** — B additionally contains a non-converted, stack-only op.
     The operation sits behind a genuine two-argument boundary: a
     `#[inline(never)]` function taking only (pc, ctx) — no TOS argument —
     called through an opaque fn pointer, so it must load both operands
     from memory and store its result back (§4 verifies this in the
     generated code). Entering it spills TOS to memory and leaving it
     reloads TOS from memory, once per loop iteration in the shim
     workload: the worst-case boundary-crossing shape the issue asks to
     bound. (The first version of this prototype implemented the operation
     inside the three-argument shim handler; LLVM kept the operand and
     result in `%edx` and eliminated the reload, so it measured no
     boundary at all. The shim numbers below are the re-measurement with
     the genuine boundary, and they supersede the first version's.)
   A and B run identical programs and must agree: the check mode verifies
   both against independent expectations, including the same results the
   telomere benches assert (`scalar_local_loop(1024) = 523776`,
   `fib(20) = 6765`). Timing is five paired rounds, A/B order alternated,
   pinned to one core (`taskset -c 3`); medians are reported.
3. **Codegen spot-check.** `objdump` of the prototype handlers (§4).

Reproduce the prototype:

```shell
rustc -O -C codegen-units=1 -o /tmp/tos_proto tools/tos-register-prototype.rs
/tmp/tos_proto check   # correctness, A == B == expected
/tmp/tos_proto bench   # paired A/B timing rounds
```

## 2. Baseline: what the current interpreter actually pays

`scalar_local_loop` (n = 1024; the produce–consume loop shape of §3,
13 guest instructions per iteration), `measure-switches` build:

| Cell | Time per run | Per iteration |
| --- | --- | --- |
| Optimizer ON (fused, today's default) | 11.67 µs | 11.4 ns |
| Optimizer OFF (unfused handlers) | 29.38 µs | 28.7 ns |

The whole optimizer pipeline is worth **2.52×** on this loop. `fib`
(recursive `fib(20)`, instantiation included per iteration by that bench)
moves much less — 1.272 ms on vs 1.453 ms off (1.14×) — because it is
call- and setup-dominated, a cost class TOS does not address either.

Two facts frame everything below:

- The unfused real interpreter costs 28.7 ns/iteration where the
  prototype's unfused A costs 18.2 ns/iteration on the same loop shape.
  The difference is per-dispatch work the prototype does not model and
  TOS does not remove: the `ctx.cont = tail_code` store on every dispatch
  (load-bearing for trap capture, #185 — dropping it measured ~+5%
  CoreMark and was not landable), the metering checkpoint, and the
  `call_code` JIT-stop check.
- Fusion removes the memory round-trips *inside* each fused family. TOS
  can only act on the traffic that remains: at family boundaries and in
  code no family covers.

## 3. Prototype results

Medians of five paired rounds. `scalar_loop` is the §2 loop shape
(n = 20 000 per run); `fib_body` is an iterative fib loop body
(n = 2 000 000 iterations per run); `*_shim` crosses the shim once per
iteration.

| Workload | A (stack) | B (TOS register) | B speedup |
| --- | --- | --- | --- |
| scalar_loop | 364.0 µs (18.2 ns/iter) | 196.4 µs (9.8 ns/iter) | **1.85×** |
| scalar_loop_shim | 359.8 µs | 266.1 µs | **1.35×** |
| fib_body | 49.98 ms | 25.59 ms | **1.95×** |

Reading:

- On unfused produce–consume code the mechanism is real and large:
  roughly half the handler-only cost is the memory round-trip.
- The shim downside is **material, not negligible** — this corrects the
  study's first measurement, whose in-handler shim never crossed a real
  boundary (see §1). With the stack-only operation behind a genuine
  two-argument boundary, crossing it every iteration costs B ~36%
  (266.1 vs 196.4 µs median, +3.5 ns/iter): the spill store, the call
  into stack-only code (which loads both operands from memory and
  stores its result back), the reload load, and the loss of the pure
  tail-jump form for that one dispatch. Even so, B does not fall off a
  cliff on mixed code: on the shim workload itself B is still **1.35×**
  faster than A, because A's stack round-trips remain in every other
  handler. Boundary cost is therefore a real tax on any incremental
  migration — mixed code pays it at every crossing — but it is not by
  itself the reason for the no-go; the no-go rests on the fusion
  comparison below and in §2, which does not involve the shim at all.
- But B's absolute number cannot be credited to the real interpreter.
  Crediting B's full prototype delta (8.4 ns/iter) against the real
  *unfused* cost gives ≈20.3 ns/iter — still **1.8× slower than today's
  fused default (11.4 ns/iter)**. Against the fused default itself the
  delta is not additive at all: fused handlers have already deleted most
  of the loads/stores B exists to delete.

## 4. Codegen evidence (x86_64 SysV, rustc 1.96.0, -O)

The three-argument signature keeps the tail-call form, and TOS stays in
its argument register (`%rdx`, the third integer argument) across the
dispatch:

```asm
<b_add>:                                   ; fn(pc, ctx, tos: u64)
    mov    0x8(%rsi),%rax                  ; stack base
    mov    0x18(%rsi),%rcx                 ; stack top
    lea    -0x4(%rcx),%r8
    mov    %r8,0x18(%rsi)                  ; top -= 4 (pop lhs)
    mov    0x10(%rdi),%r8                  ; next handler
    add    -0x4(%rax,%rcx,1),%edx          ; lhs + TOS, in the TOS register
    add    $0x10,%rdi                      ; next pc
    jmp    *%r8                            ; tail jump — no call, no frame
```

One memory load, no store, result carried in `%edx`; `b_end` takes the
result straight from `%edx`. Contrast `a_add` under the current contract:
load rhs, then `add %edx,-0x8(...)` — a read-modify-write *store* back to
the stack — plus the top update. The shim's spill is a single
`mov %edx,(%rax,%rcx,1)`.

The genuine stack-only boundary (the §3 shim, re-measured) disassembles
as a real spill / call / reload sequence — contrast the first version,
where `b_shim_opaque` computed `imul $0x9e3779b1,%edx,%edx` directly on
the TOS register and no reload load existed at all:

```asm
<b_shim_opaque>:                            ; fn(pc, ctx, tos: u64)
    mov    %edx,(%rax,%rcx,1)               ; spill: TOS store to the byte stack
    call   *(%rsp)                          ; opaque fn-ptr call across the
                                            ; two-argument boundary
    mov    -0x4(%rax,%rcx,1),%edx           ; reload: TOS load from memory

<stack_only_opaque>:                        ; fn(pc, ctx) — no TOS argument
    imul   $0x9e3779b1,-0x4(%rax,%rcx,1),%edi  ; rhs loaded from memory
    xor    %edi,-0x8(%rax,%rcx,1)           ; lhs from memory, result stored
                                            ; back to memory; %edx unused
```

The stack-only side never touches `%edx`: both operands come from
memory and the result returns to memory, and the converted side can
only recover it with the reload load after the call. (The shim's own
outgoing dispatch is a call-and-return rather than a pure tail jump —
it must regain control after the boundary call to perform the reload —
and that cost is included in the §3 shim numbers.)

So the ABI fear in the issue does not materialize on this target: extra
register *arguments* do not disturb the return classification, and the
dispatch remains a tail jump. (Verified here for x86_64 SysV only; an
aarch64 AAPCS check would be part of any future revival, as would the
dispatch-contract CI gate the issue depends on.)

## 5. Migration cost (counted on this tree)

- **376** `op_*` handler functions (`runtime/vm/*.rs`, incl. 59
  superinstruction handlers in `superinstructions.rs` and 140 in
  `numeric.rs`), every one changing signature.
- **522** `call_next`/`call_code` dispatch sites that would thread the
  extra argument, plus the two entry points that call a handler directly
  and would have to manufacture a dead TOS: the scheduler resume
  (`runtime/scheduler.rs:639`) and the JIT runtime stub
  (`runtime/jit/stubs.rs:708`).
- **509** `OP_SPECS` entries in the JIT baseline decoder
  (`runtime/jit/backend/ops.rs`), whose fn-pointer identity contract
  (#240: `#[inline(never)]`, identity barriers, ICF hazards) must be
  re-verified for every handler under the new signature.
- **Handler-count growth, not just edits.** The prototype needed separate
  live-TOS and dead-TOS producer variants (spill vs no-spill). A real
  lowering either doubles the producer handlers or pays a dynamic
  live check per dispatch — against the handler budget governed by
  [`optimizer-family-budgets.md`](optimizer-family-budgets.md).
- **Suspension audit.** Every point that can suspend, trap, or hand
  control to the host must spill TOS before any consumer reads the stack:
  `ctx.cont` has 49 write/read sites, the scheduler reconstructs resume
  state from the stack (`StablePc`, trap capture), and the host return
  paths read results out of stack memory
  (`Stack::function_return_in_place`, common/stack.rs:1246; callers in
  host.rs:298,330). None of this is impossible — a spill itself is a
  single store, though §3's genuine-boundary measurement shows that
  actually crossing into stack-only code and back is not free (~36% on
  a per-iteration crossing) — but each site is a correctness cliff if
  missed, and the failure mode is a silently wrong value, not a trap.
- The #114 loop-dispatch fallback mirrors the dispatch contract, so it
  either migrates too (double maintenance of the contract) or becomes a
  second, divergent contract.

## 6. Recommendation and revisit conditions

**NO-GO for the signature migration.** The expected win on the code the
interpreter actually runs — fused families plus boundary traffic, on top
of per-dispatch costs TOS cannot touch — is a fraction of the
prototype's unfused 1.85–1.95×, and §2's arithmetic bounds it below what
fusion already delivers on the same shapes, at a small multiple of the
effort. The migration is the largest structural change available short of
a new tier; the measured return does not clear that bar.

Revisit if any of these becomes true, with fresh measurement:

1. A workload class that matters is shown, by dispatch profiling, to be
   dominated by *unfused* produce–consume chains that no family covers
   and that resist fusion for a structural reason (not merely "not yet
   written").
2. The fused/unfused gap on the baseline corpus narrows toward 1×
   (fusion stops being the cheaper source of the same win).
3. A TOS threading *inside* one fused family — no signature change, TOS
   living only across a family's internal steps — measures a win; that
   is a local change this study does not foreclose.

The alternative the issue rejected for the study — full load-time
slot/register lowering (WAMR-style) — remains a different execution
model and would be a separate proposal, as the issue states.
