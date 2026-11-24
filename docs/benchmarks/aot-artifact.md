# AoT artifact measurements

Measurements for the AoT artifact container
([contract](../core/aot-artifacts.md), #198), taken on
fleur-du-lapin (Ryzen 9 5900X host, release profile, this change's
tree) with `crates/telomere/examples/aot_cold_start.rs`. Method:
aggregate in-process times over the fuzz-corpus `parse_core_module`
directory (the wasm-testsuite spec corpus modules extracted by the
fuzz corpus generator, 1,346 parseable modules, 5,895 defined
functions), one pass, no warm-up separation — the same corpus and
posture as the #196 codec measurements, so the numbers are
comparable to `benchmarks/precompiled-codec.md`.

| Phase | Aggregate |
| --- | --- |
| `WasmParser::parse_module` (parse + validate + optimize) | 232.3 ms |
| `build_aot_artifact` (offline production: serialize + emit + checksum + sign) | 260.5 ms |
| `open_aot_artifact` (verify + fingerprint + binding + deserialize + validate) | 227.1 ms |

Sizes: wasm 293,619 B → AoT artifacts 10,031,188 B (**~34×**). The
precompiled module inside accounts for ~4.2× of that (see the #196
measurements); the rest is native code for 5,895 functions, which
is intrinsically larger than the wasm it was compiled from.

## Reading these numbers honestly

- Opening an artifact is **not** faster than parsing on this corpus
  (227 ms vs 232 ms). The #196 deserialize step alone is ~90 ms of
  the open cost; the remainder is the fail-closed work this issue
  exists to add: signature verification over every byte, the
  module-binding digest, and per-function layout checksums that
  re-derive each function's materialized form. Tiny spec-corpus
  modules amortize none of it.
- What the load path provably skips is the *emit* phase and the
  optimizer's per-function compile work at run time: production
  (260 ms aggregate) happens offline, once, on the producer's
  machine. The phase counters that prove the skip are structural —
  `open_aot_artifact` contains no parser, optimizer, or emitter
  call, and installation patches pre-emitted bytes only.
- Whole-process cold start (the #196 CLI pair) has no AoT
  counterpart yet because the CLI does not produce or load AoT
  artifacts in this change; the in-process aggregates above are
  the recorded numbers, whatever they are.
