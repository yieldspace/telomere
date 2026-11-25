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
fn named_sub_ascribed_export_reexported_by_index_keeps_face_identity() {
    // Regression (Codex review of PR #357): a named sub-ascribed export
    // `(export $face "r1" (type $r) (type (sub resource)))` must register the
    // fresh face identity in the type index space, so that a later
    // `(export "r2" (type $face))` exports the very same resource as `r1`.
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $face "r1" (type $r) (type (sub resource)))
    (export "r2" (type $face)))
  (instance $c (instantiate $C))
  (alias export $c "r1" (type $r1))
  (alias export $c "r2" (type $r2))
  (component $eq
    (import "a" (type $a (sub resource)))
    (import "b" (type (eq $a))))
  (instance (instantiate $eq (with "a" (type $r1)) (with "b" (type $r2)))))
"#,
    )
    .expect("re-exporting a named sub-ascribed export by type index must keep the face identity");
}

#[test]
fn named_sub_ascribed_export_reexport_differs_from_underlying_resource() {
    // Companion control: the re-exported face must still differ from the
    // underlying resource itself (the #320 ascription semantics).
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export "r0" (type $r))
    (export $face "r1" (type $r) (type (sub resource)))
    (export "r2" (type $face)))
  (instance $c (instantiate $C))
  (alias export $c "r0" (type $r0))
  (alias export $c "r2" (type $r2))
  (component $eq
    (import "a" (type $a (sub resource)))
    (import "b" (type (eq $a))))
  (instance (instantiate $eq (with "a" (type $r0)) (with "b" (type $r2)))))
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

#[test]
fn chained_sub_ascription_on_named_sub_ascribed_export_compiles() {
    // Regression (Codex review of PR #357, round 2): a named sub-ascribed
    // export must itself accept a further `(type (sub resource))`
    // ascription. Round 1 registered the face as `Type::Generic(Sub)` in
    // the type index space, but the export sub-ascription check only
    // accepted `Type::Resource`, so this sequence failed with
    // `export kind mismatch` even though it compiled before #320.
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $face "r1" (type $r) (type (sub resource)))
    (export "r2" (type $face) (type (sub resource)))))
"#,
    )
    .expect("a sub ascription over a sub-ascribed export face must be accepted");
}

#[test]
fn chained_sub_ascription_mints_identity_distinct_from_face() {
    // Companion control: the chained ascription still mints a fresh
    // identity, distinct from the face it ascribes (and from the
    // underlying resource) -- it must not silently alias the face.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $face "r1" (type $r) (type (sub resource)))
    (export "r2" (type $face) (type (sub resource))))
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
fn eq_ascription_between_distinct_sub_ascribed_faces_is_invalid() {
    // Regression (Codex review of PR #357, round 3): an `(eq ...)`
    // ascription on a type export asserts identity, but the check used
    // the generic subtype relation, which accepts any two `Sub`-bounded
    // generics without comparing identities. Two faces minted from
    // distinct resources could therefore be equated, and the export
    // would denote the second face externally while its local type
    // index still denotes the first.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (export "c" (type $a) (type (eq $b)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_of_face_to_itself_is_valid() {
    // Positive control: an eq ascription naming the very same face is
    // an identity assertion that holds.
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (export "c" (type $a) (type (eq $a)))))
"#,
    )
    .expect("an eq ascription of a face to itself must be accepted");
}

#[test]
fn eq_ascription_between_resource_and_its_own_face_is_invalid() {
    // Sibling direction of the round-3 finding: the concrete resource
    // equated to a face minted from it. The subtype relation accepted
    // this (a resource is a subtype of any `Sub` generic), but the
    // face is a distinct identity from the underlying resource.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (export "c" (type $r) (type (eq $a)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_face_and_its_underlying_resource_is_invalid() {
    // Companion direction: a face equated to its underlying resource
    // must also be rejected as an identity mismatch.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (export "c" (type $a) (type (eq $r)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_own_of_distinct_faces_is_invalid() {
    // Regression (Codex review of PR #357, round 4): the round-3
    // identity check only looked at the top-level type, so wrapping
    // two distinct faces in `own` made both sides structural types
    // that return `None` from `resource_face_id`, bypassing the check;
    // the subtype relation then accepted the distinct sub-bounded
    // handle targets inside.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (export "c" (type $oa) (type (eq $ob)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_borrow_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $ba (borrow $a))
    (type $bb (borrow $b))
    (export "c" (type $ba) (type (eq $bb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_own_of_same_face_is_valid() {
    // Positive control: structurally wrapped references to the very
    // same face still satisfy the identity assertion.
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (type $oa (own $a))
    (export "c" (type $oa) (type (eq $oa)))))
"#,
    )
    .expect("an eq ascription between own of the same face must be accepted");
}

#[test]
fn eq_ascription_between_list_of_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $la (list $oa))
    (type $lb (list $ob))
    (export "c" (type $la) (type (eq $lb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_record_with_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $ra (record (field "h" $oa)))
    (type $rb (record (field "h" $ob)))
    (export "c" (type $ra) (type (eq $rb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_variant_with_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $va (variant (case "h" $oa)))
    (type $vb (variant (case "h" $ob)))
    (export "c" (type $va) (type (eq $vb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_option_of_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $pa (option $oa))
    (type $pb (option $ob))
    (export "c" (type $pa) (type (eq $pb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_result_with_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $ra (result $oa))
    (type $rb (result $ob))
    (export "c" (type $ra) (type (eq $rb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_tuple_with_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $ta (tuple $oa))
    (type $tb (tuple $ob))
    (export "c" (type $ta) (type (eq $tb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_func_param_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $fa (func (param "x" $oa)))
    (type $fb (func (param "x" $ob)))
    (export "c" (type $fa) (type (eq $fb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_func_result_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $fa (func (result $oa)))
    (type $fb (func (result $ob)))
    (export "c" (type $fa) (type (eq $fb)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_record_with_own_of_face_and_resource_is_invalid() {
    // Nested form of the round-3 sibling direction: a face on one
    // side, its underlying concrete resource on the other, both
    // wrapped in `own` inside a record.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (type $oa (own $a))
    (type $or (own $r))
    (type $ra (record (field "h" $oa)))
    (type $rr (record (field "h" $or)))
    (export "c" (type $ra) (type (eq $rr)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_own_of_distinct_concrete_resources_is_invalid() {
    // Baseline control without faces: distinct concrete resources
    // were already rejected before this PR's face machinery, at any
    // nesting depth, and must stay rejected with the same message.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (type $o1 (own $r1))
    (type $o2 (own $r2))
    (export "c" (type $o1) (type (eq $o2)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_record_with_own_of_same_face_is_valid() {
    // Positive control at depth: the same face referenced from both
    // sides inside a record satisfies the identity assertion.
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (type $oa (own $a))
    (type $ra (record (field "h" $oa)))
    (export "c" (type $ra) (type (eq $ra)))))
"#,
    )
    .expect("an eq ascription between records over own of the same face must be accepted");
}

#[test]
fn eq_ascription_between_instance_types_exporting_distinct_faces_is_invalid() {
    // The identity assertion applies through instance types too: an
    // instance type whose type export is pinned to one face is not
    // the same type as one pinned to a different face.
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $i1 (instance (export "x" (type (eq $a)))))
    (type $i2 (instance (export "x" (type (eq $b)))))
    (export "c" (type $i1) (type (eq $i2)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_instance_types_with_func_own_of_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $oa (own $a))
    (type $ob (own $b))
    (type $i1 (instance (export "f" (func (param "x" $oa)))))
    (type $i2 (instance (export "f" (func (param "x" $ob)))))
    (export "c" (type $i1) (type (eq $i2)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_component_types_exporting_distinct_faces_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $r1 (resource (rep i32)))
    (type $r2 (resource (rep i32)))
    (export $a "a" (type $r1) (type (sub resource)))
    (export $b "b" (type $r2) (type (sub resource)))
    (type $ct1 (component (export "x" (type (eq $a)))))
    (type $ct2 (component (export "x" (type (eq $b)))))
    (export "c" (type $ct1) (type (eq $ct2)))))
"#,
        "resource types are not the same",
    );
}

#[test]
fn eq_ascription_between_instance_types_exporting_same_face_is_valid() {
    // Positive control at the instance-type level: both sides pin
    // the export to the very same face.
    compile_component(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (type $i1 (instance (export "x" (type (eq $a)))))
    (type $i2 (instance (export "x" (type (eq $a)))))
    (export "c" (type $i1) (type (eq $i2)))))
"#,
    )
    .expect("instance types pinning the same face must satisfy the eq ascription");
}

#[test]
fn eq_ascription_between_instance_types_with_inline_sub_exports_is_valid() {
    // Alpha-equivalence control: abstract types declared *inside*
    // an instance type are bound by that type, not denoted by it, so
    // two separately written instance types exporting an abstract
    // resource are the same type -- as they were before faces.
    compile_component(
        r#"
(component
  (component $C
    (type $i1 (instance (export "x" (type (sub resource)))))
    (type $i2 (instance (export "x" (type (sub resource)))))
    (export "c" (type $i1) (type (eq $i2)))))
"#,
    )
    .expect("inline abstract exports of two instance types must alpha-match");
}

#[test]
fn eq_ascription_between_component_types_with_independent_sub_imports_is_valid() {
    // Alpha-equivalence control at the component-type level: two
    // component types whose imports are independent `(sub resource)`
    // generics matched by bound before this change and must keep
    // matching; only identities denoted from the enclosing scope are
    // nominal.
    compile_component(
        r#"
(component
  (component $C
    (type $ct1 (component (import "x" (type (sub resource)))))
    (type $ct2 (component (import "x" (type (sub resource)))))
    (export "c" (type $ct1) (type (eq $ct2)))))
"#,
    )
    .expect("independent abstract imports of two component types must alpha-match");
}
