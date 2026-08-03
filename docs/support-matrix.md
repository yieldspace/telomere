# Support matrix

This is a capability map for the core Wasm, Component Model, and WASI surfaces
that Telomere exposes. It deliberately distinguishes an implemented path from a
Cargo feature that merely recognizes input, and from an interface that is
registered but falls back to a generated trap.

## Core WebAssembly proposals

Here, **supported** means that the proposal passes the conformance fixtures
selected by the [WAST harness policy][wast-policy]. It does **not** mean the
proposal is spec-complete. The policy is structural rather than a frozen list:
it admits every root `.wast` fixture. With `simd` disabled, the only root
filter excludes stems beginning with `simd`; with `simd` enabled, there is no
root exclusion. It then considers only `proposal_allowlist()` families
(`threads` when that feature is enabled, plus `tail-call` and `wasm-3.0`). For
an allowed family other than `wasm-3.0`, a proposal case is selected when its
stem matches an admitted root stem or its path is explicitly listed by
`proposal_only()` (including its own feature-conditioned entries).
`wasm-3.0` admits only those explicit `proposal_only()` paths
([WAST policy][wast-policy] and [collector][wast-collector]).

The root `binary.wast` case has a narrow compatibility caveat. The eight
module SHA-256 entries in
[`BINARY_SPEC_DIVERGENCES`][binary-divergence-table] are accepted as
spec-version divergences, not parser defects: they use padded multi-memory
`memidx` LEB encodings where the older binary rule expected a literal reserved
zero. The harness records each matching module hash and its stale-entry
assertion requires every table entry to be hit; an upstream fixture change
cannot silently leave an obsolete exception behind
([stale assertion][binary-divergence-stale]).

| Proposal | Status | Gate | What happens if a guest uses it | Evidence |
| --- | --- | --- | --- | --- |
| Multi-memory | Yes | Always enabled | Executes within the selected fixture scope. | [WAST policy][wast-policy] and [collector][wast-collector]. |
| Tail calls | Yes | Always enabled | Executes within the selected fixture scope. | [WAST policy][wast-policy] and [collector][wast-collector]. |
| SIMD | Yes | `simd` (default on) | Executes within the selected fixture scope when `simd` is enabled. When it is disabled, the root collector excludes `simd*` fixtures, so that configuration is not conformance evidence for SIMD. | [Root collector][wast-collector]. |
| Threads / atomics | Yes | `threads` (default on) | Executes within the selected fixture scope when `threads` is enabled. The proposal allowlist itself is feature-gated, so a `threads`-off run is not conformance evidence for threads or atomics. | [WAST policy][wast-policy]. |
| Reference types | Yes | Always enabled | Executes within the selected fixture scope. | [WAST policy][wast-policy]. |
| Bulk memory | Yes | Always enabled | Executes within the selected fixture scope. | [WAST policy][wast-policy] and [collector][wast-collector]. |
| Multi-value | Yes | Always enabled | Executes within the selected fixture scope. | [WAST policy][wast-policy]. |
| Sign-extension operators | Yes | Always enabled | Executes within the selected fixture scope. | [WAST policy][wast-policy]. |
| Non-trapping float-to-int conversions | Yes | Always enabled | Executes saturating conversions within the selected fixture scope. | [WAST policy][wast-policy]. |
| Garbage collection | No — named rejection | None | Rejected as unsupported proposal feature `gc`. | [Proposal-support regression tests][proposal-support-tests]. |
| Exception handling | No — named rejection | None | Rejected as unsupported proposal feature `exception-handling`. | [Proposal-support regression tests][proposal-support-tests]. |
| Relaxed SIMD | No — named rejection | None | Rejected as unsupported proposal feature `relaxed-simd`. | [Proposal-support regression tests][proposal-support-tests]. |
| memory64 | No — named rejection | None | Rejected as unsupported proposal feature `memory64`. | [Proposal-support regression tests][proposal-support-tests]. |
| Extended const expressions | No — named rejection | None | Rejected as unsupported proposal feature `extended-const`. | [Proposal-support regression tests][proposal-support-tests]. |
| Custom page sizes | No — named rejection | None | Rejected as unsupported proposal feature `custom-page-sizes`. | [Proposal-support regression tests][proposal-support-tests]. |
| Wide arithmetic | No — named rejection | None | Rejected as unsupported proposal feature `wide-arithmetic`. | [Proposal-support regression tests][proposal-support-tests]. |
| 64-bit tables | No — generic rejection | None | A table flag for this feature remains a generic limit error; it is not reported as `memory64`. Follow-up work is required. | [Proposal-support regression tests][proposal-support-tests]. |
| Function references | No — generic rejection | None | Rejected generically; Telomere intentionally does not attach a proposal name. | [Proposal-support regression tests][proposal-support-tests]. |

The seven named-rejection strings above are part of the public diagnostic
contract. Their opcode ranges are deliberately narrow: an inaccurate proposal
name is worse than a generic parse error. Reserved values, holes, and adjacent
proposals remain generic errors.

## Component Model and canonical ABI

### Canonical functions

The canonical decoder implements exactly these function forms:

| Encoding | Status | Notes |
| --- | --- | --- |
| `canon lift` (`0x00 0x00`) | Implemented | Lifts a core function into a component function. |
| `canon lower` (`0x01 0x00`) | Implemented | Lowers a component function to a core function. |
| `resource.new` (`0x02`) | Implemented | Creates a resource representation. |
| `resource.drop` (`0x03`) | Implemented | Decodes the canonical resource-drop form. |
| `resource.rep` (`0x04`) | Implemented | Reads a resource representation. |
| Any other first opcode | Unsupported | Exact error: `unsupported canonical function opcode: 0xNN`. |

The nested forms have deliberately different exact errors: an unknown second
byte after `0x00` is `unsupported canonical function 0x00 0xNN`, and an
unknown second byte after `0x01` is `unsupported canonical function 0x01
0xNN`. They are not reported with the first-opcode error above. See the
[canonical-function decoder][canon-functions].

### Canonical options

| Option byte | Status | Behavior |
| --- | --- | --- |
| `0x00` | Implemented | UTF-8 string encoding. |
| `0x01` | Implemented | UTF-16 string encoding. |
| `0x02` | Implemented | Compact UTF-16 (`latin1+utf16`) string encoding. |
| `0x03` | Implemented | Core-memory option. |
| `0x04` | Implemented | `realloc` option, including its required core signature. |
| `0x05` | Implemented for lifts only | `post-return`; lowerings reject it. |
| `0x06` | Unsupported | Exact error: `async canonical ABI is not supported`. |
| `0x07` | Unsupported | Exact error: `canonical callback is not supported`. |
| `0x08` | Unsupported | Exact error: ``canonical `core type` option is not supported``. |
| `0x09` | Unsupported | Exact error: `canonical GC ABI is not supported`. |

The option behavior is defined by the [canonical-options decoder][canon-options].

### Cargo-gated Component Model proposals

The following features are declared in the [component crate's Cargo
features][component-features]. At the verified source snapshot, a source-wide
`cfg` search finds sites only for `component-gated-feature-value-imports-exports`
and `component-gated-feature-async`; the other four are declarations only.

| Feature | Current effect |
| --- | --- |
| `component-gated-feature-value-imports-exports` | Enables recognition of the component value section; it does not make every value-import/export workflow complete. |
| `component-gated-feature-async` | Enables recognition of selected async names and types, but does **not** add a component async runtime. Current sites return named `Unsupported` errors where runtime support is absent. |
| `component-gated-feature-nested-namespaces-and-packages` | Declared only; no source use at this snapshot. |
| `component-gated-feature-threading-builtins` | Declared only; no source use at this snapshot. |
| `component-gated-feature-fixed-length-lists` | Declared only; no source use at this snapshot. |
| `component-gated-feature-error-context-type` | Declared only; no source use at this snapshot. |

Async Component Model execution, streams/futures, error-context, and threading
built-ins remain non-goals for this support boundary. Their broader design and
follow-up context are tracked in [#121](https://github.com/yieldspace/telomere/issues/121)
and [#123](https://github.com/yieldspace/telomere/issues/123), rather than
being inferred from the presence of a Cargo feature.

## WASI 0.2.6 function coverage

Telomere's bundled WIT declares WASI **0.2.6**
([`WASI_VERSION`][wasi-version]). The denominator below is the number of
`func(` declarations in that bundled WIT, including unstable declarations. The
numerator counts explicit methods in the synchronous generated `Host` impls in
the provider; an async method with the same semantics is not counted again.
These are manual, interface-by-interface counts of synchronous `Host` impl
blocks: helper methods and async duplicates are excluded.

| Interface | Implemented / total | Boundary |
| --- | ---: | --- |
| `wasi:cli/environment` | 3 / 3 | Environment, arguments, and initial working directory. |
| `wasi:cli/exit` | 1 / 2 | `exit-with-code` is unstable and unregistered. |
| `wasi:cli/stdin` | 1 / 1 | Standard input stream. |
| `wasi:cli/stdout` | 1 / 1 | Standard output stream. |
| `wasi:cli/stderr` | 1 / 1 | Standard error stream. |
| `wasi:cli/terminal-input` | 0 / 0 | Resource-only interface; no functions to implement. |
| `wasi:cli/terminal-output` | 0 / 0 | Resource-only interface; no functions to implement. |
| `wasi:cli/terminal-stdin` | 1 / 1 | Optional terminal standard input. |
| `wasi:cli/terminal-stdout` | 1 / 1 | Optional terminal standard output. |
| `wasi:cli/terminal-stderr` | 1 / 1 | Optional terminal standard error. |
| `wasi:cli/run` | N/A | Guest export, not host coverage; its WIT contract contains one function. |
| `wasi:clocks/monotonic-clock` | 4 / 4 | Time, resolution, and poll subscriptions. |
| `wasi:clocks/wall-clock` | 2 / 2 | Current wall-clock time and resolution. |
| `wasi:clocks/timezone` | 0 / 2 | Provider is not registered. |
| `wasi:filesystem/types` | 10 / 29 | Read-oriented path, descriptor, directory, and error-code operations only. |
| `wasi:filesystem/preopens` | 1 / 1 | Preopened-directory enumeration. |
| `wasi:io/error` | 1 / 1 | Error string conversion. |
| `wasi:io/poll` | 3 / 3 | Pollable readiness, blocking, and polling. |
| `wasi:io/streams` | 15 / 15 | Input/output stream and splice operations. |
| `wasi:random/random` | 2 / 2 | Secure random bytes and random values. |
| `wasi:random/insecure` | 2 / 2 | Insecure random bytes and random values. |
| `wasi:random/insecure-seed` | 1 / 1 | Insecure seed. |
| `wasi:sockets/instance-network` | 0 / 1 | Registered; the generated binding's default trap handles every method. |
| `wasi:sockets/network` | 0 / 1 | Registered; the generated binding's default trap handles every method. |
| `wasi:sockets/udp` | 0 / 18 | Registered; the generated binding's default trap handles every method. |
| `wasi:sockets/udp-create-socket` | 0 / 1 | Registered; the generated binding's default trap handles every method. |
| `wasi:sockets/tcp` | 0 / 28 | Registered; the generated binding's default trap handles every method. |
| `wasi:sockets/tcp-create-socket` | 0 / 1 | Registered; the generated binding's default trap handles every method. |
| `wasi:sockets/ip-name-lookup` | 0 / 3 | Registered; the generated binding's default trap handles every method. |

The [bundled WIT][wasi-wit] is the source of the denominators; the
[provider modules][wasi-provider] are the source of the synchronous-method
numerators. The [sockets provider][wasi-sockets] explicitly registers every
socket interface but implements empty `Host`/`HostAsync` traits, which is why
the table says *registered* rather than *not linked*.

### Preview1 CLI runner

The compact core-Wasm preview1 runner supplies exactly these eight host
functions: `args_sizes_get`, `args_get`, `clock_time_get`, `fd_close`,
`fd_fdstat_get`, `fd_seek`, `fd_write`, and `proc_exit`
([source][preview1]). It intentionally does not provide `environ_get` or
`environ_sizes_get`; that gap prevents a stock `wasm32-wasip1` Rust program
from producing output through this runner.

## Pointers, not duplicate claims

- [Footprint measurements](benchmarks/footprint.md) are the source of truth for
  the minimal-embedder method, environment, and the explicit boundary that
  Telomere is not in WAMR's size class ([#139](https://github.com/yieldspace/telomere/issues/139)).
  This matrix intentionally repeats none of its measurements.
- [RELEASING.md](../RELEASING.md) and [CHANGELOG.md](../CHANGELOG.md) define
  versioning and release communication; packages remain `publish = false` until
  a maintainer decides otherwise ([#145](https://github.com/yieldspace/telomere/issues/145)).
- Disabling `threads` removes Tokio from the normal minimal dependency graph;
  CI guards that property with `cargo tree` in the
  [`minimal-embedder` job][minimal-ci]
  ([#138](https://github.com/yieldspace/telomere/issues/138)).

## Verified source snapshot

The baseline support, canonical ABI, and WASI facts in this document were
audited at committed source snapshot
[`2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba`][source-commit]. The #142
proposal-rejection mappings are verified by the
[`proposal_support` regression tests][proposal-support-tests] in this change;
this document does not claim a self-referential PR-head commit.

The committed baseline was inspected with these exact commands:

```shell
git rev-parse 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba
git show 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba:crates/telomere/tests/harnesses/wast.rs | sed -n '19,279p'
git show 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba:crates/telomere-component/src/decoder/canon/parse.rs | sed -n '1,37p'
git show 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba:crates/telomere-component/src/decoder/canon/options.rs | sed -n '1,128p'
git grep -n 'component-gated-feature' 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba -- crates/telomere-component
git grep -c 'func(' 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba -- crates/telomere-component-wasi/wit
git grep -n -E '^impl .*::Host for WasiHost|^    fn ' 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba -- crates/telomere-component-wasi/src/provider
git show 2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba:src/core_wasi_preview1.rs | sed -n '338,405p'
```

Re-run these current-fixture checks to reproduce the proposal and WAST claims:

```shell
cargo test -p telomere --release --test proposal_support
cargo test -p telomere --release --no-default-features --test proposal_support
cargo test -p telomere --release --test wast
cargo test -p telomere --release --no-default-features --test wast
```

[wast-policy]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere/tests/harnesses/wast.rs#L65-L99
[wast-collector]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere/tests/harnesses/wast.rs#L187-L279
[binary-divergence-table]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere/tests/harnesses/wast.rs#L19-L63
[binary-divergence-stale]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere/tests/harnesses/wast.rs#L148-L181
[canon-functions]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component/src/decoder/canon/parse.rs#L5-L37
[canon-options]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component/src/decoder/canon/options.rs#L4-L128
[component-features]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component/Cargo.toml#L41-L51
[wasi-version]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component-wasi/src/lib.rs#L16-L31
[wasi-wit]: https://github.com/yieldspace/telomere/tree/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component-wasi/wit
[wasi-provider]: https://github.com/yieldspace/telomere/tree/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component-wasi/src/provider
[wasi-sockets]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/crates/telomere-component-wasi/src/provider/sockets.rs#L11-L44
[preview1]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/src/core_wasi_preview1.rs#L338-L405
[minimal-ci]: https://github.com/yieldspace/telomere/blob/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba/.github/workflows/ci.yaml#L52-L91
[source-commit]: https://github.com/yieldspace/telomere/commit/2e46c7114ab0b2b4abc98ce4e40b2be95278e8ba
[proposal-support-tests]: ../crates/telomere/tests/proposal_support.rs
