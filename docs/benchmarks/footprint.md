# Footprint and Cold Start

This note records measured binary size, peak resident memory, and whole-process
cold start for both the `telomere-cli` host binary and the standalone minimal
embedder configurations added in [#139](https://github.com/yieldspace/telomere/issues/139).
Each table identifies its own source environment and measurement boundary.
Dependency-graph counts state their own method and evidence boundary. Nothing
here is estimated, and no number for another runtime is reported unless it was
measured here or quoted with a source.

## Scope and honesty boundary

- The historical CLI tables measure the `telomere-cli` host binary. It links
  `clap` and a multi-thread `tokio` runtime on top of the runtime crates, so
  those values are an upper bound on what an embedded host would carry.
- #139 measures a seven-row standalone embedder ladder. Here **minimal** means
  the minimal *supported dependency topology*, not the minimum achievable byte
  count. The headline rows keep SIMD enabled; `core-nosimd` makes its byte
  difference visible.
- Telomere's stated goal is an embedded-class Component Model runtime. These
  measurements establish how much the current configurations contain; they do
  **not** establish that Telomere is WAMR-class. Closing that distance remains a
  goal, not a result; see [Other runtimes](#other-runtimes).

## Thread-free embedder dependency graph (#138)

Issue #138 establishes a supported, compile-only minimal embedder cell for both
the core and Component Model crates:

```shell
cargo check -p telomere --no-default-features --features simd
cargo check -p telomere-component --no-default-features --features simd
```

The cell keeps `simd` enabled. `--no-default-features` by itself has a
pre-existing simd-off compile failure in `main` ([#150](https://github.com/yieldspace/telomere/issues/150)),
which is outside this issue. This is a dependency-graph and compilation claim,
not a minimal-binary-size claim.

Package counts use Cargo 1.96 and `cargo tree -e normal`. Repeated `(*)`
entries are normalized to the same package identity before counting unique
packages (including the root package). The first two rows record the
pre-implementation audit stages; the final row was re-measured on the final
configuration with the commands below.

These package counts are historical evidence from before #163; #139 makes no
claim about a current package count, so readers must not reinterpret the table
as a current-count result.

| Configuration | Unique normal packages | Evidence boundary |
| --- | ---: | --- |
| Pre-gate `--no-default-features --features simd` baseline | 48 | Pre-implementation audit |
| Baseline with Tokio narrowed to `default-features = false`, `sync,time` only, but still ungated | 43 | Pre-implementation audit |
| Final `--no-default-features --features simd` cell, with Tokio behind `threads` | 42 | Measured after #138 implementation |

The 48-to-43 narrowing removes `bytes`, `mio`, `signal-hook-registry`,
`socket2`, and `tokio-macros`; gating removes `tokio` itself for the final
43-to-42 step. SIMD is deliberately present in every row above and adds
`wide`, `safe_arch`, and `bytemuck`; those packages are not a threads cost.

With `threads` disabled, shared-memory declarations and `0xFE` atomic
instructions—including `memory.atomic.notify` and `memory.atomic.wait32`—are
rejected as `WasmParserError::UnsupportedFeature { feature:
ProposalFeature::Threads, .. }`. The final normal dependency graph has no Tokio
path. Reproduce both trees and the three-state inverse-tree guard as follows:

```shell
cargo tree -p telomere -e normal --no-default-features --features simd
cargo tree -p telomere-component -e normal --no-default-features --features simd

assert_no_tokio() {
  package="$1"
  if ! inverse="$(cargo tree -p "$package" -e normal --no-default-features --features simd -i tokio)"; then
    echo "cargo tree guard failed for $package"
    return 1
  fi
  if [[ -n "$inverse" ]]; then
    echo "tokio is present in $package's minimal normal graph"
    printf '%s\n' "$inverse"
    return 1
  fi
}

assert_no_tokio telomere
assert_no_tokio telomere-component
```

Cargo 1.96 prints `warning: nothing to print.` to stderr and exits zero when
there is no inverse path. Therefore the guard intentionally treats an empty
stdout result as pass, nonempty stdout as failure, and a nonzero `cargo tree`
exit as failure; `! cargo tree ... -i tokio` would be a false failure here.

The CI workflow adds this as a standalone `minimal-embedder` job now. It does
not wait for [#148](https://github.com/yieldspace/telomere/issues/148), which
will coordinate its eventual absorption into the broader feature matrix. #139
extends the compile-only claim with the independently built and executed
configuration ladder measured below.

## Minimal embedder footprint (#139)

This is the first measurement of how much the supported standalone embedding
topology carries. It is deliberately separate from the CLI tables: each row is
built by its own Cargo invocation, so feature unification cannot silently pull
a higher layer into a lower-layer result.

| Configuration | Linked layers and features | Fixture and expected output |
| --- | --- | --- |
| `baseline` | no Telomere runtime APIs/code retained; file-reading/linker baseline | `examples/add.wasm` → `335` |
| `core` | `telomere` with `simd` | `examples/add.wasm` → `3` |
| `component` | `core` + `telomere-component` with `simd,component` | `examples/component-add.wasm` → `42` |
| `wasi` | `component` + `telomere-component-wasi` with `simd,wasi` | `examples/wasi-component-args.wasm` → `0` |
| `core-nosimd` | `telomere` without optional features | `examples/add.wasm` → `3` |
| `core-jit` | `core` with `simd,jit` | `examples/add.wasm` → `3` |
| `wasi-threads` | `wasi` with `simd,threads,wasi` | `examples/wasi-component-args.wasm` → `0` |

The `baseline`, `core`, `component`, `wasi`, `core-nosimd`, `core-jit`, and
`wasi-threads` names are the configuration contract for
[#140](https://github.com/yieldspace/telomere/issues/140)'s RISC-V follow-up.
The headline ladder retains SIMD because it is the supported default; a
SIMD-off headline would not describe the supported path. Thus `minimal` does
not mean "fewest bytes".

### Method, environments, and source artifacts

For every row, `tools/measure-embedder-footprint.py` builds exactly one
`--bin` with exactly that row's `--no-default-features` and feature list, runs
it once and checks the expected stdout, then measures RSS and time. It performs
three warm-up runs, reports the median of 30 whole-process (`exec` through
exit) wall-clock runs, and reports the median peak RSS from five `/usr/bin/time`
runs. A failed build, execution, expected-output check, strip/size command, or
RSS parse aborts the whole measurement rather than emitting a partial number.
The CLI cold-start harness has the same abort-on-failure rule.

The `release` tables use a stripped copy as the primary file-size column and
show the raw file size alongside it. `release-size` has `strip = "symbols"` in
the Cargo profile, so its file-size and stripped-size values are identical.
Text is `__TEXT,__text` on Mach-O and `.text` on ELF. File, text, and RSS values
below are exact bytes; displayed wall-time medians are rounded to three decimal
milliseconds from the JSON samples.

| Measurement environment | Value |
| --- | --- |
| macOS date and host | 2026-08-03; macOS 26.5.2 arm64; Apple M2 Pro; 32 GiB |
| macOS toolchain and source | Rust 1.96; measurement source head `b0012a6dc87ce9323e076f74a5e687bb1c3d3d58` for both CLI and embedders |
| macOS caveat | Physical laptop; foreground/system load was uncontrolled. RSS and time are measured observations, not a controlled-machine benchmark. |
| Linux source | GitHub Actions run `30823517715` at measurement source head `b0012a6dc87ce9323e076f74a5e687bb1c3d3d58` |
| Linux host | Ubuntu 24.04; Linux 6.17.0-1020-azure x86_64; glibc 2.39 |
| Linux caveat | Exact file/text bytes are from this recorded build. The shared runner's absolute RSS and time values are indicative; same-run comparisons are more useful than cross-host absolute values. |

The retained historical CLI-only tables later in this document were measured on
a different macOS run and are labeled as such; they are not silently treated as
part of the #139 data set.

| JSON source artifact | SHA-256 |
| --- | --- |
| `/private/tmp/telomere-139-macos-b0012a6-release.json` | `5f09f4caf67737df7b6bbc5c7646abcbf423987c54a4a881d20beab78fbafe38` |
| `/private/tmp/telomere-139-macos-b0012a6-release-size.json` | `2ee83bd9d94a5b86b3ce1472691f0ea418cefe901448b575a0e90e7c51411386` |
| `/private/tmp/telomere-139-macos-b0012a6-cli-artifact.json` | `652dfe41d66fed7d3a76c3b06c8302cd0c8659eaf3522c9b3dc5132fd486a0ff` |
| `/private/tmp/telomere-139-macos-b0012a6-cli-cold-start.json` | `28c3020d579848118c79e589bc6d0883825f9c95dbe08c3275efd52935fbd4a8` |
| `/private/tmp/telomere-139-linux-b0012a6/footprint-release.json` | `6f78d9c350458e5f4d0f7b719cbf91da5eb678265cb7a2b4aaacbbb9afa29f67` |
| `/private/tmp/telomere-139-linux-b0012a6/footprint-release-size.json` | `985f1374e39685c4db8f3d934cdff7a7330761412c58bf9e5eaf5476ed7f1109` |

The commands used for the #139 measurements, from the repository root, are:

~~~shell
cargo build --release
python3 tools/measure-cold-start.py
python3 tools/measure-embedder-footprint.py --profile release
python3 tools/measure-embedder-footprint.py --profile release-size
~~~

The macOS CLI artifact measurement copied `target/release/telomere-cli` to
`/private/tmp/telomere-139-cli-b0012a6/telomere-cli`, retaining the
`telomere-cli` basename, then used `/usr/bin/size -m` before and after
`/usr/bin/strip`. An earlier long-name copy was 24 B larger after Apple
`strip`; this is an observed artifact-size difference, and no cause is
assigned. On Linux the embedder harness selected `/usr/bin/size -A` and
`/usr/bin/strip --strip-all` for the release-copy inspection. The JSON records
the exact per-artifact commands, selected section, and expected output.

### #205 feature-policy staleness and evidence ledger

This ledger is deliberately separate from the #139 measurements. The absolute
values in the #139 tables below and their measurement-source head
`b0012a6dc87ce9323e076f74a5e687bb1c3d3d58` are unchanged. It records only
what a before/after comparison in the same worktree at the same output path
observed: before `3b1237b` and after the #205 feature-policy change. It does
not rewrite the historical
measurements with current-head values.

#### A. macOS arm64 binary evidence: measured

The following deltas are `after - before` in bytes. The size and text
comparisons use the same worktree and output path before and after. The
`release` raw-file entries are `stat` values from preserved unstripped
binaries, so recording them required no additional build. They are comparative
file-layout evidence, not updated absolute footprint values.

| Profile | Configuration | Raw file delta (B) | Stripped file / File delta (B) | `__TEXT,__text` delta (B) |
| --- | --- | ---: | ---: | ---: |
| `release-size` | `wasi` | — | File: -8 | 0 |
| `release-size` | `wasi-threads` | — | File: -8 | 0 |
| `release` | `wasi` | -64 | Stripped file: -24 | 0 |
| `release` | `wasi-threads` | +16 | Stripped file: -8 | 0 |

Do not summarize these cells as an overall direction: raw file sizes moved by
small amounts in both directions (-64 to +16 bytes).

The `component` control's unstripped artifacts were bit-identical before and
after, which supports the observed difference being local to the changed WASI
dependency boundary rather than a workspace-wide build difference. As a build
determinism control, two full-clean baseline `wasi` builds, each preceded by
`cargo clean --profile release-size`, also produced bit-identical unstripped
files.

`/usr/bin/strip` is not a SHA-based comparator here: it produced
non-bit-identical output from the same unstripped input while retaining the
same file size. SHA equality is therefore not an acceptance predicate for
these stripped artifacts; the size comparisons above are robust to that tool
variance.

At the dependency-report level, the raw tree topology differs in 2 of 7 rows,
while the normalized package-plus-activated-feature set is identical in all 7
of 7 rows. The known source change is removal of the direct
`wasi -> telomere` edge. It is an inference, not a link-map attribution, that
the small file-size changes come from metadata, symbols, linker layout, or
unwind layout associated with that edge removal; the measured zero
`__TEXT,__text` deltas show no code-text change in these four macOS cells.

#### B. Linux binary evidence: unmeasured

The same source-level direct-edge removal is present for the Linux
`release` and `release-size` `wasi` and `wasi-threads` configurations, but
the resulting Linux file and text values have not been measured for the #205
feature-policy change. macOS results must not be extrapolated to Linux.

| Profile | Configurations | Fields | Status |
| --- | --- | --- | --- |
| `release-size` | `wasi`, `wasi-threads` | File, `.text` | Unmeasured; no numeric or zero value is inferred. |
| `release` | `wasi`, `wasi-threads` | Raw file, stripped file, `.text` | Unmeasured; no numeric or zero value is inferred. |

A clean GitHub Actions runner can provide a later remeasurement, but that work
is not an acceptance criterion for #205.

#### C. RSS and cold-start evidence: not re-derived

No RSS or cold-start result can be derived from the binary comparisons above.
For all four affected tables—macOS `release`, macOS `release-size`, Linux
`release`, and Linux `release-size`—those measurements remain unmeasured for
#205. The [#184](https://github.com/yieldspace/telomere/issues/184) and
existing indicative/uncontrolled-load caveats remain in force; this ledger
neither infers a change nor replaces those observations.

### macOS arm64 — Apple M2 Pro

#### `release` (stripped file size is primary)

| Configuration | Raw file (B) | Stripped file (B) | `__TEXT,__text` (B) | Peak RSS median of 5 (B) | Cold median of 30 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `baseline` | 428064 | 339808 | 221144 | 1556480 | 2.767 |
| `core` | 1681104 | 1338152 | 974664 | 2179072 | 2.966 |
| `component` | 2648368 | 2160592 | 1639664 | 2867200 | 2.944 |
| `wasi` | 3012800 | 2463832 | 1867048 | 3899392 | 3.595 |
| `core-nosimd` | 1435568 | 1142072 | 828212 | 2195456 | 3.015 |
| `core-jit` | 2181728 | 1756728 | 1323760 | 2392064 | 3.331 |
| `wasi-threads` | 3210720 | 2627848 | 1991160 | 3915776 | 3.631 |

#### `release-size` (already stripped by profile)

| Configuration | File (B) | `__TEXT,__text` (B) | Peak RSS median of 5 (B) | Cold median of 30 (ms) |
| --- | ---: | ---: | ---: | ---: |
| `baseline` | 286160 | 195284 | 1507328 | 2.878 |
| `core` | 737184 | 577224 | 2031616 | 3.071 |
| `component` | 1054176 | 859308 | 2326528 | 2.988 |
| `wasi` | 1154832 | 941672 | 3063808 | 3.482 |
| `core-nosimd` | 687120 | 535156 | 1966080 | 3.055 |
| `core-jit` | 937040 | 762236 | 2129920 | 3.052 |
| `wasi-threads` | 1188368 | 984276 | 3063808 | 3.538 |

The following are comparative text-section deltas, not a link-map attribution
of individual symbols or a proof of why a layer has that size.

| `release-size` comparison | Text delta (B) | Reading |
| --- | ---: | --- |
| `core − baseline` | +381940 | Core runtime with SIMD over the file-reading baseline |
| `component − core` | +282084 | Component Model layer over core |
| `wasi − component` | +82364 | WASI provider layer over component |
| `core − core-nosimd` | +42068 | SIMD-enabled core difference (about 41 KiB) |
| `core-jit − core` | +185012 | Experimental baseline JIT difference |
| `wasi-threads − wasi` | +42604 | Threads feature difference (about 42 KiB) |
| `wasi − baseline` | +746388 | Headline runtime text delta (about 729 KiB) |

### Linux x86_64 — GitHub Actions shared runner

#### `release` (stripped file size is primary)

| Configuration | Raw file (B) | Stripped file (B) | `.text` (B) | Peak RSS median of 5 (B) | Cold median of 30 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `baseline` | 439432 | 345672 | 261695 | 2093056 | 0.753 |
| `core` | 1801600 | 1428896 | 1087495 | 2883584 | 0.907 |
| `component` | 2921536 | 2388728 | 1905607 | 3817472 | 0.962 |
| `wasi` | 3335160 | 2727216 | 2162023 | 4734976 | 1.670 |
| `core-nosimd` | 1559664 | 1240040 | 948727 | 2891776 | 0.909 |
| `core-jit` | 2342640 | 1876408 | 1439991 | 3293184 | 0.943 |
| `wasi-threads` | 3533240 | 2889168 | 2284231 | 4866048 | 1.666 |

#### `release-size` (already stripped by profile)

| Configuration | File (B) | `.text` (B) | Peak RSS median of 5 (B) | Cold median of 30 (ms) |
| --- | ---: | ---: | ---: | ---: |
| `baseline` | 296384 | 228114 | 1859584 | 0.750 |
| `core` | 1043952 | 732743 | 2658304 | 0.911 |
| `component` | 1553584 | 1139911 | 3125248 | 0.959 |
| `wasi` | 1743744 | 1267639 | 3883008 | 1.763 |
| `core-nosimd` | 937112 | 659719 | 2539520 | 0.916 |
| `core-jit` | 1369376 | 958119 | 2854912 | 0.956 |
| `wasi-threads` | 1850016 | 1340151 | 3899392 | 1.740 |

| `release-size` comparison | Text delta (B) | Reading |
| --- | ---: | --- |
| `core − baseline` | +504629 | Core runtime with SIMD over the file-reading baseline |
| `component − core` | +407168 | Component Model layer over core |
| `wasi − component` | +127728 | WASI provider layer over component |
| `core − core-nosimd` | +73024 | SIMD-enabled core difference (about 71 KiB) |
| `core-jit − core` | +225376 | Experimental baseline JIT difference |
| `wasi-threads − wasi` | +72512 | Threads feature difference (about 71 KiB) |
| `wasi − baseline` | +1039525 | Headline runtime text delta (about 1015 KiB) |

The macOS and Linux `wasi − baseline` text deltas are both reported because
neither is *the* runtime number: macOS is 746388 B (about 729 KiB), while Linux
is 1039525 B (about 1015 KiB). The Linux value is 1.39× the macOS value for
these recorded artifacts. This is a platform-dependent observation only; the
cause was not measured and no cause is assigned.

The observation is not confined to the headline delta: the Linux/macOS
`release-size` text-layer ratios are 504629/381940 = 1.32× for core,
407168/282084 = 1.44× for the Component Model increment,
127728/82364 = 1.55× for the WASI-provider increment, and
225376/185012 = 1.22× for the JIT increment. These are recorded
platform-specific comparisons, not explanations of their causes.

For the #138 change, the important conclusion is not a claimed megabyte saving:
the byte deltas above are modest comparative observations that make optional
layers visible. #138's value is the no-Tokio dependency topology and supported
embeddability boundary; the no-threads configurations prove that path.

## Size regression gate (#164)

#139 measured the minimal-embedder ladder but shipped no gate, because a
threshold such as "measured + 15%" would have been invented rather than
derived: nobody had observed the run-to-run variance of the measurement on
CI. #164 supplies that observation first and derives the threshold from it.

### Observed run-to-run variance

On 2026-11-23, the `Embedder footprint` workflow was run three times
against the code of PR #262: workflow_dispatch runs 45096189255 and
45096200688 against the PR branch at merge commit
`4c12a5f3dcdd85e91b52db1363655caf35c5dad2` (branch head
`31e80be68aa9358cdb86994831662eca7c0a7f70` with `main` at
`3121c3a6efe48cd625b9b9d4e16469cbda7de099` merged in), and
pull-request run 45096136125, which built the same tree
(`ubuntu-24.04`, Rust 1.96.0). Every
run produced byte-identical sizes: for all fourteen
profile/configuration rows, the raw file size, the `.text` size, the
stripped file size, and the stripped `.text` size each have a run-to-run
spread of **0 B** across the three samples. The observed values are the
baselines in the table below (stripped file / stripped `.text`, in bytes).

| Profile | Configuration | Stripped file (B) | Stripped `.text` (B) |
| --- | --- | ---: | ---: |
| `release` | `baseline` | 345,656 | 261,679 |
| `release` | `core` | 1,478,816 | 1,131,383 |
| `release` | `component` | 2,475,352 | 1,983,175 |
| `release` | `wasi` | 2,821,896 | 2,245,383 |
| `release` | `core-nosimd` | 1,286,184 | 988,279 |
| `release` | `core-jit` | 2,011,688 | 1,566,455 |
| `release` | `wasi-threads` | 3,008,224 | 2,384,151 |
| `release-size` | `baseline` | 296,384 | 228,114 |
| `release-size` | `core` | 1,097,832 | 776,983 |
| `release-size` | `component` | 1,630,336 | 1,202,983 |
| `release-size` | `wasi` | 1,826,016 | 1,334,535 |
| `release-size` | `core-nosimd` | 982,992 | 697,399 |
| `release-size` | `core-jit` | 1,530,808 | 1,095,015 |
| `release-size` | `wasi-threads` | 1,967,344 | 1,428,007 |

#### Re-baseline history

The original baseline was measured on 2026-11-23 against commit
`6c43ca67ae7c975fa52641b2eeaca3ca26f8f4ca` (nine dispatches, spread 0 B).
Before PR #248 could merge, `main` absorbed PR #247 (#117,
semver-compatible export lookup, adding `runtime/name_match.rs`) and
PR #246 (#167, executor-drive rework), both of which add code to
`telomere-component`. The first CI run of PR #248 therefore tripped its
own gate: a pull-request run builds the merge commit with current `main`,
not the branch point, so the gate correctly measured the merged code. The
growth is confined to the configurations that include the component
runtime (`component`, `wasi`, `wasi-threads`; release/component stripped
file +6,952 B, release/wasi-threads +20,088 B). Over the same interval the
core configurations shrank slightly (release/core stripped file -208 B,
the net effect of PR #244 (#175) and PR #245 (#151)) and the `baseline`
configuration did not change. The baseline was re-measured on current
`main` and updated as recorded above, following the procedure below.

PR #250 (#154, debug-profile stack overflows) was branched before this
gate existed, so `main` was merged into the branch first; its first CI run
then tripped the gate on the PR's own change. The release-relevant part of
that change splits the continuation computation of
`special_function_return` into an `#[inline(never)]` helper
(`prepare_function_return` in `crates/telomere/src/runtime/vm/control.rs`)
so the tail dispatch stays a sibling call in non-LTO builds. Because the
telomere library is linked into every embedder configuration, the
resulting code-layout shift moved gated sizes slightly in both directions
(release stripped `.text` -16 B in five configurations and release
core-jit +80 B stripped file; release-size core and core-nosimd -80 B,
component, wasi, wasi-threads and core-jit +16..+32 B; 12 of 28 gated
series unchanged, largest absolute delta 80 B). The attribution was
cross-checked by measuring the PR head alone against the original
`6c43ca6` baseline (workflow_dispatch runs 45049436565 and 45049445764 on
commit `e450be821b6d628fc91f7ae9f81977e1ce8caae7`), which showed the same
delta pattern. The baseline was re-measured on the merged code and
updated as recorded above, following the procedure below. The PR's other
build change, `[profile.dev.package.telomere]` in `Cargo.toml`, applies to
dev profiles only and cannot affect these numbers.

PR #251 (#177, `interrupt()` wakes a guest parked in an indefinite
`memory.atomic.wait`) merged current `main` into its branch first; its
first CI run on the merged tree then tripped the gate on the PR's own
change. The change adds the interrupt wake path to the telomere
library: a fourth waiter state (`INTERRUPTED`) claimed by CAS and
`wait_result_interruptible` racing notify / timeout / interrupt in
`crates/telomere/src/common/memory.rs`, the `Interrupted` completion
plumbing in `runtime/memory_effect.rs` and `runtime/scheduler.rs`, and
— compiled only under the threads feature — the broadcast `Notify` in
`common/metering.rs` (`interrupt()` broadcasts after storing the flag;
waiters register interest and re-check the flag so no wakeup is lost).
Because the library is linked into every embedder configuration except
`baseline`, the growth is nearly uniform (release stripped `.text`
+608 B in core, component, wasi, core-nosimd and core-jit; stripped
file +1,016 B, component +1,000 B; release-size stripped `.text`
+272 B, core-jit +256 B; stripped file +392..+424 B) and largest in
`wasi-threads`, where the threads-only metering broadcast path lands
(release +3,520 B stripped file / +2,384 B stripped `.text`;
release-size +1,760 B / +992 B). The `baseline` configuration, which
links no library code, is unchanged. The baseline was re-measured on
the merged code and updated as recorded above, following the procedure
below.

PR #252 (#170, parse time grows cubically with block nesting) merged
current `main` into its branch first; its first CI run on the merged
tree then tripped the gate on the PR's own change. The change rewrites
two optimizer analysis passes in
`crates/telomere/src/parser/core/optimizer/pipeline/analysis.rs`:
`compute_loop_depth` becomes a dominator-tree prefix sum accumulated in
one reverse-postorder pass, and `compute_loop_parents` is inverted from
an all-pairs scan to a single pass over each loop's members; the PR's
only other change is a criterion bench, which is not linked into the
embedder. The parser/optimizer is part of the telomere library linked
into every embedder configuration except `baseline`, so the deltas are
nearly uniform across library configurations and the `baseline`
configuration is unchanged: in the `release` profile the library
configurations shrink slightly (stripped `.text` -224 B in all six;
stripped file -72 B, `wasi-threads` -88 B, `component` -56 B), while in
the `release-size` profile (`opt-level = "z"`, fat LTO, one codegen
unit) they grow (stripped file +1,120 B and stripped `.text` +960 B,
with `core-nosimd` and `core-jit` at +1,104 B / +944 B). The baseline
was re-measured on the merged code and updated as recorded above,
following the procedure below.

PR #253 (#118, canon `resource.drop` for host-provided resources)
merged current `main` into its branch first; its first CI run on the
merged tree then tripped the gate on the PR's own change. The change
is confined to the component layer: `telomere-component` gains the
linker destructor-registration API (`register_import_resource_drop` /
`_async`), the decoder keeps each imported instance declaration's
`TypeId` so instantiation can build the `ResourceId`-to-destructor
table, and the runtime tracks host-issued `own` handles so
`resource.drop` resolves them (`runtime/canonical.rs`,
`runtime/host.rs`, `runtime/env.rs`); `telomere-component-wasi`
gains `WasiState::drop_handle` and registers destructors for its
handle-issuing imports in the provider. The core library is
untouched, so `baseline`, `core`, `core-nosimd` and `core-jit` are
byte-identical in both profiles (16 of 28 gated series unchanged),
and the growth sits exactly in the configurations that link the
component runtime (release stripped file +17,256 B for `component`,
+25,144 B for `wasi`, +25,176 B for `wasi-threads`; stripped `.text`
+15,200 / +21,712 / +21,712 B; release-size stripped file +9,936 /
+14,168 / +14,184 B, stripped `.text` +8,320 / +11,024 / +11,024 B).
`wasi` and `wasi-threads` grow more than `component` because they
also link the WASI provider's `drop_handle` and destructor
registrations; `wasi-threads` tracks `wasi` almost exactly (+32 B
stripped file in release, +16 B in release-size) because the PR adds
no threads-specific code. The baseline was re-measured on the merged
code and updated as recorded above, following the procedure below.

PR #254 (#131, wast harness spec fidelity) merged current `main` into
its branch first; its first CI run on the merged tree then tripped the
gate on the PR's own change. The PR strengthens the wast test harness
(trap/invalid reason matching, result arity, bit-exact NaN patterns)
and fixes the real defects that strengthening surfaced, all of them in
the telomere core library: the parser's memarg alignment check is
unified on `natural_align_log2` (`values.rs`, `instruction.rs`,
`simd_instruction.rs`), the type checker now produces `UnknownLabel`,
unknown export indices return their per-kind error variants
(`parser.rs`), and the interpreter's rounding paths quiet signaling
NaNs (`runtime/vm/numeric.rs`, `simd.rs`, `superinstructions.rs`).
The harness changes themselves are test-only and are not linked into
the embedder. Because the core library is linked into every embedder
configuration except `baseline`, the `baseline` configuration is
byte-identical in both profiles (4 of 28 gated series unchanged) and
the growth is near-uniform across the library configurations (release
stripped file +640..+880 B, stripped `.text` +624..+832 B;
release-size stripped file +528..+2,000 B, stripped `.text`
+576..+2,048 B). `core-nosimd` grows the least in release-size
(+528 B / +576 B) because the SIMD parser and runtime paths it
excludes carry part of the change. The baseline was re-measured on
the merged code and updated as recorded above, following the
procedure below.

PR #256 (#143, VMResult std-error surface with distinct trap causes)
merged current `main` into its branch first; its first CI run on the
merged tree then tripped the gate on the PR's own change. The PR splits
the three spec trap causes that `InvalidOperand` used to merge into
payload-free `DivideByZero` / `IntegerOverflow` / `InvalidConversion`
variants across the interpreter, the JIT stubs and all three JIT
backends (the trap-code ABI appends codes 14-16 without renumbering),
implements `Display` and `std::error::Error` for `VMResult`, `TrapKind`
and `TrapInfo` (trap variants display as the spec messages), adds
`ComponentError::GuestTrap` so a guest trap's kind survives the
component layer, and centralises the `vm_try!` propagation match -
previously inlined at 1,143 call sites - into one cold
`VMResult::propagate_failure` function. A follow-up on the merged tree
classifies float-to-integer truncation failures by `is_nan` in the
interpreter handlers and the JIT stubs (NaN is an invalid conversion;
an out-of-range magnitude, including infinities, is an integer
overflow), which the #131 trap-message table merged from `main`
asserts separately. Attribution was isolated by measuring current
`main` alone (workflow_dispatch run 45066161931), which reproduces
the previous baseline exactly on all 28 gated series. The `baseline`
configuration is byte-identical in both profiles. In the `release`
profile the propagation centralisation dominates and the library
configurations move slightly in both directions (stripped file
-6,080..+4,136 B; stripped `.text` -1,728..+2,736 B), while in the
`release-size` profile (`opt-level = "z"`, fat LTO, one codegen unit)
the new Display/Error surface grows every library configuration
(stripped file +10,760..+31,432 B; stripped `.text` +4,592..+15,776 B;
the largest single delta is +1.8% of its series). The baseline was
re-measured on the merged code and updated as recorded above,
following the procedure below.

PR #257 (#186, reserve validated max operand depth at frame entry)
merged current `main` into its branch first. The change computes the
validated maximum operand depth at parse time in
`pipeline::optimize_function` from the type checker's stack snapshots,
stores it as `LoweredFunction.max_operand_bytes` carried into
`CallRecipe`, checks `frame_end + bound` against the stack length once
at frame entry on every frame-construction path (overflow still traps
`StackOverflow`, only detected earlier), and removes the per-push
bounds checks from the seven `_fast` push functions in
`crates/telomere/src/common/stack.rs`, each of which now carries a
debug_assert and the reservation invariant it relies on; checked
pushes remain on non-frame paths (embedder staging, scheduler resume,
host in-place results). Attribution was isolated by measuring current
`main` alone (workflow_dispatch run 45072035005), which reproduces
the previous baseline exactly on all 28 gated series. The `baseline`
configuration is byte-identical in both profiles (4 of 28 gated
series unchanged), and every library configuration shrinks in both
profiles — the `_fast` pushes are inlined into handlers across the
library, so their removed checks outweigh the single entry check and
the added field (release stripped file -1,720..-4,568 B, stripped
`.text` -2,096..-3,824 B; release-size stripped file -7,248..-8,160 B,
stripped `.text` -4,320..-5,216 B). No series grows, so unlike the
re-baselines above the gate had already passed on the merged tree;
the baseline was re-measured on the merged code and updated as
recorded above, following the procedure below, so the recorded
numbers keep tracking the code on `main`.

PR #259 (#258, component import lookup applies the #117 NameMap
semver rules) needed no merge commit: its branch head already sits
directly on current `main`, so its first CI run measured the PR's own
change and tripped the gate. The PR generalises
`crates/telomere-component/src/runtime/name_match.rs`
(`compatible_name` / `incompatible_name`, one shared implementation
for export and import names) and routes every import-resolution path
in `crates/telomere-component/src/linker.rs` through a
`resolve_registered` helper (exact match first, otherwise the
highest version on the compatible track): root func imports,
instance imports, funcs inside an imported instance, and the
instance part of a resource drop (the resource name itself stays
exact, which also fixes the instance half of #118's drop path);
when only an incompatible version is registered, the error now
names both the expected and found versions, in the same format as
the #117 export-side error. Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45087010624, gate
success), which reproduces the previous baseline exactly on all 28
gated series. Growth is confined to the configurations that link
`telomere-component` (`component`, `wasi`, `wasi-threads`): release
stripped file +10,016..+10,032 B and stripped `.text` +9,552 B;
release-size stripped file +4,288..+4,368 B and stripped `.text`
+3,728..+3,808 B. The `baseline` configuration and every core
configuration are byte-identical in both profiles (16 of 28 gated
series unchanged). The baseline was re-measured on the merged code
and updated as recorded above, following the procedure below.

PR #260 (#240, pin fn-pointer handler identity against duplication
and identical-code folding) was branched before PRs #256–#259
merged, so current `main` was merged into the branch first (merge
commit `5e83bf5`, no conflicts); its first CI run then tripped the
gate on the PR's own change. The PR marks all 512 identity-compared
op handlers `#[inline(never)]` so the compiler cannot duplicate
them across codegen units, adds identity barriers (a function-local
static whose address is passed to `core::hint::black_box`) to the
byte-identical twin handlers — the shared/indexed load/store
template families in `crates/telomere/src/runtime/vm/memory.rs`,
`op_f32_store` / `op_f64_store`, and `op_return` in
`crates/telomere/src/runtime/vm/control.rs` — documents the
identity contract on `Op` in `crates/telomere/src/common.rs`, and
adds `tools/check-fn-identity.py` (wired into CI) with its unit
tests plus OP_SPECS decode tests. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run 45089386493,
gate success), which reproduces the previous baseline exactly on
all 28 gated series. Growth appears in every configuration that
links the telomere library and is near-uniform, as expected when
512 handler bodies stop being inlined into or merged with their
dispatch sites: release stripped file +15,008..+22,864 B and
stripped `.text` +13,952..+21,184 B (core-jit grows most because
it also links the JIT backend handlers annotated in
`crates/telomere/src/runtime/jit/backend/ops.rs`); release-size
stripped file +9,040..+9,776 B and stripped `.text`
+10,512..+11,936 B. The `baseline` configuration is byte-identical
in both profiles (4 of 28 gated series unchanged). The baseline
was re-measured on the merged code and updated as recorded above,
following the procedure below.

PR #261 (#195, JIT code cache: pack compiled functions into
dual-view code slabs) merged current `main` into its branch first
(merge commit `eb90479`, no conflicts); its first CI run then
tripped the gate on the PR's own change. The PR rewrites
`CodeArena` in `crates/telomere-jit-codegen/src/code_memory.rs`:
compiled functions are bump-allocated at 16-byte alignment into
256 KiB dual-view slabs (one shared backing — `memfd_create` on
Linux, `shm_open` plus immediate unlink on macOS — mapped twice,
read/write and read/execute; the writable view is permanently
unmapped when a slab fills or seals, so the executable view is
never writable and the W^X contract stays one-way), oversized
functions get a dedicated slab, and environments that cannot
create a shared backing fall back to the previous per-function
mapping. `telomere-jit-codegen` gains a `cfg(unix)` `libc`
dependency, the JIT cache accounting in
`crates/telomere/src/runtime/jit/cache.rs` switches from
page-rounded sizes to `accounted_len` (aligned code bytes), and
aarch64 gains instruction-cache flushes (`__clear_cache` on
Linux, `sys_icache_invalidate` on macOS). A follow-up commit on
the branch casts the `shm_open` mode through `c_uint` so the
macOS build compiles (`mode_t` is `u16` there and cannot be
passed to the variadic `shm_open`; E0617). Attribution was
isolated by measuring current `main` alone (workflow_dispatch
run 45091421967, gate success), which reproduces the previous
baseline exactly on all 28 gated series. Only the `core-jit`
configuration links `telomere-jit-codegen` into the minimal
embedder, so the delta is confined to its four gated series
(release stripped file +3,072 B / stripped `.text` +2,320 B;
release-size stripped file +1,872 B / stripped `.text`
+1,248 B) and the other 24 gated series are byte-identical.
The baseline was re-measured on the merged code and updated as
recorded above, following the procedure below.

PR #262 (#187, scalar linear-memory access does 2–3 range checks
and a pointer chase per op; flatten to one compare against a
frame-cached (base, len)) merged current `main` into its branch
first (merge commit `4c12a5f`, no conflicts); its first CI run
then tripped the gate on the PR's own change. The PR snapshots
the default local memory's (base, committed len) in
`ExecuteContext` alongside the memory pointer, adds
`DefaultLocalMemoryView` in `crates/telomere/src/common.rs`
(checked access is a single u64 compare, `offset + N <= len`,
returning `MemoryIndexOutOfRange`), routes every scalar handler
on the default local path through it (94 call sites plus the
hand-written helpers in `crates/telomere/src/runtime/vm/memory.rs`,
`store_internal_local` in `vm.rs`, the u32 store in
`superinstructions.rs`, and the u16 load in `call.rs`), and
re-snapshots the length in the two `memory.grow` handlers;
shared memory keeps the previous locked path because its cached
pointer is null. The PR's only other change is a new integration
test file, which is not linked into the embedder. Attribution
was isolated by measuring current `main` alone
(workflow_dispatch run 45096211898, gate success), which
reproduces the previous baseline exactly on all 28 gated
series. The growth appears in every configuration that links
the telomere library: release stripped file +1,728..+4,800 B
and stripped `.text` +1,616..+4,672 B (`wasi-threads` largest);
release-size stripped file +1,680..+13,520 B and stripped
`.text` +1,600..+12,160 B, where release-size `core-jit` is
the outlier — under the release-size profile's optimisation
settings the flattened handlers land differently in the one
configuration that links both the interpreter and the JIT,
while the same configuration grows least in the release
profile (+2,064 B / +1,904 B); the per-configuration variation
is the code-layout effect of the one library change, as in
earlier re-baselines. The `baseline` configuration, which
links no library code, is byte-identical in both profiles
(4 of 28 gated series unchanged). The baseline was re-measured
on the merged code and updated as recorded above, following
the procedure below.

### Threshold derivation

With an observed spread of 0 B, the derived headroom over the baseline is
0 B: each threshold in `tools/embedder-size-baseline.json` equals its
observed baseline exactly. This is the "measured + N%" form with N = 0,
where N follows from the observed spread rather than being picked in
advance. A consequence is that any size growth — including growth caused
by a toolchain or runner-image change rather than by code — trips the gate
and forces a human to look; that is the intent, and the re-baseline
procedure below is the way through it.

### How the gate runs

`tools/check-embedder-size.py` compares the stripped file size and stripped
`.text` size of every configuration in the two JSON files the footprint
workflow already produces against the thresholds, and fails with the
measured size, the threshold, and the baseline source for each violation.
The footprint workflow runs it as the "Check embedder size regression gate"
step, so the workflow now fails when a gated size grows. The workflow also
triggers on `crates/**` changes, not only on measurement-tooling changes,
so a library change cannot grow the binary without the gate running. Peak
RSS and cold start are not gated: #139 records them as indicative on shared
runners, so they cannot carry a threshold. The checker's unit tests are
`tools/test_check_embedder_size.py`, wired into the CI feature-wiring job.

### Updating the baseline when growth is intentional

1. Make the change that grows (or shrinks) the binary, and say in the pull
   request description that the growth is intentional and why.
2. Take a fresh measurement of the exact code in the pull request: run the
   `Embedder footprint` workflow on the pull request (or dispatch it
   against the branch) and download the `embedder-footprint` artifact.
3. Update `tools/embedder-size-baseline.json` in the same pull request: set
   each changed row's `baseline` and `threshold` to the newly measured
   stripped sizes, and rewrite the `source` block to name the new commit,
   run IDs, date, and sample size the values came from. If the new values
   rest on repeated runs, record the observed spread in `source.derivation`
   and set the threshold to baseline + that spread; do not add headroom
   that no measurement supports.
4. Update the table in this section to match, so the recorded numbers and
   the enforced numbers cannot drift apart silently.

## Size-oriented release profile

The `release-size` profile inherits `release` and uses `opt-level = "z"`, fat
LTO, one codegen unit, `panic = "abort"`, symbol stripping, disabled debug
information, and disabled overflow checks. Its optimization level was selected
by a gate rather than presumed from the name:

| Candidate `opt-level` | `embed-wasi` file (B) | `__TEXT,__text` (B) | Gate result |
| --- | ---: | ---: | --- |
| `z` | 1154832 | 941672 | pass |
| `s` | 1581072 | 1377148 | pass |
| `3` | 1993632 | 1783312 | pass |

Every candidate passed `cargo test -p telomere --profile release-size --test
call_threading`, all 160 WAST cases, and the four headline binary runs
(`baseline`, `core`, `component`, `wasi`) with their expected outputs. No stack
overflow, wrong result, or miscompile was observed. The primary selection
criterion was declared in advance as `embed-wasi` file bytes; `z` was smallest,
so the text-section tie-break was not used.

`panic = "abort"` is adopted only under that four-binary run condition. A host
Rust panic consequently aborts the process; it does not make guest or
input-reachable panics safe. The latter remain tracked by
[#128](https://github.com/yieldspace/telomere/issues/128). Neither a
`-Z build-std` build nor `panic_immediate_abort` has been measured here.

## CLI host comparison (separate from layer deltas)

On the same macOS machine, at common measurement source head
`b0012a6dc87ce9323e076f74a5e687bb1c3d3d58`, and with the same `release`
profile, the stripped `embed-wasi` is 2463832 B and the stripped
`telomere-cli` is 3400768 B. The CLI shell difference is 936936 B; the CLI
artifact is 1.38× the embedder artifact in this like-for-like profile
comparison.

A different comparison describes the effect of the shipped-style profile, not
CLI overhead: the same 3400768 B release-stripped CLI divided by the
1154832 B `release-size` `wasi` artifact is 2.94×. That ratio includes the
profile change and must not be read as a CLI-overhead number.

## Whole-process cold-start boundary

No cold-start improvement is claimed. On the macOS `release-size` run, the
recorded `baseline` median is 2.877729 ms and `wasi` is 3.4815205 ms. The
shared baseline is most of the absolute value, so the measurement is dominated
by whole-process work (including process creation); the incremental gap is
0.6037915 ms. Repeated laptop medians move by roughly ±0.4 ms, so macOS
configuration differences are not distinguishable at this resolution; this is
not an instantiate-only timing.

On the common measurement-source head `b0012a6dc87ce9323e076f74a5e687bb1c3d3d58`
Linux `release-size` shared-runner data, `baseline` is 0.7502845 ms, `core` is
0.9105735 ms, and `wasi` is 1.7632145 ms: the Component Model + WASI 0.2
instantiate-and-run path is about 0.9 ms higher than the core-call path
(+0.852641 ms, about 1.94×) in that same run. Its absolute values remain
indicative because the runner is shared, but the within-run comparison is more
useful than a cross-host absolute comparison. It still includes process
creation, parsing, guest execution, and exit, rather than isolating
instantiation.

## Embedder executor boundary

The core, component, and WASI samples visibly use a small local `block_on`
implemented only with `std::task::Wake` and `std::thread::park`. They do not
depend on Tokio or on a futures executor. The discovered nesting incompatibility
is in the Component Model layer: `futures::executor::block_on` conflicts with
the executor already entered inside `telomere-component`. The follow-up is
[#167](https://github.com/yieldspace/telomere/issues/167); this issue does not
change `telomere-component` internals.

## Historical CLI measurement environment

| Item | Value |
| --- | --- |
| Date | 2026-08-01 |
| Host | macOS 26.5.2 (Darwin 25.5.0), arm64 |
| CPU | Apple M2 Pro |
| RAM | 32 GiB |
| Rust | `rustc 1.96.0 (ac68faa20 2026-05-25)`, host `aarch64-apple-darwin`, the version pinned by `rust-toolchain.toml` |
| Telomere commit | `30f049a` on `docs/positioning-and-examples` |
| Release profile | workspace default: `codegen-units = 1`, `lto = "thin"` |
| `strip` | Apple `strip` from the active Xcode command line tools |

`hyperfine` was not available on this machine, so cold start was measured with a
repeated-run median instead. Both methods used are given below.

## Binary size

All three table rows are historical measurements from `30f049a` in the
[historical CLI measurement environment](#historical-cli-measurement-environment)
above. The commands record the method used for those measurements; a current
checkout is not expected to produce the same byte counts, and #138 does not
re-measure them. The commented
`--no-default-features` command is historical only and must not be run on
current `main` because it now reaches the pre-existing simd-off failure tracked
in [#150](https://github.com/yieldspace/telomere/issues/150).

```shell
cargo build --release                          # default features (= full)
cargo build --release --features jit
# after each build:
ls -l target/release/telomere-cli
cp target/release/telomere-cli /tmp/telomere-cli-stripped
strip /tmp/telomere-cli-stripped
ls -l /tmp/telomere-cli-stripped

# Historical only (30f049a); do not run on current main:
# cargo build --release --no-default-features
```

The historical `--release --no-default-features` command below did not disable
the core defaults: feature unification in the CLI dependency graph still
enabled them at the commit being measured. It is retained as a historical CLI
measurement, but is **invalid as a feature comparison** and must not be
reinterpreted as a minimal-embedder result.

| Build | Unstripped | Stripped |
| --- | ---: | ---: |
| `--release` (default features: `full`) | 4,175,008 B (3.98 MiB) | 3,435,352 B (3.28 MiB) |
| `--release --no-default-features` (historical CLI row; invalid as a feature comparison) | 4,174,928 B (3.98 MiB) | 3,435,288 B (3.28 MiB) |
| `--release --features jit` | 4,694,160 B (4.48 MiB) | 3,870,408 B (3.69 MiB) |

Observations:

- The historical default and `--no-default-features` CLI rows do not compare
  different core feature sets. Their 80-byte delta is not minimal-embedder
  evidence, and this note makes no unmeasured minimal binary-size claim.
- The experimental JIT costs about 519 KB unstripped / 435 KB stripped.

## Peak resident memory

Peak RSS is the `maximum resident set size` line from `/usr/bin/time -l`, taken
as the median of 5 runs. On Linux use `/usr/bin/time -v` and read
`Maximum resident set size`. `tools/measure-cold-start.py` handles both platforms
and reports this alongside cold start.

```shell
/usr/bin/time -l target/release/telomere-cli examples/add.wasm main 1 2
/usr/bin/time -l target/release/telomere-cli --jit examples/add.wasm main 1 2      # jit build
/usr/bin/time -l target/release/telomere-cli examples/wasi-preview1-hello.wasm
/usr/bin/time -l target/release/telomere-cli component examples/wasi-component-args.wasm -- one
```

| Workload | Peak RSS (median of 5) |
| --- | ---: |
| `examples/add.wasm` core call, interpreter | 3,473,408 B (3,392 KiB) |
| `examples/add.wasm` core call, `--jit` | 3,571,712 B (3,488 KiB) |
| `examples/wasi-preview1-hello.wasm` preview1 command | 4,325,376 B (4,224 KiB) |
| `examples/wasi-component-args.wasm` WASI 0.2 component | 5,079,040 B (4,960 KiB) |

`examples/add.wasm` declares a 16-page (1 MiB) linear memory, so part of the core
row is the guest memory rather than runtime overhead.

The preview1 row is about 300 KiB higher than it was when this note was first
written. That is the fixture, not the runtime: `wasi-preview1-hello.wat` gained
the argv-sizing, memory-growth, and error-reporting logic it needs to be
correct, and the extra functions cost that much to parse, optimize, and
instantiate.

## Cold start

Cold start here is whole-process wall time: `exec` through exit, including
parsing, instantiation, and the guest call. Two independent methods were used
and agree.

Method 1, median of 30 runs after 3 warm-up runs, driven by
`tools/measure-cold-start.py`:

```shell
python3 tools/measure-cold-start.py
```

| Workload | Median | Min | Max |
| --- | ---: | ---: | ---: |
| `examples/add.wasm` core call, interpreter | 3.71 ms | 3.11 ms | 4.93 ms |
| `examples/add.wasm` core call, `--jit` | 3.25 ms | 3.02 ms | 3.53 ms |
| `examples/wasi-preview1-hello.wasm` preview1 command | 3.74 ms | 3.50 ms | 4.03 ms |
| `examples/wasi-component-args.wasm` WASI 0.2 component | 3.68 ms | 3.37 ms | 4.21 ms |

Repeating the whole sequence moves each median by roughly +/- 0.4 ms, so the
four workloads are not distinguishable from one another at this resolution.

Method 2, 100 sequential runs timed as a block and divided by 100, repeated
three times:

```shell
/usr/bin/time -p sh -c 'for i in $(seq 1 100); do \
  target/release/telomere-cli examples/add.wasm main 1 2 >/dev/null; done'
```

| Workload | Observed totals for 100 runs | Per run |
| --- | --- | ---: |
| `examples/add.wasm` core call | 0.33 s, 0.28 s, 0.30 s | ~2.8-3.3 ms |
| `examples/wasi-component-args.wasm` component | 0.34 s, 0.33 s, 0.33 s | ~3.3-3.4 ms |

Both methods include the operating system's process creation cost, so they are
an upper bound on the runtime's own start-up cost. They are not an in-process
instantiate-only measurement.

## Other runtimes

No other runtime was built or measured for this note. Two facts are quoted from
upstream sources so the size comparison in the repository README has something
checkable behind it:

- WAMR documents roughly 58.9K of text for its fast-interpreter core `vmlib`
  on Cortex-M4F. Source:
  <https://github.com/bytecodealliance/wasm-micro-runtime> (README, retrieved
  2026-08-01). It is a library text size under different target, libc, and
  standard-library conditions from the #139 artifacts.
- Wasmtime lists `component-model` and the WASI 0.2 interface proposals
  (`wasi-io`, `wasi-clocks`, `wasi-filesystem`, `wasi-random`, `wasi-sockets`,
  `wasi-http`) plus `wasi_snapshot_preview1` in its Tier 1 support table.
  Source: <https://docs.wasmtime.dev/stability-tiers.html> (retrieved
  2026-08-01).

The #139 `wasi − baseline` deltas (746388 B on macOS and 1039525 B on Linux)
do not make Telomere WAMR-class; closing that distance is still a goal. No
ratio to WAMR is reported, because a fair comparison requires building each
runtime from source with matched features, target, libc, standard library, and
optimization settings, which this note does not do.

The repository already has a separate, previously recorded execution-speed
comparison in [../core/coremark-benchmark.md](../core/coremark-benchmark.md).
That note was measured on a different date and is not re-measured here.

## Static linear-memory ceiling

Linear memories now reserve address space lazily: the configured maximum is
reserved, while only the module minimum is committed initially. Embedders can
set a per-memory ceiling before creating the store:

```rust
let mut runtime_config = telomere::RuntimeConfig::default();
runtime_config.memory.max_memory_pages = 256;
let store = telomere::Store::new_with_runtime_config(runtime_config);
```

The effective maximum is the lower of a module's declared maximum and this
ceiling; an unbounded memory uses the ceiling. `RuntimeConfig` and
`MemoryConfig` are intentionally `#[non_exhaustive]`, so this is a one-time
source break for direct struct literals: start from `Default` and mutate the
fields instead. Issue #126 may add further runtime-limit configuration without
requiring another literal migration.

The configured default is 65,536 pages (a 4 GiB reservation ceiling) on
64-bit targets and 4,096 pages (a 256 MiB reservation ceiling) on 32-bit
targets. These are configuration values, not measured RSS or committed-memory
results.

This makes a controlled static-ceiling measurement possible: fix the target,
module, and `max_memory_pages`, then measure process RSS and, on Linux, inspect
`Committed_AS` before and after instantiation. No RSS, `Committed_AS`, or
`memory.grow` latency values for this behavior have been measured in this
document. The inaccessible `PROT_NONE` tail is only a fail-fast guard if an
existing explicit bounds check regresses; it is not a host-containment boundary.

## Not yet measured

The following are open and should not be inferred from the numbers above:

- `riscv64gc-unknown-linux-gnu` binary size, RSS, and cold start. #140 must
  retain the `baseline`, `core`, `component`, `wasi`, `core-nosimd`,
  `core-jit`, and `wasi-threads` configuration names as its contract. The
  target is currently only cross-checked for compilation in CI; measuring it
  needs hardware or a QEMU user-mode setup.
- Linux `aarch64` numbers.
- Static memory ceiling under a constrained RAM budget (the 256 MiB class device
  target).
- Instantiation-only latency, separated from process start-up.
- `-Z build-std` and `panic_immediate_abort` footprint and behavior under the
  size-oriented profile.

## Reproducing

```shell
git submodule update --init --recursive
cargo build --release
python3 tools/measure-cold-start.py
python3 tools/measure-embedder-footprint.py --profile release
python3 tools/measure-embedder-footprint.py --profile release-size
```

The script lives in `tools/` rather than in this directory because `docs/` holds
prose, and because `cargo bench` only picks up `.rs` targets in a crate's
`benches/`, so a Python harness there would never run. The CLI harness prints
JSON and takes no arguments; it expects to be run from the repository root with
`target/release/telomere-cli` already built, and it aborts rather than reporting
a number if any invocation exits non-zero. The embedder harness takes its
profile explicitly, independently builds every selected configuration, verifies
its expected output, and likewise aborts rather than reporting a failed or
unparseable run. The CLI harness includes the `--jit` row only when
`TELOMERE_JIT_BIN` points at a binary built with `--features jit`:

```shell
cargo build --release --features jit
cp target/release/telomere-cli /tmp/telomere-cli-jit
cargo build --release
TELOMERE_JIT_BIN=/tmp/telomere-cli-jit python3 tools/measure-cold-start.py
```

The historical CLI tables in that section were filled in from a single run of
that sequence. The #139 embedder and Linux data have the separate environments,
commands, and JSON artifact records stated above.
