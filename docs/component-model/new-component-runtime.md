# New Component Runtime Spec

図つきの実装解説は [relation-driven-runtime.md](relation-driven-runtime.md) を参照。

## Public API
- `ComponentEngine::compile(bytes: &[u8]) -> Result<ComponentProgram, ComponentError>`
- `ComponentEngine::instantiate(program: &ComponentProgram, store: &mut Store, linker: &ComponentLinker) -> impl Future<Output = Result<ComponentInstance, ComponentError>>`
- `ComponentInstance::call(store: &mut Store, name: &str, args: &[ComponentValue]) -> impl Future<Output = Result<Vec<ComponentValue>, ComponentError>>`
- `ComponentInstance::get_func(name: &str) -> Result<ComponentFunc, ComponentError>`
- `ComponentInstance::get_typed_func<P, R>(name: &str) -> Result<TypedComponentFunc<P, R>, ComponentError>`
- `TypedComponentFunc<P, R>::call(store: &mut Store, params: P) -> impl Future<Output = Result<R, ComponentError>>`
- `ComponentLinker::register_import_async` / `register_export_async`
- `ComponentLinker::register_import_typed_async` / `register_import_typed`
- `ComponentLinker::register_import_core` / `register_export_core`
- `ComponentLinker::register_import` / `register_export` は `ready` future へ包む同期ラッパーとして維持する

## Host 関数の実行契約（決定記録, #147）

component runtime の host 関数は同期実行であり、`register_import_async` などの
`*_async` 登録は executor で駆動されない。guest から呼ばれた時点で、返された
future は no-op waker で **ちょうど1回だけ** poll される。初回 poll で
`Poll::Pending` を返した future は二度と poll されず、呼び出しは失敗する。
ready な future の await は初回 poll の中で完結するので問題ない。

失敗の表面化は経路によって異なる。

- component レベルの呼び出し（lift 済み export 経由）: `ComponentError::Runtime`
  が、契約と対処を記したメッセージ付きで返る。
- canon lower 経由（core module の中から呼ばれる）: core host 境界は
  `ComponentError` を運べないため、guest の `unreachable` trap として表面化する。
- `register_import_resource_drop_async` のデストラクタも同じ first-poll 契約で、
  失敗は guest trap として表面化する。

### 決定: (b) 契約の明示 — async 登録面の削除・改名はしない

issue #147 の選択肢のうち (b) を採る。(a) async 登録面の削除・改名を採らない理由:

- 削除・改名は bindgen 生成コード（`HostAsync` / `ImportsAsync` trait と
  `add_to_linker_async` 系）と WASI provider、埋め込み側すべてに及ぶ破壊的変更で、
  意味上の利益がない。同期形（`register_import` 等）は既に併存している。
- future を返す形は、本物の非同期実行が将来必要とする形そのものである。
  core runtime は #216 で `ExecutionDriver` 経由の本物の非同期 host 実行を得ており、
  今改名すると、そちらへ接続する際に二度目の破壊的変更が要る。

代わりに行ったこと:

- 全ての `*_async` 登録メソッド、`ComponentFuture`、bindgen が生成する
  async trait / 登録関数の doc に first-poll 契約を明記した
  （`register_func_async` にあった「runtime のローカル実行コンテキストで
  poll される」という実在しない説明も修正した）。
- 失敗時のエラーメッセージ自体に、契約と対処（ready な future を返すか
  同期形で登録するか）を含めた。
- 契約は `crates/telomere-component/tests/component_runtime_e2e.rs` の
  first-poll テスト群で固定している。

### 本物の非同期実行に必要なもの（ロードマップ）

- component host trampoline を core の `ExecutionDriver` 上の async host
  function として登録し、host future の `Pending` で core 実行自体を
  suspend / resume できるようにすること。
- canon lower / lift の途中状態（lift 済みの引数、結果領域）を resume 可能な
  形に分割すること。現行の実装は1回の同期呼び出しで完結する前提で書かれている。
- component の `Store` 状態は `Rc` ベースで非 `Send` のため、駆動できる
  executor とスレッドの制約を API として確定させること。

## Internal Layout
- `crates/telomere-component/src/decoder`: 独自 decoder + validator
- `crates/telomere-component/src/validate`: validator 再公開
- `crates/telomere-component/src/ir`: component intermediate data model
- `crates/telomere-component/src/runtime`: 実行ディスパッチ

## IR
- `ComponentProgram` は次を保持する。
  - root component definition
  - dense type table (`types`)
  - canonical ABI metadata (`type_infos`)
  - component / instance / func / core module / core instance / core func / core memory / core table の relation store snapshot
- runtime は `bytes` の再走査や export 名 fallback に依存せず、この IR snapshot を index ベースで解決する。
- original `bytes` は public API / debugging compatibility のため保持するが、instantiate / call の実行経路では使わない。

## Wasmtime Sync Parity Boundary
この runtime の parity 目標は Wasmtime の同期 component API に合わせている。

- dynamic values:
  - `bool`
  - `char`
  - 整数 / 浮動小数
  - `string`
  - `list`
  - `record`
  - `tuple`
  - `variant`
  - `enum`
  - `flags`
  - `option`
  - `result`
  - `own` / `borrow` resource handle
- typed API:
  - scalar / `bool` / `char`
  - `String` / `&str`
  - `Vec<T>`
  - tuple `0..=8`
  - `Option<T>`
  - `Result<T, E>`
- parser / validator feature coverage:
  - nested names (`CM_NESTED_NAMES`)
  - fixed-length lists (`CM_FIXED_LENGTH_LISTS`)

`record` / `variant` / `enum` / `flags` は dynamic API では扱うが、typed derive や reflection は追加していない。embedded Linux 向けに code size を優先し、typed layer は軽量な built-in 変換だけに絞る。

## Runtime Rules
- callable export は relation store と linker から解決する。
  - linker `Host(async callback)`
  - linker `Core(instance + export_name)`
  - `canon lift` 済みの component func
- inline instance / nested component / `instantiate (with ...)` / core alias は runtime で index 解決する。
- `canon lower` / `canon lift` は以下を実装済み。
  - scalar
  - `string`
  - `list`
  - `record`
  - `tuple`
  - `variant`
  - `enum`
  - `flags`
  - `option`
  - `result`
  - `own` / `borrow`
- flat count が閾値を超える場合は indirect parameter/result passing を使う。
- fixed-length list は validator と runtime の両方で長さを検証する。
- resource runtime は shared resource state の table を持ち、`canon resource.new/drop/rep` と destructor dispatch を処理する。
- `Core` binding は `run_module_function(...).await` で実行する。
- `Store` は `Rc<RefCell<_>>` のまま扱い、component runtime は caller task 上で await される前提とする。

## Non-goals
- AOT/JIT
- `Store` / `InstanceHandle` の `Send` 化
- async component model proposal 群
- `CM_VALUES`
- `CM_MAP`
- `CM_GC`
- `memory64`
- `tags`
- 高度な最適化メトリクスの厳密ゲート

## Local Component Testsuite
compile/validate coverage は `crates/telomere-component/tests/component_model_testsuite/` を正本にする。

- runner:
  - `crates/telomere-component/tests/component_model_wast.rs`
  - directory 配下の `.wast` をソート順で全件走査する
- supported directives:
  - top-level `component` / `component definition`
  - `assert_invalid`
  - `assert_malformed`
  - top-level core `module` は compile 対象にはせず、`assert_malformed` の core binary case のみ parser error として扱う
- failure policy:
  - `assert_invalid` / `assert_malformed` は exact match ではなく semantic error matching で判定する
  - runtime directive (`invoke`, `assert_return`, `assert_trap` など) は skip せず failure にする
- suite layout:
  - 既存 feature file: `basic`, `core`, `export`, `import`, `inlineexport`, `instance`, `instancetype`, `resource`, `subtyping`, `valtype`, `variant`
  - 追加 feature file: `alias`, `func`, `link`, `lower`, `naming`, `type_export_restrictions`, `types`, `wrong_order`

## Coverage Boundary
- parser / validator breadth は local testsuite に寄せる
- runtime coverage は `component_runtime_e2e.rs` と `component_wasmtime_sync_parity.rs` に分離する
- upstream の `values/strings.wast` と `resources/multiple-resources.wast` は移植しない
- local testsuite へ持ち込まないカテゴリ:
  - `adapt.wast`
  - `big.wast`
  - `very-nested.wast`
  - `lots-of-aliases.wast`
  - `async/*`
  - `wasmtime/*`
  - `memory64`
  - `tags`

## Local Parity Tests
Wasmtime の同期 component tests をそのまま vendoring すると async / host harness 依存が混ざるため、Telomere では `crates/telomere-component/tests/component_wasmtime_sync_parity.rs` に同期 parity 用のローカル test を置く。

この test では次を検証する。

- dynamic values の round-trip:
  - `list`
  - `record`
  - `tuple`
  - `variant`
  - `enum`
  - `flags`
  - `option`
  - `result`
- indirect parameter/result passing
- typed funcs:
  - `get_func`
  - `get_typed_func`
  - integer narrowing / reinterpretation
  - `String`
  - `Vec<u8>`
  - tuple
  - `Option`
  - `Result`
- nested names
- fixed-length list の success / failure

## Current Limitation Boundary
- 失敗判定は「compile/validate が通るべきものは通る」「invalid/malformed は適切な category の error を返す」を優先し、spec 文言との完全一致は要求しない。
- `component_model_testsuite` は compile/validate 専用で、runtime directive は acceptance target に含めない。
- current boundary は Wasmtime の同期 API 面を揃えることであり、async canonical ABI や proposal 拡張までは含めない。

## Validation Commands
- `cargo test -p telomere-component --test component_model_wast -- --nocapture`
- `cargo test -p telomere-component --test component_runtime_e2e -- --nocapture`
- `cargo test -p telomere-component --test component_wasmtime_sync_parity -- --nocapture`
- `cargo test --workspace --release`
