# Fuzzing

Telomere keeps libFuzzer harnesses in `fuzz/`, outside the main Cargo
workspace. The stable replay tests turn encodable upstream WAST directives into
deduplicated seed files for the parser and component decoder; the nightly
harnesses then mutate those files. A fuzzer finding is a failure only when it
reproduces a panic or sanitizer finding. Ordinary parser, decoder, and
canonical-ABI `Result` errors are expected inputs and must not be unwrapped by
the harnesses.

## Scope

| Target | Boundary exercised | Seed directory |
| --- | --- | --- |
| `parse_core_module` | `telomere::WasmParser::parse_module` | `fuzz/corpus/parse_core_module` |
| `decode_component` | `telomere_component::ComponentEngine::compile` | `fuzz/corpus/decode_component` |
| `canon_lift_args` | Feature-gated canonical ABI lift adapter | `fuzz/corpus/canon_lift_args` |

The first two targets receive source-derived seeds from the stable replay
tests. `canon_lift_args` uses its dedicated fixture and starts with an empty
directory unless a maintainer promotes a seed.

## Setup

The root workspace is pinned to stable Rust. The fuzz project has its own
sanitizer-capable toolchain in `fuzz/rust-toolchain.toml`:
`nightly-2026-07-31`.

From the repository root, install that toolchain and `cargo-fuzz`, then
initialize the upstream fixture submodule:

```shell
rustup toolchain install nightly-2026-07-31
cargo +nightly-2026-07-31 install cargo-fuzz
git submodule update --init --recursive
```

All `cargo fuzz` commands must run with `fuzz/` as the current directory. The
fuzz manifest deliberately has an independent workspace and lockfile.

```shell
cd fuzz
cargo +nightly-2026-07-31 fuzz list
```

## Generate The Stable Seed Corpus

Still from `fuzz/`, create all three target directories. The replay tests
require `TELOMERE_FUZZ_CORPUS_OUT` to be absolute; `$(pwd -P)/corpus` satisfies
that requirement and keeps generated files outside Git's tracked source set.

```shell
mkdir -p corpus/parse_core_module corpus/decode_component corpus/canon_lift_args

TELOMERE_FUZZ_CORPUS_OUT="$(pwd -P)/corpus" \
  cargo +1.96.0 test --manifest-path ../Cargo.toml -p telomere \
  --test fuzz_corpus_replay --release -- --nocapture

TELOMERE_FUZZ_CORPUS_OUT="$(pwd -P)/corpus" \
  cargo +1.96.0 test --manifest-path ../Cargo.toml -p telomere-component \
  --test fuzz_corpus_replay --release -- --nocapture
```

Both replay tests sort source files, extraction results, and report keys. They
classify a payload from its Wasm header/version rather than from the WAST file
that contained it. The core test writes only `parse_core_module/`; the
component test writes only `decode_component/`. Each file name is the SHA-256
hex digest of its contents. Re-running against the same output directory
verifies an existing digest's contents before leaving it in place, then replays
the bytes in memory as well.

The tests also replay checked-in inputs, when present, from
`fuzz/seeds/<target>/` and `fuzz/regressions/<target>/`. Missing or empty
directories are valid. Do not commit the mutable `fuzz/corpus/` or
`fuzz/artifacts/` directories.

## Run The Targets

Remain in `fuzz/`. `cargo fuzz run` accepts multiple corpus directories;
libFuzzer treats the first as writable and the later directories as additional
read-only input. Pass a target its own three directories, generated corpus
first, so that newly discovered entries land in the gitignored `corpus/` and the
committed `seeds/` and `regressions/` stay read-only. Do not pass one target the
corpus of another: the inputs are decoded differently per target, so the extra
directories would only waste budget.

```shell
cargo +nightly-2026-07-31 fuzz run parse_core_module \
  corpus/parse_core_module seeds/parse_core_module regressions/parse_core_module

cargo +nightly-2026-07-31 fuzz run decode_component \
  corpus/decode_component seeds/decode_component regressions/decode_component

cargo +nightly-2026-07-31 fuzz run canon_lift_args \
  corpus/canon_lift_args seeds/canon_lift_args regressions/canon_lift_args
```

This is the same directory order the workflow uses, so a local reproduction and
a CI run see the same inputs.

CI additionally passes `--target x86_64-unknown-linux-gnu`. cargo-fuzz builds for
its own build triple unless told otherwise, and the prebuilt binary CI installs
is built for musl so that it does not depend on the runner's glibc. Left alone it
therefore selects `x86_64-unknown-linux-musl` on a gnu runner, where no musl std
is installed, and the build fails with `can't find crate for core`. A locally
`cargo install`ed cargo-fuzz already matches the host, so the flag is only needed
when the binary and the host disagree.

These commands run until interrupted. For a bounded local smoke run, add a
libFuzzer limit after `--`, for example `-- -max_total_time=300` or
`-- -runs=1000`. Record the exact command, target, toolchain, platform, and
limit with any finding; do not compare unrecorded runs as performance data.

## Triage And Promotion

1. Preserve the failing input under `fuzz/artifacts/<target>/` and reproduce
   it before changing code:

   ```shell
   cargo +nightly-2026-07-31 fuzz run <target> artifacts/<target>/<failing-input>
   ```

2. Minimize the reproduced input, retaining the original until the minimized
   case has reproduced as well:

   ```shell
   cargo +nightly-2026-07-31 fuzz tmin <target> artifacts/<target>/<failing-input>
   ```

3. Fix the root cause and promote the minimized input deliberately. Put a
   crash or panic regression in `fuzz/regressions/<target>/`; put a
   non-failing, coverage-useful input in `fuzz/seeds/<target>/`. Use a
   content-derived name, document the target and failure mode in the PR, and
   rerun the stable replay test plus the focused fuzzer reproduction.

4. Do not promote a seed merely because it is new. It must exercise a useful
   boundary that a reviewer can identify. A regression must continue to prove
   the original failure would reach the target boundary without the fix.

To add a target, begin from `fuzz/`:

```shell
cargo +nightly-2026-07-31 fuzz add <target>
```

Then make the harness fail only on panics or sanitizer findings, add any
fixture or feature-gated adapter it needs, choose its corpus/seed/regression
directories, document its boundary here, and add a stable replay producer only
when a source-derived seed format exists. Include the target in the CI cadence
review below.

## CI Cadence

The repository's current GitHub Actions workspace-test job runs
`cargo test --verbose --release --workspace` for pull requests and pushes to
`main`. The replay tests are ordinary integration tests, so they run as part
of that job.

`.github/workflows/fuzz.yaml` runs all three targets for 60 seconds each on
every pull request, a five-minute campaign per target on a daily schedule, and a
thirty-minute campaign per target weekly, carrying the corpus between scheduled
runs through the Actions cache. There is no hosted continuous campaign beyond
that. Before a change that affects one of these boundaries,
run the corresponding replay test locally; before a release or a dedicated
fuzzing change, run bounded campaigns for all three targets and retain the
commands and findings in the PR or release evidence. A nightly or continuous
campaign must not be described as CI until its workflow and retained evidence
exist.

## Bring-up Evidence

The following bring-up evidence is intentionally **not measured** in this
document. Do not replace `not measured` with inferred values; add a dated,
reproducible command and its captured output when measurement is performed.

| Evidence | Status | Required proof when measured |
| --- | --- | --- |
| Determinism: rerunning one input gives the same result even when other inputs are interleaved. | `not measured` | Record input hashes, order, target, command, and outcomes. |
| Store isolation: no mutable `Store` state accumulates between iterations, or the harness rebuilds it for every input. | `not measured` | Record the harness lifecycle and a state-isolation check. |
| libFuzzer accepts all three corpus directories and writes newly discovered inputs only to the first directory. | `not measured` | Record directory snapshots before and after a bounded run. |
| Per-target clean-run time and executions per second. | `not measured` | Record machine, toolchain, sanitizer, command, duration, and libFuzzer summary. |
| Number and reasons for spec files or payloads skipped by the seed generator. | `not measured` | Capture both replay-test reports and preserve their command lines. |

## Known Limits

- These targets exercise parsing, decoding, and the canonical ABI adapter. They
  do not fuzz guest module/component instantiation, guest function execution,
  host imports, or externally visible guest side effects.
- The project is not enrolled in OSS-Fuzz. There is no OSS-Fuzz corpus,
  dashboard, crash triage service, or continuous coverage claim to rely on.
- A successful replay or fuzz run establishes only the absence of an observed
  panic or sanitizer finding for that run. It is not a proof of Wasm or
  Component Model semantic conformance.
