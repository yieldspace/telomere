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
//!
//! Round 4 (a further Codex finding on the identity comparison
//! itself): comparing two separately decoded identities was
//! exponential in the presence of shared subgraphs, because an
//! identity reaches its external ancestors both along its own
//! supertype chain and through every group member's recorded
//! external supertype, and the derived equality re-traversed the
//! shared subgraph once per path. The comparison is now memoised
//! over node pairs (`CoreFuncIdentity::eq_memoized`); the tower
//! tests at the bottom pin the scaling.
//!
//! Round 5 (a further Codex finding on the memoised comparison):
//! memoisation bounded the repeated work but not the call-stack
//! depth -- matching two towers of 100,000 two-member recursion
//! groups, whose exported members declare no supertype and whose
//! other members subtype the previous group's exported member,
//! aborted the process with a stack overflow in the alternating
//! `eq_memoized` / rec-group comparison recursion (and, once the
//! comparison itself was iterative, in the `Arc` drop cascade of
//! the same tower). The comparison now runs on an explicit work
//! stack and `CoreFuncIdentity`'s `Drop` dismantles uniquely
//! owned descendants iteratively; the deep-group tower tests at
//! the bottom pin both.
//!
//! Round 6 (a further Codex finding on the paths that still used
//! the derived equality): core-module instantiation matching was
//! bounded, but component-type matching still compared whole core
//! module types with the derived `PartialEq` -- in
//! `ComponentType::assert_subtype_of` and in the validator's
//! surface-summary version of the same check (both for core module
//! imports), and in the `ComponentExportType` /
//! `InstanceExportType` core module arms (for core module
//! exports). A component export with a component-type ascription
//! whose core module import carries an identity tower therefore
//! still cost exponential time (measured on the unmodified code:
//! ~4.4 s at depth 18, growing ~2.8x per level), and the
//! recursion-group tower still overflowed the stack on that path
//! (depth 100,000 aborted). All four comparisons now go through
//! the bounded (memoised, iterative) equality; the ascription
//! tests at the bottom pin each path.

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

/// Builds the module-type declarations for a tower of `depth`
/// two-member recursion groups: `$a0` is a standalone non-final
/// type, and each `$a{i}` is the second member of a group whose
/// two members both declare `$a{i-1}` as their (external)
/// supertype. An identity from this tower reaches the identity
/// below it along three paths -- its own supertype chain and both
/// group members' recorded external supertypes -- so comparing two
/// separately decoded towers without memoisation costs time
/// exponential in `depth` (measured on the unmemoised code: ~4.6 s
/// at depth 18, growing ~2.8x per level). With `alter_deepest`,
/// the first group's first member is a plain type of a different
/// signature, a difference only visible at the bottom of the
/// tower.
fn identity_tower_decls(depth: usize, import: bool, alter_deepest: bool) -> String {
    let mut decls = String::from("(type $a0 (sub (func)))\n");
    for i in 1..=depth {
        let first = if alter_deepest && i == 1 {
            "(type $b1 (func (param i32)))".to_string()
        } else {
            format!("(type $b{i} (sub $a{} (func)))", i - 1)
        };
        decls.push_str(&format!(
            "(rec {first} (type $a{i} (sub $a{} (func))))\n",
            i - 1
        ));
    }
    if import {
        decls.push_str(&format!("(import \"i\" \"f\" (func (type $a{depth})))"));
    } else {
        decls.push_str(&format!("(export \"f\" (func (type $a{depth})))"));
    }
    decls
}

#[test]
fn identical_identity_tower_satisfies_import() {
    // Round 4: identity comparison is memoised over node pairs, so
    // matching two separately decoded copies of this tower is
    // linear in the number of distinct pairs. At depth 40 the
    // unmemoised comparison (~3^40 subgraph traversals) would
    // never finish; this test completing at all is the assertion.
    compile(&instantiate_with_typed_module(
        &identity_tower_decls(40, true, false),
        &identity_tower_decls(40, false, false),
    ))
    .expect("identical identity towers must satisfy the import");
}

#[test]
fn identity_tower_with_deep_difference_is_rejected() {
    // The provider's tower differs only in the first (deepest)
    // group's first member; the comparison must reach the bottom
    // of the tower, decide inequality there, and the instantiation
    // is rejected.
    expect_rejected(&instantiate_with_typed_module(
        &identity_tower_decls(40, true, false),
        &identity_tower_decls(40, false, true),
    ));
}

/// Builds the module-type declarations for the round-5 tower
/// shape: `$a0` is a standalone non-final type declaring no
/// supertype, and each group `i` pairs `$b{i}` -- which subtypes
/// `$a{i-1}` -- with `$a{i}`, which declares no supertype. The
/// exported func has type `$a{depth}`. Declared subtype chains
/// are one edge deep, so the shape isolates the recursion-group
/// descent of the identity comparison: matching reaches the level
/// below only through `$b{i}`'s recorded external supertype, one
/// group per level. With `alter_deepest`, the first (deepest)
/// group's first member is a plain type of a different signature
/// instead of a subtype of `$a0` -- a difference only visible at
/// the very bottom of the tower, which keeps each side internally
/// valid (a declared subtype's signature must match its
/// supertype's, so altering `$a0` itself would be rejected at
/// validation time instead).
fn deep_group_tower_decls(depth: usize, import: bool, alter_deepest: bool) -> String {
    let mut decls = String::from("(type $a0 (sub (func)))\n");
    for i in 1..=depth {
        let first = if alter_deepest && i == 1 {
            "(type $b1 (func (param i32)))".to_string()
        } else {
            format!("(type $b{i} (sub $a{} (func)))", i - 1)
        };
        decls.push_str(&format!("(rec {first} (type $a{i} (sub (func))))\n"));
    }
    if import {
        decls.push_str(&format!("(import \"i\" \"f\" (func (type $a{depth})))"));
    } else {
        decls.push_str(&format!("(export \"f\" (func (type $a{depth})))"));
    }
    decls
}

#[test]
fn deep_group_tower_satisfies_import() {
    // Round 5: with a recursive identity comparison this aborted
    // the process with a stack overflow (measured on the pre-fix
    // code: depth 15,000 still passed, depth 20,000 aborted, in
    // this same harness; Codex reproduced the abort at 100,000
    // groups with the comparison on the stack). The work-stack
    // comparison -- and the iterative `Drop` dismantling the
    // towers afterwards -- completes; that is the assertion.
    compile(&instantiate_with_typed_module(
        &deep_group_tower_decls(100_000, true, false),
        &deep_group_tower_decls(100_000, false, false),
    ))
    .expect("identical deep group towers must satisfy the import");
}

#[test]
fn deep_group_tower_with_deep_difference_is_rejected() {
    // The provider's tower differs only in the first (deepest)
    // group's first member; the comparison must descend all
    // 100,000 groups, decide inequality there, and the
    // instantiation is rejected -- without overflowing on the
    // way down or back.
    expect_rejected(&instantiate_with_typed_module(
        &deep_group_tower_decls(100_000, true, false),
        &deep_group_tower_decls(100_000, false, true),
    ));
}

fn expect_rejected_with(text: &str, needle: &str) {
    let error = compile(text).expect_err("component must be rejected");
    assert!(
        error.contains(needle),
        "expected an error containing `{needle}`, got `{error}`"
    );
}

/// Exports the component `$c` under a component-type ascription
/// `$ct`, where `$ct` imports a core module of type `$M1` and `$c`
/// imports a core module of type `$M2`. Checking the ascription
/// compares the two module types whole (`ComponentType::
/// assert_subtype_of`, core module import arm).
fn ascription_import_component(parent_decls: &str, child_decls: &str) -> String {
    format!(
        r#"
(component
  (core type $M1 (module
    {parent_decls}
  ))
  (core type $M2 (module
    {child_decls}
  ))
  (type $ct (component
    (import "m" (core module (type $M1)))
  ))
  (component $c
    (import "m" (core module (type $M2)))
  )
  (export "x" (component $c) (component (type $ct)))
)
"#
    )
}

#[test]
fn identical_identity_tower_satisfies_type_ascription() {
    // Round 6: the ascription check compares the two module types
    // with the bounded equality, so matching two separately
    // decoded copies of this tower is linear in the number of
    // distinct identity pairs. At depth 40 the derived comparison
    // (~3^40 subgraph traversals) would never finish; this test
    // completing at all is the assertion.
    compile(&ascription_import_component(
        &identity_tower_decls(40, true, false),
        &identity_tower_decls(40, true, false),
    ))
    .expect("identical identity towers must satisfy the ascription");
}

#[test]
fn identity_tower_with_deep_difference_is_rejected_through_type_ascription() {
    // The child's tower differs only in the first (deepest)
    // group's first member; the ascription comparison must reach
    // the bottom of the tower and reject.
    expect_rejected_with(
        &ascription_import_component(
            &identity_tower_decls(40, true, false),
            &identity_tower_decls(40, true, true),
        ),
        "import kind mismatch",
    );
}

#[test]
fn deep_group_tower_satisfies_type_ascription() {
    // Round 6, the round-5 shape on the ascription path: with the
    // derived equality this aborted the process with a stack
    // overflow (measured on the unmodified code at depth 100,000;
    // depth 20,000 still completed). The bounded comparison --
    // and the iterative `Drop` dismantling the towers afterwards
    // -- completes; that is the assertion.
    compile(&ascription_import_component(
        &deep_group_tower_decls(100_000, true, false),
        &deep_group_tower_decls(100_000, true, false),
    ))
    .expect("identical deep group towers must satisfy the ascription");
}

/// Exports the component `$c` under a component-type ascription
/// whose export is itself a component type, so the inner component
/// types are compared by type id through the validator's
/// surface-summary check (`assert_component_type_ids_subtype_of`,
/// core module import arm).
fn nested_component_component(parent_decls: &str, inner_decls: &str) -> String {
    format!(
        r#"
(component
  (core type $M1 (module
    {parent_decls}
  ))
  (type $ct1 (component
    (import "m" (core module (type $M1)))
  ))
  (type $outer (component
    (export "inner" (component (type $ct1)))
  ))
  (component $c
    (core type $M2 (module
      {inner_decls}
    ))
    (type $ct2 (component
      (import "m" (core module (type $M2)))
    ))
    (component $inner
      (import "m" (core module (type $M2)))
    )
    (export "inner" (component $inner) (component (type $ct2)))
  )
  (export "x" (component $c) (component (type $outer)))
)
"#
    )
}

#[test]
fn identical_identity_tower_satisfies_nested_component_type_check() {
    // Round 6: the surface-summary comparison of the two inner
    // component types goes through the bounded equality as well.
    compile(&nested_component_component(
        &identity_tower_decls(40, true, false),
        &identity_tower_decls(40, true, false),
    ))
    .expect("identical identity towers must satisfy the nested component type check");
}

#[test]
fn identity_tower_with_deep_difference_is_rejected_through_nested_component_type_check() {
    expect_rejected_with(
        &nested_component_component(
            &identity_tower_decls(40, true, false),
            &identity_tower_decls(40, true, true),
        ),
        "import kind mismatch",
    );
}

/// Exports the component `$c` under a component-type ascription
/// `$ct` that both imports and exports a core module. Both sides
/// import a module of the same type `$ME` (so the import check
/// passes identically), while the export check compares the
/// parent's export module type `$ME1` against the child's
/// re-exported module type `$ME` (`ComponentExportType` core
/// module arm).
fn export_ascription_component(import_decls: &str, parent_export_decls: &str) -> String {
    format!(
        r#"
(component
  (core type $ME (module
    {import_decls}
  ))
  (core type $ME1 (module
    {parent_export_decls}
  ))
  (type $ct (component
    (import "m" (core module (type $ME)))
    (export "m" (core module (type $ME1)))
  ))
  (component $c
    (import "m" (core module (type $ME)))
    (export "m" (core module 0))
  )
  (export "x" (component $c) (component (type $ct)))
)
"#
    )
}

#[test]
fn identical_identity_tower_satisfies_export_ascription() {
    // Round 6: the core module *export* comparison in component
    // type matching goes through the bounded equality too.
    compile(&export_ascription_component(
        &identity_tower_decls(40, true, false),
        &identity_tower_decls(40, true, false),
    ))
    .expect("identical identity towers must satisfy the export ascription");
}

#[test]
fn identity_tower_with_deep_difference_is_rejected_through_export_ascription() {
    // Only the parent's export module type is altered, at the
    // deepest group member, so the rejection is decided by the
    // export comparison alone.
    expect_rejected_with(
        &export_ascription_component(
            &identity_tower_decls(40, true, false),
            &identity_tower_decls(40, true, true),
        ),
        "core module export mismatch",
    );
}

/// Exports an instance of `$c` under an instance-type ascription
/// `$it` whose only export is a core module, comparing the two
/// module types through the `InstanceExportType` core module arm.
fn instance_type_export_component(parent_decls: &str, child_decls: &str) -> String {
    format!(
        r#"
(component
  (core type $M1 (module
    {parent_decls}
  ))
  (core type $M2 (module
    {child_decls}
  ))
  (import "p" (core module (type $M2)))
  (type $it (instance
    (export "m" (core module (type $M1)))
  ))
  (component $c
    (import "m" (core module (type $M2)))
    (export "m" (core module 0))
  )
  (instance $i (instantiate $c (with "m" (core module 0))))
  (export "i" (instance $i) (instance (type $it)))
)
"#
    )
}

#[test]
fn identical_identity_tower_satisfies_instance_type_ascription() {
    // Round 6: the instance-type version of the core module
    // export comparison goes through the bounded equality too.
    compile(&instance_type_export_component(
        &identity_tower_decls(40, true, false),
        &identity_tower_decls(40, true, false),
    ))
    .expect("identical identity towers must satisfy the instance type ascription");
}

#[test]
fn identity_tower_with_deep_difference_is_rejected_through_instance_type_ascription() {
    // Only the ascribed instance type's module type is altered,
    // at the deepest group member.
    expect_rejected_with(
        &instance_type_export_component(
            &identity_tower_decls(40, true, true),
            &identity_tower_decls(40, true, false),
        ),
        "core module export mismatch",
    );
}
