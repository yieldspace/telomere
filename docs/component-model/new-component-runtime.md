# New Component Runtime Spec (Initial)

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
- `ComponentProgram { types, imports, callable_imports, exports, callable_exports, ops, bytes }`
- `ComponentOp`:
  - `Instantiate`
  - `Alias`
  - `CanonLower`
  - `CanonLift`
  - `Export`

## Runtime Rules (initial)
- callable export は以下の順で解決する。
1. linker に同名 export がある
2. linker に同名 import がある
3. callable import が 1 個のみならそれを fallback に使う
- linker の binding は `Host(async callback)` と `Core(instance + export_name)` の 2 種類に固定する
- `Core` binding は `run_module_function(...).await` で実行する
- `Store` は `Rc<RefCell<_>>` のまま扱い、component runtime は caller task 上で await される前提とする
- 上記で解決できない callable export があれば link error とする。

## Non-goals
- AOT/JIT
- `Store` / `InstanceHandle` の `Send` 化
- 高度な最適化メトリクスの厳密ゲート
