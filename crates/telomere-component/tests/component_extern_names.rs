//! Regression tests for #318: the split extern-name encoding (name prefix
//! 0x02 carrying `implements` / `version-suffix` / `external-id` options)
//! must decode, and malformed extern names (nested package namespaces and
//! nested projections) must be rejected.

use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn assert_valid(text: &str) {
    if let Err(error) = compile_component(text) {
        panic!("expected a valid component, got error: {error}");
    }
}

fn assert_invalid(text: &str, expected: &str) {
    match compile_component(text) {
        Ok(()) => panic!("expected an error containing `{expected}`, but compilation succeeded"),
        Err(error) => {
            let actual = error.to_string();
            assert!(
                actual.contains(expected),
                "expected an error containing `{expected}`, got `{actual}`"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Valid split-encoded names (0x02 prefix)

#[test]
fn implements_on_import_and_export_instances() {
    assert_valid(
        r#"(component
             (import "a" (implements "a:b/c") (instance))
             (import "c" (implements "a:b/c@1.0.0") (instance))
             (instance $a)
             (export "a" (implements "a:b/c") (instance $a))
             (export "my-label" (implements "ns:pkg/iface") (instance $a)))"#,
    );
}

#[test]
fn external_id_on_any_sort() {
    assert_valid(
        r#"(component
             (import "a" (external-id "id") (func))
             (import "a:b/c" (external-id "id") (instance))
             (import "uni" (external-id "☃︎") (func))
             (import "empty" (external-id "") (func))
             (import "store1" (implements "w:kv/s") (external-id "!@#") (instance)))"#,
    );
}

#[test]
fn version_suffix_valid_splits() {
    assert_valid(
        r#"(component
             (import "a:b/c@1" (version-suffix ".2.3") (instance))
             (import "a:b/d@1" (version-suffix ".2.3+sha.5114f85") (instance))
             (import "a:b/e@10" (version-suffix ".0.0") (instance))
             (import "a:b/f@0.2" (version-suffix ".6") (instance))
             (import "a:b/g@0.2" (version-suffix ".6+sha.5114f85") (instance))
             (import "a:b/h@0.10" (version-suffix ".0") (instance))
             (import "a:b/i@0.0.1" (instance))
             (import "a:b/j@0.0.1" (version-suffix "+sha.5114f85") (instance))
             (import "a:b/k@0.0.0" (instance))
             (import "a:b/l@0.0.12" (version-suffix "+b") (instance))
             (import "a:b/m@1.2.3-alpha" (instance))
             (import "a:b/n@1.2.3-alpha.1" (version-suffix "+sha.5114") (instance))
             (import "a:b/o@0.2.6-rc.1" (version-suffix "+b") (instance))
             (import "a:b/p@0.0.1-alpha" (version-suffix "+sha") (instance)))"#,
    );
}

#[test]
fn version_suffix_applies_to_implements() {
    assert_valid(
        r#"(component
             (import "a" (implements "a:b/c@1") (version-suffix ".2.3") (instance $a))
             (import "b" (implements "a:b/c@0.2") (version-suffix ".6+sha.5114f85") (instance))
             (import "c" (implements "a:b/c@0.0.1") (instance))
             (import "d" (implements "a:b/c@1.2.3") (instance))
             (export "e" (implements "a:b/c@0.2") (version-suffix ".1") (instance $a)))"#,
    );
}

#[test]
fn attributes_inside_types_and_inline_instances() {
    assert_valid(
        r#"(component
             (type (instance
               (export "a" (implements "a:b/c") (instance))
               (export "f" (implements "a:b/c@1") (version-suffix ".2.3") (instance))))
             (type (component
               (import "a" (implements "a:b/c") (instance))
               (export "b" (implements "a:b/c") (external-id "y") (instance))))
             (instance $a)
             (instance
               (export "a" (implements "a:b/c") (instance $a))))"#,
    );
}

// ---------------------------------------------------------------------------
// Malformed extern names (defect 2)

#[test]
fn nested_namespace_is_rejected() {
    assert_invalid(
        r#"(component (import "foo:bar:baz/qux" (func)))"#,
        "expected `/` after package name",
    );
}

#[test]
fn nested_projection_is_rejected() {
    assert_invalid(
        r#"(component (import "foo:bar/baz/qux" (func)))"#,
        "trailing characters found: `/qux`",
    );
}

// ---------------------------------------------------------------------------
// Attribute validation rules

#[test]
fn implements_must_be_an_interface_name() {
    assert_invalid(
        r#"(component (import "a" (implements "not-valid") (instance)))"#,
        "must be an interface",
    );
    assert_invalid(
        r#"(component (import "a" (implements "") (instance)))"#,
        "not a valid name",
    );
}

#[test]
fn implements_only_on_instances_with_plain_names() {
    assert_invalid(
        r#"(component (import "a" (implements "a:b/c") (func)))"#,
        "only instances can have an `implements`",
    );
    assert_invalid(
        r#"(component (import "a1:b/c" (implements "a2:b/c") (instance)))"#,
        "name `a1:b/c` is not valid with `implements`",
    );
}

#[test]
fn version_suffix_rules() {
    assert_invalid(
        r#"(component (import "a:b/c@1" (version-suffix ".2.3") (func)))"#,
        "only instances can have a `version-suffix`",
    );
    assert_invalid(
        r#"(component (import "a:b/c@0.0.1" (version-suffix "") (instance)))"#,
        "version suffix cannot be empty",
    );
    assert_invalid(
        r#"(component (import "a" (version-suffix ".2.3") (instance)))"#,
        "`version-suffix` requires an interface name or `implements`",
    );
    assert_invalid(
        r#"(component (import "a:b/c" (version-suffix ".2.3") (instance)))"#,
        "a version suffix requires the name to have a version",
    );
    assert_invalid(
        r#"(component (import "a:b/c@1" (version-suffix ".2.3-alpha") (instance)))"#,
        "is not the canonical version of `1.2.3-alpha`",
    );
    assert_invalid(
        r#"(component (import "a:b/c@1" (version-suffix ".2") (instance)))"#,
        "is not valid",
    );
    assert_invalid(
        r#"(component (import "a:b/c@1" (instance)))"#,
        "is not valid",
    );
    assert_invalid(
        r#"(component (import "a:b/c@0.2" (instance)))"#,
        "is not valid",
    );
}

#[test]
fn implements_version_rules() {
    assert_invalid(
        r#"(component (import "a" (implements "a:b/c@1") (instance)))"#,
        "invalid interface version",
    );
    assert_invalid(
        r#"(component (import "a" (implements "a:b/c@1") (version-suffix ".2") (instance)))"#,
        "invalid interface version",
    );
    assert_invalid(
        r#"(component (import "a" (implements "a:b/c@1") (version-suffix ".2.3-alpha") (instance)))"#,
        "is not the canonical version of `1.2.3-alpha`",
    );
    assert_invalid(
        r#"(component (import "a" (implements "a:b/c") (version-suffix ".2.3") (instance)))"#,
        "a version suffix requires the name to have a version",
    );
}

#[test]
fn attributes_do_not_relax_name_uniqueness() {
    assert_invalid(
        r#"(component
             (import "a" (implements "a:b/c") (instance))
             (import "a" (implements "a:b/c") (instance)))"#,
        "conflicts with previous name",
    );
    assert_invalid(
        r#"(component
             (import "a" (implements "a1:b/c") (instance))
             (import "a" (implements "a2:b/c") (instance)))"#,
        "conflicts with previous name",
    );
    assert_invalid(
        r#"(component
             (import "a:b/c@1" (version-suffix ".2.3") (instance))
             (import "a:b/c@1" (version-suffix ".4.5") (instance)))"#,
        "conflicts with previous name",
    );
    assert_invalid(
        r#"(component
             (import "a" (external-id "x") (func))
             (import "a" (external-id "y") (func)))"#,
        "conflicts with previous name",
    );
}
