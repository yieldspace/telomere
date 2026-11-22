# Trap-diagnostics conformance fixtures

This corpus pins the trap traces telomere publishes through `TrapInfo`
(`Store::take_last_trap`) so they cannot silently rot on either engine
(#211). Before this corpus existed, nothing in the repository asserted
anything about trap *location*: a regression in the interpreter's capture
path or in a JIT backend's trap attribution would have shipped invisibly.

The runner is `crates/telomere/tests/trap_fixtures.rs`. It executes every
fixture on the interpreter and asserts the published trace matches the
fixture's `.expected` data file exactly. When the `jit` feature is compiled
in, it then executes every fixture again on a JIT-enabled store and asserts
the two engines agree: same trap kind, same truncation flag, same frame
sequence (funcidx, function name, module name, frame kind), and the same pc
on every frame where the JIT claims one.

## Fixture layout

- `<name>.wat` — a small module with a name section that traps in a known
  shape when its `run` export is called. Leading `;;` comments may carry
  runner directives:
  - `fixture-requires: threads` — skip the fixture in builds without the
    `threads` feature (feature-reduced CI jobs only; the corpus runs in
    full wherever threads is compiled in).
  - `fixture-mode: host-reentry` — func 0 is a placeholder the runner links
    to a host function that re-enters the module's `inner` export and
    propagates its trap, producing a host frame mid-chain.
- `<name>.expected` — the expected trace, in the format below.
- `jit-skip.list` — fixtures the JIT cannot compile at all, one per line as
  `<name>: <reason>`. A listed fixture still runs on the interpreter. A
  disagreement between engines is never a reason to list a fixture; it is
  the bug this corpus exists to catch.

## Expected-file format

Line-based; `#` starts a comment. Expectations are data, not code, so the
trap-message work in #131 can extend the same files later.

```
kind: memory index out of range     # TrapKind::as_str() label
truncated: false
total-frames: 1
frame 0: func=0 name=run module=oob_load kind=wasm pc=0
```

- `frame <depth>:` lines describe retained frames innermost-first. Every
  line carries `func=` (function index or `-`), `name=` and `module=`
  (name-section names or `-`), `kind=` (`wasm`, `host`, `async-host`,
  `unresolved`), and `pc=` (decoded-stream instruction index or `-`).
  Frame 0 is the trapping instruction; older frames carry the pc after
  their call site.
- A truncated trace retains the innermost 48 frames plus the outermost 16.
  The outermost segment is written as `tail <0..15>:` lines instead of
  absolute depths, because the absolute depth of that segment depends on
  how deep the engine's stack goes before it overflows — the interpreter
  walks 3196 frames for these fixtures, the JIT fewer. For the same
  reason, `total-frames:` is asserted exactly for the interpreter but only
  as "genuinely truncated" for the JIT. Frame identities and pcs within
  both segments are asserted exactly for both engines.

## JIT pc rule

Native JIT traps currently report no pc for the trapping frame (#209
tracks trap-site tables that would give them one); caller frames carry
decoded-stream return pcs that must equal the interpreter's. The runner
therefore asserts, for the JIT pass, that every claimed pc equals the
expected pc, and that a trapping wasm frame claims none. When #209 lands,
the corpus expectations gain JIT pcs as a deliberate golden update.

## Coverage

| Fixture | Covers | Trap kind | JIT differential |
| --- | --- | --- | --- |
| `unreachable` | `unreachable` instruction | unreachable | active |
| `oob_load` | out-of-bounds load | memory index out of range | active |
| `oob_store` | out-of-bounds store | memory index out of range | active |
| `div_zero` | `i32.div_s` by zero | invalid operand | active |
| `div_overflow` | `i32.div_s` INT_MIN / -1 | invalid operand | active |
| `rem_zero` | `i32.rem_s` by zero | invalid operand | active |
| `indirect_type_mismatch` | `call_indirect` signature mismatch | call indirect invalid type | active |
| `table_uninitialized` | `call_indirect` through a null entry | table uninitialized | active |
| `unaligned_atomic` | unaligned `i32.atomic.load` on shared memory (threads) | unaligned atomic | active |
| `stack_overflow` | stack overflow via self-recursion | stack overflow | active |
| `deep_chain` | structural: nine-frame call chain | unreachable | active |
| `tail_call_chain` | structural: `return_call` chain — the tail-calling caller is elided from the trace by design (frame reuse), asserted as such | unreachable | active |
| `host_frame_chain` | structural: host frame mid-chain, inner reentrant frame intentionally not stitched in | unreachable | active |
| `recursion_truncated` | structural: truncated trace — innermost 48 + outermost 16 frames retained, middle elided | stack overflow | active |

Division and remainder traps surface as `invalid operand`: telomere's
`TrapKind` has no dedicated divide-by-zero kind, and this corpus pins the
mapping as it stands.

## Adding or updating a fixture

1. Add the `.wat` (and keep the module name equal to the fixture name).
2. Generate the expectation from the interpreter and **review it against
   the intended trap shape before committing** — bless output is a
   recording, not an oracle:
   `TELOMERE_TRAP_FIXTURES_BLESS=1 cargo test -p telomere --release --test trap_fixtures corpus_matches`
3. Add a row to the coverage table above (the runner fails if a fixture
   is missing from it).
4. If a fixture cannot pass because the runtime is wrong, file the runtime
   issue against the owning track issue instead of bending the fixture
   (#211 is test-only by design).
