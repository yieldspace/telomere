
# WebAssembly ランタイムにおけるジャンプアドレス解決の実装

## 現在の実装：静的ジャンプ解決（JumpResolver）

### 1. DSL による一括評価

ジャンプ解決は、`JumpResolverDSL` という形式を使って事前にパース時に行われる。これにより、実行時にジャンプを動的に解決するのではなく、あらかじめ決定されたアドレスに基づいて処理を進める。

```rs
pub enum JumpResolverDSL {
    EnterForwardJumpBlock,
    EnterBackwardJumpBlock(u32),
    Br(u32, u32),
    Return(u32),
    LeaveBlock(u32),
}
```

- **`EnterForwardJumpBlock`** / **`LeaveBlock`**: ブロックの開始と終了。
- **`Br(level, addr)`**: スタック上の `level` 番目のブロックにジャンプする命令。
- **`Return(addr)`**: 戻りアドレスを設定する命令。

### 2. 内部状態と評価

ジャンプの状態は次のように管理される：

- **`JumpResolverState::Lazy(Vec<u32>)`**: ジャンプ先が未解決な状態。
- **`JumpResolverState::Resolved(u32)`**: ジャンプ先が解決された状態。

`evaluate()` メソッドで全てのジャンプ先を一度に解決する。

---

## 3. if-else の取り扱い

`if` / `else` 構造では、`else` の扱いを特別に処理することなく、`end` のアドレスを基準にジャンプ先を解決する。

---

## 4. br_table の扱い

`br_table` の分岐先については、ジャンプ先の解決に関して次のように処理される：

```rs
for idx in &idxs {
    jump_resolver.push(JumpResolverDSL::Br(*idx, instrs.len() as u32));
    instrs.push(Instr {
        operand: Operand {
            u32: 0xFFFD0000 | *idx,  // デバッグ目的で使用
        },
    });
}
```

- **オペランドの数値（`u32: 0xFFFD0000 | *idx`）** はデバッグ用途の目的であり、実際のジャンプ先の解決には `JumpResolverDSL::Br` で指定されたアドレスが利用される。

---

## 5. try-catch について

現在、WebAssembly の `Exception Handling`（`try`, `catch`, `throw`）機能は未実装である。これに関する処理は現時点では考慮されていない。

---

## 6. jump_addr の表現

現在、`jump_addr` は関数の先頭を基準とした**絶対アドレス（命令インデックス）**として表現される。

### 長所

- 実行時に明確なジャンプ先を指し示すため、処理が直感的である。
- デバッグ時にアドレスの追跡が容易である。

### 改善案

- **相対アドレス化**：エンコードの効率を高めることができるが、ジャンプ先の管理が複雑になる。
- **ネイティブアドレスへの変換**：JITやAOTとの連携を考えると、実行時にアドレスを変換する方法も考慮できる。

---

## 7. 実装の総評

- 現在の実装は、**事前にジャンプ先を解決する静的な方法**を採用しており、実行時に余計な処理を避けている。
- `JumpResolverDSL` を使用したジャンプ処理の設計は、ジャンプ先の解決をシンプルにしている。
- それぞれの命令に対してジャンプ先を解決し、デバッグ時に役立つような情報も含んでいるが、実行時にはオーバーヘッドを抑える構造である。
