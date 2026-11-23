# Performance regression gate (#212)

#164 gates binary **size**; until #212 nothing gated **speed**. Three real
regressions were found by a person looking, not by the build:

- a **35%** interpreter slowdown from adding one `VMResult` variant
  (measured during #127; mechanism recorded in #143);
- an up-to-**2x** interpreter slowdown from merely compiling the `jit`
  feature, even when the JIT never runs (2026-08-04 architecture review);
- **~8.6 ns per crossing** on continuation-bridged ops, enough to erase
  the JIT's advantage in a loop (same review).

This document is the gate's design record: what is measured, what the
observed CI variance was, how the ceilings were set, what the gate would
and would not have caught, and how to move the baseline deliberately. The
enforced numbers live in `tools/perf-baseline.json`; this document and
that file must not drift apart silently.

## One methodology, consumed not reinvented

The gate does not introduce a new measurement convention. It consumes the
one documented methodology, [Interpreter Baseline
Methodology](../core/interpreter-baseline.md):

- the workloads are that methodology's L2 corpus, pinned in
  [`tools/baseline/artifacts.json`](../../tools/baseline/artifacts.json)
  and verified by SHA-256 before use;
- the timing is its L2 whole-process wall clock through `telomere-cli`,
  with the workload's exact-stdout self-validation applied to every
  sample;
- the build/run cells are its matrix restricted to the two shipped
  builds (the `measure-switches` builds are local measurement controls,
  not configurations an embedder can select, so the gate does not build
  them).

Criterion benchmarks (`crates/telomere/benches/`) remain what the
methodology says they are: L3, for local branch-to-branch investigation,
never a release gate. The retained CoreMark document is historical. The
gate's own statistic — the median of five whole-process samples per cell,
taken back to back after one warm-up — is a ceiling-check statistic, not
a published baseline: gate numbers are never quotable as performance
results, and published baselines continue to come only from the
methodology's quiet-window protocol and raw records under
`docs/core/baseline/`.

## What is gated

Two workloads × three cells, plus one derived ratio per workload:

| Workload | Shape | n |
| --- | --- | ---: |
| `loop-linear` | i32 load/add/store loop with branch and memory access | 500,000,000 |
| `repeat-fib32` | call-heavy: fixed `fib(32)` repeated by count | 24 |

| Cell | Build | Run | Catches |
| --- | --- | --- | --- |
| `default-interp` | default features | interpreter | interpreter regressions in the shipped default build |
| `jit-interp` | `--features jit` | interpreter (no `--jit`) | the JIT-feature interpreter tax, plus interpreter regressions in that build |
| `jit-jit` | `--features jit` | `--jit` | JIT execution regressions |

The derived series is the **JIT-feature tax ratio**,
`jit-interp median / default-interp median` per workload. Both cells run
on the same runner in the same job, minutes apart, so the ratio is to
first order independent of how fast that runner happens to be: it stays
meaningful even if the runner fleet changes speed, which the absolute
ceilings do not. The historical tax reached 2.0x; the ratio series exists
to catch the slide long before that.

## What is not gated, and why

- **`f64-kernel` (corpus member, excluded).** On current `main`
  (`f3d48301fef4e66b883d6445fee1882c620b174d`) the interpreter aborts
  with a host stack overflow (`fatal runtime error: stack overflow`,
  exit 134) somewhere between n = 100,000 (passes) and n = 1,000,000
  (aborts), in both interpreter cells, with or without a raised
  `RUST_MIN_STACK`; the manifest's n is 500,000,000. The JIT cell
  completes the same artifact at full n. A gate series whose
  interpreter cells cannot finish on current code cannot be gated;
  the abort itself is the more urgent finding and is reported with
  this change. Once the interpreter runs `f64-kernel` at scale, add
  the series by the re-baseline procedure below.
- **CoreMark (L1, excluded).** The corpus manifest pins the artifact by
  URL and SHA-256 (`e71e3582…`), but on 2026-11-23 the file served at
  that URL hashes to `5acb1c0e…`: the remote artifact has changed
  since the pin, so the corpus fetch fails the methodology's own
  fail-closed digest check. The gate does not re-pin another track's
  corpus. When the corpus owner re-pins, L1 can join the gate as a
  `coremark` series in the same three cells.
- **Peak RSS and cold start** are indicative on shared runners and are
  never gated — the same boundary #164 drew for size.
- **Per-op micro-costs** (the L3 layer) are not gated; see the
  bridged-ops paragraph below for what that means for one of the three
  historical regressions.

## Observed run-to-run variance on CI

@@VARIANCE@@

## Threshold: a conservative absolute ceiling

#184's prerequisite finding governs here, and this gate inherits it
deliberately: a threshold *derived* from an A/A noise floor would be
circular. A/A runs the same binary twice, so it cannot observe the
layout-dependent effects a build-comparison gate exists to catch; and a
floor observed on a machine contended by workloads outside our control
would launder that contention into the definition of "acceptable".

So the ceilings are **policy values, honestly labelled**: each absolute
ceiling is the recorded CI baseline median × 1.25, and each tax-ratio
ceiling is the recorded baseline ratio × 1.20 (rounded up to 4 decimal
places). The variance section above is the evidence that these margins
cannot be tripped by the observed run-to-run spread — the widest
observed median-to-median spread is @@MAXSPREAD@@, several times smaller
than the margin — and the catch analysis below is the evidence that the
smallest regression this gate exists to catch (35%) cannot pass under
them. The ceilings are provisional and conservative: they are **not**
derived optima, and they must not be tightened without evidence of the
kind #184 requires. If a future runner fleet is uniformly faster or
slower, absolute medians move for environmental reasons; the response is
a deliberate re-baseline that records the new environment, never a
silent edit, and the tax ratio — which does not move with fleet speed —
is the tripwire that distinguishes an environmental shift from a code
regression in the meantime.

Every failure message states the measured median, the ceiling, the
baseline, and the baseline's provenance (commit, run IDs, date, runner,
toolchain, sample size — all recorded in `tools/perf-baseline.json`).

## Would the gate have caught the three regressions?

- **35% interpreter slowdown (VMResult variant).** Yes. It was an
  interpreter-wide effect in the shipped builds; 35% exceeds the 25%
  ceiling margin in every interpreter cell of both workloads.
- **Up-to-2x JIT-feature tax.** Yes, twice over: the `jit-interp` cell's
  absolute median would have doubled (ceiling +25%), and the tax ratio
  would have moved from ~1.1–1.2 toward 2.0 (ceiling ≈ baseline × 1.20).
  A single-configuration gate could not have seen this at all, which is
  why the cell exists.
- **~8.6 ns per continuation-bridged crossing.** Only in aggregate, and
  this document does not claim more. The gate sees a bridged-op cost
  only when it moves a gated workload's median past its ceiling. The
  gated workloads' hot loops are largely native in the JIT cell, and
  the corpus member that would exercise bridged float ops most
  directly (`f64-kernel`) is currently excluded for the interpreter
  abort above. A per-crossing cost confined to an op family the gated
  workloads rarely bridge would pass under the ceiling; the instrument
  for that layer is L3 criterion work in the JIT track, not this gate.
  Closing this residual gap needs a bridged-op-dense corpus workload
  that runs at scale in both engines, which the corpus does not
  currently have.

## How the gate runs

`tools/measure-performance.py` builds nothing; the `Performance gate`
workflow (`.github/workflows/performance.yaml`) builds `telomere-cli`
in the default and `jit` configurations into separate target
directories, installs the pinned `wasm-tools` (v1.263.0, SHA-256
verified — the same tool the methodology harness requires), measures,
and runs `tools/check-performance.py` against
`tools/perf-baseline.json`. The workflow triggers on pull requests that
touch `crates/**`, `src/**`, `tools/**`, the workspace manifests, or the
workflow itself, and on `workflow_dispatch`. The measurement artifact
is uploaded even when the gate fails (the #272 pattern), because the
failing runs are the ones needed for investigation and re-baselining.
The checker's unit tests are `tools/test_check_performance.py` and the
measure script's are `tools/test_measure_performance.py`, both wired
into the CI feature-wiring job. Runs execute sequentially, never in
parallel: contended timings are not evidence.

## Updating the baseline when a change is intentionally slower

1. Make the change, and say in the pull request description that the
   slowdown is intentional and why.
2. Take a fresh measurement of the exact code in the pull request: run
   the `Performance gate` workflow on the pull request (or dispatch it
   against the branch) and download the `performance-measurement`
   artifact. The upload step runs even when the gate step fails, so the
   artifact of the failing run — the run that prompted the re-baseline
   — is available.
3. Update `tools/perf-baseline.json` in the same pull request: set each
   changed series' baseline to the newly measured median (for a series
   resting on repeated runs, the median of the runs' medians) and its
   ceiling to baseline × 1.25 (× 1.20 for the tax ratio), and rewrite
   the `source` block to name the new commit, run IDs, date, runner,
   toolchain, and sample size the values came from, with the observed
   spread recorded in `source.derivation`. Do not add headroom that no
   measurement supports beyond the policy margins above, and do not
   use a re-baseline to absorb a slowdown the pull request does not
   explain.
4. Update the variance table and the re-baseline history in this
   section to match, so the recorded numbers and the enforced numbers
   cannot drift apart silently.

### Re-baseline history

@@HISTORY@@
