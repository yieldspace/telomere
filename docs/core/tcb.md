# Core Wasm TCB Boundary

この文書は、telomere の core wasm runtime / proof work における TCB
(`Trusted Computing Base`) を固定するための台帳である。

目的は次の 2 点。

- どこまでを proof の外で「正しい」と信用しているかを明示する。
- Wasm 意味論や helper contract を TCB へ逃がさない境界を固定する。

## 1. 境界の定義

### Verified 側

ここには「意味論を置いてよい層」を置く。phase 7 時点では完全に機械証明済みではない箇所もあるが、少なくとも **TCB に入れてはいけない責務** をここへ固定する。

- `crates/telomere/src/common/formal.rs`
  - core wasm の abstract semantics
  - `StackView`, `LinearMemoryView`, `ExecContextToken` などの pure model
- `crates/telomere/src/common.rs`
  - runtime -> abstract state の projection
  - `ExecuteContextSnapshot` と token/projection bridge
- `crates/telomere/src/common/memory.rs`
  - shared-memory projection と before/after protocol wrapper
- `crates/telomere/src/runtime/vm/*`
  - thin `op_*` wrapper
  - helper family を呼ぶ runtime wiring

この側に属するコードは、間違っていれば proof/implementation bug であり、trusted primitive の責務ではない。

### Trusted 側

ここは **proof で中身を展開せず、契約だけを信用する最小領域** である。

| 項目 | 所在 | 信用する内容 |
| --- | --- | --- |
| byte/slice primitive | `crates/telomere/src/common/memory.rs` の `trusted_copy_from_slice`, `trusted_fill_slice`, `trusted_copy_within`, `trusted_read_u16/u32/u64/u128`, `trusted_write_u32/u64/u128` | 指定長 slice の copy/fill/read/write が little-endian byte semantics どおりに動くこと |
| mmap frontier | `crates/telomere/src/common/memory.rs` の `MmapRegion::{new, as_slice, as_slice_mut}` と `Drop` | `mmap` / `munmap` が予約済み領域を作り、境界内だけを slice として露出すること |
| stack frame marshalling | `crates/telomere/src/common/stack.rs` の `CallStackInfo`, `call_stack_info`, `push_call_stack_info`, `local_area_mut_ptr` | packed frame trailer の layout と byte round-trip、および raw pointer 露出の前提が守られること |
| shared wait/wake primitive | `crates/telomere/src/common/memory.rs` の `SharedWaiter`, `SharedWaitRegistration::*_protocol`, `SharedMemoryObject::{register_wait*_protocol, notify_waiters_protocol, consume_*}` | mutex / atomic / waker による runtime 同期結果が projection の before/after と一貫していること |

Trusted 側に入れてよいのは、raw byte access、raw mapping access、同期 primitive までである。

### Unverified 側

ここは現在 proof の外にある通常の Rust 実装である。ただし、**意味論の正本を置いてよい場所ではない**。

- parser / validator / host wiring
- scheduler / runtime orchestration
- proof contract をまだ theorem 化していない helper 実装

unverified 側は存在してよいが、ここへ Wasm step semantics や wait queue policy の正本を持ち込まない。

## 2. TCB に入れてはいけないもの

次の責務は TCB へ入れない。

- Wasm 1-step 意味論そのもの
- default/local/shared memory の dispatch policy
- wait queue の FIFO / timeout / notify の意味論
- `op_*` handler の分岐意味論
- `Pending` / resume pc / completion value の contract

これらは `common/formal.rs`、projection、helper contract、family theorem の側に残す。

## 3. trusted 項目ごとの前提

### byte/slice primitive

- 長さ precondition が守られる。
- endian 変換は byte 列と一致する。
- copy/fill/copy-within は指定範囲外を壊さない。

### mmap frontier

- mapping は予約長 `len` を持つ。
- `as_slice` / `as_slice_mut` は `len <= reservation` の範囲でのみ使う。
- mapping の共有性 (`MAP_SHARED` / `MAP_PRIVATE`) は `Memory::new_with_mapping` の指定に一致する。

### stack frame marshalling

- `CallStackInfo` の `repr(C, packed)` layout は push/read で一致する。
- frame tail に置かれた trailer は途中で破壊されない。
- `local_area_mut_ptr` を使う側は stack の再配置後に pointer を使い回さない。

### shared wait/wake primitive

- waiter id の発行は一意である。
- queue cleanup は wake / timeout ごとに高々 1 回起こる。
- mutex / atomic / waker の組み合わせは runtime state と projection snapshot を食い違わせない。

## 4. 運用ルール

- 新しい `#[verifier::external_body]` を足す前に、それが raw primitive かを確認する。
- raw primitive でない helper を trusted に足してはいけない。
- trusted 項目を増やしたら、この文書と関数 docstring を同時に更新する。
- helper / wrapper / theorem へ押し戻せる責務は、phase 7 以降も継続して TCB から外す。

## 5. 現時点の読み方

この文書は「現在の codebase でどこを信用しているか」の正本であり、proof の進捗ログではない。

要点は次の一文に尽きる。

> telomere の TCB は raw byte primitive、raw mapping primitive、raw synchronization primitive に限定し、Wasm 意味論と runtime policy はそこへ入れない。
