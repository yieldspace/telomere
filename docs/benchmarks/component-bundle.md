# Component bundle load measurements

Local measurement for #201 (component bundles), taken on
fleur-du-lapin (Ryzen 9 5900X, release profile) with
`crates/telomere-component/benches/component_bundle_load.rs`:

```sh
cargo bench -p telomere-component --features aot-bundle \
    --bench component_bundle_load
```

## Method

Fixtures are the component binaries in `examples/`. Every figure is
the median of 51 repetitions after one warm-up round, timed with
`std::time::Instant` on one thread.

- **compile** — the plain path: `ComponentEngine::compile` over the
  component binary (component-envelope decode plus one core
  `parse_module` per embedded core module).
- **core parse (sum)** — the core front end alone: the fixture's
  CoreModule section payloads, extracted with the bundle section
  walker, timed through `WasmParser::parse_module` one by one.
- **envelope (derived)** — `compile − core parse (sum)`: what the
  component layer itself costs (section decode, validation, type
  construction, the per-module contract check). This is the
  component-envelope decode cost the issue asked to have reported
  separately; it is derived from the two timed inputs, not timed
  directly.
- **bundle load** — `ComponentEngine::compile_bundle` over a bundle
  built from the same fixture (envelope parse, signature posture,
  keyed-set pre-check, per-artifact open, component decode with
  substitution).
- **artifact open (sum)** — the per-artifact share of the bundle
  path: each section's standalone artifact opened through
  `telomere::open_aot_artifact`, summed.
- **bundle build** — the offline producer (parse + emit per
  section), reported for completeness; it is not part of load.

## Results (2026-11-25, fleur-du-lapin)

| Fixture | Bytes | Sections | compile | core parse (sum) | envelope (derived) | bundle load | artifact open (sum) | bundle build | bundle size |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| component-add.wasm | 187 | 1 | 37.7 µs | 18.4 µs | 19.4 µs | 183.7 µs | 187.7 µs | 160.7 µs | 1,396 B |
| wasi-component-args.wasm | 873 | 3 | 213.3 µs | 142.9 µs | 70.3 µs | 609.0 µs | 534.0 µs | 576.6 µs | 6,544 B |
| wasi-component-stdout.wasm | 2,868 | 4 | 305.4 µs | 122.2 µs | 183.2 µs | 922.4 µs | 741.3 µs | 704.6 µs | 9,976 B |

A second run of the same binary produced the same picture within
noise (compile 54.2 / 228.6 / 281.2 µs; bundle load 280.8 / 593.5 /
858.6 µs). These fixtures are tiny, so absolute values are
microsecond-scale and noisy; the ratios are the content.

## Reading

- The component envelope is a minority of plain load on these
  fixtures: roughly half of `component-add` (19.4 of 37.7 µs) and a
  quarter to three-fifths of the WASI fixtures. It is paid on both
  paths — the bundle does not serialize component-level state, by
  design — and is now measured rather than implied away.
- What the bundle path provably skips is the core parse (18–143 µs
  per fixture). What it pays instead is the per-artifact open
  (188–741 µs summed), which dominates bundle load: opening a #198
  artifact (precompiled deserialize, structural validation of the
  native function tables, fingerprint and binding checks) has a
  fixed per-artifact cost of roughly 170–190 µs on this host,
  consistent with the #198 corpus aggregate (227 ms over 1,346
  modules). For fixtures whose core modules parse in tens of
  microseconds, bundle load is therefore *slower* than plain
  compile end-to-end.
- The trade reverses where the core front end is the expensive
  part: parse cost scales with module size and function count,
  while the open cost is comparatively flat per artifact. The
  bundle's other half of the win is at instantiation, not load:
  native blobs are installed instead of being emitted on demand.
- Bundle size is 3.5–7.5× the component binary on these fixtures,
  in line with the core artifact ratios (#196/#198): the
  precompiled form of each embedded module plus its native code.
