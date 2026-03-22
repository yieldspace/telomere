## Local Readdressing in WASM

### Overview

`Local readdressing` とは、WASM モジュール内で定義されたローカル変数へのアクセス命令（例：`get_local`, `set_local`）におけるインデックス (`idx`) を、実行時に使用しやすいバイトオフセットに事前変換する最適化手法です。

### 目的

- **実行速度向上**: インデックス計算を事前に済ませ、実行ループ中は即値でメモリアドレスを参照できる。
- **シンプルな命令エミュレーション**: JIT やインタープリタ実装で、命令実行時の算術オペレーションを削減。

### 基本アイデア

1. **パース／コンパイルフェイズ** で `get_local idx` をパース

2. 関数ごとにローカル変数を型別にまとめ、各型グループの合計バイト数を算出

3. 元のローカル番号 `idx` に対応するバイトオフセットを計算し、命令オペランドとして埋め込む



### Readdressing Table の構造

- テーブルは `(原始インデックス, ValType, バイトオフセット)` のタプルのリスト
  - **原始インデックス**: WASM バイトコード上で定義されたローカル変数の順序（0ベース）
  - **ValType**: ローカルの型（`i32`、`f64`、`func_ref` など）
  - **バイトオフセット**: 関数スタックフレームの先頭からのバイト距離
- このテーブルにより、実行時に `local.get k` の `k` を直接オフセットにマッピング可能
- テーブルは原始インデックス順にソートされているため、二分探索で高速にルックアップ可能(TODO: できるけどやってません)

### 注意点

- デバッグ時には元のインデックス情報を保持する仕組みが必要

### FunctionInstance 内のメモリ表現の簡素化

以下の `FunctionInstanceData` 構造体例のように、`locals` 領域とコード（またはネイティブ関数ポインタ）を `body` フィールドにまとめ、`repr(C)` を付与してメモリレイアウトを固定化することで、ストア上の表現をシンプルにできます。

```rust
#[repr(C)]
pub struct FunctionInstanceData {
    pub instance_addr: ObjectRef,
    pub funcidx: u32,
    pub function_flags: u32, // TODO: more efficient encoding
    pub body: ObjectRef,         // locals とコード（または関数ポインタ）を含むメモリブロックへの参照
}
```

- `body` フィールドは GC 管理下の単一メモリブロックを参照し、その中にローカル変数領域とコード（または関数ポインタ）を含められる
- これによりメモリ割り当てとライフサイクル管理が統一され、GC やシリアライズの実装が簡潔になる

### 参考

- [Binaryen の ](https://github.com/WebAssembly/binaryen)[`merge-locals`](https://github.com/WebAssembly/binaryen)[ パス](https://github.com/WebAssembly/binaryen)
- [WASM バイナリ仕様（locals group）](https://webassembly.github.io/spec/core/binary/modules.html#binary-local)

