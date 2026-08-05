# Cargo feature policy

This document is the single source of truth for Telomere's Cargo feature
policy. The [Cargo features section in the README](../README.md#cargo-features)
is the supported per-flag list; it does not redefine this policy.

## Library defaults

The runtime libraries `telomere`, `telomere-component`, and
`telomere-component-wasi` each enable `simd` and `threads` by default. This is
the supported library configuration. The
[footprint measurements](benchmarks/footprint.md) make their feature choices
explicit: their headline configurations retain the supported `simd` default and
the ladder documents the threads variant separately.

`telomere-minimal-embedder` deliberately has no default features: it is an
opt-in configuration ladder, not a library default. The CLI is an aggregator;
its default remains `full` and aggregates the supported configuration.

## Minimal dependency graphs

A minimal dependency is a graph-wide property, not a single dependency line.
For a project adjacent to this checkout, start with:

```toml
[dependencies]
telomere = { path = "../telomere/crates/telomere", default-features = false }
```

Apply `default-features = false` both to this direct `telomere` dependency and
to every other normal dependency in the graph that depends on `telomere` or a
Telomere-family crate. Cargo [unifies features across the dependency
graph](https://doc.rust-lang.org/cargo/reference/features.html#feature-unification):
one default-enabled edge, including one added by a careless `cargo add`, can
re-enable `threads` for every use of the resolved `telomere` package. A
downstream `default-features = false` cannot undo features enabled elsewhere.

## Forwarding roles

`simd` and `threads` gate code only in `telomere`. Other packages use a
deliberately smaller role:

- `telomere` is the definition crate: it owns both feature gates.
- `telomere-component`, `telomere-component-wasi`, and
  `telomere-minimal-embedder` are forwarders. Their `simd` and `threads`
  features forward each flag to every non-dev Telomere-family dependency that
  declares it. Optional dependencies use Cargo's `?/` forwarding form.
- `telomere-cli` is an aggregator. Its `full` feature reaches the required
  direct core, Component Model, and WASI feature paths without exposing separate
  CLI `simd` or `threads` knobs.

Every non-dev dependency edge between Telomere-family packages must set
`default-features = false`. A new workspace package must choose one of these
roles rather than introducing a separate SIMD or threads policy.

## `wide` feature removal

Optional dependencies are enabled through `dep:` entries, so there is no
`wide` feature. `simd` is the only supported knob for the optional `wide`
dependency. Issue #234 removed Cargo's former implicit `wide` feature.

## Scope and enforcement

The `fuzz/` directory is a separate Cargo workspace. It deliberately depends
on `telomere` with default features and is outside both this policy and the main
workspace's `cargo metadata` view.

[`tools/check-feature-wiring.py`](../tools/check-feature-wiring.py) enforces the
declared roles, forwarding paths, normal-edge `default-features = false` rule,
and bare-name references from declared features to optional dependencies when
they leak an implicit optional-dependency feature. The
[`Manifest feature wiring` CI job](../.github/workflows/ci.yaml) runs that
checker and structurally verifies the minimal-embedder graph's Tokio boundary,
including a positive control.

For feature costs, measurement methods, and supported configuration ladders,
use [the footprint measurements](benchmarks/footprint.md). This policy
intentionally does not repeat their measurements.
