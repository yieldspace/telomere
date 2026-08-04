# Interpreter Baseline Measurement Attempts

> Status: **non-baseline evidence**. Nothing in this directory is an accepted
> interpreter baseline, a performance result, or a substitute for one. Accepted
> first-record and comparison artifacts belong only in
> [`../baseline/`](../baseline/).

This directory preserves the failed-but-informative observations from issue
`yieldspace/telomere#184`. They show why this host could not produce a trusted
baseline on 2026-08-04, while leaving the normal measurement harness runnable
for a later quiet host or quiet interval.

The JSON files are immutable copies of the original artifacts. Verify them with:

```shell
shasum -a 256 docs/core/measurement-attempts/*.json
```

| Artifact | Purpose | Publication status | SHA-256 |
| --- | --- | --- | --- |
| [`busy-machine-attempt-2026-08-04.json`](busy-machine-attempt-2026-08-04.json) | First-record load-gate attempt | `publishable: false` | `d91a590ba1edcd283dfe715054cad466833ae201092286bdfcbfe983975e9b5d` |
| [`environment-characterization-2026-08-04.json`](environment-characterization-2026-08-04.json) | Host characterization, not timing | `published_baseline_eligible: false` | `9486b45bef9753e07c94fdf08e47b6d14305f21d83db9cd60493c3c4429c4423` |
| [`finite-schedule-bias-audit-2026-08-04.json`](finite-schedule-bias-audit-2026-08-04.json) | Audit of the superseded finite shuffled schedule | `published_baseline_eligible: false` | `3f68da0d0a78a6bd8e30c27cf0709c53d5ad9075a1c9144d23b1e8edde48069a` |

## Attempt 1: gate correctly refused measurement

The reproducible first-record procedure is:

```shell
python3 tools/measure-interpreter-baseline.py --build-only
python3 tools/measure-interpreter-baseline.py \
  --mode first-record \
  --skip-build \
  --out /private/tmp/telomere-interpreter-baseline-2026-08-04.json
```

At the attempted start, the committed artifact records a one-minute load of
`4.44970703125`. The configured maximum start load was `1.2` and the allowed
one-minute rise was `0.5`. The harness therefore returned
`status: "invalid"`, `invalid_reason: "busy_machine"`, and marked all L1/L2
workloads `not_run`. It did not run a timing sample, relax a threshold, or
create a baseline result.

This is an expected fail-closed outcome. Re-running the command is valid only
after the environment conditions below are met; an old `busy_machine` artifact
must never be promoted to `../baseline/`.

## Environment characterization: the low observed value was not a clean floor

The characterization tool intentionally bypasses the timing gate because it is
not a timing measurement:

```shell
python3 tools/characterize-baseline-environment.py \
  --duration 300 \
  --interval 15 \
  --top-count 12 \
  --out /private/tmp/telomere-184-environment-characterization.json
```

The preserved five-minute observation ran from `2026-08-04T07:15:54Z` through
`2026-08-04T07:20:54Z` and took 21 samples. Its one-minute load was first
`2.97607421875`, minimum `1.85546875`, maximum `3.29248046875`, and last
`2.41162109375`. The raw sample rows retain the simultaneous 1/5/15-minute
series and top CPU processes.

Those samples are **not** a GUI-only idle-floor estimate. Early samples showed
`WindowServer` at about 42--46% CPU and Claude/Codex GUI helpers. Later samples
also showed unrelated `dragapult_ai` Python workers at 87.9%, 91.8%, and 99.6%
CPU (with a later sample containing workers at 98.4% and 78.7%). One-minute
load is a decaying average, so neither a before/after subset nor the minimum
can causally isolate that external workload. The only supported conclusion is
that this particular series was contaminated and cannot derive a replacement
gate threshold.

For a fresh investigation, use the same command after unrelated jobs and GUI
load have been stopped or controlled, retain the full raw JSON, and report the
1/5/15-minute series and process snapshots. It remains characterization only:
the tool always writes `purpose: "environment_characterization"`,
`timing_measurement: false`, `load_gate_applied: false`, and
`published_baseline_eligible: false`.

## Why an A/A control could not rescue this attempt

The third artifact audits the old, independently shuffled 15-round plan before
it was replaced by the carryover-balanced Williams plan. In that realized seed,
CoreMark arm mean positions ranged from 2.13 to 3.87; six of seven arms never
visited at least one position. Pairwise CoreMark position gaps reached five or
six positions. In each L2 workload, the three nominally paired scales could be
separated by as many as 20 intervening positions.

An A/A contrast can bound drift for two executions of the *same* binary. It
cannot observe build-specific effects such as code-layout/cache/bandwidth or
thermal interactions, and it cannot repair the recorded position imbalance in
a different-build comparison. Thus a small A/A result would not validate an
optimizer-build delta under this host interference. The audit is evidence for
the schedule correction, not performance data.

## Conditions for a later first record

Before retrying, retain the conservative load gate rather than deriving a lower
one from these contaminated samples. A valid run requires all of the following:

1. The host is quiet enough for the configured start threshold and end-of-run
   load-rise gate; per-sample load observations are retained for audit. If
   either gate fails, keep the resulting `busy_machine` JSON and stop.
2. Unrelated CPU workloads (including external worktrees) and uncontrolled GUI
   activity are absent or otherwise controlled; characterize the new condition
   separately rather than reusing this series.
3. The harness preflight has built and hashed its artifacts and the required
   tail-call witnesses pass for each build configuration.
4. Run the build and first-record commands above without changing the gate or
   blending characterization/A-A samples into the published record.

When those conditions hold, the harness can produce a new raw first-record
artifact, which may be evaluated under the methodology's publication rules and
committed under `../baseline/`. Until then, the honest result is this recorded
measurement refusal, not a numerical baseline.
