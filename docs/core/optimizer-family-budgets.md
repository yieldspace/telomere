# Optimizer Family Budgets

全体像と各 phase の as-built 説明は [`current-optimizer.md`](./current-optimizer.md) を参照。

This document fixes the Phase 0 family-selection contract and the Phase 7
handler-layout contract used by the current optimizer implementation.

## Phase 0

- `top-k`: `16`
- family priority: `local/control > memory > call/select`
- packed stream growth budget:
  - relative: `+10%`
  - absolute slack: `+8 instrs`
  - acceptance rule: the packed stream must fit within `original + max(10%, 8)`
- runtime handler count budget:
  - current handler count baseline: `285`
  - current as-built handler count: `288`
  - current delta: `+3` (`op_i32_load_const_base`, `op_i32_store_const_base_local4`, `op_i32_load_const_base_local_get4_i32_add_set4`)
  - Phase 7 keeps the fixed layout order while the scalar memory family, including shared/scaled-index handlers and bounded const-base fixed-cost families, stays within the current handler set

## Candidate Groups

- `local/control`
  - `op_local_get4_br_if`
  - `op_local_get4_i32_eqz_br_if`
  - `op_local_get4_i32_const_add_tee4_br_if`
  - `op_local_binop32`
  - `op_local_binop32_set_tee4`
  - `op_local_binop32_br_if`
  - `op_local_binop64`
  - `op_local_binop64_set_tee8`
  - `op_local_unary32`
  - `op_local_unary32_set_tee4`
  - `op_local_unary64`
  - `op_local_unary64_set_tee8`
  - `op_local_cmp`
  - `op_local_cmp_set_tee4`
  - `op_local_cmp_br_if`
- `memory`
  - `memory.local_base`
  - `memory.indexed_local_base`
  - `memory.shared_local_base`
  - `memory.indexed_shared_local_base`
  - `memory.local_scaled_index`
  - `memory.indexed_local_scaled_index`
  - `memory.shared_local_scaled_index`
  - `memory.indexed_shared_local_scaled_index`
  - `memory.const_base_load`
  - `memory.const_base_store_local4`
  - `memory.const_base_load_local4_add_set4`
  - relower matches scalar load/store in `const-base -> adjacent -> residual AddressShape -> temp-local fallback` order; residual `AddressShape` canonically covers `BaseOffset` and `ScaledIndexOffset`, accepts `EntryLocal` / `SpillLocal` / `TempLocal` 4-byte base/index locals, and keeps same-block `base +/- const` and `base + index * scale + const` on folded family paths before temp-buffering. Const-base specialization is bounded to default local memory `i32` load/store and `i32.load + local.get4 + i32.add + local.set4`; address-side temp-local normalization covers enumerable same-block and cross-block rooted `i32` trees, but same-block `MemoryLoad`-derived address roots stay on generic fallback after the current CoreMark safety guard. Store value side uses temp-local suffix normalization for scalar trees. Remaining memory-family fallbacks are same-block `memory-derived address root`, SIMD, atomics, bulk memory, wider const-base shapes, and cross-function rewrite
- `call/select`
  - `call.direct`
    - relower canonicalizes direct-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, trailing-suffix partial apply, replay-only pure trees blocked by `multi-use` / `needs_spill` / same-block memory-address sharing, and CFG-aware temp-local windowing for anchored / merge-fed / mixed sites across same-block store/control boundaries and enumerable predecessor-edge regions, without adding handlers; standalone inner `return_call*` result, cross-function consumers, non-enumerable incoming edges, and type-mismatched edge merges stay on generic fallback
  - `call.return_direct`
    - relower canonicalizes direct-return-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, trailing-suffix partial apply, replay-only pure trees blocked by `multi-use` / `needs_spill` / same-block memory-address sharing, and CFG-aware temp-local windowing for anchored / merge-fed / mixed sites across same-block store/control boundaries and enumerable predecessor-edge regions, without adding handlers; standalone inner `return_call*` result, cross-function consumers, non-enumerable incoming edges, and type-mismatched edge merges stay on generic fallback
  - `call.import_direct`
    - relower canonicalizes import direct-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, trailing-suffix partial apply, replay-only pure trees blocked by `multi-use` / `needs_spill` / same-block memory-address sharing, and CFG-aware temp-local windowing for anchored / merge-fed / mixed sites across same-block store/control boundaries and enumerable predecessor-edge regions, without adding handlers; standalone inner `return_call*` result, cross-function consumers, non-enumerable incoming edges, and type-mismatched edge merges stay on generic fallback
  - `call.return_import_direct`
    - relower canonicalizes import direct-return-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, trailing-suffix partial apply, replay-only pure trees blocked by `multi-use` / `needs_spill` / same-block memory-address sharing, and CFG-aware temp-local windowing for anchored / merge-fed / mixed sites across same-block store/control boundaries and enumerable predecessor-edge regions, without adding handlers; standalone inner `return_call*` result, cross-function consumers, non-enumerable incoming edges, and type-mismatched edge merges stay on generic fallback
  - `call.indirect`
    - relower canonicalizes indirect-call arg and table-index materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, trailing-suffix partial apply, replay-only pure trees blocked by `multi-use` / `needs_spill` / same-block memory-address sharing, and CFG-aware temp-local windowing for anchored / merge-fed / mixed sites across same-block store/control boundaries and enumerable predecessor-edge regions, without adding handlers; standalone inner `return_call*` result, cross-function consumers, non-enumerable incoming edges, and type-mismatched edge merges stay on generic fallback
  - `call.return_indirect`
    - relower canonicalizes indirect-return-call arg and table-index materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, trailing-suffix partial apply, replay-only pure trees blocked by `multi-use` / `needs_spill` / same-block memory-address sharing, and CFG-aware temp-local windowing for anchored / merge-fed / mixed sites across same-block store/control boundaries and enumerable predecessor-edge regions, without adding handlers; standalone inner `return_call*` result, cross-function consumers, non-enumerable incoming edges, and type-mismatched edge merges stay on generic fallback
  - `select.4`
  - `select.8`
  - `select.16`

## Phase 7 Layout Order

The runtime source order is fixed to this logical order:

1. `locals`
2. `superinstructions`
3. `memory`
4. `call`
5. `control`
6. `numeric`
7. `globals`
8. `tables`
9. `refs`
10. `bulk_memory`
11. `atomics`
12. `simd`
13. `traps`

Layout retuning is only allowed after the family set stays stable for `2`
consecutive phases. Handler replication remains out of scope for v1.
