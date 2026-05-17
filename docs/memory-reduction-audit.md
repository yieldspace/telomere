# Memory Reduction Audit

This note records the memory-focused audit after the relation-driven component
runtime and current core optimizer work. The default priority is compile-time
peak RSS, then runtime resident memory, then transient allocation. Public API
compatibility is preserved.

## Measurements

All measurements use macOS `/usr/bin/time -l`. Warm runs reuse target
directories so the RSS mostly reflects the Telomere test process rather than
rustc. Fresh Cargo target runs were also used during the audit for CI build
noise checks, but they are not the primary memory signal because Rust compiler
RSS dominates those runs.

| Case | Before max RSS | After max RSS | Delta |
| --- | ---: | ---: | ---: |
| `cargo test -p telomere-component --test component_model_wast --release`, warm target | 60588032 | 60407808 | -180224 |
| `cargo test -p telomere --test memory_semantics --release`, warm target | 60751872 | 60276736 | -475136 |
| ignored 32MiB cross-memory copy fixture, warm target | 104054784 | 70516736 | -33538048 |

The cross-memory copy fixture isolates a previous `Vec<u8>` temporary. The
after run copies through a fixed 4KiB stack buffer, so RSS drops by the copied
32MiB region.

## Implemented

Core parser and optimizer:

- `parse_vec` preallocates from the decoded length.
- type-checker stack snapshots use `SmallVec<[ValType; 8]>`.
- type-checker and jump-resolver block stacks use stack-shaped `Vec` instead of `VecDeque`.
- jump-resolver lazy target lists use `SmallVec<[u32; 4]>`.
- instruction generator unreachable state uses `SmallVec<[bool; 8]>`.
- CFG decoded operands use `SmallVec<[Operand; 2]>`, matching the common 0-2 operand shape.
- optimizer fail-closed paths no longer clone the materialized instruction stream eagerly.
- fallback materialized previews shrink `instrs` and `op_lens` capacity before storage.
- local reassign tables use `SmallVec<[(u32, ValType, u32); 8]>`.

Core runtime and memory:

- wasm body materialization during instantiate is delayed until direct-call recipe slots are available, avoiding an early `Arc<[Instr]>` and `Arc<[u16]>` that would be replaced later.
- cross-memory `memory.copy` prechecks source and destination ranges, then copies through a fixed 4KiB stack buffer instead of allocating `Vec<u8>` proportional to the copy length.
- component support exposes fixed-width memory reads as `[u8; N]` so scalar canonical ABI reads avoid tiny heap vectors.

Component compile/runtime:

- validator `ValueStore` snapshots shrink the returned `HashMap` capacity.
- `ComponentProgram` vectors and maps are shrunk immediately before the program is built.
- `RuntimeEnv::clone_shallow` shares kind caches through `Rc<RuntimeCaches>` instead of cloning every runtime cache `HashMap`.
- canonical ABI scalar reads use `[u8; N]`, and UTF-8 string lowering writes `&str::as_bytes()` directly without `to_vec()`.

## Audited And Left

- `LoweredOp.operands` remains `Vec<LoweredOperand>`. It is part of the long-lived canonical lowered artifact, and many lowered ops have no operands. A direct `SmallVec<[LoweredOperand; 2]>` would enlarge every `LoweredOp` record; the better future direction is a flat operand side table or packed encoding tied to the kernel-op registry.
- `Func.expr` / `Func.op_lens`, `LoweredFunction.code`, `materialized_preview`, and runtime `Arc<[Instr]>` are not fully unified. Parser and optimizer still need lowered code as the canonical artifact while fail-closed and diagnostics need exact materialized fallback. This pass removes eager duplicate materialization and shrinks fallback capacity without changing verifier semantics.
- `GlobalValue` uses a fixed 16-byte payload plus length so the JIT can address
  globals through one stable slot layout. This is a tradeoff: small scalar
  globals take more per-global storage than the previous width-specific enum,
  while native global get/set avoids a per-width layout side table. A future
  JIT layout descriptor could recover the scalar-global footprint without
  changing public semantics.
- `ComponentProgram.bytes` is retained for public API/debugging compatibility even though instantiate/call do not rescan bytes.
- relation stores stay as `HashMap<GlobalIdx<T>, Relation<T>>`. Dense tables need a stricter global-index density contract across nested components and aliases. That is a larger semantic refactor and should be done with dedicated validation tests.
- public `ComponentValue` keeps `String` / `Vec` forms for API compatibility. Borrowed views or `SmallVec` fields would be breaking and are documented as future candidates only.
- runtime instance exports stay as name maps. Sorting or dense export slots can reduce memory, but it changes lookup construction and error-path behavior and needs a focused compatibility pass.
- canonical ABI layout caches are limited to existing `type_infos`. More aggressive cache entries for every lowered value path may improve allocation churn, but they risk growing resident memory for components that compile many types and call only a few.

## Future API-Breaking Candidates

- Replace public collection-heavy `ComponentValue` variants with borrowed views or compact owned containers.
- Remove or feature-gate `ComponentProgram.bytes` once users do not rely on retrieving the original binary.
- Make `GlobalIdx` allocation dense by construction and replace relation/runtime cache maps with boxed slices.
- Encode `LoweredFunction` operands in a side table and keep `LoweredOp` records fixed-width and smaller.
