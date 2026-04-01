# Current Core Optimizer

このリポジトリの core optimizer は、再設計後の単一 pipeline を正本として扱う。外部バージョン切替はなく、parser は常に `LoweredFunction` を生成し、runtime instantiate は常に instance ごとの recipe slot を反映した `materialize_with_recipe_slots(...)` を経て実行する。

現状の構成は次のとおり。

1. `build_program` で CFG を組む
2. `CanonFunc` ベースの IR を作る
3. `analysis` をかける
   ここで dominator / loop / liveness / effect version に加えて、GVN site、loop-preheader hoist candidate、`local.set/get` coalescing pair も出す
4. `transform` で const folding / branch canonicalization を先に行い、その後 analysis を再計算して PRE/LICM、GVN-based caching、slot coalescing を適用する
5. `select` と late `versioning` で `KernelOp` / specialized block を決める。memory family は const-base に加えて local-base / local-scaled-index を含む
6. `lower` で `KernelFunction` を family 付き `LoweredKernelFunction` へ落とす
7. `encode` で `LoweredFunction` と const pool を作る

旧 monolith (`pass.rs` / `sink.rs` / `versioning.rs` / `expr.rs`) は残していない。optimizer の failure shield は `LoweredFunction::from_materialized(...)` だけで、旧 optimizer backend へ委譲する経路はない。

runtime の local fast family は hand-written matcher ではなく descriptor decode を正本にし、handler は decode 後の descriptor を使って direct-threaded に実行する。

詳細は [`optimizer.md`](./optimizer.md) を参照。
