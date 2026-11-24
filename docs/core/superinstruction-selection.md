# Superinstruction selection: quarantine, frequency evidence, and decode-vs-split (#188)

This document is the landing record for #188. It covers, in order:

1. the quarantine of the benchmark-shaped instantiate-time recognizers
   behind `RuntimeConfig::recognizers`, with the CoreMark ON/OFF record;
2. the vm-profile pair/triple frequency reports over a six-workload corpus
   and the promotion shortlist with budget accounting;
3. the coverage finding that settles the "new general family" question for
   this round;
4. the decode-vs-split decision for the descriptor-decode local families,
   per hot kind, with the measured bound on what a split could save;
5. the resulting handler-count and `.text` deltas against
   `optimizer-family-budgets.md` and the #164 embedder-size gate.

All measurements were taken on fleur-du-lapin (x86_64, 72 threads) with the
repository-pinned Rust 1.96.0, on 2026-11-24. Timing runs were pinned to one
core (`taskset -c 3`) and alternated; the host is shared, so contrasts are
reported with their spread and read against the A/A noise pair, following
the #212 within-run methodology. CoreMark runs through `telomere-cli`
require a raised stack limit (`ulimit -s 524288`, the same 512 MiB the wast
runner uses); on the default 8 MiB main-thread stack the CLI aborts with a
stack overflow on this module, on unmodified main as well.

## 1. Quarantine of the benchmark-shaped recognizers

### 1.1 What is actually live today (measured, not assumed)

The issue text describes the instantiate-time rewrites and the specialized
call handlers as live behaviour to be preserved behind a default-ON flag.
Reading and tracing the current tree shows that premise is out of date:

- In `runtime/instantiate.rs`, the five whole-function rewrites
  (`rewrite_crc16_update_masked_wrapper`, `rewrite_list_crc_summary_function`,
  `rewrite_matrix_i16_crc_summary_function`,
  `rewrite_core_state_benchmark_function`, `rewrite_list_crc_pair_loops`)
  and the call-quickening helper `rewrite_direct_calls_for_slots` were
  `#[cfg(test)]`-only or `#[allow(dead_code)]` with no production caller.
- The kernel ops they produce (`op_i32_crc16_update16[_masked]`,
  `op_i32_list_crc_summary`, `op_i32_core_state_benchmark`,
  `op_i32_numeric_token_state_transition`, `op_i32_matrix_i16_crc_summary`,
  `op_i32_list_crc_pair_loop`) have no producer in the current optimizer:
  the parser-side emission and the instantiate wiring were both removed by
  the baseline-JIT commit `f56abfc` (PR #110). The specialized call handlers
  in `runtime/vm/call.rs` survive, but they self-guard on the target's first
  op and can only be installed by the dead quickening.
- `docs/core/coremark-benchmark.md` already records the reachability
  correction: the historical CoreMark record (2026-05-19) postdates
  `f56abfc`, so it excludes all five rewrites. No current CoreMark result
  included them either.

So the recognizers have been disconnected since PR #110, and "current
behaviour" — the thing a default must preserve — is recognizers-off.

### 1.2 The flag

`RuntimeConfig` gains a `recognizers: RecognizerConfig` field
(`crates/telomere/src/common/store.rs`), re-exported as
`telomere::RecognizerConfig`:

- `benchmark_shaped: bool`, **default `false`** — deliberately not the
  default-ON the issue sketched, because ON would *change* current
  behaviour (see 1.1) by reconnecting a pass that has not run in production
  since PR #110, against a JIT and call layer that have never coexisted
  with it. The default preserves the behaviour every current build has.
- When `true`, `instantiate` runs `apply_benchmark_shaped_recognizers`
  after materialization: functions whose lowered stream starts with the
  numeric-token or CRC16 kernel op, functions matching the
  masked-CRC-wrapper shape, and functions matching the cached-u16 low-7
  guard shape are classified by recipe slot; direct calls to them are
  quickened to the specialized call handlers, and wrapper bodies are
  rewritten onto the masked kernel. Every specialized handler
  re-validates its target shape at call time and falls back to the
  generic call path, so a stale classification costs the quickening,
  never correctness. The list-CRC quickening is the one deliberate
  omission: its kernel and call handlers are currently
  dead-code-eliminated, and merely referencing them resurrects about
  60 KB of `.text` into embedder builds for a quickening that can never
  fire while the kernel has no producer (measured; see section 5).
- The four whole-function rewrites that never had a production call site
  (list/matrix CRC summaries, core-state, list pair loops) are **not**
  reconnected: with no producer for their kernel ops, reconnecting them
  would be decoration. They remain in the tree, documented as dead, for
  the separate removal decision the issue reserves.
- `telomere-cli` exposes the flag as `--benchmark-recognizers` (core and
  preview1 paths). The wast harness honours
  `TELOMERE_WAST_BENCHMARK_RECOGNIZERS=1` (see 1.4).

Selection stays at load time; no runtime matcher was added, call ops keep
their tail-call dispatch identity, and nothing was added to
`pipeline/select.rs` — no new benchmark-named recognizer exists anywhere
in this change.

### 1.3 CoreMark ON/OFF record

One release binary (this change, default features), flag selected per run,
six alternating pairs with reversal, plus an adjacent A/A pair for the
noise floor. Artifact: `coremark.wasm`, sha256 `5acb1c0e…` (the file
served from wasm3.github.io; the `tools/baseline/artifacts.json` pin
`e71e3582…` does not match it — that mismatch predates this change).
Scores are CoreMark's self-reported Iterations/Sec; every run printed
"Correct operation validated".

| Arm | Scores | Median |
| --- | --- | --- |
| A/A (OFF, OFF, adjacent) | 1559, 1559 | — |
| OFF (recognizers off) | 1594, 1582, 1581, 1527, 1573, 1595 | 1581.5 |
| ON (recognizers on) | 1590, 1591, 1548, 1590, 1577, 1614 | 1590 |

Paired contrasts (ON−OFF)/OFF: −0.25%, +0.57%, −2.09%, +4.13%, +0.25%,
+1.19% (median +0.41%). The two low outliers (1527 OFF, 1548 ON) sit in
opposite arms and track host load, not the flag. **Verdict: parity within
noise.**

The parity has a precise cause, verified with `vm-profile` (whose call
handlers count themselves): a full CoreMark run with recognizers ON
executes 3,912,772 `op_call` dispatches and **zero** specialized call
handlers — no `op_call_cached_u16_low7_guard`, no
`op_call_i32_crc16_update16[_masked]`, no
`op_call_i32_numeric_token_state_transition`, no
`op_call_i32_list_crc_summary` (report:
`docs/benchmarks/vm-profile/coremark-recognizers-on.txt`). The kernel-op
quickenings are inert because nothing emits the kernel ops (1.1), and the
two materialized-stream matchers no longer match anything the current
optimizer produces for this module: their shapes were carved against the
pre-#110 lowering, and the current pipeline lowers the same source into
different fusions (for example `op_i32_load16_u_local_base_tee4_i32_eqz_br_if`
chains in the list code). The quarantine therefore preserves, measurably,
a no-op on the benchmark it was shaped for — which is the honest state of
the code, now behind a flag with a measured record instead of an
unexamined assumption.

### 1.4 Wast under both settings

- OFF (default): the standard wast suite result is unchanged by this
  change (the flag defaults off and the pass is unreachable).
- ON: `TELOMERE_WAST_BENCHMARK_RECOGNIZERS=1` runs the same suite with
  the pass reconnected for every wast store.

Both settings pass: the full workspace release test run for this change
(which includes the wast suite at the default OFF setting) is green,
and the dedicated ON run passes all 166 wast cases
(`cargo test -p telomere --release --test wast`, 166 passed, 0 failed).

## 2. Frequency reports and promotion shortlist

Raw reports: `docs/benchmarks/vm-profile/` (with a README covering
provenance, invocations, and the counting scope). Corpus: fib
(`repeat-fib32`), CoreMark, loop-linear, f64-kernel, and two real compiled
Rust modules (a JSON parser/aggregator and a text inverted-index builder,
sources committed beside the reports).

Counting scope matters and is stated in the README: `vm-profile` counts
only handlers that call `dispatch_profile_count` — the fused families, the
call family, and the local-get family. Pair/triple frequencies are over
that counted subsequence. Within that scope, the corpus totals
(322,509,214 counted dispatches) aggregate to:

| Counted family | Count | Reading |
| --- | ---: | --- |
| `op_local_get4` | 60,679,554 | single local get (also inside pairs below) |
| `op_i32_load_const_base` | 52,328,216 | loop-linear's whole-loop kernel, self-paired 50,000,000× |
| `op_call` | 37,669,415 | call boundary |
| `op_local_get4_local_get4` | 30,020,925 | fused get pair |
| `op_local_get8` | 11,791,763 | single 8-byte local get (Rust modules) |
| `op_i32_inc_local_base` | 7,538,640 | fused loop increment (CoreMark) |
| `op_i32_load8_u_local_base_tee4_i32_eqz_br_if` | 7,078,360 | fused loop test (CoreMark) |
| `op_i32_load16_u` / `op_i32_load16_s` | 6,391,264 / 5,709,961 | plain narrow loads (CoreMark matrix/list code) |

Top counted pairs (aggregate): the loop kernel's self-pair (50.0M);
`op_local_get4 ⇒ op_call` (16.2M) and `op_call ⇒ op_local_get4` (14.8M) —
argument setup and return-side traffic across call boundaries;
`op_local_get4 ⇒ op_local_get4_local_get4` (13.5M);
`op_local_get4 ⇒ op_local_get4` (10.2M);
`op_local_get8 ⇒ op_local_get8` (8.5M, concentrated in the two Rust
modules: 3.36M in json-agg, 5.19M in text-index).

### Promotion shortlist (per the budgets process)

Budget frame (`optimizer-family-budgets.md`): top-k 16; packed-stream
growth ≤ +10% / +8 instrs; handler baseline 285, as-built 302 (+17);
family priority local/control > memory > call/select. This change adds
**no handlers and no families**, so the as-built count stays 302 and the
stream-growth budget is untouched. The shortlist below is the evidence
base for the next family round, not a landing:

1. **`op_local_get8` pair fusion (local/control)** — the only repeated,
   truly-adjacent counted pair in the corpus that is not already a
   family: in Rust-compiled code the two gets are the two halves of an
   i64/stack value being materialized (typically feeding an i64 binop or
   a call). Estimated cost: +1 handler (303 as-built). Expected gain is
   one dispatch per pair on ~8.5M counted pairs; the pair's own work is
   two local loads, so the ceiling is a few percent of those sequences'
   cost and well under 1% end-to-end on the two modules. Shortlisted,
   not landed: the win does not clear the bar for spending handler
   budget this round, and the counted-subsequence caveat means true
   adjacency should be confirmed from materialized dumps before
   committing a FamilySpec.
2. **Plain `op_i32_load16_u` / `op_i32_load16_s` in CoreMark (memory)** —
   already surrounded by fused variants (scaled-index, local-base
   chains, dot4 loops); the plain singles are the residual addressing
   shapes the existing families decline. No new family proposed: the
   relower order in the budgets doc already routes what it can, and the
   residue is shape-diverse rather than one hot pattern.
3. **Call-boundary pairs (call/select)** — `get4 ⇒ call`, `call ⇒ get4`,
   `call ⇒ call` dominate fib and rank high everywhere. These are not
   fusable sequences; they are the call layer's dispatch cost, which is
   #191/#193 territory (direct calls, register-resident state), not
   superinstruction selection. Recorded here so the frequency is on the
   record; no action in this issue.

## 3. Coverage finding (criterion: new family or covered)

No new general family is landed by this change, on the following measured
grounds:

- The hottest counted sequences in every corpus workload are already
  fused families: loop-linear executes 100.00% of its counted dispatches
  inside one whole-loop kernel; CoreMark's hot list is fused load/store
  and loop-test families plus local-get fusions; fib's counted stream is
  the call/get boundary traffic of a call-bound workload whose arithmetic
  is already in descriptor families.
- The one uncovered repeated pair (get8⇒get8, shortlist item 1) has an
  expected end-to-end effect under 1% on the only workloads that show
  it, which does not justify a handler-budget spend under the budgets
  process this round.
- The descriptor families' internal kind distribution (section 4) shows
  the remaining arithmetic concentrated in a few const-rhs kinds whose
  evaluation is already a single match arm; section 4 measures the
  decode overhead those kinds pay and finds the split ceiling too small
  to land.

So: **the top general sequences are already covered**; the residual
opportunities are recorded in the shortlist with their budget accounting
instead of being landed on spec.

## 4. Decode-vs-split for the descriptor families

The runtime local fast families decode their kind operand on every
execution (`LocalBinop32Descriptor::decode` in
`runtime/vm/superinstructions.rs`). The alternative is splitting hot
kinds into dedicated handlers chosen at materialize time. Two
measurements settle it.

### 4.1 Kind census (which kinds are actually hot)

A scratch build with per-kind counters in the two descriptor eval paths
(counters not committed) ran the corpus. Encoded kind = op index × 2 +
shape (0 = local rhs, 1 = const rhs). Totals are executions of the
`op_local_binop32*` family (local form) and the `op_i32_const_binop*`
family (stack form), last full dump per workload:

Top kinds per workload (share of that family's executions in the
workload; snapshots are cumulative at the last periodic dump, within
4.2M executions of the final count):

| Workload | Family | Top kinds |
| --- | --- | --- |
| json-agg (1,613,854 / 6,774,754) | local binop32 / stack const-binop | `and/const` 23.1%, `shl/const` 22.7%, `sub/local` 18.2%, `or/const` 18.1%, `shr_u/const` 12.2% / `add/const` 42.3%, `and/const` 28.2%, `sub/const` 9.3%, `or/const` 9.2%, `shr_u/const` 5.9% |
| text-index (3,857,072 / 8,725,840) | local binop32 / stack const-binop | `and/const` 28.1%, `shl/const` 23.6%, `sub/local` 19.8%, `or/const` 17.6%, `shr_u/const` 5.7% / `add/const` 51.1%, `and/const` 18.5%, `mul/const` 10.1%, `shr_u/const` 8.3% |
| fib (25,165,824 / 0) | local binop32 / stack const-binop | `sub/const` 100% / — |
| CoreMark (58,424,699 / 247,759,493) | local binop32 / stack const-binop | `shr_u/const` 62.6%, `and/const` 18.9%, `shl/const` 18.4% / `add/const` 33.3%, `and/const` 32.6%, `shr_u/const` 17.7%, `shl/const` 15.0% |

Findings:

- The classic split candidate, `i32.add` with a local rhs, is
  **essentially absent** (0 executions in fib, json-agg, text-index; the
  optimizer's stack forms and fusions absorb it). Splitting by the
  textbook guess would have optimized a cold kind.
- The hot kinds are const-rhs forms: for the stack form, `add/const`
  is the #1 kind in all three real workloads; for the local form,
  `and/const`, `shl/const`, and `shr_u/const` dominate CoreMark, and
  `sub/const` is fib's only local-binop32 kind.
- `add`/`sub` already have hand-carved fast paths inside
  `LocalBinop32Descriptor::eval`; the hot const kinds (`and`, `shl`,
  `shr_u`) take the generic `eval_local_binop32` match.

### 4.2 The split ceiling, measured

A dedicated handler can save at most: the kind decode
(`decode_local_binop32_kind` + descriptor construction), the fast-path
branch chain, and the generic-eval match — it cannot save the operand
loads, the arithmetic, the sink, or the dispatch itself. To bound that
saving end-to-end, a scratch build (not committed) added exactly those
savings as in-handler fast paths for the hot const kinds
(`and`/`or`/`xor`/`shl`/`shr_s`/`shr_u` const-rhs in the local form;
`add`/`and`/`shl`/`shr_u` in the stack form) and was A/B-measured
against the unmodified build, alternating, five pairs per workload:

Results (wall-clock seconds via the CLI, five alternating pairs with
reversal; CoreMark by self-reported Iterations/Sec, four pairs):

| Workload | Unmodified median | Fast-path median | Paired contrasts (fast vs plain) |
| --- | ---: | ---: | --- |
| json-agg `run 20000` | 5.745 s | 5.664 s | −12.7%, −2.6%, −0.3%, −1.4%, −1.8% (median −1.8%) |
| text-index `run 20000` | 4.826 s | 4.846 s | +1.2%, −0.8%, +5.2%, +5.5%, −3.0% (median +1.2%) |
| fib `run 16` (control) | 6.149 s | 6.380 s | +9.0%, +5.7%, +4.4%, +6.2%, +5.0% (median +5.7%) |
| CoreMark | 1408 /s | 1485 /s | −3.3%, +17.1%, +6.7%, +1.2% (arm spread 13%) |

The fib row is the control that disciplines the whole table: fib's only
descriptor kind is `sub/const`, which already had a fast path, and the
scratch build's added code is unreachable for it — yet the scratch
binary measures +5.7% *slower*. That is a pure binary-layout effect
between two builds of nearly identical source (the layout sensitivity
#191 documented), and it sets the noise band for every other row at
roughly ±5%. json-agg (−1.4% by medians) and text-index (+0.4%) sit
inside the band; CoreMark's +5.4% by medians sits at its edge with a
13% arm spread, so it cannot be credited either. **No fast-path — and
therefore no split — effect is established on any workload.**

A full handler split would save strictly less than this fast-path
variant on top of what it already captures (the remaining difference is
one predictable branch and the descriptor's stack traffic), so the
fast-path contrast is an upper bound on the split's end-to-end value:
at most low single digits on the most favourable workload, and
indistinguishable from zero on the two real Rust modules.

### 4.3 Decision per hot kind

| Kind (family) | Executions (corpus) | Decision |
| --- | ---: | --- |
| `add/const` (stack const-binop) | hottest kind in all 3 real workloads | **Keep decode.** |
| `and/const`, `shl/const`, `shr_u/const` (local binop32) | CoreMark-hot | **Keep decode.** |
| `and/const`, `shl/const`, `shr_u/const` (stack const-binop) | CoreMark-hot | **Keep decode.** |
| `sub/const` (local binop32, fib) | fib's only kind | **Keep decode** (already fast-pathed). |
| all remaining kinds | ≤2% each | **Keep decode.** |

Rationale, uniform across kinds: the measured ceiling (4.2) is inside
noise on every workload, while a split is not free — each dedicated
handler must be registered in the optimizer's op table, the encode and
versioning identity checks, and the JIT backend's `OP_SPECS`, and every
registration is a new fn-pointer-identity surface of exactly the class
that produced #277/#282/#285. Handler budget is likewise unspent: the
count stays at the as-built 302. Decode stays the single mechanism for
the descriptor families; if a future profile shows a kind exceeding
roughly a tenth of a workload's dispatches with a straight-line body,
the split question can be reopened with this census method.

## 5. Size accounting

- Handler count: **+0** (302 as-built, unchanged). No layout-order
  change; no new `FamilySpec`; packed-stream growth 0.
- #164 gate (embedder footprint): this change adds the quarantine pass
  and the config surface to the telomere crate, so gated sizes move.
  Measured locally with `tools/measure-embedder-footprint.py` (both
  profiles) against a same-environment unmodified-main control; the
  control reproduces the checked-in CI baseline within 8 bytes on every
  series, so the local deltas are attributable to this change:

| Configuration | Release file / .text | Release-size file / .text |
| --- | ---: | ---: |
| baseline | +0 / +0 | +0 / +0 |
| core | +6,136 / +4,768 | +4,896 / +3,600 |
| component | +6,088 / +4,720 | +4,560 / +3,264 |
| wasi | +6,056 / +4,720 | +4,560 / +3,264 |
| core-nosimd | +6,136 / +4,768 | +4,928 / +3,632 |
| core-jit | +4,456 / +3,408 | +3,312 / +2,400 |
| core-aot | +6,696 / +5,312 | +4,752 / +3,504 |
| wasi-threads | +6,184 / +4,864 | +4,848 / +3,616 |

  The delta decomposes (symbol-level diff of `embed-core`) into the pass
  itself (2,256 B), the two previously-eliminated handler pairs the pass
  legitimately resurrects — masked CRC (kernel 504 B + call pair
  903 B) and the low-7 guard (call pair 485 B) — the
  `instantiate` closure (+230 B), and LTO placement noise. The first
  measurement of this change showed +70,816 / +66,784 on core: that was
  the list-CRC handler pair (`op_i32_list_crc_summary` 30,276 B +
  `internal_op_call_i32_list_crc_summary` 30,267 B) being resurrected
  by a quickening reference, which is why the list-CRC quickening was
  removed from the pass (section 1.2). On the largest gated series
  (release component file, checked-in baseline 2,667,344 B against the
  original #164 baseline of 2,433,656 B) this change adds +6,088 B,
  i.e. +0.25 points on the cumulative growth against the original #164
  baseline (from +9.60% to ≈ +9.85% on that series; exact re-baseline
  values come from the CI runs).

The checked-in baseline is intentionally not updated here; re-baselining
follows the established separate process.
