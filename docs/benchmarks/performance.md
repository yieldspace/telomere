# Performance regression gate (#212)

#164 gates binary **size**; until #212 nothing gated **speed**. Three real
regressions were found by a person looking, not by the build:

- a **35%** interpreter slowdown from adding one `VMResult` variant
  (measured during #127; mechanism recorded in #143);
- an up-to-**2x** interpreter slowdown from merely compiling the `jit`
  feature, even when the JIT never runs (2026-08-04 architecture review);
- **~8.6 ns per crossing** on continuation-bridged ops, enough to erase
  the JIT's advantage in a loop (same review).

This document is the gate's design record: what is measured, the
run-to-run variance actually observed on CI, how the ceilings follow
from that evidence — including the series the evidence says cannot be
gated — what the gate would and would not have caught, and how to move
the baseline deliberately. The enforced numbers live in
`tools/perf-baseline.json`; this document and that file must not drift
apart silently.

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
gate's statistics — per-cell medians of five whole-process samples taken
back to back after one warm-up, and ratios derived from them — are
ceiling-check statistics, not published baselines: gate numbers are
never quotable as performance results, and published baselines continue
to come only from the methodology's quiet-window protocol and raw
records under `docs/core/baseline/`.

## What is measured

Two workloads × three cells, measured by `tools/measure-performance.py`
on every gated run:

| Workload | Shape | n |
| --- | --- | ---: |
| `loop-linear` | i32 load/add/store loop with branch and memory access | 500,000,000 |
| `repeat-fib32` | call-heavy: fixed `fib(32)` repeated by count | 24 |

| Cell | Build | Run |
| --- | --- | --- |
| `default-interp` | default features | interpreter |
| `jit-interp` | `--features jit` | interpreter (no `--jit`) |
| `jit-jit` | `--features jit` | `--jit` |

## What is gated, and what is only recorded

The establishment runs below forced a distinction the size gate never
had to make. **The GitHub-hosted `ubuntu-24.04` fleet is heterogeneous**:
five runs of identical code landed on five different CPU models, and
their absolute medians for the same series differ by 60–200% purely
from host allocation. An absolute wall-time ceiling derived from those
medians would fail pull requests for the runner they happened to draw —
establishment run 2, with zero code change, would have failed every
absolute series against a +25% ceiling. That is the #184 contamination
trap in fleet form: a gate whose failures do not mean anything is worse
than no gate.

Ratios computed **inside one run on one host** are the fleet-invariant
quantities, so those are what the gate enforces:

| Gated series | Definition | Catches |
| --- | --- | --- |
| `jit_feature_tax_ratio` | `jit-interp` median / `default-interp` median | the JIT-feature interpreter tax — invisible to any single-configuration gate |
| `jit_speed_ratio` | `jit-jit` median / `jit-interp` median | the JIT losing ground against the interpreter on the same host — the aggregate signature of the bridged-crossing regression |

The absolute cell medians are still measured, recorded in the baseline,
and compared against **advisory ceilings** (baseline × 1.25). Exceeding
one prints a warning and uploads the same artifact; it never fails the
gate. A uniform slowdown that moves all cells together is visible there
— and in the accumulated run artifacts — for a human to judge with the
runner's CPU model in hand, which is the most this fleet supports
honestly.

Not gated, with evidence:

- **`f64-kernel` (corpus member, excluded).** On the code this gate was
  established against (`f3d4830`), the interpreter aborts with a host
  stack overflow (`fatal runtime error: stack overflow`, exit 134)
  somewhere between n = 100,000 (passes) and n = 1,000,000 (aborts), in
  both interpreter cells, with or without a raised `RUST_MIN_STACK`;
  the manifest's n is 500,000,000. The JIT cell completes the same
  artifact at full n. A series whose interpreter cells cannot finish
  cannot be gated; the abort itself is the more urgent finding and is
  reported with this change. Once the interpreter runs `f64-kernel` at
  scale, add the series by the re-baseline procedure below.
- **CoreMark (L1, excluded).** The corpus manifest pins the artifact by
  URL and SHA-256 (`e71e3582…`), but on 2026-11-23 the file served at
  that URL hashes to `5acb1c0e…`: the remote artifact has changed since
  the pin, so the corpus fetch fails the methodology's own fail-closed
  digest check. (The methodology harness's materialization test fails
  on current `main` for the same reason.) The gate does not re-pin
  another track's corpus. When the corpus owner re-pins, L1 can join
  the gate in the same three cells.
- **Peak RSS and cold start** are indicative on shared runners and are
  never gated — the same boundary #164 drew for size.
- **Per-op micro-costs** (the L3 layer) are not gated; see the
  bridged-ops paragraph in the catch analysis.

## Observed run-to-run variance on CI

On 2026-11-23 the `Performance gate` workflow was run five times
against the same tree (`252ff04c50ba40a120c1be7bba92a9905ad136ab` —
branch commits `fd074ac`, the gate itself, and `43feb4c`, an
establishment-only push trigger, on `main` at `f3d4830`; the four later
runs were pushed as empty commits, so all five runs built and measured
byte-identical code and tooling): runs 45146333434, 45148203571,
45148210616, 45148216951, 45148222925 (`ubuntu-24.04`, Rust 1.96.0).
Each run is five samples per cell after one warm-up; the table shows
each run's per-cell **median** in seconds, and the derived ratios.

`loop-linear`:

| Run | Runner CPU | default-interp | jit-interp | jit-jit | tax ratio | speed ratio |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 45146333434 | AMD EPYC 9V45 | 3.666 | 3.832 | 2.624 | 1.0453 | 0.6847 |
| 45148203571 | AMD EPYC 7763 | 6.168 | 6.843 | 7.784 | 1.1094 | 1.1376 |
| 45148210616 | AMD EPYC 9V74 | 4.845 | 5.323 | 4.169 | 1.0985 | 0.7833 |
| 45148216951 | Intel Xeon 6973P-C | 4.197 | 4.612 | 2.462 | 1.0990 | 0.5339 |
| 45148222925 | Intel Xeon 6975P-C | 4.164 | 4.934 | 2.546 | 1.1849 | 0.5160 |

`repeat-fib32`:

| Run | Runner CPU | default-interp | jit-interp | jit-jit | tax ratio | speed ratio |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 45146333434 | AMD EPYC 9V45 | 5.166 | 5.499 | 10.156 | 1.0645 | 1.8467 |
| 45148203571 | AMD EPYC 7763 | 8.630 | 8.887 | 21.254 | 1.0299 | 2.3915 |
| 45148210616 | AMD EPYC 9V74 | 6.456 | 7.113 | 12.921 | 1.1018 | 1.8164 |
| 45148216951 | Intel Xeon 6973P-C | 4.410 | 4.924 | 9.943 | 1.1165 | 2.0193 |
| 45148222925 | Intel Xeon 6975P-C | 4.531 | 4.940 | 10.601 | 1.0903 | 2.1460 |

Spread, as (max − min) / median across the five runs:

| Series kind | loop-linear | repeat-fib32 |
| --- | ---: | ---: |
| absolute cell medians | 59.6–202.8% | 72.1–106.7% |
| `jit_feature_tax_ratio` | 12.7% | 7.9% |
| `jit_speed_ratio` | 90.8% | 28.5% |

Within-run samples are far tighter than the cross-run numbers (the
median-of-five absorbed, for example, a single 10.9 s sample in a cell
whose other samples were ~3.6 s in run 45146333434). The cross-run
variance is a property of the fleet, not of the measurement: the same
host that made every absolute series ~1.7–3x slower in run 45148203571
left its tax ratios at 1.1094 / 1.0299, inside the family of the other
four runs.

## Thresholds: conservative ceilings over the fleet-invariant series

#184's prerequisite finding governs here: a threshold *derived* from an
A/A noise floor would be circular — A/A cannot observe the
layout-dependent effects a build comparison exists to catch, and a
floor observed under contention launders that contention into the
definition of acceptable. The ceilings below are therefore **policy
margins over the recorded baselines, honestly labelled as provisional
conservative ceilings**, each resting on the observed spread above:

- `jit_feature_tax_ratio` ceiling = baseline (median of the five runs)
  × 1.20. The worst observed run sits at baseline × 1.08
  (`loop-linear`) and × 1.02 (`repeat-fib32`), so the margin leaves
  ≥ 11% headroom over observed behaviour while the historical 2.0x tax
  would exceed it by more than 50 points.
- `jit_speed_ratio` ceiling = worst observed run × 1.25.
  `loop-linear`'s speed ratio is genuinely variable across the fleet
  (its worst run already sits at baseline × 1.66), so a median-derived
  margin would false-fire on behaviour already observed; the ceiling
  instead targets the > 2x relative swing that is the bridged-crossing
  regression's signature. `repeat-fib32` uses the same rule for
  uniformity.
- Absolute medians: advisory ceiling = baseline × 1.25, warning only,
  for the reasons in "What is gated".

The resulting enforced numbers (in `tools/perf-baseline.json`):
`loop-linear` tax ≤ 1.3188, speed ≤ 1.4220; `repeat-fib32` tax ≤ 1.3084,
speed ≤ 2.9894. Every failure message states the measured ratio, the
ceiling, the baseline, and the baseline's provenance (commit, tree, run
IDs, date, runner, toolchain, sample size — all recorded in the
baseline file). The ceilings must not be tightened without evidence of
the kind #184 requires; if the fleet changes character, the response is
a deliberate re-baseline that records the new environment, never a
silent edit.

## Would the gate have caught the three regressions?

- **Up-to-2x JIT-feature tax.** Yes. The tax ratio would have moved
  from ~1.09–1.10 toward 2.0 against ceilings of 1.3084–1.3188. This is
  the regression the gate is best shaped for, and the one no
  single-configuration measurement could have seen.
- **~8.6 ns per continuation-bridged crossing.** In its aggregate form,
  yes; per-crossing, no — and this document does not claim more. The
  historical harm was that the crossing cost *erased the JIT's
  advantage in a loop*: on `loop-linear` the speed ratio would have
  moved from its typical ~0.52–0.78 through 1.0 (parity) to the 1.4220
  ceiling region as the JIT became slower than the interpreter it was
  supposed to beat. A per-crossing cost confined to an op family the
  gated workloads rarely bridge would pass under the ceiling; the
  instrument for that layer is L3 criterion work in the JIT track, and
  the corpus member that would exercise bridged float ops most directly
  (`f64-kernel`) is currently excluded for the interpreter abort above.
- **35% interpreter slowdown (VMResult variant).** Not as a CI failure,
  and the reason is measured, not assumed: if the slowdown lands evenly
  in both interpreter cells — as a `VMResult` variant addition does —
  no within-run ratio moves, and the absolute series that would show it
  varies 60–200% across this fleet from host allocation alone, so an
  absolute ceiling tight enough to convict 35% would also have failed
  establishment run 45148203571 (and nearly failed several series in
  other runs) with no code change at all. What the gate does do: the
  run's three advisory warnings fire together, the measurement artifact
  preserves the medians and the runner CPU model, and the accumulated
  artifacts make the shift visible to a human comparing like hosts.
  Convicting this class automatically requires either a homogeneous
  runner pool this repository does not have, or the methodology's
  quiet-window baselines, which remain the instrument for it. A future
  per-CPU-model absolute gate could be layered on once enough
  same-model runs exist to baseline each model honestly; five runs
  across five models is not that evidence.

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
   — is available. Because single runs are host-dependent, rest a
   re-baseline on at least three runs of the same tree, as the initial
   establishment did with five.
3. Update `tools/perf-baseline.json` in the same pull request: set each
   changed series' baseline to the median of the runs' values, its tax
   ceiling to baseline × 1.20, its speed ceiling to the worst observed
   run × 1.25, and each cell's baseline/advisory pair to the median
   absolute median and that × 1.25; rewrite the `source` block to name
   the new commit, tree, run IDs, date, runner, toolchain, and sample
   size, with the observed spread recorded in `source.derivation`. Do
   not add headroom that no measurement supports beyond the policy
   margins above, and do not use a re-baseline to absorb a slowdown the
   pull request does not explain.
4. Update the variance table and the re-baseline history in this
   document to match, so the recorded numbers and the enforced numbers
   cannot drift apart silently.

### Re-baseline history

- **Initial establishment (2026-11-23, this change).** Five runs of
  tree `252ff04c…` (see the variance section for run IDs and hosts).
  Baselines are the runs' medians; ceilings follow the policy margins
  above. The establishment runs were produced with a temporary push
  trigger on the branch because `workflow_dispatch` requires the
  workflow to exist on the default branch; the trigger was removed in
  the same pull request before merge, and the measured code and tooling
  are identical with or without it.
