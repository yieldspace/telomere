# Core Optimizer

## Summary (English)

The body of this document is written in Japanese. It is the reference for the
core optimizer, which is now a single pipeline with `LoweredFunction` as the one
canonical artifact; switching optimizer implementations through features or
environment variables has been removed. It lists the seven stages - `ir`,
`analysis`, `transform`, `select`, `versioning`, `lower`, `encode` - and what
each one computes or applies, including GVN sites and loop-preheader hoist
candidates in analysis, LICM/PRE and reachability pruning in transform,
`FamilySpec`-registry kernel op selection, and bounded block cloning capped at
one generic plus two specialised versions with unprovable edge facts falling
back to generic. The runtime boundary section states that the parser always
keeps a `LoweredFunction` in `Func.lowered`, that `Func.expr` and `Func.op_lens`
are only a preview of `lowered.materialize()`, and that instantiate uses
`lowered.materialize_with_recipe_slots(...)` instead of the parser preview.
Status notes record that `optimize_function` is the sole entry point, that the
older pass/sink/versioning/expr modules were removed with no path back, and that
the runtime has no matcher and executes the optimizer-chosen op family through
direct-threaded handlers. The document ends with the regression commands to run.

core optimizer は、`LoweredFunction` を唯一の正本 artifact にした単一 pipeline である。外部 feature や環境変数で optimizer 実装を切り替える構成は廃止した。

## Pipeline

optimizer は次の 7 段を持つ。

1. `ir`
   `cfg::build_program` を入口に `CanonFunc` / `CanonBlock` / `CanonInst` / `BlockParam` を構築する。
2. `analysis`
   reverse postorder、idom、dominator tree、loop depth、live-in/live-out、effect version に加えて、GVN site、loop-preheader hoist candidate、`local.set/get` coalescing pair を計算する。
3. `transform`
   block-local const folding、const branch canonicalization を先に適用し、その後 analysis 出力を使って loop-invariant PRE/LICM、GVN-based local expression caching、`local.set/get -> local.tee` coalescing、reachability pruning を行う。
4. `select`
   `FamilySpec` registry で `KernelOp` を選ぶ。現在の registry は `local/control`、`call/select`、`memory` を持ち、local unary/binop/cmp、`local_get4_i32_const_add*` / `local_get4_local_get4_i32_add*`、`br_if` 系、`select4/8/16`、const-base / local-base / local-scaled-index memory family を直接選べる。
5. `versioning`
   bounded block cloning を late pass として適用する。上限は `generic 1 + specialized 2` で、証明できない edge fact は generic に落とす。
6. `lower`
   `KernelFunction` を `LoweredKernelFunction` に落とし、block label / family 付き op 列へ正規化する。
7. `encode`
   `LoweredFunction { code, const_pool, call_recipes, jump_table, block_map }` に変換し、const pool を組み立てたうえで instantiate 時に `Instr` へ materialize する。

## Runtime Boundary

- parser は常に `Func.lowered` に `LoweredFunction` を保持する
- `Func.expr` / `Func.op_lens` は `lowered.materialize()` の preview である
- runtime instantiate は parser preview を使わず、instance ごとの recipe slot を反映した `lowered.materialize_with_recipe_slots(...)` を実行コードにする

## Current Implementation Status

- optimizer の唯一の入口は `parser/core/optimizer/mod.rs::optimize_function`
- canonical module 境界は `parser/core/optimizer/pipeline/*`
- 旧 `pass.rs` / `sink.rs` / `versioning.rs` / `expr.rs` は廃止し、選択・versioning・encode はすべて `pipeline/*` にある
- fail-closed path は `LoweredFunction::from_materialized(...)` のみで、旧 optimizer 実装へ戻る経路はない
- runtime は matcher を持たず、optimizer が選んだ op family をそのまま direct-threaded handler で実行する。local unary/binop/cmp fast family は runtime 側で descriptor decode して実行し、handler 本体は decode 後の descriptor を読むだけに寄せている

## Regression Checks

- `cargo fmt --all`
- `cargo check -p telomere`
- `cargo clippy -p telomere --lib -- -D warnings`
- `cargo test -p telomere parser::core::optimizer --lib`
- `cargo test -p telomere --test optimizer_runtime`
