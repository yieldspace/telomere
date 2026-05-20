use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn compile_component_bytes(bytes: &[u8]) -> Result<(), telomere_component::ComponentError> {
    let engine = ComponentEngine::new();
    engine.compile(bytes).map(|_| ())
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

#[test]
fn unsupported_async_canonical_builtins_fail_closed_with_names() {
    let component = [
        0x00, 0x61, 0x73, 0x6d, // component magic
        0x0d, 0x00, // component version
        0x01, 0x00, // component layer
        0x08, // canonical section
        0x02, // section size
        0x01, // one canonical function
        0x08, // backpressure.set
    ];
    let error = compile_component_bytes(&component)
        .expect_err("unsupported async canonical builtin must fail closed");

    assert!(
        error
            .to_string()
            .contains("canonical function `backpressure.set` is not implemented"),
        "unexpected error: {error}"
    );
}

#[test]
fn error_context_canonical_builtins_decode_and_validate() {
    compile_component(
        r#"
(component
  (core module $libc
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 32))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
  )
  (core instance $libc (instantiate $libc))
  (core func $new
    (canon error-context.new (memory $libc "memory"))
  )
  (core func $debug
    (canon error-context.debug-message
      (memory $libc "memory")
      (realloc (func $libc "realloc")))
  )
  (core func $drop
    (canon error-context.drop)
  )
)
"#,
    )
    .expect("error-context canonical built-ins should decode and validate");
}

#[test]
fn error_context_debug_message_requires_realloc() {
    let error = compile_component(
        r#"
(component
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $debug
    (canon error-context.debug-message (memory $libc "memory"))
  )
)
"#,
    )
    .expect_err("debug-message without realloc must fail");

    assert!(
        error
            .to_string()
            .contains("canonical option `realloc` is required"),
        "unexpected error: {error}"
    );
}

#[test]
fn waitable_set_new_and_drop_decode_and_validate() {
    compile_component(
        r#"
(component
  (core func $new
    (canon waitable-set.new)
  )
  (core func $drop
    (canon waitable-set.drop)
  )
)
"#,
    )
    .expect("waitable-set new/drop should decode and validate");
}

#[cfg(feature = "component-gated-feature-async")]
#[test]
fn future_stream_new_and_drop_decode_and_validate() {
    compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (type $stream-u8 (stream u8))
  (core func $future-new
    (canon future.new $future-u32)
  )
  (core func $future-drop-readable
    (canon future.drop-readable $future-u32)
  )
  (core func $future-drop-writable
    (canon future.drop-writable $future-u32)
  )
  (core func $stream-new
    (canon stream.new $stream-u8)
  )
  (core func $stream-drop-readable
    (canon stream.drop-readable $stream-u8)
  )
  (core func $stream-drop-writable
    (canon stream.drop-writable $stream-u8)
  )
)
"#,
    )
    .expect("stream/future new/drop should decode and validate");
}

#[cfg(feature = "component-gated-feature-async")]
#[test]
fn future_read_write_decode_and_validate() {
    compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (type $stream-u32 (stream u32))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $stream-read
    (canon stream.read $stream-u32 (memory $libc "memory"))
  )
  (core func $stream-write
    (canon stream.write $stream-u32 (memory $libc "memory"))
  )
  (core func $stream-cancel-read
    (canon stream.cancel-read $stream-u32 async)
  )
  (core func $stream-cancel-write
    (canon stream.cancel-write $stream-u32 async)
  )
  (core func $future-read
    (canon future.read $future-u32 (memory $libc "memory"))
  )
  (core func $future-write
    (canon future.write $future-u32 (memory $libc "memory"))
  )
  (core func $future-cancel-read
    (canon future.cancel-read $future-u32 async)
  )
  (core func $future-cancel-write
    (canon future.cancel-write $future-u32 async)
  )
)
"#,
    )
    .expect("stream/future read/write/cancel should decode and validate");
}

#[test]
fn task_cancel_decode_and_validate() {
    compile_component(
        r#"
(component
  (core func $task-cancel
    (canon task.cancel)
  )
)
"#,
    )
    .expect("task.cancel should decode and validate");
}

#[test]
fn subtask_cancel_and_drop_decode_and_validate() {
    compile_component(
        r#"
(component
  (core func $subtask-cancel
    (canon subtask.cancel async)
  )
  (core func $subtask-drop
    (canon subtask.drop)
  )
)
"#,
    )
    .expect("subtask canonical built-ins should decode and validate");
}

#[test]
fn waitable_set_wait_poll_join_decode_and_validate() {
    compile_component(
        r#"
(component
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $wait
    (canon waitable-set.wait cancellable (memory $libc "memory"))
  )
  (core func $poll
    (canon waitable-set.poll cancellable (memory $libc "memory"))
  )
  (core func $join
    (canon waitable.join)
  )
)
"#,
    )
    .expect("waitable-set wait/poll and waitable.join should decode and validate");
}

#[cfg(not(feature = "component-gated-feature-async"))]
#[test]
fn async_canonical_option_requires_async_feature() {
    let error = compile_component(
        r#"
(component
  (type $host-func (func))
  (import "host" (func $host (type $host-func)))
  (core func $lowered
    (canon lower (func $host) async)
  )
)
"#,
    )
    .expect_err("async canonical ABI must be feature-gated");
    assert!(
        error.to_string().contains(
            "canonical option `async` requires the component-gated-feature-async feature"
        ),
        "unexpected error: {error}"
    );
}

#[cfg(feature = "component-gated-feature-async")]
#[test]
fn async_component_value_types_decode_and_validate() {
    compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (type $stream-string (stream string))
  (type $host-func
    (func
      (param "future" $future-u32)
      (param "stream" $stream-string)
      (param "context" error-context)
      (result $future-u32)))
  (import "[async]host-future" (func $host (type $host-func)))
)
"#,
    )
    .expect("async gated value types should decode and validate");

    compile_component(
        r#"
(component
  (type $future-string (future string))
  (import "host" (func $host (result $future-string)))
  (core func $lowered (canon lower (func $host)))
)
"#,
    )
    .expect("future handles should not require canonical memory or realloc");
}

#[cfg(feature = "component-gated-feature-async")]
#[test]
fn async_canonical_options_decode_validate_and_shape_core_signatures() {
    compile_component(
        r#"
(component
  (type $host-func (func (param "value" u32) (result u32)))
  (import "host" (func $host (type $host-func)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $lowered
    (canon lower (func $host) async (memory $libc "memory"))
  )
  (core module $caller
    (import "" "host" (func $host (param i32 i32) (result i32)))
  )
)
"#,
    )
    .expect("async lower should decode and validate the async ABI signature");

    compile_component(
        r#"
(component
  (type $guest-func (func (param "value" u32) (result u32)))
  (core module $guest
    (func (export "run") (param i32))
  )
  (core instance $guest (instantiate $guest))
  (func $lifted (type $guest-func)
    (canon lift (core func $guest "run") async)
  )
)
"#,
    )
    .expect("async lift should decode and validate the stackful async ABI signature");

    compile_component(
        r#"
(component
  (type $guest-func (func (param "value" u32) (result u32)))
  (core module $guest
    (func (export "run") (param i32) (result i32)
      i32.const 0
    )
    (func (export "callback") (param i32 i32 i32) (result i32)
      i32.const 0)
  )
  (core instance $guest (instantiate $guest))
  (func $lifted (type $guest-func)
    (canon lift (core func $guest "run") async (callback (func $guest "callback")))
  )
)
"#,
    )
    .expect("async lift should decode callback and validate the stackless async ABI signature");
}

#[cfg(feature = "component-gated-feature-async")]
#[test]
fn async_canonical_options_fail_closed_for_invalid_shapes() {
    let error = compile_component(
        r#"
(component
  (type $host-func (func))
  (import "host" (func $host (type $host-func)))
  (core module $callbacks
    (func (export "callback") (param i64))
  )
  (core instance $callbacks (instantiate $callbacks))
  (core func $lowered
    (canon lower (func $host) async (callback (func $callbacks "callback")))
  )
)
"#,
    )
    .expect_err("callback on lowerings must fail closed");
    assert!(
        error.to_string().contains(
            "canonical option `callback` uses a core function with an incorrect signature"
        ) || error
            .to_string()
            .contains("canonical option `callback` cannot be specified for lowerings"),
        "unexpected error: {error}"
    );

    let error = compile_component(
        r#"
(component
  (core type $module (module))
  (type $host-func (func (param "value" u32) (result u32)))
  (import "host" (func $host (type $host-func)))
  (core func $lowered
    (canon lower (func $host) gc (core-type (type $module)))
  )
)
"#,
    )
    .expect_err("core-type option must reference a function core type");
    assert!(
        error
            .to_string()
            .contains("canonical option `core type` must reference a core function type"),
        "unexpected error: {error}"
    );
}
