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
  - v1 ceiling: `288`
  - Phase 7 must improve layout without adding new handlers

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
- `call/select`
  - `call.direct`
    - relower canonicalizes direct-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, and trailing-suffix partial apply, without adding handlers
  - `call.return_direct`
    - relower canonicalizes direct-return-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, and trailing-suffix partial apply, without adding handlers
  - `call.import_direct`
    - relower canonicalizes import direct-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, and trailing-suffix partial apply, without adding handlers
  - `call.return_import_direct`
    - relower canonicalizes import direct-return-call arg materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, and trailing-suffix partial apply, without adding handlers
  - `call.indirect`
    - relower canonicalizes indirect-call arg and table-index materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, and trailing-suffix partial apply, without adding handlers
  - `call.return_indirect`
    - relower canonicalizes indirect-return-call arg and table-index materialization, including stable slot alias, safe scalar select trees, const-like `ref/v128` zero-input leaves, nested `op_call*`, numeric trap-sensitive trees, `global.get`, `table.get`, contiguous `memory.load` leaves, and trailing-suffix partial apply, without adding handlers
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
