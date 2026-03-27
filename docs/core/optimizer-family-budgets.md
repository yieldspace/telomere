# Optimizer Family Budgets

This document fixes the Phase 0 family-selection contract and the Phase 7
handler-layout contract referenced from `PLAN.md`.

## Phase 0

- `top-k`: `16`
- family priority: `local/control > memory > call/select`
- packed stream growth budget:
  - relative: `+10%`
  - absolute slack: `+8 instrs`
  - acceptance rule: the packed stream must fit within `original + max(10%, 8)`
- runtime handler count budget:
  - current handler count baseline: `264`
  - v1 ceiling: `288`
  - Phase 7 must improve layout without adding new handlers

## Candidate Groups

- `local/control`
  - `op_local_get4_br_if`
  - `op_local_get4_i32_eqz_br_if`
  - `op_local_get4_i32_const_compare_br_if`
  - `op_local_get4_local_get4_compare_br_if`
  - `op_local_get4_i32_const_add*`
  - `op_local_get4_local_get4_i32_add*`
- `memory`
  - `memory.local_base`
  - `memory.indexed_local_base`
- `call/select`
  - `call.direct`
  - `call.return_direct`
  - `call.indirect`
  - `call.return_indirect`
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
