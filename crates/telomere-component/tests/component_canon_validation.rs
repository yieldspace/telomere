use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

#[test]
fn canon_lower_allows_indirect_non_memory_results_without_realloc() {
    compile_component(
        r#"
(component
  (type $status (tuple u32 u32))
  (type $host-func (func (result $status)))
  (import "host" (func $host (type $host-func)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $host-lower
    (canon lower (func $host) (memory $libc "memory"))
  )
)
"#,
    )
    .expect("resource-backed indirect result should not require realloc");
}

#[test]
fn canon_lower_requires_realloc_for_memory_backed_results() {
    let error = compile_component(
        r#"
(component
  (type $host-func (func (result string)))
  (import "host" (func $host (type $host-func)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $host-lower
    (canon lower (func $host) (memory $libc "memory"))
  )
)
"#,
    )
    .expect_err("memory-backed result without realloc must fail");

    assert!(
        error
            .to_string()
            .contains("canonical option `realloc` is required"),
        "unexpected error: {error}"
    );
}

#[test]
fn imported_instance_types_may_export_named_handle_carrying_types() {
    compile_component(
        r#"
(component
  (type $wasi-error
    (instance
      (export "error" (type (sub resource)))
    )
  )
  (import "wasi:io/error@0.2.6" (instance $wasi:io/error@0.2.6 (type $wasi-error)))
  (alias export $wasi:io/error@0.2.6 "error" (type $error))
  (type $wasi-streams
    (instance
      (alias outer 1 $error (type (;0;)))
      (export "error" (type (eq 0)))
    )
  )
  (import "wasi:io/streams@0.2.6" (instance (type $wasi-streams)))
)
"#,
    )
    .expect("instance import should accept exported named types that carry visible handles");
}
