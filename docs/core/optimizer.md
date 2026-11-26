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
direct-threaded handlers. A closing section records the #114 loop-dispatch
fallback: an opt-in build mode in which the interpreter's dispatch does not
depend on tail-call optimisation, with its measured cost. The document ends
with the regression commands to run.

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

## Loop-dispatch fallback (#114)

runtime の direct-threaded dispatch は、各 handler が `call_next` / `call_code` 経由で次の handler を末尾呼び出しすることに依存している。Rust は sibling call を保証しないため、この契約は toolchain の codegen 次第である。実測では 1.97.0 以降（1.97.0 / 1.98.0 / 1.99.1 / 1.100.0 で確認）で `op_call` の callee への dispatch が通常の `call` に編まれ、guest call のたびに約 88 B の host frame が segment 終了まで残るようになった（call cycle あたり約 42–168 B の消費として観測）。pin している 1.96.0 では従来どおり `jmp` 形である。

fallback はこの契約が壊れた toolchain でも正しく動くための build configuration であり、移行先ではない。

- 選択機構: telomere crate の cargo feature `loop-dispatch-fallback`。既定は無効で、無効時の主経路のコード生成は不変（`op_call` の逆アセンブルで `jmp` 形の維持を確認済み）。
- 機構: 有効時、`call_code` は次の handler を dispatch せず、次の命令を `ctx.cont` に記録して trampoline driver（`runtime::vm::drive_loop_dispatch`）へ戻る。driver は 1 iteration に 1 handler だけを dispatch するため、host stack 深度は命令数にも guest の call 深度にも比例しない。segment の入口（scheduler、JIT の interpreter-continuation bridge / stub、lazy-call 継続入口）は全て driver 経由に揃えてあり、trap・suspend・完了・JIT stop の segment 終端の意味は threaded 版と同一。
- 実測コスト（fleur、x86_64、Rust 1.96.0、criterion、median、2 反復）: threaded を 1.00 とした fallback の時間比は、fib 約 1.19、return_call_chain 約 1.10、tail_recursive_accumulate 約 1.15、scalar_local_loop 約 1.59–1.73、memory_load_store_loop 約 1.78–1.98。dispatch 密度の高い形状ほど高くつく。これは安全網の価格であり、主経路の性能目標とは別に扱う。
- 検証: 1.97.0 上で fallback 有効時、call loop（200,000 cycle）が 2 / 4 / 8 / 32 MiB の各スタックで完走する（threaded 版は 2 / 8 MiB で overflow、感度テスト形状の 4 MiB でも abort）。pin では両モードとも通常スイートが全 pass し、vendored component suite の数値（files=79 ran=21 checked=774）は両モードで不変。
- 性能 gate（`tools/measure-performance.py` / `perf-baseline.json`)は default / jit 構成の CLI を測るもので、どちらの構成も本 feature を有効化しないため gate のセルに fallback は含まれない。feature 無効時の A/B（base vs 本変更、同一 bench）では fib と return_call_chain は符号が反復間で一致せず、最もタイトな dispatch loop でも差は ±5% の測定ノイズ帯域内だった。

## Regression Checks

- `cargo fmt --all`
- `cargo check -p telomere`
- `cargo clippy -p telomere --lib -- -D warnings`
- `cargo test -p telomere parser::core::optimizer --lib`
- `cargo test -p telomere --test optimizer_runtime`
