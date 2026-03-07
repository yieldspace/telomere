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
- `component/decoder`: 独自 decoder + validator
- `component/validate`: validator 再公開
- `component/ir`: component intermediate data model
- `component/runtime`: 実行ディスパッチ

## IR
- `ComponentProgram` は次を保持する。
  - root component definition
  - dense type table (`types`)
  - canonical ABI metadata (`type_infos`)
  - component / instance / func / core module / core instance / core func / core memory / core table の relation store snapshot
- runtime は `bytes` の再走査や export 名 fallback に依存せず、この IR snapshot を index ベースで解決する。

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

## Upstream Testsuite Snapshot
- 追加した snapshot:
  - `crates/telomere/tests/component_model_upstream/c7176a512c0bbe4654849f4ba221c1a71c7cf514/`
- pin:
  - upstream commit `c7176a512c0bbe4654849f4ba221c1a71c7cf514`
- manifest:
  - `crates/telomere/tests/component_model_upstream/manifest.txt`
- harness:
  - `crates/telomere/tests/component_model_upstream.rs`
- 実行方針:
  - vendored subset は manifest の `mode` に従って走らせる
  - `compile-only`: component/module directive の compile/validate を検証し、runtime directive は skip
  - `invalid-only`: `assert_invalid` / `assert_malformed` を主対象にし、同じく runtime directive は skip
  - `execute-scalars`: runtime directive を実行する。現在は `values/strings.wast` と `resources/multiple-resources.wast` で使用する
  - `precompiled`: `wasm-tools json-from-wast` で生成した sidecar を使う。binary decoder に直接通し、極端に重い text-side stress case を安定化する

## Harness Support Matrix
| 項目 | harness 状態 | vendored snapshot での扱い |
| --- | --- | --- |
| top-level `component` / `component definition` compile | 対応 | 実行 |
| top-level core `module` / `module definition` compile | subset 外 | vendored snapshot では未使用 |
| `assert_invalid` | 対応 | 実行 |
| `assert_malformed` | 対応 | 実行 |
| semantic error matching | 対応 | 文言完全一致ではなく category + token overlap で判定 |
| `component instance` directive | best-effort | harness 実装あり、現 snapshot では未使用 |
| `assert_unlinkable` | best-effort | harness 実装あり、現 snapshot では未使用 |
| `invoke` / `assert_return` / `assert_trap` with scalar component values | 対応 | 実行 |
| `invoke` / `assert_return` / `assert_trap` with strings/resources | 対応 | `values/strings.wast`, `resources/multiple-resources.wast` で実行 |
| `invoke` / `assert_return` / `assert_trap` with lists/records/variants/flags/option/result | ローカル parity tests で対応 | `component_wasmtime_sync_parity.rs` で実行 |
| `register`, `thread`, `wait`, `assert_exception`, `assert_suspension`, `assert_exhaustion` | 未対応 | skip |

## Upstream Inclusion / Exclusion
| Path | 状態 | 理由 |
| --- | --- | --- |
| `names/kebab.wast` | include | naming validation coverage |
| `resources/multiple-resources.wast` | include, `execute-scalars` | resource table / destructor dispatch / string passing を含む runtime coverage |
| `values/strings.wast` | include, `execute-scalars` | string canonical ABI runtime coverage |
| `wasm-tools/*.wast` | include | parser/validator coverage。`very-nested.wast` は precompiled sidecar 併用 |
| `wasm-tools/memory64.wast` | exclude | core `memory64` proposal 依存 |
| `wasm-tools/tags.wast` | exclude | core exception tags proposal 依存 |
| `values/trap-in-post-return.wast` | exclude | post-return / async canonical ABI 依存 |
| `async/*` | exclude | async builtins / async stackful semantics は初期スコープ外 |
| `wasmtime/*` | exclude | wasmtime 固有 harness / extension で telomere acceptance 外 |

## Local Parity Tests
Wasmtime の同期 component tests をそのまま vendoring すると async / host harness 依存が混ざるため、Telomere では `crates/telomere/tests/component_wasmtime_sync_parity.rs` に同期 parity 用のローカル test を置く。

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
- `very-nested.wast` の size-limit / malformed stress は precompiled sidecar で実行する。binary decoder と invalid 判定は維持しつつ、text-side expansion のオーバーヘッドだけ外している。
- 失敗判定は「compile/validate が通るべきものは通る」「invalid/malformed は適切な category の error を返す」を優先し、spec 文言との完全一致は要求しない。
- current boundary は Wasmtime の同期 API 面を揃えることであり、async canonical ABI や proposal 拡張までは含めない。
