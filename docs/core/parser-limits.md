# Parser recursion limits

Telomere bounds the two guest-input-driven parser recursions that previously
made acceptance depend on the host thread stack. The limits are public so an
embedder can inspect the policy:

- `telomere::MAX_CONTROL_NESTING_DEPTH = 512` for structured core control flow.
- `telomere_component::MAX_COMPONENT_NESTING_DEPTH = 100` for component
  sections and component/instance type declarations together.

## Depth convention

`depth` is the number of enclosing constructs. The root starts at `depth = 0`,
and a parser rejects on `depth > LIMIT`, not on `depth >= LIMIT`. Consequently,
the core parser accepts 512 nested control constructs and rejects 513; the
component decoder accepts 100 nested component/type constructs and rejects 101.
The shared component counter means a document cannot alternate nested component
sections with component or instance type declarations to obtain two separate
budgets.

## Optimized-build stack contract

In an optimized build, the input-driven recursion in `parse_instrs` /
`parse_inst` and in `_parse_component` / `parse_component_type` /
`parse_instance_type` is explicitly depth-bounded. The release fuzz-corpus
replay harnesses use a 512 KiB thread for that bounded input-decoder scope.

The numerical 512 KiB / 471,272 B composition is narrower still: it is the
measured core-control plus component cycle-A (nested section) composition.
Component cycle B shares the same limit, so it cannot grow without bound, but
its per-level stack cost is unmeasured and it is not included in that measured
stack-budget arithmetic.

It is **not** a guarantee for `ComponentEngine::compile` as a whole. In
particular, a component's type graph is materialized and then walked by code
outside this input-decoder recursion. Those paths require their own bound,
error, and tests; they are follow-up work under #120.

The measured composition that re-enters the core parser from nested component
sections is:

```
512 × 681 B   = 348,672 B
100 × 1,226 B = 122,600 B
                ---------
                  471,272 B  (about 460.2 KiB)
```

`512 KiB` is 524,288 B, leaving 53,016 B in that measured composition. This is
a local stack-budget calculation for the bounded input-driven paths, not a
claim that every accepted `ComponentEngine::compile` call has that stack bound.

## Measurements and their limits

The measurements were made in a release build on macOS arm64. For each path,
the smallest surviving thread stack was bisected at 8 KiB resolution for
100, 200, 400, and 800 nesting levels, then the linear per-level cost was
derived.

- Core structured control (`parse_instrs` / `parse_inst`): 681 B per level.
- Component cycle A (nested component sections through `_parse_component`):
  1,226 B per level.
- Component cycle B (nested component/instance type declarations): unmeasured.

The 100-depth component and instance type boundary fixtures also compile on a
512 KiB release thread; this boundary evidence does not measure cycle B's
per-level slope.

Cycle B must not be assigned cycle A's number by inference. The available probe
was run from a different worktree with `default-features = false`, so it does
not describe this configuration. The current implementation also rejects input
above 100 before an equivalent deeper measurement can be taken. A matching
cycle-B measurement remains required before treating the arithmetic above as a
per-level measurement for that different call shape.

## Deliberate component-model strictness

The component limit is shared across section and type nesting. `wasmparser`'s
100-limit applies to structural component type depth rather than this combined
measure, and its nested component-section rule is a count rather than the same
recursive-depth counter. Telomere is therefore deliberately stricter for mixed
section/type nesting; this is not a wasmparser-parity claim.

## Unbounded paths outside this change

Five type-graph walks on the component compile path remain unbounded in depth:

- validator `compute_effective_type_size`;
- validator `contains_resource_handle_with_tracker`;
- validator `resource_owner_summary`;
- validator subtyping;
- engine `build_type_infos` through `type_info` / `valtype_info`.

They use cycle tracking, but do not have a depth limit. They are intentionally
outside this parser-recursion change and belong to #120.

## Debug builds and #154

This contract is optimized-build only. In an unoptimized build, `parse_inst`'s
frame is about 130 KiB, roughly 190 times its 681 B release frame. A limit low
enough to fit a normal debug test thread would reject valid conformance inputs,
so it does not resolve #154. The replay harnesses accordingly use 512 KiB only
in release and retain a 64 MiB debug stack while #154 is addressed separately.

## Reproducible regression fixtures

`tools/generate_recursion_regressions.py` produces the canonical core and
component nesting fixtures. It records the expected size and SHA-256 for each
generated file and checks those values against the committed bytes; it also
checks the preserved component stack-overflow artifact. Run:

```sh
python3 tools/generate_recursion_regressions.py --check
```

The deep core fixture is 36,028 bytes, and the deep component fixture is
152,106 bytes.
