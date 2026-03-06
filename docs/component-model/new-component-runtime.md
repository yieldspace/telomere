# New Component Runtime Spec

図つきの実装解説は [relation-driven-runtime.md](relation-driven-runtime.md) を参照。

## Public API
- `ComponentEngine::compile(bytes: &[u8]) -> Result<ComponentProgram, ComponentError>`
- `ComponentEngine::instantiate(program: &ComponentProgram, store: &mut Store, linker: &ComponentLinker) -> impl Future<Output = Result<ComponentInstance, ComponentError>>`
- `ComponentInstance::call(store: &mut Store, name: &str, args: &[ComponentValue]) -> impl Future<Output = Result<Vec<ComponentValue>, ComponentError>>`
- `ComponentLinker::register_import_async` / `register_export_async`
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
  - type map
  - component / instance / func / core module / core instance / core func / core memory / core table の relation store snapshot
- runtime は `bytes` の再走査や export 名 fallback に依存せず、この IR snapshot を index ベースで解決する。

## Runtime Rules
- callable export は relation store と linker から解決する。
  - linker `Host(async callback)`
  - linker `Core(instance + export_name)`
  - `canon lift` 済みの component func
- inline instance / nested component / `instantiate (with ...)` / core alias は runtime で index 解決する。
- `canon lower` / `canon lift` は以下を実装済み。
  - scalar
  - `string`
  - `own` / `borrow`
- resource runtime は shared resource state の table を持ち、`canon resource.new/drop/rep` と destructor dispatch を処理する。
- `Core` binding は `run_module_function(...).await` で実行する。
- `Store` は `Rc<RefCell<_>>` のまま扱い、component runtime は caller task 上で await される前提とする。

## Non-goals
- AOT/JIT
- `Store` / `InstanceHandle` の `Send` 化
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
| `invoke` / `assert_return` / `assert_trap` with lists/records/variants/flags/option/result | 未対応 | compile/validate coverage のみ |
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

## Current Limitation Boundary
- runtime の canonical ABI は `string` と resource handle までは実装済みだが、複合値の full flattening はまだ未対応。
- `very-nested.wast` の size-limit / malformed stress は precompiled sidecar で実行する。binary decoder と invalid 判定は維持しつつ、text-side expansion のオーバーヘッドだけ外している。
- 失敗判定は「compile/validate が通るべきものは通る」「invalid/malformed は適切な category の error を返す」を優先し、spec 文言との完全一致は要求しない。
