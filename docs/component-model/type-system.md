# 存在型
以下の2つの例は、2回`instantiate`されたときに各instanceに型が依存するかどうかが異なる。前者はinstanceの状態に依存しない、例えばOSが管理するリソースなどへのインターフェースをcomponentとして定義する場合に用いることができ、後者はinstanceの状態にリソースが依存する場合に用いる。
```wasm
  (export "r2" (type (resource (rep i32))))
```
```wasm
  (export "r2" (type (resource (rep i32))) (type (sub resource)))
```
