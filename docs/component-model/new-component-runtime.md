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
- unscoped async component model proposal work outside the pinned WASI 0.3 /
  Preview 3 target in [wasi-0.3-preview3.md](wasi-0.3-preview3.md)
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
- current implemented boundary is Wasmtime の同期 API 面を揃えることであり、
  async canonical ABI や WASI 0.3 / Preview 3 proposal 拡張は
  [wasi-0.3-preview3.md](wasi-0.3-preview3.md) の snapshot / support matrix
  に従って段階的に追加する。

## Validation Commands
- `cargo test -p telomere-component --test component_model_wast -- --nocapture`
- `cargo test -p telomere-component --test component_runtime_e2e -- --nocapture`
- `cargo test -p telomere-component --test component_wasmtime_sync_parity -- --nocapture`
- `cargo test --workspace --release`
