//! Regression tests for issue #325: exports introduce new indices.
//!
//! In the component model, an export appends the exported item to the
//! index space of its sort, so later items may refer to the exported
//! item by the export-introduced index. The vendored conformance suite
//! (`validation/indicies.wast`, "test that exports introduce new
//! indices") exercises this for core modules and components, and the
//! validator rejected the valid shapes with `type idx not found 1`:
//!
//! - `parse_export`'s core-module arm registered the export in the
//!   runtime state store but never appended the module's type to the
//!   validator's core-module store, so `(core instance (instantiate 1))`
//!   after `(export "a" (core module 0))` could not resolve index 1.
//! - The component arm appended the type to the validator's component
//!   store but never registered the export-introduced index in the
//!   runtime state store, so `(instance (instantiate 1))` after
//!   `(export "c" (component 0))` failed the same way.
//!
//! The other sorts (type, instance, func, value) already maintained
//! both stores; these tests pin the behaviour for all of them plus the
//! negative controls that out-of-range indices stay rejected.

use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn expect_valid(text: &str) {
    compile_component(text).expect("component must be accepted");
}

fn expect_invalid_with(text: &str, needle: &str) {
    let error = compile_component(text).expect_err("component must be rejected");
    let message = error.to_string();
    assert!(
        message.contains(needle),
        "expected error containing `{needle}`, got `{message}`"
    );
}

// ---------------------------------------------------------------------
// validation/indicies.wast — core module sort
// ---------------------------------------------------------------------

#[test]
fn core_module_export_introduces_index_for_core_instantiate() {
    expect_valid(
        r#"
(component
  (core module)
  (export "a" (core module 0))
  (core instance (instantiate 1))
)"#,
    );
}

#[test]
fn core_module_exported_twice_introduces_two_indices() {
    expect_valid(
        r#"
(component
  (core module)
  (export "a" (core module 0))
  (export "b" (core module 1))
  (core instance (instantiate 2))
)"#,
    );
}

#[test]
fn core_module_alias_then_export_introduces_index() {
    // indicies.wast lines 358-367: the alias introduces core module 0,
    // the export of that alias introduces core module 1, and the core
    // instantiation refers to the export-introduced index.
    expect_valid(
        r#"
(component
  (component
    (core module)
    (export "a" (core module 0))
  )
  (instance (instantiate 0))
  (alias export 0 "a" (core module))
  (export "a" (core module 0))
  (core instance (instantiate 1))
)"#,
    );
}

#[test]
fn exports_introduce_new_indices_upstream_case() {
    // The full upstream case at indicies.wast lines 341-368, covering
    // both the instance half (which already worked) and the core
    // module half.
    expect_valid(
        r#"
(component
  (component
    (component
      (component
        (component)
        (instance (instantiate 0))
        (export "a" (instance 0))
      )
      (instance (instantiate 0))
      (export "a" (instance 0))
    )
    (instance (instantiate 0))
    (alias export 0 "a" (instance))
    (export "a" (instance 1))
    (alias export 2 "a" (instance))
    (export "inner-a" (instance 3))
  )
  (component
    (component
      (core module)
      (export "a" (core module 0))
    )
    (instance (instantiate 0))
    (alias export 0 "a" (core module))
    (export "a" (core module 0))
    (core instance (instantiate 1))
  )
)"#,
    );
}

#[test]
fn module_definitions_interleaved_with_imports_and_aliases() {
    // indicies.wast lines 328-338: definitions, imports and an alias
    // interleaved in one component.
    expect_valid(
        r#"
(component
  (component
    (core module
      (func (export "")))
    (import "a" (core module))
    (core module
      (func (export "") (result i32) i32.const 5))
    (import "b" (instance (export "a" (core module))))
    (alias export 0 "a" (core module))
  )
)"#,
    );
}

// ---------------------------------------------------------------------
// validation/indicies.wast — component sort
// ---------------------------------------------------------------------

#[test]
fn component_export_introduces_index_for_instantiate() {
    expect_valid(
        r#"
(component
  (component)
  (export "c" (component 0))
  (instance (instantiate 1))
)"#,
    );
}

#[test]
fn component_exported_twice_introduces_two_indices() {
    expect_valid(
        r#"
(component
  (component)
  (export "c" (component 0))
  (export "d" (component 1))
  (instance (instantiate 2))
)"#,
    );
}

// ---------------------------------------------------------------------
// Negative controls: indices past the defined/exported ones stay
// rejected, and exports of unknown indices stay rejected.
// ---------------------------------------------------------------------

#[test]
fn instantiate_past_export_introduced_core_module_index_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (core module)
  (export "a" (core module 0))
  (core instance (instantiate 2))
)"#,
        "type idx not found 2",
    );
}

#[test]
fn export_of_unknown_core_module_index_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (core module)
  (export "a" (core module 1))
)"#,
        "type idx not found 1",
    );
}

#[test]
fn instantiate_unknown_component_index_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component)
  (instance (instantiate 1))
)"#,
        "type idx not found 1",
    );
}

#[test]
fn instantiate_past_export_introduced_component_index_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (component)
  (export "c" (component 0))
  (instance (instantiate 2))
)"#,
        "type idx not found 2",
    );
}
