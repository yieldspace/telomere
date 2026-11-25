//! Regression tests for issue #321: outer-alias and exportability
//! validation gaps found by the vendored upstream conformance suite
//! (`validation/outer-alias.wast`, `validation/external-visibility.wast`,
//! `linking/unit.wast`).
//!
//! - An outer alias inside a component *type* that crosses a child
//!   component boundary and transitively refers to the outer
//!   component's resources was accepted (outer-alias.wast:68).
//! - Alias export / alias outer inside type declarators accepted
//!   items other than types and instances (outer-alias.wast:172-195).
//! - A flags type mentioned by an exported function was treated
//!   as anonymous-able even though flags types must be named at a
//!   surface (external-visibility.wast flags section).
//!
//! Not covered here: whether a *plain* type export names the
//! underlying type for the component's other exports
//! (external-visibility.wast:28, 70, 80, 130). Telomere's surface
//! model names the underlying type; the reference validator
//! distinguishes the export's created type from the underlying
//! type. See the exclusion note in
//! `component_model_upstream.rs`.
//! - Freshening an imported instance type replaced *every* abstract
//!   generic it contained, including generics that belong to the
//!   enclosing component and are only referenced through the type, so
//!   a valid instance import was rejected with `surface type requires
//!   exported/imported name` (unit.wast:799, and the dependent
//!   count-1/count-2 failures of issue #347).

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

fn expect_valid(text: &str) {
    compile_component(text).expect("component must be accepted");
}

// ---------------------------------------------------------------------
// validation/outer-alias.wast
// ---------------------------------------------------------------------

#[test]
fn alias_outer_resource_through_child_component_in_type_is_invalid() {
    // outer-alias.wast line 68: the alias sits in a component type
    // declared in a child component; the resource belongs to the
    // outer component, so the aliased type transitively refers to
    // resources of a different component.
    expect_invalid_with(
        r#"
(component $C
  (type $R (resource (rep i32)))
  (component (type (component
    (alias outer 2 0 (type $a))
  ))))
"#,
        "not defined in the current component",
    );
}

#[test]
fn alias_export_of_func_in_component_type_is_invalid() {
    // outer-alias.wast line 172.
    expect_invalid_with(
        r#"
(component
  (type (component
    (import "i" (instance $i (export "f" (func))))
    (alias export $i "f" (func $a))
  )))
"#,
        "may only refer to types or instances",
    );
}

#[test]
fn alias_outer_of_component_in_component_type_is_invalid() {
    // outer-alias.wast line 181.
    expect_invalid_with(
        r#"
(component $C
  (component $D)
  (type (component (alias outer $C $D (component $a)))))
"#,
        "may only refer to types or instances",
    );
}

#[test]
fn alias_outer_of_core_module_in_component_type_is_invalid() {
    // outer-alias.wast line 188.
    expect_invalid_with(
        r#"
(component $C
  (core module $M)
  (type (component (alias outer $C $M (core module $a)))))
"#,
        "may only refer to types or instances",
    );
}

#[test]
fn alias_outer_of_component_in_instance_type_is_invalid() {
    // outer-alias.wast line 195.
    expect_invalid_with(
        r#"
(component $C
  (component $D)
  (type (instance (alias outer $C $D (component $a)))))
"#,
        "may only refer to types or instances",
    );
}

#[test]
fn alias_outer_across_type_boundary_only_is_valid() {
    // Only a `type` boundary is crossed, so referring to the
    // enclosing component's own resources stays valid.
    expect_valid(
        r#"
(component $C
  (type $R (resource (rep i32)))
  (type $B (borrow $R))
  (type (component
    (alias outer $C $R (type $r))
    (alias outer $C $B (type $b))
  ))
  (type (instance
    (alias outer $C $R (type $r))
  )))
"#,
    );
}

#[test]
fn alias_outer_of_core_type_and_type_in_component_type_is_valid() {
    expect_valid(
        r#"
(component $C
  (core type $FT (func (param i32) (result i32)))
  (type $T (record (field "x" u32)))
  (type (component
    (alias outer $C $FT (core type $ct))
    (alias outer $C $T (type $t))
  )))
"#,
    );
}

#[test]
fn alias_export_of_type_and_instance_in_component_type_is_valid() {
    expect_valid(
        r#"
(component $C
  (type (component
    (import "i" (instance $i
      (export "t" (type (sub resource)))
      (export "j" (instance))
    ))
    (alias export $i "t" (type $t))
    (alias export $i "j" (instance $j))
  )))
"#,
    );
}

// ---------------------------------------------------------------------
// validation/external-visibility.wast
// ---------------------------------------------------------------------

const CORE_PRELUDE: &str = r#"
  (core module $m (func (export "f") (result i32) unreachable))
  (core instance $i (instantiate $m))
"#;

#[test]
fn unexported_flags_func_export_is_invalid() {
    // A flags type must be named by an import or export wherever a
    // surface mentions it; a purely local flags type is not a valid
    // export surface.
    expect_invalid_with(
        &format!(
            r#"
(component
{CORE_PRELUDE}
  (type $Fl (flags "a" "b"))
  (func $f (result $Fl) (canon lift (core func $i "f")))
  (export "f" (func $f)))
"#
        ),
        "func not valid to be used as export",
    );
}

#[test]
fn ascribed_func_export_may_use_the_exported_resource_name() {
    // The export ascription mentions the resource through its
    // exported name, which is a valid surface type.
    expect_valid(&format!(
        r#"
(component
{CORE_PRELUDE}
  (type $R (resource (rep i32)))
  (func $f (result (own $R)) (canon lift (core func $i "f")))
  (export $R' "r" (type $R))
  (export "f" (func $f) (func (result (own $R')))))
"#
    ));
}

#[test]
fn record_field_may_not_use_the_local_resource() {
    // Same shape, but the field mentions the local resource type,
    // which has no exported name.
    expect_invalid_with(
        &format!(
            r#"
(component
{CORE_PRELUDE}
  (type $R (resource (rep i32)))
  (type $Rec (record (field "r" (own $R))))
  (export $Rec' "rec" (type $Rec))
  (func $f (result $Rec') (canon lift (core func $i "f")))
  (export "f" (func $f)))
"#
        ),
        "type not valid to be used as export",
    );
}

#[test]
fn unexported_resource_func_export_is_invalid() {
    expect_invalid_with(
        &format!(
            r#"
(component
{CORE_PRELUDE}
  (type $R (resource (rep i32)))
  (func $f (result (own $R)) (canon lift (core func $i "f")))
  (export "f" (func $f)))
"#
        ),
        "func not valid to be used as export",
    );
}

#[test]
fn external_visibility_opening_example_is_valid() {
    // The opening definition of external-visibility.wast: a function
    // exported with an ascription through the exported name, a
    // function defined after the export whose signature mentions the
    // exported name directly, and imported abstract resources.
    expect_valid(&format!(
        r#"
(component
{CORE_PRELUDE}
  (type $R (resource (rep i32)))
  (func $f (result (own $R)) (canon lift (core func $i "f")))
  (export $R' "r" (type $R))
  (export "f" (func $f) (func (result (own $R'))))
  (func $f2 (result (own $R')) (canon lift (core func $i "f")))
  (export "f2" (func $f2))
  (import "r2" (type $R2 (sub resource)))
  (import "f3" (func $f3 (result (own $R2))))
  (func $f4 (result (own $R2)) (canon lift (core func $i "f")))
  (export "f4" (func $f4)))
"#
    ));
}

// ---------------------------------------------------------------------
// linking/unit.wast line 799 (and issue #347)
// ---------------------------------------------------------------------

#[test]
fn instance_import_may_reference_enclosing_abstract_resource() {
    // unit.wast line 799 (the middleman's sink import): the inline
    // instance type brings the enclosing component's abstract
    // resource in through an outer alias and mentions it in the
    // method signature. Importing an instance of that type must
    // keep the reference to the enclosing resource instead of
    // replacing it with a fresh, unnamed generic.
    expect_valid(
        r#"
(component $User
  (import "def" (instance $def
    (export "r" (type $Rd (sub resource)))
    (export "make" (func (param "rep" u32) (result (own $Rd))))))
  (alias export $def "r" (type $R))
  (import "sink" (instance $sink
    (alias outer $User $R (type $Rs))
    (export "take" (func (param "h" (own $Rs)))))))
"#,
    );
}
