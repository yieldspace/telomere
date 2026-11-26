//! Regression tests for the PR #371 Codex round-2 finding: a core
//! module type declarator's non-final func type lost its identity
//! when `resolve_core_func_type` converted it into a signature-only
//! descriptor, so instantiation argument matching accepted any
//! func with a matching signature. A plain `(func)` is final and is
//! not a subtype of a declared `(sub (func))`, so wasmparser
//! rejects the instantiation below with a type mismatch.
//!
//! Module import/export func descriptors now carry the declared
//! type's canonical identity (signature, finality, declared
//! supertype ancestry), and matching requires the provided type to
//! be a subtype of the declared type under the chain rule: the
//! declared type must occur on the provided type's declared
//! supertype chain.
//!
//! Round 3 (two further Codex findings on the identity itself):
//! the identity now also survives the core func index space — a
//! func aliased out of a core instance and re-exported through an
//! inline core instance keeps its declared type instead of
//! collapsing to the bare type of its signature — and it carries
//! its recursion-group context, because canonicalisation treats a
//! `rec` group as a unit: a member of a multi-member group is a
//! different type from a standalone definition of the same shape.

use telomere_component::ComponentEngine;

fn compile(text: &str) -> Result<(), String> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine
        .compile(&bytes)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn expect_rejected(text: &str) {
    let error = compile(text).expect_err("component must be rejected");
    assert!(
        error.contains("core module import mismatch"),
        "expected the instantiation mismatch error, got `{error}`"
    );
}

/// Instantiates an imported module of type `$M` with an instance of
/// an inline module exporting a plain `(func)` as `f`.
fn instantiate_with_plain_module(module_type: &str) -> String {
    format!(
        r#"
(component
  (core type $M (module
    {module_type}
  ))
  (import "m" (core module $m (type $M)))
  (core module $argmod (func (export "f")))
  (core instance $arg (instantiate $argmod))
  (core instance (instantiate $m (with "i" (instance $arg))))
)
"#
    )
}

/// Instantiates an imported module of type `$M` with an instance of
/// an imported module of type `$A`.
fn instantiate_with_typed_module(module_type: &str, provider_type: &str) -> String {
    format!(
        r#"
(component
  (core type $M (module
    {module_type}
  ))
  (core type $A (module
    {provider_type}
  ))
  (import "m" (core module $m (type $M)))
  (import "p" (core module $p (type $A)))
  (core instance $arg (instantiate $p))
  (core instance (instantiate $m (with "i" (instance $arg))))
)
"#
    )
}

#[test]
fn plain_func_does_not_satisfy_nonfinal_sub_import() {
    // The Codex shape: the import is declared with the non-final
    // `(sub (func))` type local to the module type; the argument
    // module exports a plain `(func)`.
    expect_rejected(&instantiate_with_plain_module(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
    ));
}

#[test]
fn plain_func_satisfies_final_import() {
    compile(&instantiate_with_plain_module(
        r#"(type $t (func))
           (import "i" "f" (func (type $t)))"#,
    ))
    .expect("a final declared type is satisfied by the plain func of its signature");
}

#[test]
fn identical_declared_type_satisfies_import() {
    // The provider declares a structurally identical non-final
    // type in its own module type; canonical identity agrees.
    compile(&instantiate_with_typed_module(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (sub (func)))
           (export "f" (func (type $t)))"#,
    ))
    .expect("the same declared type must satisfy the import");
}

#[test]
fn declared_subtype_satisfies_import() {
    compile(&instantiate_with_typed_module(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (sub (func)))
           (type $s (sub $t (func)))
           (export "f" (func (type $s)))"#,
    ))
    .expect("a declared subtype must satisfy the import");
}

#[test]
fn final_declared_subtype_satisfies_import() {
    compile(&instantiate_with_typed_module(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (sub (func)))
           (type $s (sub final $t (func)))
           (export "f" (func (type $s)))"#,
    ))
    .expect("a final declared subtype must satisfy the import");
}

#[test]
fn declared_subtype_does_not_satisfy_bare_import() {
    // The chain rule also runs in the other direction: a declared
    // subtype is not the bare type, even with the same signature.
    expect_rejected(&instantiate_with_typed_module(
        r#"(type $t (func))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (sub (func)))
           (type $s (sub $t (func)))
           (export "f" (func (type $s)))"#,
    ));
}

#[test]
fn supertype_does_not_satisfy_subtype_import() {
    // The provider exports the supertype itself where a declared
    // subtype of it is expected.
    expect_rejected(&instantiate_with_typed_module(
        r#"(type $t (sub (func)))
           (type $v (sub $t (func)))
           (import "i" "f" (func (type $v)))"#,
        r#"(type $u (sub (func)))
           (export "f" (func (type $u)))"#,
    ));
}

#[test]
fn aliased_component_type_satisfies_import() {
    // Both module types alias the same component-level core type;
    // the identity recorded for it reaches both descriptors.
    let text = r#"
(component
  (core type $t (sub (func)))
  (core type $M (module
    (alias outer 1 0 (type $lt))
    (import "i" "f" (func (type $lt)))
  ))
  (core type $A (module
    (alias outer 1 0 (type $lt))
    (export "f" (func (type $lt)))
  ))
  (import "m" (core module $m (type $M)))
  (import "p" (core module $p (type $A)))
  (core instance $arg (instantiate $p))
  (core instance (instantiate $m (with "i" (instance $arg))))
)
"#;
    compile(text).expect("the aliased declared type must satisfy the import");
}

#[test]
fn subtype_of_aliased_component_type_satisfies_import() {
    // The provider declares a subtype of the aliased component-level
    // type inside its own module type.
    let text = r#"
(component
  (core type $t (sub (func)))
  (core type $M (module
    (alias outer 1 0 (type $lt))
    (import "i" "f" (func (type $lt)))
  ))
  (core type $A (module
    (alias outer 1 0 (type $lt))
    (type $s (sub $lt (func)))
    (export "f" (func (type $s)))
  ))
  (import "m" (core module $m (type $M)))
  (import "p" (core module $p (type $A)))
  (core instance $arg (instantiate $p))
  (core instance (instantiate $m (with "i" (instance $arg))))
)
"#;
    compile(text).expect("a subtype of the aliased declared type must satisfy the import");
}

/// Builds the round-3 alias shape: a func exported by an imported
/// module of type `$A` is aliased out of its instance and
/// re-exported through an inline core instance, which is then
/// passed as the instantiation argument for `"i"`.
fn instantiate_with_aliased_func(module_type: &str, provider_type: &str) -> String {
    format!(
        r#"
(component
  (core type $M (module
    {module_type}
  ))
  (core type $A (module
    {provider_type}
  ))
  (import "m" (core module $m (type $M)))
  (import "p" (core module $p (type $A)))
  (core instance $pin (instantiate $p))
  (alias core export $pin "f" (core func $g))
  (core instance $arg (export "f" (func $g)))
  (core instance (instantiate $m (with "i" (instance $arg))))
)
"#
    )
}

#[test]
fn aliased_nonfinal_func_satisfies_import_through_inline_instance() {
    // The Codex round-3 shape: matching `(sub (func))` types on the
    // provider and the consumer. wasmparser accepts; the alias /
    // inline-export round trip must not lose the identity on the
    // way through the core func index space.
    compile(&instantiate_with_aliased_func(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (sub (func)))
           (export "f" (func (type $t)))"#,
    ))
    .expect("the aliased declared type must satisfy the import");
}

#[test]
fn aliased_declared_subtype_satisfies_import_through_inline_instance() {
    // The declared supertype chain survives the round trip too.
    compile(&instantiate_with_aliased_func(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (sub (func)))
           (type $s (sub $t (func)))
           (export "f" (func (type $s)))"#,
    ))
    .expect("the aliased declared subtype must satisfy the import");
}

#[test]
fn aliased_plain_func_does_not_satisfy_nonfinal_import_through_inline_instance() {
    // A plain `(func)` keeps the bare identity through the round
    // trip, so the round-2 rejection still applies to it.
    expect_rejected(&instantiate_with_aliased_func(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(type $t (func))
           (export "f" (func (type $t)))"#,
    ));
}

#[test]
fn rec_group_member_does_not_satisfy_standalone_import() {
    // The Codex round-3 shape: the provider's `$t` is member 0 of
    // a two-member recursion group; the consumer imports the
    // standalone type of the same shape. wasmparser rejects with a
    // type mismatch: the group is part of the canonical type.
    expect_rejected(&instantiate_with_typed_module(
        r#"(type $t (func))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type $t (func)) (type (func (param i32))))
           (export "f" (func (type $t)))"#,
    ));
}

#[test]
fn identical_rec_group_member_satisfies_import() {
    // The same group, declared identically on both sides, is the
    // same canonical type.
    compile(&instantiate_with_typed_module(
        r#"(rec (type $t (func)) (type (func (param i32))))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type $t (func)) (type (func (param i32))))
           (export "f" (func (type $t)))"#,
    ))
    .expect("the same group member must satisfy the import");
}

#[test]
fn rec_group_second_member_does_not_satisfy_standalone_import() {
    // The position inside the group is part of the identity:
    // member 1 has the same shape as the standalone type but is a
    // different canonical type.
    expect_rejected(&instantiate_with_typed_module(
        r#"(type $t (func))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type (func (param i32))) (type $t (func)))
           (export "f" (func (type $t)))"#,
    ));
}

#[test]
fn rec_group_with_different_member_does_not_satisfy_import() {
    // The whole group contents are part of the identity, not just
    // the exported member: a group differing in its other member
    // is a different group.
    expect_rejected(&instantiate_with_typed_module(
        r#"(rec (type $t (func)) (type (func (param i32))))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type $t (func)) (type (func (param i64))))
           (export "f" (func (type $t)))"#,
    ));
}

#[test]
fn singleton_rec_group_member_satisfies_standalone_import() {
    // A recursion group of one member is the standalone definition
    // of that member: canonicalisation does not distinguish them.
    compile(&instantiate_with_typed_module(
        r#"(type $t (func))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type $t (func)))
           (export "f" (func (type $t)))"#,
    ))
    .expect("a singleton group member is the standalone type");
}

#[test]
fn rec_group_subtype_satisfies_group_supertype_import() {
    // A subtype declared inside a group satisfies an import of its
    // in-group supertype when the consumer declares the same group.
    compile(&instantiate_with_typed_module(
        r#"(rec (type $t (sub (func))) (type $s (sub $t (func))))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type $t (sub (func))) (type $s (sub $t (func))))
           (export "f" (func (type $s)))"#,
    ))
    .expect("the in-group subtype must satisfy the in-group supertype import");
}

#[test]
fn rec_group_subtype_does_not_satisfy_standalone_supertype_import() {
    // The same provider against a consumer whose supertype is the
    // standalone declaration: the provider's chain runs through
    // its in-group supertype, which is not the standalone type.
    expect_rejected(&instantiate_with_typed_module(
        r#"(type $t (sub (func)))
           (import "i" "f" (func (type $t)))"#,
        r#"(rec (type $t (sub (func))) (type $s (sub $t (func))))
           (export "f" (func (type $s)))"#,
    ));
}
