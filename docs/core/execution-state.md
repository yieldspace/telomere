# Execution state placement

Status: decision record for
[#213](https://github.com/yieldspace/telomere/issues/213), landed with the
classification already in effect. This document is the contract for where
per-execution state lives, so later features stop choosing individually.

## Decision

1. **Per-execution state lives in `ExecuteContext`.** Any state that is
   read or written during one execution — above all anything read per
   dispatch — is a field of `ExecuteContext` (`crates/telomere/src/common.rs`).
   The context pointer is already threaded to every handler and live in a
   register on the hot path, so a field check is a single load and compare
   that preserves the tail-call dispatch form.
2. **Thread-locals are for boundaries the context does not cross.** A
   thread-local is permitted only for state that must be reachable from a
   place `ExecuteContext` never reaches — host-function re-entry, where
   host code runs without a context, and thread-exit lifecycle hooks.
   A thread-local must never be read per dispatch.
3. **Process-global configuration is not per-execution state.**
   Process-wide configuration and process-wide counters live in statics,
   initialised once. Moving them into `ExecuteContext` would change their
   meaning, so it is a behaviour change, not a placement refactor.
4. **Parse/decode recursion state travels as a parameter.** Recursion
   depth during parsing and decoding is a property of one recursive
   descent, carried explicitly through its call chain and bounded by
   named constants. It is not execution state and never exists while the
   dispatch loop runs. The limits themselves are owned by
   [parser-limits.md](parser-limits.md).
5. **Bounds over counters where a counter can reset.** When a mechanism
   limits a resource the execution does not fully own (the host stack),
   carry a watermark established from the environment — not a depth
   counter that silently resets when the execution is re-dispatched or
   re-entered while the resource stays spent.

## Why

The failure mode this rule prevents is measured, not hypothetical. While
the JIT bridge stop target lived in the `JIT_INTERPRETER_STOP_AT`
thread-local, `call_code` read a thread-local on every dispatch, and
merely compiling the `jit` feature slowed the interpreter by up to 2x
even when the JIT never ran (#190): on Darwin the read is a
`__tls_get_addr` thunk call per dispatch. Moving the target into
`ExecuteContext::jit_stop_pc` restored parity with the no-feature build
(Darwin loop-linear median 961 ms → 560 ms against a 620 ms no-feature
median; recorded with PR #290). The replacement check also had to be a
single equality — a two-condition form made LLVM materialise the boolean
through a `setcc`/`and` chain and measured slower than the TLS read it
replaced — which is the #178 lesson in miniature: on this dispatch loop,
the only per-dispatch addition that reliably survives codegen is a field
load and compare against the already-live context pointer.

## Classification of the five mechanisms

| Mechanism | At filing (#213) | Now | Class | Verdict |
| --- | --- | --- | --- | --- |
| JIT bridge stop signal | `JIT_INTERPRETER_STOP_AT` thread-local | `ExecuteContext::jit_stop_pc` (#190) | per-execution, read per dispatch | **Moved** to the context. Mechanism and owner named below. |
| JIT entry depth | `JIT_ENTRY_DEPTH` thread-local | abolished (#191): recursion is bounded by the value-stack reservation, and `ExecuteContext::jit_native_stack_floor` bounds native entry against the host stack | per-execution | **Moved / abolished.** No counter anywhere: the floor is established lazily at the first native entry from the current thread's OS stack bounds plus a reserve, per rule 5 — a context depth counter would reset across scheduler re-dispatch and host re-entry while the host stack stays spent. |
| Parser/decoder recursion depth (#166) | explicit `depth: u32` parameter | unchanged | parse-time recursion state | **Kept** as a parameter, per rule 4. Bounds: `MAX_CONTROL_NESTING_DEPTH` = 512 (core parser), `MAX_COMPONENT_NESTING_DEPTH` = 100 and `MAX_VALUE_NESTING` = 256 (component decoder). A thread-local or global would be actively wrong here: parses run concurrently on many threads and component decoding re-enters itself. |
| Execution fuel (#127) | `ExecuteContext` | `ExecuteContext::{budget, reserved, budget_epoch}`, spent per dispatch and refilled from the store's metering ledger | per-execution, read per dispatch | **Kept.** This is the model the rule generalises from. |
| Diagnostics dispatch budget | `vm-diagnostics` feature, env-var configured | unchanged: a process-global `OnceLock` budget (`TELOMERE_VM_INSTR_BUDGET`, `TELOMERE_VM_INSTR_LOG_EVERY`) with an `AtomicU64` remaining count | process-global diagnostics | **Kept** as a process global, per rule 3. It budgets the *process* for a debugging session, across all executions and threads; per-execution placement would silently change that meaning. The whole mechanism is compiled out unless the `vm-diagnostics` feature is enabled, so its per-dispatch cost in shipped builds is zero. |

The two further proposals named in the issue have also landed, both in
conformance with the rule:

- **Frame-entry stack reservation** (#186): the validated maximum operand
  depth travels in the `CallRecipe` and is reserved on the value stack at
  frame entry — per-execution state reached through the context's stack,
  checked once per frame instead of per push.
- **Frame-cached memory `(base, len)`** (#187):
  `ExecuteContext::{default_local_memory_base, default_local_memory_len}`,
  refreshed at every frame transition alongside
  `default_local_memory_ptr`, so scalar handlers validate an access with
  one compare instead of dereferencing the `Memory` struct per access.

## Remaining thread-locals (audit)

Four `thread_local!` sites remain in the workspace's runtime crates.
None is read on the interpreter dispatch path in a default build:

| Site | What it holds | Why it is permitted |
| --- | --- | --- |
| `common/store.rs`: `ACTIVE_STORE_RUNTIME`, `ACTIVE_STORE_REENTRY` | Which stores are active / re-entered on this thread | Rule 2: host functions re-enter the runtime without an `ExecuteContext`; the registry must be discoverable from that boundary. Not read per dispatch. |
| `runtime/vm.rs`: `DISPATCH_PROFILE_SESSION` (plus `#[cfg(test)]` companions) | `vm-profile` session state | Feature-gated profiling, not a shipped build; session state, not execution state. |
| `runtime/jit/profile.rs`: `REPORT_GUARD` | A guard whose drop reports JIT profile data at thread exit | Rule 2: a thread-exit lifecycle hook; the context does not exist there. |
| `common/memory.rs`: `TEST_MEMORY_MAPPING_FAILURE` | Injected mapping failure | `#[cfg(test)]` only. |

## JIT bridge stop signal: mechanism and owner

- **Mechanism:** `ExecuteContext::jit_stop_pc`. The innermost active JIT
  continuation bridge records the pc at which the interpreter must return
  control; the per-dispatch check (`should_stop_interpreter_at`, called
  from the dispatch loop in `runtime/vm.rs`) is the single equality
  `ctx.jit_stop_pc == pc` -- a null target can never equal a real pc, so
  no separate active check is needed on the hot path -- and return
  handlers consult `interpreter_stop_active` before resuming the loop.
- **Owner:** the JIT continuation bridge in `runtime/jit.rs`. The field
  is written only through `JitInterpreterStopGuard`, which saves the
  previous value on entry and restores it on drop, so nested bridges keep
  their nesting semantics and every access shares the guard's borrow
  provenance. Changes to the stop protocol are changes to that guard's
  discipline; no other code may write the field.

## Enforcement

The rule's failure mode is incremental drift — one new thread-local at a
time — so it carries a mechanical guard in the style of the other
`tools/check-*.py` gates: `tools/check-execution-state.py` runs in CI and
fails if a new `thread_local!` appears in the dispatch-path files
(`crates/telomere/src/common.rs`, `crates/telomere/src/runtime/vm.rs`,
`crates/telomere/src/runtime/jit.rs`) beyond the recorded allowlist (the
`vm-profile` session in `vm.rs`). The rule is also stated in the doc
comment on `ExecuteContext` itself, at the point where a new field — or
a decision to put state somewhere else — would be made.

## Measurements

Per-dispatch cost was measured before and after this change with the
performance gate's methodology (#212): the L2 workloads `loop-linear`
and `repeat-fib32` through `telomere-cli`, whole-process wall clock,
medians of five samples after one warm-up, on fleur-du-lapin
(x86_64 Linux), for the gate's three cells. This change set contains no
dispatch-path code change (a doc comment, this document, and the CI
guard), so the expectation is identical ratios within noise, and the
gate's own checker is the arbiter:

Runs: before = `3922f93` (origin/main), after = this change, both
measured back to back on the same host (fleur-du-lapin, Xeon Gold 6140,
load average 13.9 before / 2.1 after -- the medians below are stable
across that load difference, and the gate's within-run ratios are the
quantities that carry the verdict). Absolute times exceed the gate's
advisory ceilings on this host, as they do for every local run; absolute
ceilings are not gated, per the methodology.

| Workload | Cell | Before median | After median | Delta |
| --- | --- | ---: | ---: | ---: |
| loop-linear | default-interp | 6,733 ms | 6,668 ms | -0.97% |
| loop-linear | jit-interp | 8,740 ms | 8,510 ms | -2.64% |
| loop-linear | jit-jit | 6,517 ms | 6,406 ms | -1.69% |
| repeat-fib32 | default-interp | 9,173 ms | 9,291 ms | +1.29% |
| repeat-fib32 | jit-interp | 9,514 ms | 9,392 ms | -1.28% |
| repeat-fib32 | jit-jit | 14,782 ms | 14,668 ms | -0.78% |

Within-run ratios (the gated quantities) are unchanged to within noise:
loop-linear jit-interp/default 1.298 -> 1.276, jit-jit/default
0.968 -> 0.961; repeat-fib32 jit-interp/default 1.037 -> 1.011,
jit-jit/default 1.612 -> 1.579. `tools/check-performance.py` reports
"all gated ratios are within their ceilings" for the after record,
exactly as for the before record. This is the expected result: the
classification above required no placement change, so the decision costs
no per-dispatch time -- the cost it removes is the *future* one, by
making the next thread-local a guarded, reviewed act instead of a
silent 2x.

## Out of scope

- Making the diagnostics dispatch budget per-execution. That changes
  what the budget means (rule 3) and is a behaviour change; if it is
  ever wanted it needs its own issue.
- Moving the `vm-profile` session or the store re-entry registries.
  Both are classified above as correctly placed; nothing in the
  measurements argues otherwise.
- Any change to metering, depth limits, or the bridge protocol. This
  decision records where state lives; it does not alter what the state
  does.
