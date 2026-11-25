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
| `core-aot` | `core` with `simd,aot-load` (#200; load path without emitters) | `examples/add.wasm` → `3` |
| `wasi-threads` | `wasi` with `simd,threads,wasi` | `examples/wasi-component-args.wasm` → `0` |

The `baseline`, `core`, `component`, `wasi`, `core-nosimd`, `core-jit`, and
`wasi-threads` names are the configuration contract for
[#140](https://github.com/yieldspace/telomere/issues/140)'s RISC-V follow-up.
`core-aot` was added later by #200 and is gated by the same #164 mechanism.
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

### Platform text-delta attribution (#169)

Issue [#169](https://github.com/yieldspace/telomere/issues/169) asked why the
Linux x86_64 text deltas above are larger than the macOS arm64 ones, with
evidence rather than inference. This section records that attribution. The
macOS numbers are the #139 values recorded above; they were not re-measured
here. Everything else was measured on Linux (x86_64 host, Rust 1.96.0) at the
#139 measurement head `b0012a6dc87ce9323e076f74a5e687bb1c3d3d58`, in the
`release-size` profile, using the same per-configuration Cargo invocations as
`tools/measure-embedder-footprint.py`.

**Reproduction.** Rebuilt at `b0012a6` on Linux x86_64 (glibc), all seven
`.text` values reproduce the #139 Linux table byte-for-byte (baseline 228114,
core 732743, component 1139911, wasi 1267639, core-nosimd 659719, core-jit
958119, wasi-threads 1340151). The attribution below is therefore an
attribution of the recorded artifacts, not of a neighbouring build.

**Method.** Two extra target builds at the same head separate the factors:
`x86_64-unknown-linux-musl` (same ISA as the recorded Linux build, different
libc) and `aarch64-unknown-linux-musl` (different ISA, same ELF format and
linker family, static musl libc). Cross-target links used `rust-lld`; those
binaries were measured, not executed. Layer increments are the unit of
comparison, because the libc carried by the `baseline` configuration differs
between dynamic glibc and static musl and cancels in increments.

| Increment | x86_64 glibc (recorded) | x86_64 musl | aarch64 musl | macOS arm64 (recorded) |
| --- | ---: | ---: | ---: | ---: |
| `core - baseline` | 504629 | 505011 | 400320 | 381940 |
| `component - core` | 407168 | 407200 | 291824 | 282084 |
| `wasi - component` | 127728 | 128068 | 85984 | 82364 |
| `wasi - baseline` | 1039525 | 1040279 | 778128 | 746388 |
| `core - core-nosimd` (SIMD) | 73024 | 73040 | 43360 | 42068 |
| `wasi-threads - wasi` | 72512 | 72512 | 43952 | 42604 |
| `core-jit - core` (JIT) | 225376 | link failed | no backend | 185012 |

**Result.** The gap is mostly instruction-set code density, not OS, libc, or
file format. Splitting each recorded ratio into an ISA factor (x86_64-musl /
aarch64-musl) and a platform factor (aarch64-musl / macOS arm64):

| Increment | ISA factor | Platform factor | Product | Recorded ratio |
| --- | ---: | ---: | ---: | ---: |
| `core - baseline` | 1.261 | 1.048 | 1.322 | 1.32 |
| `component - core` | 1.395 | 1.035 | 1.444 | 1.44 |
| `wasi - component` | 1.489 | 1.044 | 1.555 | 1.55 |
| `wasi - baseline` | 1.337 | 1.043 | 1.394 | 1.39 |
| SIMD increment | 1.685 | 1.031 | 1.736 | 1.74 |
| Threads increment | 1.650 | 1.032 | 1.702 | 1.70 |

For the headline delta, the ISA factor accounts for about 88% of the gap on a
log scale and the platform factor about 13%. The libc factor on x86_64
(glibc / musl increments) is 0.997-1.000 in every row: nil. In absolute terms
static musl adds 70541 B to the `baseline` `.text` (its libc and libunwind,
which the dynamic glibc build does not carry), which is why absolute
cross-libc comparisons mislead and increments are used here.

The JIT increment is not decomposed: at this head there is no aarch64-Linux
JIT backend. `crates/telomere/src/runtime/jit/backend.rs` gates the aarch64
backend to macOS, and `telomere_jit_codegen::target::supported()` lists only
macOS/aarch64, macOS-or-Linux/x86_64, and Linux/riscv64-gnu, so no aarch64
ELF build containing JIT code exists to measure. (The aarch64-musl `core-jit`
cell, 618152 B, is *below* its `core` cell; it measures the
unsupported-target shape of the feature, not JIT code.) The recorded 1.22x
JIT ratio is a pair of supported targets and is not split here. The
x86_64-musl `core-jit` link also failed in this environment (`__clear_cache`
undefined under the self-contained musl link), so that cell is unmeasured.

**Per-symbol evidence, core layer.** `nm` attribution of an unstripped
`core` build (identical `.text` to the stripped artifact; symbol coverage
99.5% of `.text`) gives, on Linux x86_64: the `telomere` crate 396209 B
(54.1%) - `parser` 222395, `runtime` 160175, `common` 13131; `core`/`std`/
`alloc` together 196051 B (26.8%), of which the std backtrace machinery
(`gimli` 33816, `addr2line` 12977, `rustc_demangle` 19768, `miniz_oxide`
9570, plus `std::backtrace_rs` 43578) totals about 119709 B (16.3%);
`core::fmt` paths 33250 B (4.5%); panic handlers 3202 B (0.4%, expected
under `panic = "abort"`); `hashbrown` 13207 B; the embedder harness's own
code 21401 B. The largest single symbols are the parser and optimizer entry
points (`parse_inst` 40472 B, `optimize_function` 33234 B, `parse_module`
27413 B).

Comparing the same demangled symbols in the x86_64 and aarch64-musl `core`
binaries: 2258 shared symbols total 718782 B on x86_64 against 559716 B on
aarch64 (1.28x); for the `telomere` crate alone the shared-symbol ratio is
1.41x. The ratio grows with function size (functions of 4 KiB or more:
1.50x; under 64 B: 0.98x), consistent with fixed-length 4-byte A64 encoding
winning on small functions while x86_64 pays for longer encodings of 64-bit
immediates and addressing and for spill traffic in larger ones. The aarch64
build also contains about 1587 tiny LLVM machine-outliner functions (26164 B)
with no x86_64 counterpart, a deduplication mechanism that reduces aarch64
text. The std backtrace machinery (about 120 KB of the core layer on both
platforms) is the largest single reduction candidate this attribution
quantifies; removing it needs `-Z build-std` std feature control, already
listed under [Not yet measured](#not-yet-measured).

**Implications for unmeasured targets.**

- Linux aarch64 should look like macOS arm64, not like Linux x86_64: the
  OS/libc/format factor on increments is only 1.03-1.05, so a Linux aarch64
  embedder should land within a few percent of the macOS figures above. The
  platform axis is mostly an ISA axis.
- riscv64 should be expected near the x86_64 end, not the macOS end.
  Object-level evidence (the `telomere` crate only, `opt-level = "z"`, LTO
  disabled by profile override, per-function `.text` sections summed):
  x86_64 437332 B, aarch64 364496 B, riscv64 436656 B - riscv64 is 0.999x
  x86_64 and 1.20x aarch64 before LTO. This is not a linked-binary
  measurement: LTO, the layer mix, std, and the riscv64 JIT backend (which
  exists for riscv64-gnu) all move the final number, so
  [#140](https://github.com/yieldspace/telomere/issues/140)'s linked
  measurement is still required. The planning-relevant statement is the
  direction: budgeting a riscv64 appliance from the macOS numbers would
  under-budget by roughly the same factor seen on x86_64.
- 32-bit ARM is not measured or extrapolated here.

**Current-head stability.** The same decomposition at `c86bee7` (main when
this was measured) gives a headline ISA factor of 1.349 for the musl pair
(headline increments: x86_64 glibc 1091285 B, x86_64 musl 1092055 B,
aarch64 musl 809472 B), so the attribution is not an artefact of the #139
head.

**Not measured in this environment.** macOS re-measurement (no macOS host);
aarch64-linux-glibc linked builds (no aarch64 glibc sysroot); riscv64
linked builds (no riscv64 libc sysroot; the Rust 1.96 riscv64 musl std is
not self-contained and its link requires a real musl toolchain).

For the #138 change, the important conclusion is not a claimed megabyte saving:
the byte deltas above are modest comparative observations that make optional
layers visible. #138's value is the no-Tokio dependency topology and supported
embeddability boundary; the no-threads configurations prove that path.

### riscv64gc under QEMU user-mode (#140)

First measurement of `riscv64gc-unknown-linux-gnu`, the stated primary
product target. File and `.text` sizes are exact bytes of the riscv64
artifacts. RSS and cold-start values are **not** RISC-V hardware values:
the binaries were executed by `qemu-riscv64` user-mode emulation on an
x86_64 host, so both describe the emulator process (translation, sysroot
loading, and guest work combined). They establish a behavioural baseline
and a repeatable method, not an appliance performance figure.

| Measurement environment | Value |
| --- | --- |
| Date and host | 2026-11-25; Debian 12 x86_64 (Linux 6.1.0-55-amd64); 72 threads; 157 GiB |
| Toolchain and source | Rust 1.96.0 (repo pin); source head `c86bee7` (origin/main) |
| Emulation | qemu-riscv64 7.2.22 (Debian `qemu-user`); sysroot `/usr/riscv64-linux-gnu` (gcc-riscv64-linux-gnu 12.2.0); linker `riscv64-linux-gnu-gcc` |
| Method | `tools/measure-embedder-footprint.py --target riscv64gc-unknown-linux-gnu --runner "qemu-riscv64 -L /usr/riscv64-linux-gnu"`; same run counts as the host tables (cold median of 30 after 3 warmups, peak RSS median of 5 via `/usr/bin/time -v` around the emulator process) |

Behavioural signal (acceptance evidence, not footprint): the full
non-JIT workspace test suite was executed on the target with
`cargo test --release --workspace --target riscv64gc-unknown-linux-gnu`,
the Cargo runner above, and `QEMU_LD_PREFIX=/usr/riscv64-linux-gnu` set
so test-spawned subprocesses re-enter the emulator through binfmt with
the sysroot visible: **984 passed, 0 failed** (61 test binaries and 6
doc-test suites; the wast harness passed 166/166). Without
`QEMU_LD_PREFIX`, the six `cli_component` tests fail because the
binfmt-launched emulator cannot find the guest loader
(`/lib/ld-linux-riscv64-lp64d.so.1`) — an environment artifact, not a
product failure; all six pass with it. The suite found no product
defects on this target.

`core-jit` is sizes-only in both profiles: the build succeeds, but
executing the JIT embedder under qemu-user segfaults (exit status -11)
in both profiles. That is the already-tracked riscv64 JIT emulation
failure (#161); #140 does not require JIT execution.

#### `release` (stripped file size is primary)

| Configuration | Raw file (B) | Stripped file (B) | `.text` (B) | Peak RSS median of 5 (B) | Cold median of 30 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `baseline` | 446720 | 321072 | 224800 | 16433152 | 28.048 |
| `core` | 2257520 | 1679392 | 1211958 | 45309952 | 107.454 |
| `component` | 3429192 | 2626288 | 1971280 | 23580672 | 99.638 |
| `wasi` | 3721832 | 2855728 | 2135692 | 77258752 | 222.527 |
| `core-nosimd` | 1889896 | 1377968 | 987308 | 45232128 | 108.828 |
| `core-jit` | 2868960 | 2162912 | 1590780 | — (sizes only; #161) | — |
| `core-aot` | 2307696 | 1720456 | 1240720 | 45502464 | 111.697 |
| `wasi-threads` | 4039176 | 3120072 | 2324620 | 77393920 | 218.621 |

#### `release-size` (already stripped by profile)

| Configuration | File (B) | `.text` (B) | Peak RSS median of 5 (B) | Cold median of 30 (ms) |
| --- | ---: | ---: | ---: | ---: |
| `baseline` | 283856 | 201366 | 16375808 | 27.879 |
| `core` | 1029568 | 707916 | 44367872 | 108.343 |
| `component` | 1467848 | 1042286 | 19881984 | 85.106 |
| `wasi` | 1570296 | 1106904 | 75677696 | 207.309 |
| `core-nosimd` | 931264 | 636884 | 44515328 | 104.567 |
| `core-jit` | 1381888 | 958964 | — (sizes only; #161) | — |
| `core-aot` | 1066472 | 734296 | 44605440 | 103.772 |
| `wasi-threads` | 1689144 | 1189330 | 75657216 | 197.696 |

Cross-architecture observation, recorded without a cause: the riscv64
`release-size` `core` `.text` (707916 B) is slightly smaller than the
Linux x86_64 value (732743 B), and `core-jit` `.text` is at near parity
(958964 B vs 958119 B). The `wasi − baseline` text delta on riscv64
`release-size` is 905538 B, between the recorded macOS (746388 B) and
Linux x86_64 (1039525 B) values.

#### Default CLI (`release`)

Artifact: raw file 5440040 B; stripped file 4242840 B; `.text` 3071386 B
(strip applied to a copy under the fixed basename `telomere-cli`, per
the #139 basename note). Cases mirror `tools/measure-cold-start.py`
minus the JIT row, run under the same emulator; the `.tpre` artifact
was produced by the riscv64 CLI itself under emulation.

| Case | Cold median of 30 (ms) | Peak RSS median of 5 (B) |
| --- | ---: | ---: |
| `add.wasm` core call, interpreter | 330.01 | 112803840 |
| `wasi-preview1-hello.wasm` preview1 command | 374.87 | 115458048 |
| `wasi-component-args.wasm` WASI 0.2 component | 434.97 | 143941632 |
| `wasi-preview1-stock-hello.wasm` preview1 command | 1305.27 | 121274368 |
| `wasi-preview1-stock-hello.tpre` precompiled preview1 command | 350.04 | 114925568 |

## Size regression gate (#164)

#139 measured the minimal-embedder ladder but shipped no gate, because a
threshold such as "measured + 15%" would have been invented rather than
derived: nobody had observed the run-to-run variance of the measurement on
CI. #164 supplies that observation first and derives the threshold from it.

### Observed run-to-run variance

On 2026-11-25, the `Embedder footprint` workflow was run four
times against the code of PR #357: workflow_dispatch runs
45606680284, 45606698309 and 45606714950 against the PR branch at
head `b875a3196ba36583d4fd4e49b80fcf3891d75119` (the Codex P2 fix
on top of the #320 change —
resource type validation: structural type roots may carry handles
to abstract resources that are part of the surface, `(sub
resource)` export ascriptions mint fresh resource identities, and
own/borrow mismatches report the specific reason — merged with
current `main` at `b20ed79` (PR #353)), and pull-request run
45605539246, which
built the same tree (`ubuntu-24.04`, Rust 1.96.0). A three-run
control against unmodified `main` at `b20ed79` (workflow_dispatch
runs 45598772332, 45598789232 and 45598803018) isolates the
attribution recorded in the re-baseline history below. Every run
produced byte-identical sizes for its tree: for all sixteen
profile/configuration rows, the raw file size, the `.text` size,
the stripped file size, and the stripped `.text` size each have a
run-to-run spread of **0 B** across the samples. The table below
shows the checked-in baselines (stripped file /
stripped `.text`, in bytes): the six `release-size` series of the
three configurations that link `telomere-component` were
re-baselined to this campaign's branch measurements, and every
other baseline is unchanged — on all remaining series the branch
measured byte-identical to the checked-in baseline.

| Profile | Configuration | Stripped file (B) | Stripped `.text` (B) |
| --- | --- | ---: | ---: |
| `release` | `baseline` | 345,656 | 261,679 |
| `release` | `core` | 1,591,952 | 1,204,487 |
| `release` | `component` | 2,704,688 | 2,145,927 |
| `release` | `wasi` | 2,932,168 | 2,320,295 |
| `release` | `core-nosimd` | 1,400,840 | 1,059,319 |
| `release` | `core-jit` | 2,131,288 | 1,638,871 |
| `release` | `core-aot` | 1,637,928 | 1,237,751 |
| `release` | `wasi-threads` | 3,147,872 | 2,487,607 |
| `release-size` | `baseline` | 296,384 | 228,114 |
| `release-size` | `core` | 1,115,544 | 787,159 |
| `release-size` | `component` | 1,694,600 | 1,252,119 |
| `release-size` | `wasi` | 1,815,832 | 1,333,895 |
| `release-size` | `core-nosimd` | 1,000,752 | 707,607 |
| `release-size` | `core-jit` | 1,538,400 | 1,091,895 |
| `release-size` | `core-aot` | 1,154,608 | 812,359 |
| `release-size` | `wasi-threads` | 1,958,552 | 1,428,599 |

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

PR #264 (#263, const-folded load with effective address ≥ 2^32
does not trap (returns 0)) merged current `main` into its branch
first (merge commit `9e41714`, no conflicts); its first CI run
then tripped the gate on the PR's own change. The PR changes the
optimizer's three copies of `fold_const_base_memarg`
(`crates/telomere/src/parser/core/optimizer/pipeline/analysis.rs`,
`select.rs`, and `versioning.rs`) from `wrapping_add` to
`checked_add`, refusing the const-base specialisation when
base + offset overflows u32 so the generic op's checked runtime
address computation traps with `MemoryIndexOutOfRange`; its only
other changes are regression tests, which are not linked into
the embedder. Attribution was isolated by measuring current
`main` alone (workflow_dispatch run 45098248974, gate success),
which reproduces the previous baseline exactly on all 28 gated
series. The delta is a flat +16 B in stripped file and stripped
`.text` in every release configuration that links the telomere
library (`core`, `component`, `wasi`, `core-nosimd`, `core-jit`,
`wasi-threads`) and in the release-size `core`, `core-nosimd`,
and `wasi-threads` configurations — the few extra instructions
of the checked fold landing in the linked library code; the
release-size `component`, `wasi`, and `core-jit` configurations
are byte-identical, as is the `baseline` configuration, which
links no library code, in both profiles (10 of 28 gated series
unchanged). The baseline was re-measured on the merged code and
updated as recorded above, following the procedure below.

PR #265 (#197, record relocations in the JIT emitters) merged
current `main` into its branch first (merge commit `83d2372`,
no conflicts); its first CI run then tripped the gate on the
PR's own change. The PR generalises the three baseline JIT
emitters over a monomorphised `EmitEnv`: the JIT implementation
reproduces the previous behaviour from the live store, while
the AoT implementation supplies recipes from the parsed
`Module` and records stub addresses and funcaddr immediates as
placeholders plus a relocation table
(`RelocKind::{Stub(StubId), FuncAddr(funcidx)}`), with a StubId
table and fingerprint as the single source of truth for the 44
stubs; the patcher rewrites each site at the same length after
verifying the placeholder. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run
45112486937, gate success), which reproduces the previous
baseline exactly on all 28 gated series. Only the `core-jit`
configuration links `telomere-jit-codegen` into the minimal
embedder, so the delta is confined to its four gated series
(release stripped file +2,552 B / stripped `.text` +848 B;
release-size stripped file +720 B / stripped `.text` −672 B —
the release-size `.text` shrink is the code-layout effect of
the emitter refactor under that profile's optimisation
settings) and the other 24 gated series are byte-identical.
The baseline was re-measured on the merged code and updated as
recorded above, following the procedure below.

PR #266 (#209, per-function JIT trap-site tables) merged
current `main` into its branch first (merge commit `41dc4b0`);
the merge conflicted in the three backend files and
`backend.rs`, because #197 had generalised the same emitters
over `EmitEnv` while #209 changed their trap exits and return
type. The resolution keeps #197's structure: the per-arch
emitters stay generic over `EmitEnv`, their `Emission` result
gains the trap-site table, `Emitter::finish()` returns the
code plus the table, and `return_trap` keeps recording sites
and packing the exit value (`code | site << 8`) in both modes.
The PR itself records, for every native trap site, the
decoded-stream pc index of the emitting instruction in a
per-function table, resolves the site against the trapping
function's own table at the innermost JIT entry boundary, and
charges the table bytes to the code-cache budget. Attribution
was isolated by measuring current `main` alone
(workflow_dispatch run 45115401237, gate success), which
reproduces the previous baseline exactly on all 28 gated
series. The growth is confined to the `core-jit`
configuration, the only one that links the JIT backends into
the minimal embedder (release stripped file +2,384 B /
stripped `.text` +2,128 B; release-size stripped file +880 B /
stripped `.text` +816 B), plus a +16 B code-layout shift in
the release-size `component` configuration (stripped file and
stripped `.text`), attributable to the `ExecuteContext` field
the PR appends in the shared runtime — the only part of the
change linked into non-JIT configurations. The other 25
gated series are byte-identical. The baseline was re-measured
on the merged code and updated as recorded above, following
the procedure below.

PR #267 (#200, split the AoT load path from the JIT emitters)
merged current `main` into its branch first (merge commit
`d5acd0b`); its first CI run then tripped the gate on the
merged tree. The PR adds a non-default `aot-load` feature to
the telomere crate (store code cache, runtime stubs,
native-call ABI and executable code memory, but no compiler)
and makes `jit` imply it plus the codegen crate's new `emit`
feature for the assembler/emitter layer; in an aot-load-only
build a compile request consults the cache only and otherwise
reports the typed `VMResult::Unimplemented` so the
interpreter takes over. It also adds the `core-aot`
configuration to the footprint ladder and to this gate, and a
symbol checker (`tools/check-aot-load-symbols.py`) asserting
emitter symbols are absent from an aot-load-only embedder.
The merge conflicted in the module declarations of
`runtime/jit.rs`, because #197 had added emitter-side modules
while this PR reclassified the load-side ones; the resolution
keeps the PR's classification (stubs and profile are load
side, `aot-load`; `emit_env` and the reloc tests are emitter
side, `jit`). Two follow-up fixes were needed: #209's
`resolve_trap_site`, whose only caller is the aot-load entry
bridge, is gated `aot-load` instead of `jit` (body unchanged),
and the PR's corpus measurement harness reads
`EmittedFunction.code` now that #209 returns the code plus
the trap-site table. Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45120292330,
gate success), which reproduces the previous baseline exactly
on all 14 gated series main measures (main has no `core-aot`
configuration). The deltas are confined to the two
configurations that link the native-code machinery and are
pure code-layout shift from the cfg refactor: `core-jit`
(release stripped file +336 B / stripped `.text` +304 B;
release-size stripped file +32 B / stripped `.text` +16 B)
and `core-aot` (release stripped file +88 B / stripped
`.text` +80 B; release-size stripped file +56 B / stripped
`.text` +80 B, against the first values this PR recorded for
the new series from a fleur measurement; the CI-measured
values recorded here supersede those). The other 12 of the
16 gated series are byte-identical. The baseline was
re-measured on the merged code and updated as recorded
above, following the procedure below.

PR #268 (#241, component model value index space: `Sort::Value`, sort
0x02, value imports/exports) was branched on current `main`, so its
first CI run tripped the gate on the PR's own change. The PR implements
the value index space end to end in `telomere-component` (decode,
validation, instantiation and link-time resolution of value
imports/exports, static literal decoding of the value section, and the
linker's `register_import_value`). A follow-up commit answers a Codex P1
review finding: a list whose element type has an empty encoding decoded
its element count without consuming any input bytes, so a 26-byte
component could materialise billions of values and abort the embedding
process on allocation; the value decoder now charges every node
materialised without consuming input bytes against a zero-width budget
(`ValueBudget`) and checks list element counts against the remaining
encoding bytes plus that budget before any element is materialised.
Attribution was isolated by measuring current `main` alone
(workflow_dispatch run 45138848073, gate success), which reproduces the
previous baseline exactly on all 32 gated series. The growth is confined
to the three configurations that link the component runtime: `component`
(release stripped file +50,568 B / stripped `.text` +44,720 B;
release-size stripped file +28,664 B / stripped `.text` +25,024 B),
`wasi` (release +48,056 B / +42,640 B; release-size +28,856 B / +25,216
B) and `wasi-threads` (release +48,040 B / +42,640 B; release-size
+28,824 B / +25,200 B). The other 20 gated series are byte-identical.
Against the original #164 baseline, the largest cumulative growth is now
release-size `component` stripped `.text` at +5.05% (1,169,031 B to
1,228,023 B), up from +3.54%. The baseline was re-measured on the branch
code and updated as recorded above, following the procedure below.

PR #269 (#216, replacing the raw host-linking ABI with a designed
public host API: `telomere::host_abi` removed in favour of
`HostContext` / `HostFn` / `HostModule` in
`crates/telomere/src/host.rs`, the raw surface retained only behind
`unstable-internals`) merged current `main` into its branch first; its
first CI run on the merged tree then tripped the gate on the PR's own
change. Four follow-up commits answer Codex review findings on the new
boundary: the adapter future no longer touches the execution stack
(completions resolve to values carried in an opaque outcome, and the
scheduler validates and writes the return slot against the
submission-time frame reference); completions are bound to the
execution that submitted them by a process-wide monotonic task
identity, widened to u64 so the counter cannot wrap and recycle an
identity a retained completion could match; and the raw code-pointer
accessor matches `FunctionBody::Host` explicitly and returns `None`
for public callbacks instead of reaching `unreachable!`. Attribution
was isolated by measuring current `main` alone (workflow_dispatch run
45155688403, gate success), which reproduces the previous baseline
exactly on all 32 gated series. Because the telomere library is linked
into every embedder configuration except `baseline`, the growth covers
the seven library configurations and `baseline` is unchanged: `core`
(release stripped file +14,896 B / stripped `.text` +12,048 B;
release-size stripped file +8,088 B / stripped `.text` +5,312 B),
`core-nosimd` (+14,912 B / +12,048 B; +8,152 B / +5,376 B), `core-jit`
(+14,784 B / +11,952 B; +8,088 B / +5,328 B), `core-aot` (+14,640 B /
+11,808 B; +8,152 B / +5,376 B), `component` (+11,648 B / +9,056 B;
+6,840 B / +4,240 B), `wasi` (+11,616 B / +9,056 B; +6,840 B / +4,224
B) and `wasi-threads` (+12,064 B / +9,456 B; +6,936 B / +4,400 B).
Against the original #164 baseline, the largest cumulative growth is
now release-size `component` stripped `.text` at +5.41% (1,169,031 B
to 1,232,263 B), up from +5.05%. The baseline was re-measured on the
merged code and updated as recorded above, following the procedure
below.

PR #273 (#171, execution metering covers JIT-compiled execution: the
#127 normalisation that warned and disabled the JIT for metered stores
is removed, native checkpoints are emitted at guest loop headers and
fused-loop back-edges in all three JIT backends, a
`MeteringCheckpoint` stub refills or raises through `vm::checkpoint`,
and the bulk-memory, table and shared-notify stubs charge with the
interpreter’s formulas) merged current `main` into its branch
first; its first CI run on the merged tree then tripped the gate on
the PR’s own change. Attribution was isolated by measuring current
`main` alone (workflow_dispatch run 45160951447, gate success), which
reproduces the previous baseline exactly on all 32 gated series. The
growth is confined to `core-jit`, the only configuration linking the
JIT (release stripped file +6,280 B / stripped `.text` +5,248 B;
release-size +4,744 B / +3,840 B): the per-backend checkpoint
emitters, the new stub and the stub charge calls; code emitted for
unmetered stores is byte-identical by construction. In `release` the
other library configurations shrink slightly (−144 B stripped file
and `.text` in `core`, `component`, `wasi`, `core-nosimd` and
`wasi-threads`; −176 B in `core-aot`), the code-layout effect of
removing the normalisation block from the store constructor, while in
`release-size` every configuration except `core-jit` is
byte-identical and `baseline` is unchanged in both profiles. Against
the original #164 baseline, the largest cumulative growth remains
release-size `component` stripped `.text` at +5.41% (1,169,031 B to
1,232,263 B), unchanged, because that series does not move here. The
baseline was re-measured on the merged code and updated as recorded
above, following the procedure below.

PR #275 (#271, host-export binding lookup retains the resolved
(version-matched) export name: `RuntimeComponentInstance::resolve_export`
keeps the key returned by `resolve_component_export_key` in scope and
passes it to `ComponentLinker::resolve_export`, instead of discarding
it and looking the binding up under the requested name) merged current
`main` into its branch first; the merge conflicted in
`crates/telomere-component/src/runtime/env.rs`, where `main` (PR #268)
had added a `Value` arm to the same match, and the resolution keeps
the branch’s block structure and keyed lookup with the `Value` arm
added inside it. The first CI run on the merged tree passed the gate,
because every delta is negative. Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45165921937, gate
success), which reproduces the previous baseline exactly on all 32
gated series. The shrinkage is confined to the three configurations
that link the component runtime (`component`, `wasi`, `wasi-threads`):
−48 B stripped file and stripped `.text` in `release`, and in
`release-size` −16 B in `component` and `wasi` and −32 B in
`wasi-threads`, the code-layout effect of restructuring the
`Component` arm of `resolve_export` from a match expression into a
block with an early return. The other 20 gated series are
byte-identical. Against the original #164 baseline, the largest
cumulative growth remains release-size `component` stripped `.text`
at +5.41% (1,169,031 B to 1,232,247 B), unchanged at two decimal
places, because that series moves by only −16 B here. The baseline
was re-measured on the merged code and updated as recorded above,
following the procedure below.

PR #276 (#270, atomic.wait timer threads are released when the wait
resolves: the detached timer thread spawned per finite-timeout shared
wait slept until the original deadline even after notification or
interrupt resolved the wait, and registrations already resolved
before polling still spawned one; `SharedWaiter` now carries a
release (`Mutex`, `Condvar`) pair signalled by the successful CAS
claim in `try_mark_notified` / `try_mark_interrupted`, the timer
sleeps in `wait_timeout` and exits as soon as the wait is resolved,
and already-resolved registrations spawn no thread) merged current
`main` into its branch first; the merge was conflict-free (main's
only change to `crates/telomere/src/common/memory.rs` was the
`push_to_stack` visibility narrowing from PR #269, disjoint from the
wait-timer code). Its first CI run on the merged tree then tripped
the gate on the PR's own change. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run 45169608064,
gate success), which reproduces the previous baseline exactly on
all 32 gated series. The growth is confined to `wasi-threads`, the
configuration with the threads feature that compiles the shared-wait
timer code (release stripped file +1,608 B / stripped `.text`
+1,280 B; release-size +1,088 B / +800 B). The other 30 gated
series are byte-identical. Against the original #164 baseline, the
largest cumulative growth remains release-size `component` stripped
`.text` at +5.41% (1,169,031 B to 1,232,247 B), unchanged, because
that series does not move here. The baseline was re-measured on the
merged code and updated as recorded above, following the procedure
below.

PR #280 (#128, internal-invariant violations abort the host: panics
and unwraps reachable from module, guest and link input in
instantiation, linking, `call_indirect` and the async scheduler are
converted to fail-closed error returns, and the 33 `expect` /
`unreachable!` sites in the fused-op evaluation path ride the
existing `Option` chain into `InvalidOperand`) merged current `main`
into its branch first; the merge conflicted in
`runtime/scheduler.rs` (a test-module union — the branch's WasmAsync
tests and `main`'s u64 task-id boundary test are both kept) and in
`tests/public_api_fail_closed.rs`, and one auto-merged asymmetry had
to be repaired by hand: the public host-linking implementations that
PR #269 (#216) rewrote in `runtime/instantiate.rs` still indexed and
unwrapped on embedder input, so the branch's fail-closed conversions
were ported to the four public link implementations (the raw
counterparts already carried them); the two regression tests were
ported to the public API in the process. The first CI run on the
merged tree passed the gate, because every delta is negative.
Attribution was isolated by measuring current `main` alone
(workflow_dispatch run 45176281284, gate success), which reproduces
the previous baseline exactly on all 32 gated series. Removing
panic paths and their formatting machinery shrinks all seven library
configurations in both profiles (release stripped file
−3,352..−3,984 B and stripped `.text` −528..−720 B; release-size
stripped file −3,144..−3,688 B and stripped `.text` −416..−688 B);
the `baseline` configuration, which links no library code, is
unchanged. Against the original #164 baseline, the largest
cumulative growth remains release-size `component` stripped `.text`
at +5.36% (1,169,031 B to 1,231,735 B), down from +5.41%. The
baseline was re-measured on the merged code and updated as recorded
above, following the procedure below.

PR #281 (#279, the parser validates kind-6 element segments
against table 0 instead of the declared table: `parse_elem`
parsed the segment's `tableidx` and checked the table exists,
but passed a literal `0` to `validate_active_elem`, so a segment
declared for another table was type-checked against table 0's
element type — an externref segment declared for a funcref
table was accepted, and a funcref segment for such a table was
rejected) merged current `main` into its branch first; the
merge was conflict-free. Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45184187750, gate
success), which reproduces the previous baseline exactly on
all 32 gated series. The one-line fix (passing the parsed
`tableidx`) moves 14 of the 32 gated series by exactly +16 B
each, in both the stripped file and stripped `.text` sizes,
confined to the configurations that link the parser (`release`
`component` / `wasi` / `wasi-threads`; `release-size` `core` /
`component` / `core-jit` / `wasi-threads`); the remaining 18
series are byte-identical. Against the original #164 baseline,
the largest cumulative growth remains release-size `component`
stripped `.text` at +5.37% (1,169,031 B to 1,231,751 B), up
from +5.36%. The baseline was re-measured on the merged code
and updated as recorded above, following the procedure below.

PR #283 (#277, a long `f64.const` loop aborts the host with a
stack overflow under the interpreter: the #240 identity barrier
in `op_f64_const` passed the address of its function-local
static to `black_box`, which materialises its argument in an
address-taken stack slot, and that slot made LLVM drop the
sibling call for the dispatch, so every execution left a live
host frame behind — the f64-kernel aborted at n = 1,000,000,
with the overflow threshold at n ≈ 258,683; the fix replaces
the barrier in that one handler with a volatile load of the
same static, which keeps the body distinct from `op_i64_const`
but leaves no frame state, and the `Op` contract comment now
records the hazard) merged current `main` into its branch
first; the merge was conflict-free (`main` does not touch
`numeric.rs`, and the two sides' `common.rs` changes are in
disjoint regions). Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45189182011, gate
success), which reproduces the previous baseline exactly on
all 32 gated series. 9 of the 32 gated series change: the
`release` stripped file size grows by exactly +16 B in `core`,
`component`, `wasi`, `core-aot` and `wasi-threads` (stripped
`.text` unchanged there), and `release-size` `component` and
`wasi-threads` shrink by exactly −16 B in both the stripped
file and stripped `.text` sizes; the remaining 23 series are
byte-identical. Against the original #164 baseline, the
largest cumulative growth remains release-size `component`
stripped `.text` at +5.36% (1,169,031 B to 1,231,735 B), down
from +5.37%. The baseline was re-measured on the merged code
and updated as recorded above, following the procedure below.

PR #287 (#285, all 264 identity-barrier handlers dispatch
non-tail: the #240 identity barrier at the 41 remaining source
sites — 38 in `memory.rs`, 2 in `simd.rs` and 1 in
`numeric.rs` (`op_i32_crc16_update16_masked`) — is replaced
with the volatile-load form proven in #277/#282, restoring
tail dispatch for the whole handler set; `op_return` keeps
its `black_box` as the one intentional exception because its
dispatch target is terminal) merged current `main` into its
branch first; the merge was conflict-free, and
`op_f64_const`'s volatile load from PR #283 is present
exactly once afterwards (full disassembly audit of the
merged tree: 261 TAIL / 1 NONTAIL, the only NONTAIL being
`op_return`). Attribution was isolated by measuring current
`main` alone (workflow_dispatch run 45196725415, gate
success), which reproduces the previous baseline exactly on
all 32 gated series. 28 of the 32 gated series change; only
the `baseline` configuration is byte-identical in both
profiles. In `release`, stripped `.text` grows by +1,728 B
in `core`, `component`, `core-aot`, `core-nosimd` and `wasi`
and by +2,112 B in `wasi-threads`, while `core-jit` shrinks
by −2,368 B. In `release-size`, stripped `.text` grows by
+1,248–+2,000 B and stripped file by +8,064–+9,856 B across
the library configurations. Against the original #164
baseline, the largest cumulative growth remains release-size
`component` stripped `.text` at +5.47% (1,169,031 B to
1,232,999 B), up from +5.36%. The baseline was re-measured
on the merged code and updated as recorded above, following
the procedure below.

PR #286 (#282, `op_ref_is_null` has the same non-tail dispatch
as `op_f64_const`: the #240 identity barrier in `refs.rs` is
replaced with the volatile-load form proven in #277,
restoring tail dispatch) merged current `main` into its
branch first; the only conflict was in
`tests/call_threading.rs`, where both sides had appended a
same-shaped loop test at the same location (`main`'s
`f64.const` test from #277 and this branch's `ref.is_null`
test), resolved by keeping both tests. After the merge the
replacement is present exactly once, the only remaining
`black_box` barrier is `op_return`'s intentional exception,
and a full disassembly audit of the merged tree shows 261
TAIL / 1 NONTAIL among the named barrier handlers (the only
NONTAIL being `op_return`), with `op_ref_is_null` verified
TAIL by direct disassembly. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run
45206965424, gate success), which reproduces the previous
baseline exactly on all 32 gated series. 18 of the 32 gated
series change, all by ±16 B except one at −32 B: in
`release`, stripped file grows by +16 B in `core`,
`component`, `core-aot`, `core-nosimd`, `wasi` and
`wasi-threads` and stripped `.text` is unchanged; in
`release-size`, stripped file shrinks by −16 B to −32 B
in the library configurations except `core-jit` (+16 B),
and stripped `.text` shrinks by −16 B in `component`,
`core`, `core-aot`, `core-nosimd` and `wasi-threads`. The
`baseline` configuration is byte-identical in both profiles.
Against the original #164 baseline, the largest cumulative
growth remains release-size `component` stripped `.text` at
+5.47% (1,169,031 B to 1,232,983 B), unchanged from +5.47%.
The baseline was re-measured on the merged code and updated
as recorded above, following the procedure below.

PR #284 (#147, the component async registration surface
states its first-poll contract: canonical contract text on
`ComponentFuture`, first-poll warnings on all six `*_async`
registration methods and the bindgen traits/functions, a
corrected false doc claim on `register_func_async`, and a
self-explanatory `Pending` error message; the decision and
roadmap are recorded in
`docs/component-model/new-component-runtime.md`) merged
current `main` into its branch first, with no conflicts.
Attribution was isolated by measuring current `main` alone
(workflow_dispatch run 45219214659, gate success), which
reproduces the previous baseline exactly on all 32 gated
series. Only 6 of the 32 gated series change, all in
stripped file bytes with stripped `.text` unchanged
everywhere: `release` `component` and `wasi` grow by
+208 B and `wasi-threads` by +224 B; `release-size`
`component` grows by +208 B and `wasi` and `wasi-threads`
by +224 B. The growth is confined to the configurations
that link the component runtime and carries no `.text`
change, consistent with the new contract/warning string
literals this PR adds to `telomere-component`. The
remaining 26 series, including the `baseline`
configuration in both profiles, are byte-identical.
Against the original #164 baseline, the largest cumulative
growth remains release-size `component` stripped `.text` at
+5.47% (1,169,031 B to 1,232,983 B), unchanged from +5.47%.
The baseline was re-measured on the merged code and updated
as recorded above, following the procedure below.

PR #288 (#144, `release_max_level_off` in `crates/telomere`'s
tracing dependency statically disabled release tracing for
the embedder's whole dependency graph; the feature is
removed and the runtime diagnostics are published as
structured tracing events on the documented `telomere::trap`
/ `telomere::jit` / `telomere::stack` targets) merged
current `main` into its branch first, with no conflicts.
The branch also carries the Codex P2 fix (`39cdc18`) keeping
the JIT compile-trace stderr cap from suppressing the new
structured events. Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45250773315,
gate success), which reproduces the previous baseline
exactly on all 32 gated series, so every delta below is
this PR's own. The deltas are the disclosed cost of the
fix: with the max-level feature gone, the tracing callsites
that were statically erased from release builds exist
again. In `release`, every configuration that links the
telomere library grows: stripped file +90,336 B in `core`
to +152,368 B in `wasi-threads`, stripped `.text`
+52,752 B in `core-nosimd` to +101,200 B in
`wasi-threads`. The `baseline` configuration, which links
no telomere code, is byte-identical in both profiles. In
`release-size` the size optimizer still eliminates most of
the unused callsite machinery: the core configurations
shrink slightly (stripped file −1,856 B to −1,104 B),
`component` and `wasi` grow by +272 B stripped file
(+64 B and +80 B stripped `.text`), and `wasi-threads`
grows by +192 B stripped file with stripped `.text`
unchanged. Against the original #164 baseline, the largest
cumulative growth is now the `release` `component`
stripped file series at +9.50% (2,433,656 B to
2,664,760 B), up from +5.47%; the `release-size`
`component` stripped `.text` series moves from +5.47% to
+5.48% (1,169,031 B to 1,233,047 B). The baseline was
re-measured on the merged code and updated as recorded
above, following the procedure below.

PR #289 (#126, embedder-configurable resource limits:
`RuntimeConfig` gains `LimitsConfig` — the store-wide
total-memory ceiling, a per-table element cap, the guest
value-stack size, and the JIT code-cache ceiling folded
in from `JitConfig`) merged current `main` into its branch
first, with no conflicts. The merged tree's first CI run
failed clippy, feature coverage, and both JIT-test jobs
on a single stale field reference: the `jit_code_cache`
regression test added by PR #288 still set the code-cache
ceiling through `JitConfig::code_cache_max_bytes`, which
this PR moves to `LimitsConfig`; the test now sets
`limits.max_code_cache_bytes`. Review of this PR then
found that the total-page ceiling, checked in the store's
grow dispatch, did not cover host callbacks: a callback
growing memory through the public `Memory` handle handed
out by the `HostContext` accessors never passes through
that dispatch, and with a one-page cap its `grow(1)`
succeeded and the guest observed two pages. The ceiling
now lives in `Memory::grow` itself, charged against a
shared `TotalMemoryBudget` that every memory registers
with when the store adopts it; regression tests cover the
caller accessors and a host function linked into a
memory-owning instance. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run
45266215787, gate success), which reproduces the previous
baseline exactly on all 32 gated series, so every delta
below is this PR's own. The deltas are the cost of the
limits machinery in the telomere library: in `release`,
every configuration that links the library grows —
stripped file +2,056 B in `component` and `core-nosimd`
to +3,240 B in `wasi-threads`, stripped `.text` +1,664 B
to +2,912 B — largest in `wasi-threads`, where the
shared-memory budget paths land. The `baseline`
configuration, which links no telomere code, is
byte-identical in both profiles. In `release-size` the
growth is smaller and near-uniform: stripped file
+1,120 B to +1,280 B, stripped `.text` +848 B to
+1,056 B. Against the original #164 baseline, the largest
cumulative growth is now the `release` `component`
stripped file series at +9.58% (2,433,656 B to
2,666,816 B), up from +9.50%. The baseline was
re-measured on the merged code and updated as recorded
above, following the procedure below.

A review follow-up on PR #289 re-baselined the gate again.
The total-page ceiling was still being enforced inside the
instantiation loop that creates a module's defined memories
one by one, so a module whose aggregate minimum exceeded
the ceiling was refused only after its earlier memories had
already been created and adopted by the store — and since
memories are never removed, their pages permanently
consumed the store total. Instantiation now preflights the
aggregate minimum of the defined memories before creating
any of them; imported memories already live in the store
and are already counted in the total, so they are not
summed again, and a refused module leaves the store total
unchanged. A regression test covers a two-memory module
whose minima sum past the cap, followed by a module that
uses the whole cap. Attribution was isolated by measuring
current `main` alone (workflow_dispatch run 45277043309,
gate success), which reproduces the pre-#126 baseline
exactly on all 32 gated series; the only code change since
the previous baseline on this branch is the preflight
commit, so every delta below is that commit's own. In
`release`, every configuration that links the library
shrinks slightly — stripped file −48 B in `component` and
`wasi` to −544 B in `core-jit`, stripped `.text` −32 B to
−512 B — as the preflight replaces the per-memory check
inside the creation loop; the `baseline` configuration is
byte-identical in both profiles. In `release-size` the
deltas are small and mixed: stripped file −144 B in
`core-aot` to +240 B in `core`, stripped `.text` −128 B to
+256 B. Against the original #164 baseline, the largest
cumulative growth remains the `release` `component`
stripped file series at +9.58% (2,433,656 B to
2,666,768 B), unchanged to two decimals. The baseline was
re-measured on the branch and updated as recorded above,
following the procedure below.

A further review follow-up on PR #289 re-baselined the gate
again; two findings remained in the same area. First, with
both the total-page and table-element ceilings configured,
instantiation created and adopted the module's memories
before it ever checked the defined tables, so a module
refused for an oversized table left its memories — and
their charged pages — in the store permanently. Every
limit check instantiation performs now completes before any
memory or table is created: the defined-table minima are
preflighted beside the memory aggregate, and the memory
aggregate itself is no longer a read-then-register pair.
Second, the preflight compared a read of the store total
against the ceiling while creation registered initial pages
with an unconditional addition; a shared memory can grow
through a retained handle without taking the store's
runtime lock, so a racing growth could claim the last page
between the check and the registration and push the total
past the ceiling. The preflight is now a single atomic
claim on the shared page budget — a reservation the
creation loop pays each memory out of, releasing any
unspent remainder when creation fails — and adoption
itself charges initial pages through the same claim,
refusing without residue when the budget is full.
Regression tests cover a table-refused module followed by a
module that uses the whole memory cap, budget claims under
contention stopping exactly at the cap, and concurrent
shared growth racing adoptions. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run
45285648242, gate success), which reproduces the pre-#126
baseline exactly on all 32 gated series; the only code
change since the previous baseline on this branch is the
reservation commit, so every delta below is that commit's
own. In `release` the library configurations grow slightly
(stripped file +48 B in `core-aot` and `core-jit` to +368 B
in `component`; stripped `.text` −32 B in `core-aot` and
`core-jit` to +256 B), and in `release-size` stripped file
grows +128 B in `core-nosimd` to +528 B in `core-jit`
(stripped `.text` +64 B to +432 B); the `baseline`
configuration is byte-identical in both profiles. Against
the original #164 baseline, the largest cumulative growth
remains the `release` `component` stripped file series, now
at +9.59% (2,433,656 B to 2,667,136 B). The baseline was
re-measured on the branch and updated as recorded above,
following the procedure below.

PR #290 (#190, the `jit` feature tax: compiling the `jit` feature
slowed the interpreter even when the JIT never ran) moves the JIT
interpreter stop target out of the `JIT_INTERPRETER_STOP_AT`
thread-local into a new trailing field of `ExecuteContext`
(`jit_stop_pc`). `JitInterpreterStopGuard` now saves and restores
that field through a slot pointer, keeping the nesting semantics,
and the per-dispatch check in the dispatch helper and the
return/branch handlers compares the current pc against the ctx
field in a single equality. On Darwin the thread-local read went
through a TLV thunk call on every dispatch; the field read removes
that call. The branch is based directly on current `main`
(`b39b116`), which had not moved since the branch was cut, so the
pull-request run already built the final tree and no merge commit
was needed. Attribution was isolated by measuring current `main`
alone (workflow_dispatch run 45308616450, gate success), which
reproduces the previous baseline exactly on all 32 gated series,
so every delta below is this PR's own. 28 of the 32 gated series
change; the `baseline` configuration, which links no telomere
code, is byte-identical in both profiles. In `release`, the
non-JIT library configurations shrink by 16 B (stripped file and
stripped `.text` alike), `core-jit` shrinks by 10,240 B, and
`core-aot` grows by 15,536 B stripped file / 12,512 B stripped
`.text`. In `release-size`, `core`, `component`, `wasi`,
`core-nosimd` and `wasi-threads` grow by 64..80 B stripped file
(48..80 B stripped `.text`), `core-jit` shrinks by 18,272 B
stripped file / 18,224 B stripped `.text`, and `core-aot` grows
by 19,568 B / 13,600 B. Against the original #164 baseline, the
largest cumulative growth remains the `release` `component`
stripped file series, at +9.59% (2,433,656 B to 2,667,120 B),
unchanged in rounded terms. The baseline was re-measured on the
branch and updated as recorded above, following the procedure
below.

PR #293 (#191, JIT-to-JIT calls round-tripping through the
runtime with a per-call mutex) makes native-to-native calls enter
the compiled callee directly: after the ordinary frame
preparation, the call helpers resolve the callee through the
per-store entry table (the `StoreInner` funcaddr slots, upgraded
from `Weak`) and enter it via `enter_compiled_from_jit_call`,
falling back to the previous lazy-compile path on a miss. The
per-call cache mutex touch is gone: recency lives in an
`AtomicU64` stamp on `CompiledFunction`, advanced on every direct
call but only rewritten every 32 ticks. The TLS entry-depth
counter (the 256 nested-entry cap) is removed; native recursion
depth is bounded by the same value-stack accounting as
interpreter recursion (#166, #126 limits). The branch was cut
from `main` at `b39b116` before PR #290 (#190) merged, so current
`main` (`41bca95`) was merged in before measurement (merge commit
`159796d`); the merge combines #191's removal of the entry-depth
thread-local with #190's move of the interpreter stop target
into `ExecuteContext`, and both thread-locals are gone from the
merged tree. Attribution was isolated by measuring current
`main` alone (workflow_dispatch run 45320550102, gate success),
which reproduces the previous baseline exactly on all 32 gated
series, so every delta below is this PR's own. Only 8 of the 32
gated series change — the two configurations that link the JIT
entry code. In `release`, `core-jit` grows by 1,152 B stripped
file / 976 B stripped `.text` and `core-aot` shrinks by 560 B /
448 B. In `release-size`, `core-jit` grows by 272 B / 240 B and
`core-aot` shrinks by 448 B / 304 B. Against the original #164
baseline, the largest cumulative growth remains the `release`
`component` stripped file series, unchanged at +9.59%
(2,433,656 B to 2,667,120 B). The baseline was re-measured on
the branch and updated as recorded above, following the
procedure below.

PR #293 was re-baselined a second time after the Codex P1 fix
(`de0de8f`) and the merge of current `main` (`1b25df0`, PR #291;
merge commit `1fa6db7`). The P1 fix bounds native entry by the
host stack: removing the TLS entry-depth counter had left
recursion depth bounded only by the value-stack reservation,
which does not charge the real host stack each native level
spends, so on a 1 MiB host thread deep guest recursion aborted
the process before the value-stack bound was reached. Both
native entry boundaries (generic `enter_current_frame_raw` and
direct `enter_compiled_from_jit_call`) now compare the current
stack pointer against a floor recorded on the `ExecuteContext`
(the executing thread's OS stack low bound plus a 256 KiB
reserve, established lazily at first entry, with a fixed
512 KiB budget as fallback) and decline the entry through the
existing fallback results when the remaining stack runs low,
so the frame runs in the interpreter and deep recursion reaches
the same value-stack `StackOverflow` on any host stack size.
No TLS and no depth counter are reintroduced. Attribution was
isolated by measuring current `main` alone (workflow_dispatch
run 45327968623, gate success), which reproduces the pre-#191
baseline exactly on all 32 gated series, so every delta below
is this PR's own. Against `main`, 18 of the 32 gated series
change. In `release`, `core-jit` grows by 1,904 B stripped
file / 1,616 B stripped `.text`, `core-aot` grows by 32 B
stripped `.text` only, and `component`, `core`, `core-nosimd`,
`wasi` and `wasi-threads` each grow by 16 B / 16 B (the
`ExecuteContext` stack-floor field in shared code). In
`release-size`, `core-jit` grows by 896 B / 688 B, `core-aot`
shrinks by 128 B / 80 B, and `wasi` grows by 16 B stripped
`.text` only. Against the first PR #293 re-baseline, the
increment attributable to the P1 fix alone is: `release`
`core-jit` +752 B / +640 B, `core-aot` +560 B / +480 B, and
+16 B / +16 B on `component`, `core`, `core-nosimd`, `wasi`
and `wasi-threads`; `release-size` `core-jit` +624 B / +448 B,
`core-aot` +320 B / +224 B, `wasi` +16 B stripped `.text`.
Against the original #164 baseline, the largest cumulative
growth remains the `release` `component` stripped file series,
unchanged at +9.59% (2,433,656 B to 2,667,136 B). The baseline
was re-measured on the branch and updated as recorded above,
following the procedure below.

PR #294 was re-baselined after the merge of current `main`
(`3922f93`, PR #293; merge commit `46931a9`). The PR fixes a
Miri-found uninitialized read in the optimizer pipeline: the
`Operand` union's narrow fields leave the high four bytes of
the raw encoding uninitialized in union literals, and the raw
lowering (`lower_operand`) reads all eight bytes, so building
the `[u8; 8]` value was undefined behaviour. The fix adds
canonical constructors on `Operand` (`from_i32`, `from_u32`,
`from_f32`, `from_jump_addr`, `from_drop_size`,
`from_local_addr`, `from_select`), each zeroing the tail, and
replaces 78 narrow union literals across the parser and the
pipeline `raw_*` helpers with them. Attribution was isolated
by measuring current `main` alone (workflow_dispatch run
45334673188, gate success), which reproduces the pre-#292
baseline exactly on all 32 gated series, so every delta below
is this PR's own. Against `main`, 28 of the 32 gated series
change; the `baseline` configuration is unchanged. In
`release` every changed series grows by exactly its stripped
`.text` delta (pure code growth, no data change): `core`,
`core-aot` and `core-jit` each grow by 448 B stripped file /
448 B stripped `.text`, `component` and `wasi` by 208 B /
208 B, `wasi-threads` by 224 B / 224 B, and `core-nosimd` by
144 B / 144 B. In `release-size` the deltas are small and
mixed: `component` grows by 96 B / 96 B, `wasi` by 96 B /
96 B, `wasi-threads` by 64 B / 64 B, `core` shrinks by 48 B /
16 B, `core-jit` by 48 B / 32 B, `core-nosimd` by 48 B / 32 B,
and `core-aot` by 32 B / 16 B. Against the original #164
baseline, the largest cumulative growth is the `release`
`component` stripped file series at +9.60% (2,433,656 B to
2,667,344 B). The baseline was re-measured on the branch and
updated as recorded above, following the procedure below.

PR #298 was re-baselined after the merge of current `main`
(`6d29650`, PR #297; merge commit `d6e6aa8`). The PR makes
`wasi:filesystem` read-only by contract: a new bindgen
`exclude` option omits named functions from the generated
`add_to_linker`, the twelve mutating filesystem functions
are excluded from registration, the remaining seven
non-mutating functions are implemented, and `open-at`
rejects mutate-directory opens, so the registered surface
(17 of 29 functions) is fully implemented and write imports
fail at instantiation instead of trapping mid-call. A Codex
P2 fix on top makes `is-same-object` compare filesystem
identity from `fs::metadata` ((device, inode) on Unix)
instead of canonical path strings, so hard links to one
file compare as the same object. Attribution was isolated
by measuring current `main` alone (workflow_dispatch run
45359420790, gate success). Unlike previous rounds, that
control does not reproduce the checked-in baseline on the
eight `wasi` and `wasi-threads` gated series: PR #297
(#135, `wasi:sockets` stub removal) landed on `main` after
the PR #294 re-baseline and shrank those series itself,
and the gate only fails on growth, so no re-baseline was
taken then. This update absorbs both parts, split by the
control measurement. PR #297's part: in `release`, `wasi`
and `wasi-threads` each shrink by 96,960 B stripped file /
71,424 B stripped `.text`; in `release-size`, `wasi`
shrinks by 62,448 B / 41,952 B and `wasi-threads` by
62,448 B / 41,936 B. This PR's own part: 8 of the 32 gated
series change, all in `wasi` and `wasi-threads` and all
shrinking — in `release`, `wasi` shrinks by 19,456 B /
14,256 B and `wasi-threads` by 19,440 B / 14,256 B; in
`release-size`, `wasi` shrinks by 12,288 B / 8,096 B and
`wasi-threads` by 12,272 B / 8,096 B. Every other gated
series, including the `baseline` configuration, is
unchanged. Against the original #164 baseline, the
largest cumulative growth is unchanged: the `release`
`component` stripped file series at +9.60% (2,433,656 B
to 2,667,344 B). The baseline was re-measured on the
branch and updated as recorded above, following the
procedure below.

PR #300 (#188, superinstruction selection) quarantines the
benchmark-shaped recognizers behind a runtime flag:
`RuntimeConfig.recognizers` (`RecognizerConfig`,
`benchmark_shaped` default false, CLI
`--benchmark-recognizers`) gates a reconnect pass
(`apply_benchmark_shaped_recognizers`) in instantiate.rs
that reclassifies kernel-op starts, masked-wrapper and
low7-guard shapes, and call quickenings only when the flag
is on. The recognizers had no production callers since
PR #110, so the default keeps current behaviour. The
`list_crc` quickening is deliberately not referenced:
referencing its handlers resurrects about 60 KB of
dead-code-eliminated `.text` for a quickening that can
never fire. A Codex P1 fix on top moves the two executable
vm-profile corpus workloads (`json_agg.rs`,
`text_index.rs`) out of `docs/benchmarks/vm-profile/`
into `tools/vm-profile/` — `AGENTS.md` prohibits
executable measurement code under `docs/` — a pure rename
that changes no gated size. Attribution was isolated by
measuring current `main` alone (workflow_dispatch run
45398496895, gate success), which reproduces the previous
baseline exactly on all 32 gated series, so every delta
below is this PR's own. 28 of the 32 gated series change;
the `baseline` configuration, which links no telomere
code, is byte-identical in both profiles. In `release`,
every library configuration grows — stripped file
+4,472 B in `core-jit` to +6,680 B in `core-aot`, stripped
`.text` +3,408 B to +5,312 B (the reconnect pass, the
resurrected masked-CRC and low7 handler pairs, and the
instantiate closure, per the symbol decomposition in the
PR record). In `release-size` the growth is smaller:
stripped file +3,312 B to +4,928 B, stripped `.text`
+2,400 B to +3,632 B. Against the original #164 baseline,
the largest cumulative growth is now the `release`
`core-nosimd` stripped file series at +9.91%
(1,275,448 B to 1,401,864 B), overtaking the `release`
`component` stripped file series at +9.85% (2,433,656 B
to 2,673,416 B). The baseline was re-measured on the
branch and updated as recorded above, following the
procedure below.

PR #302 (#189, TOS-in-register dispatch study) changes no
code linked into any embedder configuration: it adds the
study document `docs/core/tos-register-study.md` and the
standalone prototype `tools/tos-register-prototype.rs` (a
single-file `rustc` program, not a Cargo target; the Codex
fix on the PR moved it out of `docs/`, which `AGENTS.md`
reserves for non-executable notes). Its first footprint run
nevertheless tripped the gate on two series, each +16 B.
Attribution was isolated by measuring current `main` alone
three times (workflow_dispatch runs 45454023404, 45454038378
and 45454054158 on `main` at `04bca9a`): unmodified `main`
produces byte-identical sizes to this branch on all 32
gated series and fails the gate on the same two series, so
the delta is entirely main-side baseline staleness. The
previous baseline was measured on the PR #300 branch tree
(`3421c44`: `main` at `93581e8` plus PR #300) before
PR #299 (precompiled-module codec, #196) merged; on the
merged `main` (`04bca9a`), 26 of the 32 gated series differ
from that baseline. Most library configurations shrink (in
`release`, stripped file -1,000 B in `core` to -1,896 B in
`wasi`, stripped `.text` -1,056 B to -1,952 B; in
`release-size`, stripped file -32 B to -128 B and stripped
`.text` -16 B to -80 B where they change); the `baseline`
configuration is unchanged in both profiles. Only two
series grow, each by 16 B: `release-size` `core` stripped
`.text` (787,111 B to 787,127 B) and `release-size`
`wasi-threads` stripped file (1,940,824 B to 1,940,840 B).
Against the original #164 baseline, the largest cumulative
growth is now the `release` `core-nosimd` stripped file
series at +9.83% (1,275,448 B to 1,400,792 B), followed by
the `release` `component` stripped file series at +9.77%
(2,433,656 B to 2,671,528 B) — both slightly below the
PR #300 cumulative figures, because the merged `main`
shrank against the PR #300 branch tree. The baseline was
re-measured on the branch and updated as recorded above, following the
procedure below.

PR #304 (#198, AoT artifact container) tripped the gate on
four `release-size` series. Attribution was isolated by
measuring current `main` alone three times
(workflow_dispatch runs 45459064907, 45459081941 and
45459101432 on `main` at `04bca9a`; the values reproduce
the PR #302 control measurements exactly). Two failures
are main-side baseline staleness alone, shared with
PR #302: the previous baseline was measured on the
PR #300 branch tree (`3421c44`: `main` at `93581e8` plus
PR #300) before PR #299 (precompiled-module codec, #196)
merged; on the merged `main`, 26 of the 32 gated series
differ from that baseline and most library configurations
shrink (in `release`, stripped file -1,000 B in `core` to
-1,896 B in `wasi`, stripped `.text` -1,056 B to -1,952 B;
in `release-size`, stripped file -32 B to -128 B and
stripped `.text` -16 B to -80 B where they change), while
the `baseline` configuration is unchanged in both
profiles. The two staleness-only failures are each +16 B:
`release-size` `core` stripped `.text` (787,111 B to
787,127 B) and `release-size` `wasi-threads` stripped
file (1,940,824 B to 1,940,840 B) — this branch is
byte-identical to `main` on both. This PR's own delta
against the main control is confined to the only two
configurations that link the AoT/JIT code path
(`core-jit` and `core-aot`); the other six configurations
are byte-identical to `main` on every gated series,
because the new `aot_artifact` module is gated behind
the `aot-load`/`jit` features and the unreferenced
remainder is link-time eliminated in the minimal
embedder. The seven differing series are: `release`
`core-aot` stripped file -256 B / stripped `.text`
-192 B, `release` `core-jit` stripped file +16 B /
stripped `.text` -352 B, `release-size` `core-aot`
stripped file +16 B (stripped `.text` unchanged), and
`release-size` `core-jit` stripped file +384 B /
stripped `.text` +208 B. Against the checked-in baseline
the remaining two gate failures are therefore
`release-size` `core-jit` stripped file +352 B
(1,538,080 B to 1,538,432 B) and stripped `.text` +192 B
(1,091,575 B to 1,091,767 B), the net of this PR's growth
and the main-side shrink. Against the original #164
baseline, the largest cumulative growth remains the
`release` `core-nosimd` stripped file series at +9.83%
(1,275,448 B to 1,400,792 B), followed by the `release`
`component` stripped file series at +9.77% (2,433,656 B
to 2,671,528 B) — unchanged from the PR #302 figures,
because this PR does not grow those series. The baseline was
re-measured on the branch and updated as recorded above,
following the procedure below.

PR #306 (#199, `telomere compile`: offline AoT artifact
producer, fail-closed CLI load path, and the AoT wast lane
on both targets) merged current `main` into its branch
first; its CI run on the merged tree then tripped the gate
on 28 of the 32 gated series. Attribution was isolated by
measuring current `main` alone three times
(workflow_dispatch runs 45522134432, 45522139809 and
45522144666 on `main` at `bfc30e4`; all three runs pass
the gate). The main control reproduces the checked-in
baseline exactly on 28 of the 32 gated series; on the
four `release-size` `core-aot`/`core-jit` series it
measures 16–32 B below the baseline (stripped file
-32 B / stripped `.text` -32 B in `core-jit`, -16 B /
-16 B in `core-aot`), a main-side shrink since the
PR #304 re-baseline tree (`ba0feb7`, which predates the
merges of PRs #302, #303 and #304 and PR #305's
minimal-embedder `instances` bin), so no part of the
gate failure is main-side growth. This PR's own delta
against the main control is small and nearly uniform:
the embedder-linked part of the change is confined to
the library frontend phase counters
(`common/frontend_phases.rs`) and their wiring, while
the producer and the CLI load path live in the
`telomere-cli` binary the minimal embedder does not
link, and the AoT wast lane is CI configuration. In
`release`, stripped file / stripped `.text` grow
+48 B / +48 B in `core` and `core-nosimd`, +32 B /
+48 B in `core-aot`, +64 B / +64 B in `core-jit`, and
+384 B / +384 B in `component`, `wasi` and
`wasi-threads`; in `release-size` the growth is
+16 B to +64 B across the library configurations
(largest: `core-jit` stripped file +64 B). The
`baseline` configuration, which links no library code,
is byte-identical in both profiles. Against the
original #164 baseline, the largest cumulative growth
remains the `release` `core-nosimd` stripped file
series at +9.83% (1,275,448 B to 1,400,840 B), followed
by the `release` `component` stripped file series at
+9.79% (2,433,656 B to 2,671,912 B). The baseline was
re-measured on the branch and updated as recorded above,
following the procedure below.

PR #315 (#201, component bundles: hash-keyed AoT artifacts for
the core modules embedded in a component) merged current
`main` into its branch first; its CI run on the merged tree
then tripped the gate on 12 of the 32 gated series.
Attribution was isolated by measuring current `main` alone
three times (workflow_dispatch runs 45534050177, 45534060121
and 45534068404 on `main` at
`b868802f3ae377dccc28d54ba73560bb5e55bd79`; all three runs
pass the gate). The main control reproduces the checked-in
baseline exactly on all 32 gated series, so no part of the
gate failure is main-side staleness. This PR's own delta
against the main control is confined to the three
configurations that link `telomere-component` (`component`,
`wasi`, `wasi-threads`): in `release`, stripped file
+1,456 B and stripped `.text` +1,248 B in each; in
`release-size`, stripped file +192 B / stripped `.text`
+144 B in `component` and `wasi-threads`, and +208 B /
+160 B in `wasi`. Every other configuration is
byte-identical to `main` in both profiles. The bundle
implementation proper (`bundle.rs`, the substitution decode
session, the instantiate-time install path) is gated behind
`telomere-component`'s `aot-bundle` feature, which the
minimal embedder does not enable, so the linked delta is
the code-layout effect of the PR's component-layer changes
on those configurations; the core library is untouched.
The delta covers the whole PR, including the Codex
follow-ups on it: the bundle load harness moved to
`benches/`, the checked length arithmetic in the bundle
reader, and the alias-identity resolution in
`runtime/env.rs` (a core instance instantiated through an
`alias export` of a nested component's module now resolves
to the module's definition index before the artifact
lookup, so aliased modules receive their AoT code instead
of silently running interpreted). Against the original
#164 baseline, the largest cumulative growth is now the
`release` `component` stripped file series at +9.85%
(2,433,656 B to 2,673,368 B), overtaking the `release`
`core-nosimd` stripped file series at +9.83% (1,275,448 B
to 1,400,840 B), which this PR does not change. The
baseline was re-measured on the branch and updated as
recorded above, following the procedure below.

PR #338 (#313, check every declared import at instantiation,
used or not) merged current `main` into its branch first; its
CI run on the merged tree then tripped the gate on 12 of the
32 gated series. Attribution was isolated by measuring
current `main` alone three times (workflow_dispatch runs
45541261935, 45541270512 and 45541278948 on `main` at
`726fdd964b5b6c5dd264397d8616154cb7a9b242`; all three runs
pass the gate). The main control reproduces the checked-in
baseline exactly on all 32 gated series, so no part of the
gate failure is main-side staleness. This PR's own delta
against the main control is confined to the three
configurations that link `telomere-component` (`component`,
`wasi`, `wasi-threads`): in `release`, stripped file
+2,560 B in `component` and `wasi` and +2,544 B in
`wasi-threads`, stripped `.text` +2,096 B in each; in
`release-size`, stripped file +2,048 B / stripped `.text`
+1,840 B in `component`, and +1,648 B / +1,440 B in `wasi`
and `wasi-threads`. Every other configuration is
byte-identical to `main` in both profiles. The change is
confined to `telomere-component`'s runtime:
`check_imports_satisfied` in `runtime/env.rs` resolves
every declared import through the existing
`lookup_import_*` helpers at the two component-instance
creation sites (the root `instantiate_sync` and the
`Defined` arm of `resolve_instance`), so an import that
nothing references fails at instantiation with the same
error it would have produced at first use; the core library
is untouched. Against the original #164 baseline, the
largest cumulative growth is now the `release` `component`
stripped file series at +9.96% (2,433,656 B to 2,675,928 B);
the `release` `core-nosimd` stripped file series is
unchanged at +9.83% (1,275,448 B to 1,400,840 B). The
baseline was re-measured on the branch and updated as
recorded above, following the procedure below.

PR #339 (#192, the three baseline JIT backends merged into one
shared lowering over a per-target `BaselineMasm` trait) merged
current `main` into its branch first; its CI run on the merged
tree then tripped the gate on the `core-jit` series. The branch
also carries the Codex P1 fix for the shared lowering (AArch64
`cset_w` emitted the inverted condition code from the shared
semantic `Cond` discriminant instead of the target encoding;
fixed by mapping through `cond_code`, with an encoder unit test
pinning the CSET condition field for all ten conditions). The
branch merged `main` twice during integration (`726fdd9`, then
`7db0701` after PRs #336 and #338 landed); the recorded values
come from the second measurement round. Attribution was
isolated by measuring current `main` alone three times
(workflow_dispatch runs 45549096511, 45549121384 and
45549145537 on `main` at
`7db0701fdf188885150683e531fd6a246235d55d`; all three runs
pass the gate). The main control reproduces the checked-in
baseline exactly on all 32 gated series, so no part of the
gate failure is main-side staleness. This PR's own delta
against the main control is confined to `core-jit`, the only
embedder configuration that links the JIT backends: in
`release`, stripped file +576 B and stripped `.text` +384 B;
in `release-size`, stripped file -64 B and stripped `.text`
+112 B. Every other configuration is byte-identical to `main`
in both profiles, and all four branch samples
(workflow_dispatch runs 45549109565, 45549132836 and
45549156346, and pull-request run 45549088433) agree
byte-for-byte on every series. The delta is identical to the
first measurement round against `main` at `726fdd9`, in which
the pull-request run measured the three component
configurations 848–2,344 B lower than every other sample; the
second round shows no such outlier. Against the original
#164 baseline, the largest cumulative growth is now the
`release` `component` stripped file series at +9.96%
(2,433,656 B to 2,675,928 B), raised by PR #338's landing and
not changed by this PR. The baseline was re-measured on the
branch and updated as recorded above, following the procedure
below.

PR #340 (#312, canonical ABI loads and stores trap on unaligned
pointers; the canon-lower trampoline keeps the trap cause instead
of flattening it to `unreachable`) merged current `main` into its
branch first; its CI run on the merged tree then tripped the gate
on four of the 32 gated series (the `release-size` `wasi` and
`wasi-threads` file and `.text` series). Attribution was isolated
by measuring current `main` alone three times (workflow_dispatch
runs 45553891986, 45553905012 and 45553917777 on `main` at
`3acb5566b26769d7b9d1cd11838c87ff2391acca`; all three runs pass
the gate). The main control reproduces the checked-in baseline
exactly on 20 of the 32 gated series; on the 12 series of the
three configurations that link `telomere-component` it measures
below the checked-in baseline by exactly PR #336's (#316)
shrinkage — in `release`, stripped file −2,680 B (`component`),
−2,696 B (`wasi`), −2,664 B (`wasi-threads`) and stripped `.text`
−2,464 B in each; in `release-size`, stripped file
−1,568/−1,040/−1,024 B and stripped `.text` −1,488/−944/−944 B —
which was never re-baselined because the gate passes on shrinkage,
so part of this PR's growth is absorbed by that staleness and the
gate failure is confined to the four series where the growth
exceeds it. This PR's own delta against the main control is
confined to the same 12 series: in `release`, stripped file +1,248
B in `component` and +1,264 B in `wasi` and `wasi-threads`,
stripped `.text` +368 B in each; in `release-size`, stripped file
+1,296 B / stripped `.text` +1,008 B in `component`, +1,312 B /
+1,008 B in `wasi`, and +1,328 B / +1,024 B in `wasi-threads`.
Every other configuration is byte-identical to `main` in both
profiles. The change is confined to `telomere-component`'s
canonical ABI layer: `validate_pointer_alignment` runs at the
load/store dispatcher entry for every canonical type, the utf16
and latin1+utf16 string contents are checked on the same paths,
and the trampoline records the trap it raised so the single
conversion point can restore the canonical cause; the core library
is untouched. The branch also carries a test-only integration
follow-up: the #312 regression fixtures were written against wast
243 and are modernised to the strict component WAT syntax wast 263
requires (no assertion changes), `values/alignment.wast` is re-
enabled in the vendored upstream suite now that its blocker is
fixed (files=79 ran=10 excluded=69 checked=273, all passing in the
real tree), and the stale #312 citation is dropped from
`values/realloc.wast`'s exclusion reason (its line 93 no longer
fails; the file stays excluded for #322 and the empty-list
lowering defect). Against the original #164 baseline, the largest
cumulative growth is now the `release` `component` stripped file
series at +9.90% (2,433,656 B to 2,674,496 B), down from +9.96%
because PR #336's shrinkage is now reflected in the baseline; the
`release` `core-nosimd` stripped file series is unchanged at
+9.83% (1,275,448 B to 1,400,840 B). The baseline was re-measured
on the branch and updated as recorded above, following the
procedure below.

PR #340's branch was re-baselined a second time before it could
merge: PRs #341 (#317, per-instance resource handle tables),
#343 (#322, bounds-checked canonical realloc results) and #346
(#333, defined instances materialised in definition order) landed
on `main` after the first re-baseline, the branch merged current
`main` again (merge commits `825f42f` and `72c02cd`, `main` at
`bf8de4597b16df23cfb6f5a8b10e71d47faae429`), and its CI run on the
merged tree tripped the gate again. The merge conflicted in
`crates/telomere-component/src/runtime/canonical.rs`, where #312
had added `validate_pointer_alignment` next to the helper #322
replaced: the resolution keeps #312's
`validate_pointer_alignment` and #322's centralised
`validate_realloc_result` (both function bodies byte-identical to
their respective sources) and drops the superseded
`validate_realloc_pointer_alignment`, whose call sites all moved
into `call_realloc`. Two test assertions written against the
flattened trampoline behaviour were updated to the preserved trap
cause #312 establishes — the latin1+utf16 out-of-bounds shrink
test in `component_canonical_abi.rs` now pins the `realloc
return: beyond end of memory` message, and the sibling-handle
drop test in `component_resource_handles.rs` now pins the
`resource handle 1 is invalid` message; both tests still require
the trap. Attribution was isolated by measuring current `main`
alone three times (workflow_dispatch runs 45565216579,
45565225662 and 45565239398 on `main` at
`bf8de4597b16df23cfb6f5a8b10e71d47faae429`; all three runs pass
the gate against `main`'s checked-in baseline). Against this
branch's previously checked-in baseline, the main control is
byte-identical on 20 of the 32 gated series; on the 12 series of
the three configurations that link `telomere-component`, `main`
measures above that baseline in `release` — stripped file
+1,264 B (`component`), +1,184 B (`wasi`), +1,184 B
(`wasi-threads`) and stripped `.text` +1,104 B / +1,024 B /
+1,024 B — from the #317/#322/#333 code that landed after the
baseline snapshot, and below it in `release-size` — stripped
file −2,384/−2,512/−2,528 B and stripped `.text`
−2,256/−2,368/−2,384 B in
`component`/`wasi`/`wasi-threads`. This PR's own delta against
the main control is confined to the same 12 series: in
`release`, stripped file +1,520 B and stripped `.text` +224 B in
each of `component`, `wasi` and `wasi-threads`; in
`release-size`, stripped file +1,312 B and stripped `.text`
+1,008 B in each. Every other configuration is byte-identical to
`main` in both profiles. Against the original #164 baseline,
the largest cumulative growth is now the `release` `component`
stripped file series at +10.01% (2,433,656 B to 2,677,280 B), up
from +9.90% because the intervening `main` growth is now
reflected in the baseline; the `release` `core-nosimd` stripped
file series is unchanged at +9.83% (1,275,448 B to 1,400,840 B).
The baseline was re-measured on the branch and updated as
recorded above, following the procedure below.

PR #348 (#342, canonical lift/lower transfers resource handles
across component call boundaries) merged current `main` into
its branch first (merge commit `634bfd3`, `main` at
`90f1437b1445b8523daa2f78eecb366de415ed11` — PRs #340 (#312),
#345 (#332) and #346 (#333) on top of the #341/#343 base the
branch started from); its CI run on the merged tree then
tripped the gate on the PR's own change. The merge conflicted
only in the vendored-suite exclusion table
(`component_model_upstream.rs`, the `linking/unit.wast`
reason): `main` had dropped the #333 clause (fixed by #346)
while this branch dropped the #317 clause (superseded by #317
and this PR); the resolution drops both clauses, leaving #321
(line 799), #324 (lines 2010, 2043) and the alias
export-resolution failure at lines 865-866, filed as #347.
One test assertion was updated for the trap cause #312's
preservation now surfaces — the wrong-type drop test in
`component_resource_handles.rs` pins the `resource handle 2 is
invalid` message; the test still requires the trap.
Attribution was isolated by measuring current `main` alone
three times (workflow_dispatch runs 45574513443, 45574531337
and 45574550157 on `main` at `90f1437`;
all three runs pass the gate against `main`'s checked-in
baseline). Against the checked-in baseline, the main control
is byte-identical on 20 of the 32 gated series; on the 12
series of the three configurations that link
`telomere-component` it measures uniformly below that
baseline — stripped file and stripped `.text` −96 B in
`release` and −32 B in `release-size`, in each of
`component`, `wasi` and `wasi-threads` — because that
baseline was snapshotted from PR #340's branch tree and the
squash-merged `main` measures slightly smaller (the gate
passes shrinkage, so the staleness was inert). This PR's own
delta against the main control is confined to the same 12
series: in `release`, stripped file +6,744 B (`component`),
+6,744 B (`wasi`) and +6,728 B (`wasi-threads`) and stripped
`.text` +5,552 B in each; in `release-size`, stripped file
+4,648 B / +4,648 B / +4,664 B and stripped `.text` +3,664 B
in each. Every other configuration is byte-identical to
`main` in both profiles. Against the original #164 baseline,
the largest cumulative growth is now the `release`
`component` stripped file series at +10.29% (2,433,656 B to
2,683,928 B), up from +10.01%; the `release` `core-nosimd`
stripped file series is unchanged at +9.83% (1,275,448 B to
1,400,840 B). The baseline was re-measured on the branch and
updated as recorded above, following the procedure below.

PR #350 (#330, core-to-component `char` conversions trap
with the canonical ``invalid `char` bit pattern`` message)
merged current `main` into its branch first (merge commit
`3f20c2e`, `main` at
`21edef0f0b147af7a43dcbec002673227ff8516d` — PRs #348
(#342) and #351 (#331) on top of the `90f1437` main the
branch had merged during integration); the merge was
conflict-free, and its CI run on the merged tree tripped
the gate on six series, all in the `release-size` profile.
Attribution was isolated by measuring current `main` alone
three times (workflow_dispatch runs 45581400712, 45581419487
and 45581439929 on `main` at `21edef0`;
all three runs pass the gate against `main`'s checked-in
baseline). Against the checked-in baseline, the main control
is byte-identical on 25 of the 32 gated series; on seven
series it measures below that baseline — stripped file and
stripped `.text` −32 B in `release` for `component`,
stripped file −48 B and stripped `.text` −32 B in `release`
for `wasi` and `wasi-threads`, and stripped file −16 B in
`release-size` for `wasi-threads` — because that baseline
was snapshotted from PR #348's branch tree and the
squash-merged `main` measures slightly smaller (the gate
passes shrinkage, so the staleness was inert). This PR's own
delta against the main control is confined to the series of
the three configurations that link `telomere-component`: in
`release`, stripped `.text` +32 B in each of `component`,
`wasi` and `wasi-threads` and stripped file +16 B in each of
`wasi` and `wasi-threads` (`component` stripped file
unchanged); in `release-size`, stripped file +128 B / +128 B
/ +144 B and stripped `.text` +80 B / +80 B / +96 B. Every
other configuration is byte-identical to `main` in both
profiles. In `release` the growth is absorbed by the
staleness headroom, so only the six `release-size` series
are re-baselined, to the branch measurements. Against the
original #164 baseline, the largest cumulative growth is
unchanged: the `release` `component` stripped file series
at +10.29% (2,433,656 B to 2,683,928 B), whose baseline does
not move here. The baseline was re-measured on the branch
and updated as recorded above, following the procedure below.

PR #353 (#318, split extern-name encodings (name prefix
`0x02`: `implements` / `version-suffix` / `external-id`)
decode, and extern names are validated against the
package-name grammar) merged current `main` into its branch
first (merge commit `0dd39b5`, `main` at
`d5af7b8e004daff28d1735657155d3df5802d8a3` — PRs #348
(#342), #351 (#331) and #350 (#330) on top of the `90f1437`
`main` the branch was based on); the merge was conflict-free,
and its CI run on the merged tree tripped the gate on the
twelve series of the three configurations that link
`telomere-component`. Attribution was isolated by measuring
current `main` alone three times (workflow_dispatch runs
45591120085, 45591130821 and 45591141591 on `main` at
`d5af7b8`; all three runs pass the gate against `main`'s
checked-in baseline). Against the checked-in baseline, the
main control is byte-identical on 29 of the 32 gated series;
on the three `release` stripped file series of `component`,
`wasi` and `wasi-threads` it measures −32 B below that
baseline — the branch-tree snapshot staleness recorded for
PRs #348 and #350 (the gate passes shrinkage, so the
staleness was inert). This PR's own delta against the main
control is confined to the same 12 series: in `release`,
stripped file +17,960 B (`component`), +17,976 B (`wasi`) and
+17,976 B (`wasi-threads`) and stripped `.text` +16,416 B in
each; in `release-size`, stripped file +10,328 B / +10,328 B
/ +10,296 B and stripped `.text` +9,056 B / +9,056 B / +9,040
B. Every other configuration is byte-identical to `main` in
both profiles. All twelve series exceed the checked-in
baseline, so those twelve are re-baselined, to the branch
measurements. Against the original #164 baseline, the
largest cumulative growth is now the `release` `component`
stripped file series at +11.02% (2,433,656 B to 2,701,856 B),
up from +10.29%; the `release` `core-nosimd` stripped file
series is unchanged at +9.83% (1,275,448 B to 1,400,840 B).
The baseline was re-measured on the branch and updated as
recorded above, following the procedure below.

PR #357 (#320, resource type validation: structural type
roots may carry handles to abstract resources that are part
of the surface, `(sub resource)` export ascriptions mint
fresh resource identities per instantiation, and own/borrow
mismatches report the specific expected/found reason) merged
current `main` into its branch first (merge commit
`daf5548`, `main` at
`b20ed79513bbb12f6e4447158fd4028808ac129b` — PR #353 (#318)
on top of the `d5af7b8e004daff28d1735657155d3df5802d8a3`
`main` the branch was based on); the merge was conflict-free,
and its CI run on the merged tree tripped the gate on the
twelve series of the three configurations that link
`telomere-component`. Attribution was isolated by measuring
current `main` alone three times (workflow_dispatch runs
45598772332, 45598789232 and 45598803018 on `main` at
`b20ed79513bbb12f6e4447158fd4028808ac129b`; all three runs
pass the gate against `main`'s checked-in baseline). Against
the checked-in baseline, the main control is byte-identical
on all 32 gated series. This PR's own delta against the main
control is confined to the same 12 series: in `release`,
stripped file +2,832 B (`component`), +2,816 B (`wasi`) and
+2,816 B (`wasi-threads`) and stripped `.text` +2,544 B in
each; in `release-size`, stripped file +1,632 B / +1,648 B
/ +1,664 B and stripped `.text` +1,360 B in each. Every
other configuration is byte-identical to `main` in both
profiles. All twelve series exceed the checked-in
baseline, so those twelve are re-baselined, to the branch
measurements. Against the original #164 baseline, the
largest cumulative growth is now the `release` `component`
stripped file series at +11.14% (2,433,656 B to 2,704,688 B),
up from +11.02%; the `release` `core-nosimd` stripped file
series is unchanged at +9.83% (1,275,448 B to 1,400,840 B).
The baseline was re-measured on the branch and updated as
recorded above, following the procedure below.

PR #357 Codex fix (register the fresh resource identity of
a sub-ascribed type export in the type index and export
info) on top of the tree re-baselined above (fix commit
`b875a31`); its CI run tripped the gate on six series.
Attribution was isolated against the previous branch
head's measurements (workflow_dispatch runs 45598730533,
45598745727 and 45598759671 on
`daf55480a32bd26e89c499644a2044cbc425cd64`): the delta is
confined to the `release-size` stripped file and stripped
`.text` of the three configurations that link
`telomere-component`, +48 B in each of `component`, `wasi`
and `wasi-threads`; every other series is byte-identical,
and against unmodified `main` at
`b20ed79513bbb12f6e4447158fd4028808ac129b` the delta
remains the twelve series recorded above, with the
`release-size` component-linking series a further +48 B.
All six series exceed the checked-in baseline, so those
six are re-baselined, to the branch measurements. Against
the original #164 baseline, the largest cumulative growth
is unchanged: the `release` `component` stripped file
series at +11.14% (2,433,656 B to 2,704,688 B); the
`release` `core-nosimd` stripped file series is unchanged
at +9.83% (1,275,448 B to 1,400,840 B). The baseline was
re-measured on the branch and updated as recorded above,
following the procedure below.

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
   The upload step runs even when the size gate step fails, so the
   artifact of the failing run — the run that prompted the re-baseline —
   is available; there is no need to recover values from the measure
   step's log output.
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

#### Measured instance counts under a 256 MiB budget (#140)

Measured with the `embed-instances` bin of `telomere-minimal-embedder`
(added for this measurement): it holds N instances of
`examples/add.wasm` — memory minimum 16 pages (1 MiB), no declared
maximum, so each instance carries the default 4 GiB reservation ceiling
with 1 MiB committed — in one `Store`, optionally calling each
instance's `main(1, 2)` once, and reports the count while
`/usr/bin/time -v` records the process peak RSS. `release-size`, `core`
configuration.

| Setup | Result |
| --- | --- |
| qemu-riscv64, `add.wasm`, instantiate only | N=0: 18792 KiB; N=1: 43384 KiB; N=9: 240396 KiB (largest measured N inside 256 MiB); N=10: 264872 KiB (exceeds 262144 KiB); slope ≈ 24.6 MiB/instance |
| qemu-riscv64, `add.wasm`, one call per instance | Same slope (N=1: 43572 KiB; N=10: 265304 KiB) |
| qemu-riscv64, bounded variant (same module with declared maximum 16 pages → 1 MiB reservation) | N=100: 19692 KiB total; slope ≈ 8 KiB/instance |
| Native x86_64 control (same harness, no emulation) | N=100 instantiate only: 2776 KiB total; N=5000 with one call each: 9828 KiB total (≈1.4 KiB/instance between N=1000 and N=5000) |

Reading: the ≈24.6 MiB/instance QEMU slope tracks the *reservation
size*, not committed guest memory — it collapses to ≈8 KiB/instance
when the reservation is bounded to 1 MiB, and the native control holds
5,000 executed instances in ≈9.6 MiB. The QEMU figure measures
emulator handling of large inaccessible reservations and must not be
read as the guest's static memory cost. What is established: (i) under
QEMU user-mode as configured here, a process holding instances of an
unbounded-max module crosses a 256 MiB budget between 9 and 10
instances; (ii) the guest-intrinsic static cost per held instance is
the committed-but-untouched 1 MiB initial memory plus a measured RSS
slope of ≈1.4–8 KiB. A real-hardware RSS ceiling for unbounded-max
modules remains unmeasured.

## Not yet measured

The following are open and should not be inferred from the numbers above:

- `riscv64gc-unknown-linux-gnu` on real hardware. Every riscv64 number in
  this document was measured under QEMU user-mode on an x86_64 host (see
  the #140 section above); hardware RSS and cold start, and the hardware
  static-memory ceiling for unbounded-max modules, remain unmeasured.
- Linux `aarch64` numbers.
- Static memory ceiling under a constrained RAM budget on real hardware
  (the 256 MiB class device target); the QEMU user-mode measurement and
  its reservation-size caveat are recorded under
  [Static linear-memory ceiling](#static-linear-memory-ceiling).
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

## AOT load path split (#200)

#200 splits the native-code *load* path from the JIT *emitters* so an
embedder can carry precompiled code without carrying a compiler.

- The `telomere` crate gains a non-default `aot-load` feature: the runtime
  stubs, the native-call ABI, the per-store code cache, and
  `telomere-jit-codegen::code_memory` (allocation, protection, relocation
  application). It pulls in no emitter and no assembler.
- `jit` now implies `aot-load` and additionally enables the new non-default
  `emit` feature of `telomere-jit-codegen`, which gates that crate's
  architecture backends and macro-assembler layer (its `arch` and `masm`
  modules). `code_memory` stays available without `emit`.
- In an `aot-load`-only build, compile-on-miss in the call path becomes
  cache-consult-only: a miss returns the typed `Unimplemented` VM error and
  the call falls back to the interpreter, exactly like a function the JIT
  rejects. Nothing in the build can compile at runtime; the two
  function-layout structs the emitters consume (`MemoryJitLayout`,
  `GlobalValueJitLayout`) stay behind `jit`.

### Symbol-absence check

`tools/check-aot-load-symbols.py` builds the `core-aot` embedder and fails
if any emitter symbol (`emit_baseline_function`, `emit_baseline_plan`,
`build_baseline_plan`) is present, with load-path symbols (`CodeArena`,
`StoreJitCache`, `instantiate`) as positive controls. It is wired into the
`aot-load` CI job and into the footprint workflow after the size gate. The
check deliberately builds the `dev` profile: under the workspace release
profiles, LTO inlines `emit_baseline_function` away even in a real `jit`
binary (measured on a control build), so a release-profile absence check
would pass vacuously; in a dev build the emitter entry points survive as
symbols whenever the emitter layer is linked. The checker's unit tests
(`tools/test_check_aot_load_symbols.py`) cover the symbol-matching rules.

### Measured ladder

Measured 2026-11-23 with `tools/measure-embedder-footprint.py --sizes-only`
on the #200 change branch (stripped file / stripped `.text`, bytes; RSS and
timing steps skipped locally because the measuring host lacks
`/usr/bin/time` — CI runs the full tool). The interpreter-only cell is the
existing `core` configuration; the `jit` + `aot-load` cell is `core-jit`
(`jit` implies `aot-load`, so it is the same build).

Linux x86_64 (fleur-du-lapin, Intel Xeon Gold 6140, Rust 1.96.0):

| Profile | Configuration | Stripped file (B) | Stripped `.text` (B) |
| --- | --- | ---: | ---: |
| `release` | `core` (interp-only) | 1,478,824 | 1,131,399 |
| `release` | `core-aot` | 1,506,264 | 1,150,503 |
| `release` | `core-jit` | 2,012,016 | 1,566,759 |
| `release-size` | `core` (interp-only) | 1,097,856 | 776,999 |
| `release-size` | `core-aot` | 1,117,160 | 788,423 |
| `release-size` | `core-jit` | 1,530,864 | 1,095,079 |

macOS arm64 (Mac mini, Apple M2 Pro, Rust 1.96.0):

| Profile | Configuration | Stripped file (B) | Stripped `.text` (B) |
| --- | --- | ---: | ---: |
| `release` | `core` (interp-only) | 1,388,264 | 1,013,268 |
| `release` | `core-aot` | 1,438,632 | 1,030,808 |
| `release` | `core-jit` | 1,873,176 | 1,430,536 |
| `release-size` | `core` (interp-only) | 770,640 | 598,428 |
| `release-size` | `core-aot` | 787,376 | 607,604 |
| `release-size` | `core-jit` | 1,004,416 | 815,464 |

Derived deltas (stripped file / stripped `.text`):

| Delta | Linux `release` | Linux `release-size` | macOS `release` | macOS `release-size` |
| --- | ---: | ---: | ---: | ---: |
| Load path (`core-aot` − `core`) | +27,440 / +19,104 | +19,304 / +11,424 | +50,368 / +17,540 | +16,736 / +9,176 |
| Emitters (`core-jit` − `core-aot`) | +505,752 / +416,256 | +413,704 / +306,656 | +434,544 / +399,728 | +217,040 / +207,860 |

The load path costs roughly 11–19 KB of `.text`; the emitters cost
208–416 KB of `.text` depending on profile and target — that emitter cost
is what an `aot-load` embedder no longer pays. The interpreter-only cell
did not regress: in the same Linux runs, `core` measured −8 B (release
stripped file) and +8 B (release-size) against the #164 baseline above,
which is code-layout noise from the cfg refactor, and the `core-aot` rows
were added to `tools/embedder-size-baseline.json` so the #164 gate covers
the new cell from its first CI run.

### Artifact side

No serialized AoT artifact format exists yet: the codec (#196), the
artifact container and its load path (#198), and the `telomere compile`
producer (#199) own it, so there are no artifact bytes to record against
wasm bytes. As the closest measurable proxy for what the load path would
carry, an in-crate measurement harness
(`runtime::jit::artifact_measure`, an ignored test run on demand) emitted
every function of the pinned wasm-testsuite corpus with the baseline
backend and summed the emitted code bytes against the source wasm bytes:

| Corpus | Modules | Functions emitted | Wasm bytes | Emitted code bytes | Ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| 147 root `.wast` files + `examples/add.wasm` | 1,465 instantiated (134 failed: imports/unsupported) | 5,904 (0 rejected) | 272,481 | 8,031,517 | 29.48× |

The ratio is dominated by fixed per-function emission overhead: the spec
suite's modules are mostly tiny single-purpose functions, so 29.48× is a
property of this corpus, not a bulk-code expansion factor, and it excludes
any container or relocation-record overhead. The artifact-bytes column of
this comparison belongs to #198/#201, once the format exists to measure.
