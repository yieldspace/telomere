# Core Runtime Memory Model

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
