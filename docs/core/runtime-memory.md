# Core Runtime Memory Model

## Summary (English)

The body of this document is written in Japanese. It defines the core runtime's
memory model as of issue #106 and states that the old `MemoryPool` and
compacting GC are no longer assumed. On ownership: `Store` holds module,
instance, function, table, global, and memory metadata in a store-local
append-only arena; `InstanceHandle` carries store identity plus instance
index/id and does not rely on root-slot cleanup; and same-store execution is
serialised single-flight, so a `Store` may be shared but running one `Store`
concurrently is not a supported assumption. Linear memory is split into unshared
`mmap`-backed `LocalMemoryObject` and store-independent
`Arc<SharedMemoryObject>`, and `instantiate`, `run_module_function`,
`get_global`, host linking, and aliasing all serialise under the same execution
lease, with nested synchronous reentry allowed only through a reentry token
passed from the component host trampoline. A compact-representation section
lists the allocation-reduction rules in force, and the bulk-memory section
records that same-memory `memory.copy` keeps memmove semantics while
cross-memory copies validate ranges up front and copy through a fixed 4 KiB
stack buffer rather than a temporary `Vec<u8>`.

issue #106 以降の core runtime は、旧 `MemoryPool` / compacting GC を前提にしません。現在の設計上の正本は次の通りです。

## Ownership

- `Store` は module / instance / function / table / global / memory metadata を store-local の append-only arena で保持する
- `InstanceHandle` は store identity と instance index / instance id を保持し、root slot cleanup には依存しない
- same-store の実行は single-flight で直列化する。`Store` 自体は共有できるが、1つの `Store` を同時に複数走らせることは前提にしない

## Linear Memory

- unshared memory は `mmap` backend の `LocalMemoryObject` で保持する
- shared memory は store から独立した `Arc<SharedMemoryObject>` で保持する
- shared memory の grow は複数 instance から見える単一オブジェクトに反映される

## Execution Contract

- `instantiate` / `run_module_function` / `get_global` / host link / aliasing は同じ execution lease の下で直列化される
- nested sync reentry は component host trampoline から渡す reentry token 経由だけを許可する
- reentrancy の拒否判定は `Store::lock_runtime` に一元化する。各入口は内部の型付き `StoreExecutionError` を消費し、値を返す API は既存の fail-closed 値を返す。`()` API は内部 `Result` を公開ラッパーで明示的に潰す。これは内部配線の変更であり、公開 API / 公開型は変更しない
- component support は `MemoryPool` / `ObjectRef` を直接露出せず、memory handle と sync reentry helper を使う

## Effects

- unshared memory の load/store は direct `mmap` access を使う
- shared memory も現状は同一 object への同期付きアクセスとして扱い、将来の threads / wait-notify 拡張点だけを残す

## Compact Runtime Representation

- parser の length-known vector は capacity を先に確保し、type-checker / jump-resolver の block stack は `VecDeque` ではなく stack 型の `Vec` で扱う
- per-instruction stack snapshot や decoded operand のような小さい一時列は `SmallVec` を使い、典型的な 0-2 要素のために heap allocation しない
- `LoweredFunction` は引き続き canonical artifact で、fail-closed verifier / budget path も `LoweredFunction::from_materialized(...)` へ閉じる
- fail-closed path は元の materialized instruction を eager clone せず、fallback preview の `instrs` / `op_lens` capacity を縮めて保持する
- instantiate は direct-call recipe slot が決まるまで wasm body の final materialization を遅延し、空の placeholder から一度だけ `Arc<[Instr]>` / `Arc<[u16]>` に詰める

## Bulk Memory Temporaries

- same-memory `memory.copy` は既存の memmove semantics を保つ
- cross-memory `memory.copy` は source / destination range を先に検証し、固定 4KiB stack buffer で chunked copy する
- これにより local/shared をまたぐ大きな copy でも `Vec<u8>` temporary を copy length 分確保しない
- component support の fixed-width memory read は `[u8; N]` へ直接読むため、canonical ABI の scalar lift で小さな `Vec<u8>` を作らない
