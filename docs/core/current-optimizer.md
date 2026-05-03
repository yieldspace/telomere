# Current Core Optimizer

このリポジトリの core optimizer は、再設計後の単一 pipeline を正本として扱う。外部バージョン切替はなく、parser は常に `LoweredFunction` を生成し、runtime instantiate は常に instance ごとの recipe slot を反映した `materialize_with_recipe_slots(...)` を経て実行する。

現状の構成は次のとおり。

1. `build_program` で CFG を組む
2. `CanonFunc` ベースの IR を作る
3. `analysis` をかける
   ここで dominator / loop / liveness / effect version に加えて、GVN site、loop-preheader hoist candidate、`local.set/get` coalescing pair も出す
4. `transform` で const folding / branch canonicalization を先に行い、その後 analysis を再計算して PRE/LICM、GVN-based caching、slot coalescing を適用する
5. `select` と late `versioning` で `KernelOp` / specialized block を決める。memory family は scalar load/store の `const-base` / `local-base` / `local-scaled-index` を local/shared/default/indexed で扱い、same-block の single-use `memory-derived address root` は temp-local buffering で family 側へ寄せる
6. `lower` で `KernelFunction` を family 付き `LoweredKernelFunction` へ落とす
7. `encode` で `LoweredFunction` と const pool を作る。隣接 lowered op だけで安全に判定できる hot tail はここで追加 fusion する

旧 monolith (`pass.rs` / `sink.rs` / `versioning.rs` / `expr.rs`) は残していない。optimizer の failure shield は `LoweredFunction::from_materialized(...)` だけで、旧 optimizer backend へ委譲する経路はない。

runtime の local fast family は hand-written matcher ではなく descriptor decode を正本にし、handler は decode 後の descriptor を使って direct-threaded に実行する。

## Research basis

The current memory-family expansion follows the same load-time specialization boundary as WAMR's fast interpreter design: resolve/reshape provider bytecode during loading, reduce stack traffic and decode work, and keep runtime dispatch simple. It also applies the interpreter literature's superinstruction direction from Ertl/Gregg/Casey by combining hot VM instruction sequences without changing Telomere's existing tail-call threading contract.

- WAMR fast interpreter: load-time handler/predecode, provider removal, and stack-to-slot/register style lowering
  https://www.intel.com/content/www/us/en/developer/articles/technical/webassembly-interpreter-design-wasm-micro-runtime.html
- Casey/Ertl/Gregg, "Optimizing Indirect Branch Prediction Accuracy in Virtual Machine Interpreters": superinstructions reduce interpreter dispatch work and BTB pressure
  https://www.scss.tcd.ie/David.Gregg/papers/toplas05.pdf
- Titzer, "A fast in-place interpreter for WebAssembly": Wasm is compact and fast to validate/compile, but direct interpretation needs careful execution-tier design
  https://arxiv.org/abs/2205.01183

This is why the optimizer keeps `call_next` / `call_code` and `op_call` / `op_return_call` dispatch identity intact: the implemented boundary is load-time selection, operand predecode, and family-specific handlers, not a runtime engine replacement.

## Static verification boundary

There is no Verus proof in tree yet. The current build/test shield is a fail-closed static verifier in the optimizer pipeline:

- `CanonFunc` is built from parser-recorded stack snapshots (`stack_before` / `stack_after`) and block metadata
- `transform::verify_block_stacks` checks that every rewritten block's instruction metadata still forms a contiguous stack transition from block params to block exit
- `analysis`, `select`, `versioning`, `lower`, and `encode` each run `verify`; any failure returns the original materialized instruction stream
- runtime tests cover the optimized families that cannot currently be proven from handler signatures alone

The verifier proves optimizer IR stack-continuity before execution and prevents malformed optimized output from replacing the original stream. It does not yet prove every runtime handler's byte-level stack effect against a formal spec.

memory family の current as-built は次のとおり。

- scalar load/store (`i32/i64/f32/f64` と既存 narrow integer load/store) は `memory.local_base` / `memory.indexed_local_base` / `memory.shared_local_base` / `memory.indexed_shared_local_base` / `memory.local_scaled_index` / `memory.indexed_local_scaled_index` / `memory.shared_local_scaled_index` / `memory.indexed_shared_local_scaled_index` へ落ちる
- bounded const-base は default local memory の `i32/i64/f32/f64` full-width load/store に入っている。既存の fused path は `i32.load + local.get4 + i32.add + local.set4` だけを維持する
- CoreMark hot path 向けに、`i32` counter increment (`load + const 1 add + store`)、signed 16-bit dot4 loop、state-machine の `local.get + const and/add + compare + br_if`、`local` copy + const-compare branch、`local add/set + load8_u tee eqz branch` と `load/tee + load8_u/tee branch` tail fusion を追加している
- same-block `memory-derived address root` は scalar `memory.load*` の single-use / only-once / no-replay ケースだけ temp-local buffering で family path に寄せる
- 残る fallback は SIMD memory、atomics、bulk memory、hard `memory-derived address root`、narrow const-base、cross-function rewrite

詳細は [`optimizer.md`](./optimizer.md) を参照。
