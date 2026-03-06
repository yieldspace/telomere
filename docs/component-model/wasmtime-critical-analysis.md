# Wasmtime 批判的分析（Telomere 新 Component Runtime 向け）

## 対象
- Wasmtime の埋め込み設計を、軽量 component runtime を作る観点で評価する。
- ここでは「まず動く」ことと、常駐メモリの単純性を優先する。

## 観察
1. `Engine` と `Store` の分離は安全だが、`Store` を長寿命にすると保持オブジェクトが増えやすい。
- 参照: https://docs.wasmtime.dev/contributing-architecture.html

2. コンパイル戦略の既定は Cranelift 寄りで、起動時コストと実行用メモリの管理が重くなりやすい。
- 参照: https://docs.wasmtime.dev/api/wasmtime/enum.Strategy.html
- 参照: https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html

3. Pulley は可搬性が高い一方、性能は native 実行より大きく劣るケースが明示されている。
- 参照: https://docs.wasmtime.dev/examples-pulley.html

4. pooling allocator は高速化に有効だが、事前上限の設計を誤るとメモリ利用に悪影響が出る。
- 参照: https://docs.wasmtime.dev/examples-fast-instantiation.html

5. backend/target の対応差があり、embedded Linux を広く同時最適化するのは初期段階で難しい。
- 参照: https://docs.wasmtime.dev/stability-tiers.html
- 参照: https://docs.wasmtime.dev/stability-platform-support.html

## Telomere での判断
- 初期実装では JIT/AOT を採用せず、component 層は軽量インタプリタ方式で実装する。
- decode/validate と実行を分離し、`ComponentProgram` を小さな線形 IR として保持する。
- API は刷新し、旧 component API は移行対象外とする。
- core wasm runtime の `instantiate` / `run_module_function` が async なので、component 側の `instantiate` / `call` も async に揃える。
- `Store` が `Rc<RefCell<_>>` で `!Send` のため、`tokio::spawn` ではなく caller task 上で await する local async を採用する。

## 採用しないもの（初期）
- AOT/JIT backend
- async component ABI
- `Store` の `Send` 化を前提にした multi-thread task dispatch
- GC/threads 連動機能
