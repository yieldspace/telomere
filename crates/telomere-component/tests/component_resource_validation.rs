//! Regression tests for issue #320: resource type validation gaps found by
//! the vendored upstream conformance suite (`validation/resources.wast`).
//!
//! - Valid components whose nested component imports a `(type (eq ...))`
//!   definition mentioning `own`/`borrow` of an imported abstract resource
//!   were rejected with `type not valid to be used as import`
//!   (resources.wast lines 270, 291, 312).
//! - Exporting a resource with a `(type (sub resource))` ascription must
//!   produce a fresh resource identity; treating it as the underlying
//!   resource let an identity mismatch through (line 576).
//! - own/borrow mismatches were reported as a generic
//!   `defvaltype mismatch` instead of `expected own, found borrow`
//!   (lines 101, 179).

use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn expect_invalid_with(text: &str, needle: &str) {
    let error = compile_component(text).expect_err("component must be rejected");
    let message = error.to_string();
    assert!(
        message.contains(needle),
        "expected error containing `{needle}`, got `{message}`"
    );
}

#[test]
fn eq_import_of_record_with_own_of_imported_resource_is_valid() {
    compile_component(
        r#"
(component
  (type $R (resource (rep i32)))
  (type $Rec (record (field "h" (own $R))))
  (component $c
    (import "T" (type $t (sub resource)))
    (type $expected (record (field "h" (own $t))))
    (import "a" (type (eq $expected))))
  (instance (instantiate $c (with "T" (type $R)) (with "a" (type $Rec)))))
"#,
    )
    .expect("record with own of an imported resource is a valid import type");
}

#[test]
fn eq_import_of_variant_with_own_of_imported_resource_is_valid() {
    compile_component(
        r#"
(component
  (type $R (resource (rep i32)))
  (type $Var (variant (case "c" (own $R))))
  (component $c
    (import "T" (type $t (sub resource)))
    (type $expected (variant (case "c" (own $t))))
    (import "a" (type (eq $expected))))
  (instance (instantiate $c (with "T" (type $R)) (with "a" (type $Var)))))
"#,
    )
    .expect("variant with own of an imported resource is a valid import type");
}

#[test]
fn eq_import_of_multi_resource_record_is_valid() {
    compile_component(
        r#"
(component
  (type $R1 (resource (rep i32)))
  (type $R2 (resource (rep i32)))
  (type $Rec (record (field "f1" (own $R1))
    (field "f2" (borrow $R2))
    (field "f3" (own $R1))
    (field "f4" (borrow $R2))
    (field "f5" (list (own $R1)))
    (field "f6" (option (borrow $R2)))
    (field "f7" (own $R1))
    (field "f8" (borrow $R2))))
  (component $c
    (import "T1" (type $t1 (sub resource)))
    (import "T2" (type $t2 (sub resource)))
    (import "T3" (type $t3 (sub resource)))
    (import "T4" (type $t4 (sub resource)))
    (import "T5" (type $t5 (eq $t1)))
    (import "T6" (type $t6 (eq $t2)))
    (import "T7" (type $t7 (eq $t1)))
    (import "T8" (type $t8 (eq $t4)))
    (type $expected (record (field "f1" (own $t1))
      (field "f2" (borrow $t2))
      (field "f3" (own $t3))
      (field "f4" (borrow $t4))
      (field "f5" (list (own $t5)))
      (field "f6" (option (borrow $t6)))
      (field "f7" (own $t7))
      (field "f8" (borrow $t8))))
    (import "a" (type (eq $expected))))
  (instance (instantiate $c
    (with "T1" (type $R1))
    (with "T2" (type $R2))
    (with "T3" (type $R1))
    (with "T4" (type $R2))
    (with "T5" (type $R1))
    (with "T6" (type $R2))
    (with "T7" (type $R1))
    (with "T8" (type $R2))
    (with "a" (type $Rec)))))
"#,
    )
    .expect("multi-resource record is a valid import type");
}

#[test]
fn eq_import_with_mismatched_resource_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $R1 (resource (rep i32)))
  (type $R2 (resource (rep i32)))
  (type $RecU (record (field "h" (own $R2))))
  (component $c
    (import "T" (type $t (sub resource)))
    (type $expected (record (field "h" (own $t))))
    (import "a" (type (eq $expected))))
  (instance (instantiate $c (with "T" (type $R1)) (with "a" (type $RecU)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn multiple_exports_of_one_resource_share_identity_without_ascription() {
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export "r1" (type $r))
    (export "r2" (type $r)))
  (instance $c (instantiate $C))
  (alias export $c "r1" (type $r1))
  (alias export $c "r2" (type $r2))
  (component $eq
    (import "a" (type $a (sub resource)))
    (import "b" (type (eq $a))))
  (instance (instantiate $eq (with "a" (type $r1)) (with "b" (type $r2)))))
"#,
    )
    .expect("two plain exports of one resource have the same identity");
}

#[test]
fn export_sub_ascription_creates_distinct_resource_identity() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export "r1" (type $r))
    (export "r2" (type $r) (type (sub resource))))
  (instance $c (instantiate $C))
  (alias export $c "r1" (type $r1))
  (alias export $c "r2" (type $r2))
  (component $eq
    (import "a" (type $a (sub resource)))
    (import "b" (type (eq $a))))
  (instance (instantiate $eq (with "a" (type $r1)) (with "b" (type $r2)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn borrow_provided_for_own_import_reports_expected_own_found_borrow() {
    expect_invalid_with(
        r#"
(component
  (import "T" (type $T (sub resource)))
  (import "f" (func $f (param "x" (borrow $T))))
  (component $c
    (import "T" (type $T (sub resource)))
    (import "g" (func (param "x" (own $T)))))
  (instance (instantiate $c (with "T" (type $T)) (with "g" (func $f)))))
"#,
        "expected own, found borrow",
    );
}

#[test]
fn list_borrow_provided_for_list_own_import_reports_expected_own_found_borrow() {
    expect_invalid_with(
        r#"
(component
  (import "T" (type $T (sub resource)))
  (type $LB (list (borrow $T)))
  (import "f" (func $f (param "x" $LB)))
  (component $c
    (import "T" (type $T (sub resource)))
    (type $LO (list (own $T)))
    (import "g" (func (param "x" $LO))))
  (instance (instantiate $c (with "T" (type $T)) (with "g" (func $f)))))
"#,
        "expected own, found borrow",
    );
}
