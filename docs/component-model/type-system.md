# 存在型

> Status: background design note. The current component runtime architecture is
> documented in [relation-driven-runtime.md](relation-driven-runtime.md), and the
> current implementation plan/status is in
> [new-component-runtime.md](new-component-runtime.md).

## Summary (English)

Parts of this document are written in Japanese. The document is a background
note on existential types in the Component Model. It contrasts two forms of
`(export "r2" (type (resource ...)))`, one of which yields a type independent of
instance state - suitable for describing something like an OS-managed resource -
while the other makes the resource depend on instance state. For representing
type parameters supplied at `instantiate`, it weighs giving each instance a type
environment against substituting type parameters into type terms, and chooses
substitution, because the environment approach would still require tracking
which environment each parameter-dependent export belongs to. The final section
records that the substitution semantics were kept while the implementation moved
from repeatedly walking a `HashMap<TypeId, Type>` to a dense arena indexed by a
compile-local `TypeId`, with memoised metadata, import/export surface comparison
as a merge walk over sorted interned names, and per-substitution transform
caches - the stated purpose being to hold down compile and validate cost on
embedded Linux without reverting to the type-environment model.

以下の2つの例は、2回`instantiate`されたときに各instanceに型が依存するかどうかが異なる。前者はinstanceの状態に依存しない、例えばOSが管理するリソースなどへのインターフェースをcomponentとして定義する場合に用いることができ、後者はinstanceの状態にリソースが依存する場合に用いる。
```wasm
  (export "r2" (type (resource (rep i32))))
```
```wasm
  (export "r2" (type (resource (rep i32))) (type (sub resource)))
```

# 実装
instanceが実体化され、型が環境に導入されるとき、そのパターンは二つあり、一つは`instantiate`であり、もう一つは`import`である。
`import`についてはよくわからないので`instantiate`について議論することにする。
`instantiate`は型としてはcomponentとその型パラメータの引数を与えられ、instanceを返す関数であるが、
ここでinstanceの型を表現するときに`instantiate`で与えられる型パラメータを、どのように表現するかということが課題になる。
ここで、アプローチが二種類あり、一つは、instanceの型は型環境を持つというアプローチであり、もう一つは、type termを型パラメータで置換してしまうというアプローチである。
前者では、instanceからexportされている型パラメータに依存する型がどの型環境に属しているか結局のところ管理しておかなければならないため、ここでは後者のアプローチをとる。具体的にどのように置換していくかというと、WASM Component Modelにおいては、依存する型はその型が現れるよりまえに環境に持ち込まれている必要があるため、単純に型パラメータの定義と使用を見つけた時に、といっても複合型の場合その内部の型まで検査する必要があるが、それを環境に定義し、それを置換すればよい。

# 現在の実装戦略
意味論は上記の置換ベースを維持しているが、実装は `HashMap<TypeId, Type>` を都度再帰 walk する形ではなく、compile-local な `TypeId` を index に使う dense arena へ寄せている。

- 型本体は arena に連続配置し、`TypeId` から直接参照する
- effective size、resource 可視性、surface 可視性は type metadata に memoize する
- import/export surface の比較は名前を intern した sorted vector 上の merge walk で行う
- import freshening / instantiate は置換環境ごとの transform cache を使うが、fresh resource の一意性は top-level session 単位で維持する

この構成により、型環境モデルへ戻さずに、embedded Linux 向けの compile/validate コストを抑える。
