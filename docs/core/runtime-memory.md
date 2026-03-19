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
- component support は `MemoryPool` / `GcRef` を直接露出せず、memory handle と sync reentry helper を使う

## Effects

- unshared memory の load/store は direct `mmap` access を使う
- shared memory も現状は同一 object への同期付きアクセスとして扱い、将来の threads / wait-notify 拡張点だけを残す
