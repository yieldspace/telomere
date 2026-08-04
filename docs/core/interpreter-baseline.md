# Interpreter Baseline Methodology

> Status: current measurement methodology. No baseline record or performance
> number has been committed yet.

This document defines the reproducible interpreter-baseline method used by the
performance track. It is a measurement contract, not a result: a value is
publishable only when the harness has emitted a valid raw JSON record under
[`baseline/`](baseline/). The retained 2026-05-19 CoreMark values are historical
context; see [CoreMark Runtime Comparison](coremark-benchmark.md).

The method preserves Telomere's chained tail-call dispatch. Measurements record
its form with witnesses, but do not change the dispatch implementation.

## Scope and terminology

The optimizer switch measures the **whole optimizer pipeline**, not a subset of
CoreMark-shaped recognizers. Its OFF state uses the pipeline's existing
materialized-function fallback. Therefore an `opt-on` versus `opt-off` result
is an **upper bound on the whole optimizer pipeline's contribution**. It must
never be labelled as a recognizer-only cost.

The `measure-switches` feature is non-default and has no dispatch-path check.
It is a measurement control, not an embedder-facing runtime setting.

## Three measurement layers

| Layer | Question | Method | Comparable result |
| --- | --- | --- | --- |
| L1 — cross-runtime | Where does Telomere sit relative to other runtimes? | CoreMark's validated `Iterations/Sec` through `telomere-cli`. | The historical CoreMark convention and external runtimes. |
| L2 — workload corpus | What does one unit of guest work cost in a build? | A paired whole-process wall-clock slope through `telomere-cli`. | Telomere builds and commits on the same workload. |
| L3 — in-process micro | Did a handler-level change help? | Existing Criterion benchmarks in `crates/telomere/benches/telomere_bench.rs`. | Local branch-to-branch investigation only. |

L1 preserves the only score with an external comparison point. L2 includes
startup, parsing, optimization, instantiation, execution, and teardown. L3 is
never a release gate or a cross-runtime claim.

## Build and run matrix

The matrix consists of four builds and six run cells. Interpreter cells do not
pass `--jit`; a `jit` feature build run without that flag is still an
interpreter cell.

| Cell | Build | `TELOMERE_OPTIMIZER` at run time | Purpose |
| --- | --- | --- | --- |
| `default` | `cargo build --locked --release -p telomere-cli` | n/a | Control for measurement-feature inertness; scheduled twice as A1/A2 for noise. |
| `jit` | `cargo build --locked --release -p telomere-cli --features jit` | n/a | JIT-feature interpreter control; no `--jit`. |
| `opt-on` | `cargo build --locked --release -p telomere-cli --features measure-switches` | unset | Optimizer pipeline enabled. |
| `opt-off` | same `measure-switches` build | exactly `off` | Existing materialized fallback; upper bound on the whole optimizer pipeline's contribution. |
| `jit,opt-on` | `cargo build --locked --release -p telomere-cli --features jit,measure-switches` | unset | JIT-feature interpreter-tax comparison. |
| `jit,opt-off` | same `jit,measure-switches` build | exactly `off` | Existing materialized fallback; upper bound on the whole optimizer pipeline's contribution with the JIT feature compiled in. |

`default` and `jit` establish whether a `measure-switches` build with the
optimizer on is an admissible stand-in for the corresponding normal build.

The four copied binaries are named after their **build**, not their run cell:
`target/baseline/bin/{default,jit,opt,jit-opt}`. In particular, `opt-on` and
`opt-off` share the single `opt` copy, while `jit,opt-on` and `jit,opt-off`
share the single `jit-opt` copy. The environment setting selects the run cell;
it never selects or rebuilds a different binary.

### Switch resolution and observation

`TELOMERE_OPTIMIZER` has exactly these meanings when `measure-switches` is
compiled:

| Environment state | Resolved state |
| --- | --- |
| unset | `on` |
| exactly `off` | `off` |
| any other set value | invalid |

Resolution is lazy and process-scoped through `OnceLock::get_or_init`. Library
callers, including WAST, therefore resolve it when they parse instead of
depending on the CLI. The feature-gated CLI resolves it once at startup. An
invalid value is fail-closed: the CLI exits non-zero before measurement, while
a direct library parse panics with the variable name, rejected value, and
accepted values. No path silently treats an invalid value as `on`.

The feature-gated probe reports the library's effective state without running a
module:

```shell
cargo run --locked --release --features measure-switches -- measure-switches-probe
TELOMERE_OPTIMIZER=off \
  cargo run --locked --release --features measure-switches -- measure-switches-probe
```

Its respective stdout payloads are `{"state":"on"}` and `{"state":"off"}`.
The harness invokes it in a separate process and records both the requested
environment value and observed state in raw JSON. A process cannot flip this
switch after it has resolved it.

## Workload corpus

Every L2 workload must satisfy all of the following before it can contribute a
slope:

1. Its interpreter run at the chosen `n` takes roughly 3–30 seconds.
2. Its i32 parameter multiplies work **linearly**; merely accepting a parameter
   is not enough.
3. It self-validates through exact stdout or an equivalent explicit check.
4. A remote artifact is pinned by URL and SHA-256. A small in-tree artifact is
   allowed only when its licensing and source are unambiguous.
5. It covers a guest-work shape not represented by another corpus member.

The checked-in workload manifest is
[`tools/baseline/artifacts.json`](../../tools/baseline/artifacts.json). The
intended roles are:

| Workload | Layer | Shape and validation boundary |
| --- | --- | --- |
| CoreMark | L1 | Mixed integer/state-machine/matrix workload; its validated `Iterations/Sec` is the metric. |
| [`repeat-fib32.wat`](../../tools/baseline/repeat-fib32.wat) | L2 | Call-heavy fixed `fib(32)` repeated by `repeat_count`; only the repeat count is linear. Recursive `fib(n)` is not an L2 multiplier. |
| [`loop-50m.wat`](../../tools/baseline/loop-50m.wat) | L2 | i32 load/add/store loop with branch and memory access; the #190 sensitivity shape. |
| Float/dense arithmetic workload | L2 | A non-WASI compute kernel, such as [`f64-kernel.wat`](../../tools/baseline/f64-kernel.wat), selected only with manifest source/hash evidence. |

CoreMark is not evidence that a CoreMark-specific rewrite is useful. Sightglass
is excluded from this first corpus because its WASI dependency would make the
result partly a preview1-host-shim measurement.

### Linear-work check

For each L2 round, the harness measures `t(n)`, `t(2n)`, and `t(3n)` and keeps:

```text
d12 = (t(2n) - t(n)) / n
d23 = (t(3n) - t(2n)) / n
```

It reports a slope only when

```text
abs(median(d23) - median(d12)) / median(d12) <= floor_for_this_workload
```

after first rejecting non-positive or non-finite median increments as
`non_positive_increment` or `non_finite_increment`. The slope is the median of
the round-aligned paired differences, not a difference of independent medians.
Raw JSON includes all three times, paired increments, slope and interval when
valid, the linearity verdict, and the implied constant term.

## Sampling, noise floor, and reporting

The harness estimates a fresh floor on every run rather than borrowing a
threshold from an older baseline.

### Schedule and samples

- The `default` binary is scheduled twice under distinct A1/A2 labels.
- Each round contains each arm once in a seeded randomized or counterbalanced
  order. Fixed round-robin is prohibited because position bias is not noise.
- A normal baseline run uses 15 interleaved rounds (and one warm-up round), and
  may not use fewer than 10 rounds. A published baseline requires at least 10
  paired contrasts **for every metric, including CoreMark**.
- `--rounds <10` is accepted only together with `--quick`; `--quick` defaults
  to three rounds with no warm-up and is always non-publishable.
- JSON retains raw vectors plus median, minimum, maximum, and count. The
  minimum is diagnostic only, not an estimator.

Floors are per metric. L1's base metric is `Iterations/Sec`; L2's is a
per-arm slope in ns/iteration. Gates operate on a dimensionless symmetric
contrast, so an L1 floor is never reused for L2.

### Exact contrast and floor

For paired A/A observations in round `i`:

```text
c_i = (a1_i - a2_i) / ((a1_i + a2_i) / 2)
```

For two measured arms, use the same formula:

```text
D = (x - y) / ((x + y) / 2)
```

The floor is the larger of:

1. the upper bound of a two-sided 95% bootstrap percentile interval on
   `median(abs(c_i))`, using 10,000 resamples and the recorded seed; and
2. the empirical 95th percentile of `abs(c_i)`.

Only `abs(D) > floor` may be reported as a numeric delta. If `abs(D) <= floor`,
the result is `below_noise_floor` with both values and the floor, not a
percentage claim.

Below ten paired contrasts, the floor falls back to `max(abs(c_i))`; below
three it is `insufficient_samples_for_interval`. Such a small-sample schedule
must be an explicit `--quick` run. `--quick` may emit only
`below_noise_floor` or `invalid_reason`; it never creates a publishable or
quotable delta.

### Quiet-window protocol

1. Build all four binaries before requesting the quiet window; compilation is
   itself a source of contention.
2. Stop unrelated builds, benchmark runs, and background pipelines, then begin
   the seeded schedule.
3. The default start gate is one-minute load
   `max(1.0, logical_cpu_count * 0.10)`, and the maximum permitted one-minute
   load rise across timing is `0.5`. `--max-start-load` and
   `--max-load-rise` may override these values; the effective thresholds are
   recorded in JSON.
4. Let the harness capture its documented one-minute-load checks, platform,
   CPU model, kernel, commit, Cargo/Rust versions, artifact hashes, and
   start/end load. Do not replace a rejected run with an unrecorded retry.
5. If the host is busy, retain JSON with `invalid_reason` and exit non-zero. A
   contended median is not valid evidence. Every non-zero harness exit still
   emits its JSON record on stdout, so an attempted cell is never silent.

## Tail-call witnesses

Every measured binary records two independent witnesses. A timing result is
invalid if the required witness cannot establish its contract.

| Witness | Contract |
| --- | --- |
| A — behavioural | The existing 200,000-nested-call regression verifies direct call threading completes without stack overflow, using the measured feature set. |
| B — codegen | A target-aware disassembly inspection follows successful dispatch edges for `op_i32_and`, `op_local_get4_br_if`, `op_i32_load_const_base`, and `op_call`. |

Witness B requires every reachable non-error dispatch exit of every probe to be
an indirect tail branch: `br x<N>` on arm64 or `jmp *%r<N>`/`jmpq *` on x86-64.
A call/`blr`, or a following `ret`, fails. Cold panic/error blocks are recorded
as excluded rather than mistaken for the dispatch edge. Unsupported
architectures and unavailable tools produce `witness_unavailable`, not pass.
The record retains raw instructions and reports `probe_coverage: N of N probes
verified`, never a whole-binary claim.

| Mode | Absolute dispatch contract | Relative comparison |
| --- | --- | --- |
| `first-record` | Fail closed for a failed or unavailable required probe. | Not applicable: no prior record. |
| `compare` | Fail closed with the same absolute contract. | Also fail closed for regression against the named baseline. |

The first record is not exempt from the absolute tail-call contract; only a
relative comparison is unavailable on that first run.

## Attribution profile

`release-profiling` inherits `release` and enables `debug = 1` with
`strip = "none"`. It exists for attribution only. No performance number,
baseline threshold, or delta may be taken from that binary.

Use flat/leaf attribution because tail-call threading replaces frames:

- macOS: `sample <pid> <seconds> -f <out>`;
- Linux: `perf record -F 999`, then `perf report --no-children --sort=symbol`.

Do not use `--call-graph=fp`: the profile does not force frame pointers and a
tail-call call tree is not the intended view. Fold leaf symbols into the same
handler-family vocabulary used by VM profiling. A `vm-profile` build can supply
structural instruction/pair/triple counts alongside it, but its per-family
`elapsed_ms` is total elapsed time times count share (`count / total_instrs`),
not a timing measurement.

## Reproducing a candidate record

Run from the repository root after corpus and artifact hashes are validated.
The harness is the only command that may write a baseline record. Do not run
four direct Cargo builds into one `target/release` directory and then use
`--skip-build`: each later build would overwrite the binary needed for an
earlier cell.

`--build-only` and `--skip-build` both require a clean **tracked** worktree.
The build-only phase resolves the source `HEAD`, builds the four feature sets,
copies each resulting executable, hashes each copy, and passes Witness A for
each feature set. It stores that source commit, exact feature list, expected
copy path, SHA-256, and passing Witness A command/status in
`target/baseline/build-manifest.json`. Before timing, `--skip-build` reads the
manifest and revalidates the current commit, all four feature sets, the
expected copy paths, the copied-binary hashes, and the recorded passing
Witness A evidence. A mismatch fails closed before a timing sample is taken.

```shell
# Before the quiet window: build all four feature sets, copy each binary out of
# the shared Cargo target, hash it, pass Witness A, and write build provenance.
python3 tools/measure-interpreter-baseline.py --build-only

# Start the quiet window, then measure only the copied, hashed cell binaries.
python3 tools/measure-interpreter-baseline.py \
  --mode first-record \
  --skip-build \
  --out /private/tmp/telomere-interpreter-baseline.json
```

`--build-only` materializes the four builds under
`target/baseline/bin/{default,jit,opt,jit-opt}`; the later `--skip-build` run
uses those copies only after its manifest checks. `opt-on`/`opt-off` share
`opt`, and `jit,opt-on`/`jit,opt-off` share `jit-opt`. `--quick` is diagnostic
only. Before accepting a
measurement-feature build, also run WAST in separate processes:

```shell
env -u TELOMERE_OPTIMIZER \
  cargo test -p telomere --release --features measure-switches --test wast
TELOMERE_OPTIMIZER=off \
  cargo test -p telomere --release --features measure-switches --test wast
```

## Publishing and intentionally moving a baseline

There is currently no committed file under `docs/core/baseline/`, and no
current-results table to fill. A successful first record is published only by
committing raw JSON at:

```text
docs/core/baseline/YYYY-MM-DD-<short-sha>.json
```

The human summary links to that file and states its commit, workload/artifact
hashes, requested and observed switch states, machine facts, sample count,
floors, witnesses, and any `below_noise_floor` or `invalid_reason`. It must not
transcribe or round a result absent from JSON.

To update a baseline deliberately:

1. Keep the prior JSON immutable and name it explicitly with `--baseline`.
2. Run the same corpus and quiet-window protocol in `compare` mode; do not use
   `--quick` for a baseline move.
3. Require both absolute witnesses and the comparison contract. Keep
   failed-with-reason output rather than silently retrying.
4. Commit a new date-and-short-SHA JSON rather than overwriting the old one.
5. Explain the reason for the move and link both records. A changed artifact
   hash or invalid cell makes the comparison non-equivalent until resolved.

```shell
python3 tools/measure-interpreter-baseline.py \
  --mode compare \
  --skip-build \
  --baseline docs/core/baseline/<prior-record>.json \
  --out /private/tmp/telomere-interpreter-baseline-compare.json
```

Raw JSON is the reviewable source of truth. Markdown tables summarize it; they
never replace it.
