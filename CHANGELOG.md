# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- A standalone minimal-embedder configuration ladder, size-oriented release
  profile, and footprint harness/CI coverage for measured file size, text, RSS,
  and whole-process cold start.
- Opt-in Store-scoped interpreter metering with finite or unlimited fuel,
  cumulative fuel accounting, and cross-thread cancellation through
  `MeteringHandle`, including per-checkpoint cancellation observation and
  proportional precharges for native bulk operations. Fuel exhaustion and
  cancellation are exposed as payload-free `VMResult` variants with
  `InterruptReason` conversion helpers.

### Changed

- Defined the Cargo feature policy and simplified feature wiring: the CLI keeps
  its `full` default while forwarding `simd` and `threads` directly, and
  `telomere` no longer has its redundant `full` alias.
- Curated the default `telomere` embedder surface: module AST representation
  (except the `host_abi::CodeSection` compatibility exception) and raw
  `Op`/`Operand` are internal, while the default host-linking compatibility
  carve-out is documented and frozen through `host_abi` plus the public-surface
  snapshot. Auxiliary interpreter representations and helpers are opt-in through
  `unstable-internals`; reduction and replacement of the default-public raw host
  ABI remain tracked by #216. The documented JIT cache statistics surface stays
  public with `jit` so the minimal-embedder JIT configuration remains stable.

### Fixed

- No unreleased entries yet.

### Removed

- No unreleased entries yet.

### Security

- Metered Stores disable the experimental JIT, preventing a fuel-configured
  Store from executing an unmetered native JIT path.

## [0.1.0-alpha.1] - 2026-08-03

This is the planned, provisional tag date. If the release PR's merge or tag
date moves, update this heading before merging the release PR. This first entry
is a snapshot of the project's current state, not a difference from an earlier
release.

### Added

- The `telomere` core WebAssembly parser, validator-facing optimizer,
  interpreter, host-linking APIs, and optional experimental baseline JIT.
- Component Model decoding, validation, linking, and runtime support through
  `telomere-component`, plus WIT binding generation through
  `telomere-component-bindgen`.
- A partial WASI 0.2.6 component provider in `telomere-component-wasi`, and
  core WASI preview1 and component command paths in `telomere-cli`.
- Runnable core, preview1, and component fixtures, together with the upstream
  core Wasm testsuite submodule and local Component Model test suite.

### Changed

- The eight crates use one workspace version and remain intentionally
  unpublished to crates.io (`publish = false`).
- Minimal `telomere` and `telomere-component` normal dependency graphs exclude
  Tokio; threaded and async support remains feature-gated.

### Fixed

- In-place return-frame sizing for host and async-host calls that return values.

### Removed

- No removals are recorded in this initial snapshot.

### Security

- There is no supported version line or backport branch. Pre-release tags carry
  no support commitment; fixes land on `main`.
