mod common;

use common::instantiate_wat;
use std::cell::Cell;
use std::rc::Rc;
use std::task::Poll;
use telomere::Registry;
use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

#[tokio::test]
async fn component_runtime_calls_async_import_as_lifted_export() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "host-add" (func (type 0)))
  (export "add" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    linker.register_import_async("host-add", |_store, args| {
        Box::pin(async move {
            if args.len() != 2 {
                return Err(ComponentError::InvalidArgument(
                    "host-add expects exactly 2 arguments".to_owned(),
                ));
            }
            let lhs = args[0]
                .as_i32()
                .ok_or_else(|| ComponentError::InvalidArgument("arg[0] must be i32".to_owned()))?;
            let rhs = args[1]
                .as_i32()
                .ok_or_else(|| ComponentError::InvalidArgument("arg[1] must be i32".to_owned()))?;
            Ok(vec![ComponentValue::I32(lhs + rhs)])
        })
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_resumes_host_future_after_pending() {
    let bytes = compile_component(
        r#"
(component
  (type (func (result s32)))
  (import "host-value" (func (type 0)))
  (export "value" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let polled_pending = Rc::new(Cell::new(false));

    let mut linker = ComponentLinker::new();
    linker.register_import_async("host-value", {
        let polled_pending = Rc::clone(&polled_pending);
        move |_store, _args| {
            let polled_pending = Rc::clone(&polled_pending);
            Box::pin(futures::future::poll_fn(move |cx| {
                if !polled_pending.get() {
                    polled_pending.set(true);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(vec![ComponentValue::I32(7)]))
                }
            }))
        }
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "value", &[])
        .await
        .expect("pending host future should resume");

    assert!(polled_pending.get(), "test future must yield Pending once");
    assert_eq!(result, vec![ComponentValue::I32(7)]);
}

#[tokio::test]
async fn component_runtime_resolves_semver_compatible_function_imports() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" u32) (result u32)))
  (import "host:math/add@0.2.0" (func $host-add (type 0)))
  (export "add-one" (func $host-add))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host:math/add@0.2.6", |_store, (value,): (u32,)| {
        Ok(value + 1)
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("semver-compatible function import should instantiate");
    let result = instance
        .call(&store, "add-one", &[ComponentValue::U32(41)])
        .await
        .expect("semver-compatible import should call");

    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn async_canonical_lower_resumes_pending_host_import() {
    let bytes = compile_component(
        r#"
	(component
	  (type (func (param "value" u32) (result u32)))
	  (import "host" (func $host (type 0)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
	  (core func $host-lower
	    (canon lower (func $host) async (memory $libc "memory"))
	  )
	  (core module $caller
	    (import "" "host" (func $host (param i32 i32) (result i32)))
	    (import "env" "memory" (memory 1))
	    (func (export "run") (param $value i32) (result i32)
	      (local $state i32)
	      (local.set $state (call $host (local.get $value) (i32.const 16)))
	      (if (result i32)
	        (i32.eq (local.get $state) (i32.const 2))
	        (then (i32.load (i32.const 16)))
	        (else (i32.const -1))))
	  )
	  (core instance $caller
	    (instantiate $caller
	      (with "" (instance
	        (export "host" (func $host-lower))
	      ))
	      (with "env" (instance
	        (export "memory" (memory $libc "memory"))
	      ))
	    )
	  )
	  (func (export "run") (param "value" u32) (result u32)
	    (canon lift (core func $caller "run") (memory $libc "memory"))
	  )
	)
	"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let polled_pending = Rc::new(Cell::new(false));

    let mut linker = ComponentLinker::new();
    linker.register_import_async("host", {
        let polled_pending = Rc::clone(&polled_pending);
        move |_store, args| {
            let polled_pending = Rc::clone(&polled_pending);
            let value = args[0].as_u32().expect("host value must be u32");
            Box::pin(futures::future::poll_fn(move |cx| {
                if !polled_pending.get() {
                    polled_pending.set(true);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(vec![ComponentValue::U32(value + 1)]))
                }
            }))
        }
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    let result = instance
        .call(&store, "run", &[ComponentValue::U32(41)])
        .await
        .expect("async canonical lower should resume pending host import");

    assert!(polled_pending.get(), "host future must yield Pending once");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn async_canonical_lift_uses_task_return_after_pending_lower() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "value" u32) (result u32)))
  (import "host" (func $host (type 0)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $host-lower
    (canon lower (func $host) async (memory $libc "memory"))
  )
  (core func $task-return
    (canon task.return (result u32) (memory $libc "memory"))
  )
  (core module $guest
    (import "" "host" (func $host (param i32 i32) (result i32)))
    (import "" "task-return" (func $task-return (param i32)))
    (import "env" "memory" (memory 1))
    (func (export "run") (param $value i32)
      (local $state i32)
      (local.set $state (call $host (local.get $value) (i32.const 16)))
      (if (i32.eq (local.get $state) (i32.const 2))
        (then
          (call $task-return (i32.load (i32.const 16))))))
  )
  (core instance $guest
    (instantiate $guest
      (with "" (instance
        (export "host" (func $host-lower))
        (export "task-return" (func $task-return))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "run") (param "value" u32) (result u32)
    (canon lift (core func $guest "run") async (memory $libc "memory"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let polled_pending = Rc::new(Cell::new(false));

    let mut linker = ComponentLinker::new();
    linker.register_import_async("host", {
        let polled_pending = Rc::clone(&polled_pending);
        move |_store, args| {
            let polled_pending = Rc::clone(&polled_pending);
            let value = args[0].as_u32().expect("host value must be u32");
            Box::pin(futures::future::poll_fn(move |cx| {
                if !polled_pending.get() {
                    polled_pending.set(true);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(vec![ComponentValue::U32(value + 1)]))
                }
            }))
        }
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    let result = instance
        .call(&store, "run", &[ComponentValue::U32(41)])
        .await
        .expect("async canonical lift should use task.return after pending lower");

    assert!(polled_pending.get(), "host future must yield Pending once");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn async_canonical_lift_runs_stackless_callback_until_exit() {
    let bytes = compile_component(
        r#"
(component
  (type (func (result u32)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $task-return
    (canon task.return (result u32) (memory $libc "memory"))
  )
  (core module $guest
    (import "" "task-return" (func $task-return (param i32)))
    (global $callbacks (mut i32) (i32.const 0))
    (func (export "run") (result i32)
      (i32.const 1)
    )
    (func (export "callback") (param $event i32) (param $index i32) (param $payload i32) (result i32)
      (if (i32.ne (local.get $event) (i32.const 0))
        (then unreachable))
      (if (i32.ne (global.get $callbacks) (i32.const 0))
        (then unreachable))
      (global.set $callbacks (i32.const 1))
      (call $task-return (i32.const 44))
      (i32.const 0)
    )
  )
  (core instance $guest
    (instantiate $guest
      (with "" (instance
        (export "task-return" (func $task-return))
      ))
    )
  )
  (func (export "run") (result u32)
    (canon lift
      (core func $guest "run")
      async
      (memory $libc "memory")
      (callback (func $guest "callback")))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("stackless async callback should run until EXIT");

    assert_eq!(result, vec![ComponentValue::U32(44)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn async_canonical_callback_wait_receives_ready_future_event() {
    let bytes = compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (type (func (result u32)))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $future-new
    (canon future.new $future-u32)
  )
  (core func $future-read
    (canon future.read $future-u32 async (memory $libc "memory"))
  )
  (core func $future-write
    (canon future.write $future-u32 (memory $libc "memory"))
  )
  (core func $waitable-set-new
    (canon waitable-set.new)
  )
  (core func $waitable-join
    (canon waitable.join)
  )
  (core func $task-return
    (canon task.return (result u32) (memory $libc "memory"))
  )
  (core module $guest
    (import "" "future-new" (func $future-new (result i64)))
    (import "" "future-read" (func $future-read (param i32 i32) (result i32)))
    (import "" "future-write" (func $future-write (param i32 i32) (result i32)))
    (import "" "waitable-set-new" (func $waitable-set-new (result i32)))
    (import "" "waitable-join" (func $waitable-join (param i32 i32)))
    (import "" "task-return" (func $task-return (param i32)))
    (import "env" "memory" (memory 1))
    (func (export "run") (result i32)
      (local $future i64)
      (local $readable i32)
      (local $writable i32)
      (local $set i32)
      (local.set $future (call $future-new))
      (local.set $readable (i32.wrap_i64 (local.get $future)))
      (local.set $writable
        (i32.wrap_i64 (i64.shr_u (local.get $future) (i64.const 32))))
      (local.set $set (call $waitable-set-new))
      (call $waitable-join (local.get $readable) (local.get $set))
      (if (i32.ne
          (call $future-read (local.get $readable) (i32.const 20))
          (i32.const -1))
        (then unreachable))
      (i32.store (i32.const 16) (i32.const 55))
      (if (i32.ne
          (call $future-write (local.get $writable) (i32.const 16))
          (i32.const 0))
        (then unreachable))
      (i32.or (i32.const 2) (i32.shl (local.get $set) (i32.const 4)))
    )
    (func (export "callback") (param $event i32) (param $index i32) (param $payload i32) (result i32)
      (if (i32.ne (local.get $event) (i32.const 4))
        (then unreachable))
      (if (i32.ne (local.get $payload) (i32.const 0))
        (then unreachable))
      (call $task-return (i32.load (i32.const 20)))
      (i32.const 0)
    )
  )
  (core instance $guest
    (instantiate $guest
      (with "" (instance
        (export "future-new" (func $future-new))
        (export "future-read" (func $future-read))
        (export "future-write" (func $future-write))
        (export "waitable-set-new" (func $waitable-set-new))
        (export "waitable-join" (func $waitable-join))
        (export "task-return" (func $task-return))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "run") (result u32)
    (canon lift
      (core func $guest "run")
      async
      (memory $libc "memory")
      (callback (func $guest "callback")))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("stackless async callback should receive a ready future event");

    assert_eq!(result, vec![ComponentValue::U32(55)]);
}

#[tokio::test]
async fn component_runtime_executes_error_context_canonical_builtins() {
    let bytes = compile_component(
        r#"
(component
  (core module $libc
    (memory (export "memory") 1)
    (data (i32.const 8) "hello")
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
  (core func $ec-new
    (canon error-context.new (memory $libc "memory"))
  )
  (core func $ec-debug
    (canon error-context.debug-message
      (memory $libc "memory")
      (realloc (func $libc "realloc")))
  )
  (core func $ec-drop
    (canon error-context.drop)
  )
  (core module $caller
    (import "env" "memory" (memory 1))
    (import "" "ec-new" (func $ec-new (param i32 i32) (result i32)))
    (import "" "ec-debug" (func $ec-debug (param i32 i32)))
    (import "" "ec-drop" (func $ec-drop (param i32)))
    (func (export "roundtrip") (result i32)
      (local $handle i32)
      (local.set $handle (call $ec-new (i32.const 8) (i32.const 5)))
      (call $ec-debug (local.get $handle) (i32.const 0))
      (call $ec-drop (local.get $handle))
      (i32.const 0)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "ec-new" (func $ec-new))
        (export "ec-debug" (func $ec-debug))
        (export "ec-drop" (func $ec-drop))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "roundtrip") (result string)
    (canon lift (core func $caller "roundtrip") (memory $libc "memory"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("error-context built-ins should execute");

    assert_eq!(result, vec![ComponentValue::String("hello".to_owned())]);
}

#[tokio::test]
async fn component_runtime_executes_waitable_set_new_and_drop() {
    let bytes = compile_component(
        r#"
(component
  (core func $wset-new
    (canon waitable-set.new)
  )
  (core func $wset-drop
    (canon waitable-set.drop)
  )
  (core module $caller
    (import "" "wset-new" (func $wset-new (result i32)))
    (import "" "wset-drop" (func $wset-drop (param i32)))
    (func (export "roundtrip") (result i32)
      (local $handle i32)
      (local.set $handle (call $wset-new))
      (call $wset-drop (local.get $handle))
      (i32.const 7)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "wset-new" (func $wset-new))
        (export "wset-drop" (func $wset-drop))
      ))
    )
  )
  (func (export "roundtrip") (result u32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("waitable-set new/drop should execute");

    assert_eq!(result, vec![ComponentValue::U32(7)]);
}

#[tokio::test]
async fn component_runtime_executes_task_cancel_acknowledgement() {
    let bytes = compile_component(
        r#"
(component
  (core func $task-cancel
    (canon task.cancel)
  )
  (core module $caller
    (import "" "task-cancel" (func $task-cancel))
    (func (export "roundtrip") (result i32)
      (call $task-cancel)
      (i32.const 11)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "task-cancel" (func $task-cancel))
      ))
    )
  )
  (func (export "roundtrip") (result u32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("task.cancel should execute as a local acknowledgement");

    assert_eq!(result, vec![ComponentValue::U32(11)]);
}

#[tokio::test]
async fn component_runtime_fails_closed_for_subtask_builtins_without_subtask_handles() {
    let bytes = compile_component(
        r#"
(component
  (core func $subtask-cancel
    (canon subtask.cancel)
  )
  (core func $subtask-drop
    (canon subtask.drop)
  )
  (core module $caller
    (import "" "subtask-cancel" (func $subtask-cancel (param i32) (result i32)))
    (import "" "subtask-drop" (func $subtask-drop (param i32)))
    (func (export "cancel") (result i32)
      (call $subtask-cancel (i32.const 1))
    )
    (func (export "drop")
      (call $subtask-drop (i32.const 1))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "subtask-cancel" (func $subtask-cancel))
        (export "subtask-drop" (func $subtask-drop))
      ))
    )
  )
  (func (export "cancel") (result s32)
    (canon lift (core func $caller "cancel"))
  )
  (func (export "drop")
    (canon lift (core func $caller "drop"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let cancel_error = instance
        .call(&store, "cancel", &[])
        .await
        .expect_err("subtask.cancel should fail closed without subtask handles");
    assert!(
        matches!(cancel_error, ComponentError::Trap(message) if message.contains("unreachable"))
    );

    let drop_error = instance
        .call(&store, "drop", &[])
        .await
        .expect_err("subtask.drop should fail closed without subtask handles");
    assert!(matches!(drop_error, ComponentError::Trap(message) if message.contains("unreachable")));
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn waitable_set_poll_receives_future_read_completion() {
    let bytes = compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $future-new
    (canon future.new $future-u32)
  )
  (core func $future-read
    (canon future.read $future-u32 async (memory $libc "memory"))
  )
  (core func $future-write
    (canon future.write $future-u32 (memory $libc "memory"))
  )
  (core func $wset-new
    (canon waitable-set.new)
  )
  (core func $wset-poll
    (canon waitable-set.poll cancellable (memory $libc "memory"))
  )
  (core func $waitable-join
    (canon waitable.join)
  )
  (core module $caller
    (import "" "future-new" (func $future-new (result i64)))
    (import "" "future-read" (func $future-read (param i32 i32) (result i32)))
    (import "" "future-write" (func $future-write (param i32 i32) (result i32)))
    (import "" "wset-new" (func $wset-new (result i32)))
    (import "" "wset-poll" (func $wset-poll (param i32 i32) (result i32)))
    (import "" "waitable-join" (func $waitable-join (param i32 i32)))
    (import "env" "memory" (memory 1))
    (func (export "roundtrip") (result i32)
      (local $future i64)
      (local $readable i32)
      (local $writable i32)
      (local $set i32)
      (local.set $future (call $future-new))
      (local.set $readable (i32.wrap_i64 (local.get $future)))
      (local.set $writable
        (i32.wrap_i64 (i64.shr_u (local.get $future) (i64.const 32))))
      (local.set $set (call $wset-new))
      (call $waitable-join (local.get $readable) (local.get $set))
      (if (i32.ne
          (call $future-read (local.get $readable) (i32.const 20))
          (i32.const -1))
        (then unreachable))
      (i32.store (i32.const 16) (i32.const 42))
      (if (i32.ne
          (call $future-write (local.get $writable) (i32.const 16))
          (i32.const 0))
        (then unreachable))
      (if (i32.ne
          (call $wset-poll (local.get $set) (i32.const 32))
          (i32.const 4))
        (then unreachable))
      (if (i32.ne (i32.load (i32.const 32)) (local.get $readable))
        (then unreachable))
      (if (i32.ne (i32.load (i32.const 36)) (i32.const 0))
        (then unreachable))
      (i32.load (i32.const 20))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "future-new" (func $future-new))
        (export "future-read" (func $future-read))
        (export "future-write" (func $future-write))
        (export "wset-new" (func $wset-new))
        (export "wset-poll" (func $wset-poll))
        (export "waitable-join" (func $waitable-join))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "roundtrip") (result s32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("waitable-set.poll should receive future.read completion");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn future_cancel_read_clears_pending_read_without_losing_payload() {
    let bytes = compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $future-new
    (canon future.new $future-u32)
  )
  (core func $future-read
    (canon future.read $future-u32 async (memory $libc "memory"))
  )
  (core func $future-write
    (canon future.write $future-u32 (memory $libc "memory"))
  )
  (core func $future-cancel-read
    (canon future.cancel-read $future-u32)
  )
  (core module $caller
    (import "" "future-new" (func $future-new (result i64)))
    (import "" "future-read" (func $future-read (param i32 i32) (result i32)))
    (import "" "future-write" (func $future-write (param i32 i32) (result i32)))
    (import "" "future-cancel-read" (func $future-cancel-read (param i32) (result i32)))
    (import "env" "memory" (memory 1))
    (func (export "roundtrip") (result i32)
      (local $future i64)
      (local $readable i32)
      (local $writable i32)
      (local.set $future (call $future-new))
      (local.set $readable (i32.wrap_i64 (local.get $future)))
      (local.set $writable
        (i32.wrap_i64 (i64.shr_u (local.get $future) (i64.const 32))))
      (if (i32.ne
          (call $future-read (local.get $readable) (i32.const 20))
          (i32.const -1))
        (then unreachable))
      (if (i32.ne
          (call $future-cancel-read (local.get $readable))
          (i32.const 0))
        (then unreachable))
      (i32.store (i32.const 16) (i32.const 42))
      (if (i32.ne
          (call $future-write (local.get $writable) (i32.const 16))
          (i32.const 0))
        (then unreachable))
      (if (i32.ne
          (call $future-read (local.get $readable) (i32.const 24))
          (i32.const 0))
        (then unreachable))
      (i32.add (i32.load (i32.const 20)) (i32.load (i32.const 24)))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "future-new" (func $future-new))
        (export "future-read" (func $future-read))
        (export "future-write" (func $future-write))
        (export "future-cancel-read" (func $future-cancel-read))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "roundtrip") (result s32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("future.cancel-read should clear pending read");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn stream_cancel_read_clears_pending_read_and_keeps_queued_payload() {
    let bytes = compile_component(
        r#"
(component
  (type $stream-u32 (stream u32))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $stream-new
    (canon stream.new $stream-u32)
  )
  (core func $stream-read
    (canon stream.read $stream-u32 async (memory $libc "memory"))
  )
  (core func $stream-write
    (canon stream.write $stream-u32 (memory $libc "memory"))
  )
  (core func $stream-cancel-read
    (canon stream.cancel-read $stream-u32)
  )
  (core module $caller
    (import "" "stream-new" (func $stream-new (result i64)))
    (import "" "stream-read" (func $stream-read (param i32 i32 i32) (result i32)))
    (import "" "stream-write" (func $stream-write (param i32 i32 i32) (result i32)))
    (import "" "stream-cancel-read" (func $stream-cancel-read (param i32) (result i32)))
    (import "env" "memory" (memory 1))
    (func (export "roundtrip") (result i32)
      (local $stream i64)
      (local $readable i32)
      (local $writable i32)
      (local.set $stream (call $stream-new))
      (local.set $readable (i32.wrap_i64 (local.get $stream)))
      (local.set $writable
        (i32.wrap_i64 (i64.shr_u (local.get $stream) (i64.const 32))))
      (if (i32.ne
          (call $stream-read (local.get $readable) (i32.const 20) (i32.const 1))
          (i32.const -1))
        (then unreachable))
      (if (i32.ne
          (call $stream-cancel-read (local.get $readable))
          (i32.const 0))
        (then unreachable))
      (i32.store (i32.const 16) (i32.const 42))
      (if (i32.ne
          (call $stream-write (local.get $writable) (i32.const 16) (i32.const 1))
          (i32.const 16))
        (then unreachable))
      (if (i32.ne
          (call $stream-read (local.get $readable) (i32.const 24) (i32.const 1))
          (i32.const 16))
        (then unreachable))
      (i32.add (i32.load (i32.const 20)) (i32.load (i32.const 24)))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "stream-new" (func $stream-new))
        (export "stream-read" (func $stream-read))
        (export "stream-write" (func $stream-write))
        (export "stream-cancel-read" (func $stream-cancel-read))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "roundtrip") (result s32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("stream.cancel-read should clear pending read");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn component_runtime_executes_future_stream_new_and_drop() {
    let bytes = compile_component(
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
  (core module $caller
    (import "" "future-new" (func $future-new (result i64)))
    (import "" "future-drop-readable" (func $future-drop-readable (param i32)))
    (import "" "future-drop-writable" (func $future-drop-writable (param i32)))
    (import "" "stream-new" (func $stream-new (result i64)))
    (import "" "stream-drop-readable" (func $stream-drop-readable (param i32)))
    (import "" "stream-drop-writable" (func $stream-drop-writable (param i32)))
    (func (export "roundtrip") (result i32)
      (local $future i64)
      (local $stream i64)
      (local.set $future (call $future-new))
      (call $future-drop-readable (i32.wrap_i64 (local.get $future)))
      (call $future-drop-writable
        (i32.wrap_i64 (i64.shr_u (local.get $future) (i64.const 32))))
      (local.set $stream (call $stream-new))
      (call $stream-drop-readable (i32.wrap_i64 (local.get $stream)))
      (call $stream-drop-writable
        (i32.wrap_i64 (i64.shr_u (local.get $stream) (i64.const 32))))
      (i32.const 9)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "future-new" (func $future-new))
        (export "future-drop-readable" (func $future-drop-readable))
        (export "future-drop-writable" (func $future-drop-writable))
        (export "stream-new" (func $stream-new))
        (export "stream-drop-readable" (func $stream-drop-readable))
        (export "stream-drop-writable" (func $stream-drop-writable))
      ))
    )
  )
  (func (export "roundtrip") (result s32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("stream/future new/drop should execute");

    assert_eq!(result, vec![ComponentValue::S32(9)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn component_runtime_executes_future_read_write_payload() {
    let bytes = compile_component(
        r#"
(component
  (type $future-u32 (future u32))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $future-new
    (canon future.new $future-u32)
  )
  (core func $future-read
    (canon future.read $future-u32 (memory $libc "memory"))
  )
  (core func $future-write
    (canon future.write $future-u32 (memory $libc "memory"))
  )
  (core func $future-drop-readable
    (canon future.drop-readable $future-u32)
  )
  (core func $future-drop-writable
    (canon future.drop-writable $future-u32)
  )
  (core module $caller
    (import "" "future-new" (func $future-new (result i64)))
    (import "" "future-read" (func $future-read (param i32 i32) (result i32)))
    (import "" "future-write" (func $future-write (param i32 i32) (result i32)))
    (import "" "future-drop-readable" (func $future-drop-readable (param i32)))
    (import "" "future-drop-writable" (func $future-drop-writable (param i32)))
    (import "env" "memory" (memory 1))
    (func (export "roundtrip") (result i32)
      (local $future i64)
      (local $readable i32)
      (local $writable i32)
      (local.set $future (call $future-new))
      (local.set $readable (i32.wrap_i64 (local.get $future)))
      (local.set $writable
        (i32.wrap_i64 (i64.shr_u (local.get $future) (i64.const 32))))
      (i32.store (i32.const 16) (i32.const 42))
      (if (i32.ne
          (call $future-write (local.get $writable) (i32.const 16))
          (i32.const 0))
        (then unreachable))
      (if (i32.ne
          (call $future-read (local.get $readable) (i32.const 20))
          (i32.const 0))
        (then unreachable))
      (call $future-drop-readable (local.get $readable))
      (call $future-drop-writable (local.get $writable))
      (i32.load (i32.const 20))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "future-new" (func $future-new))
        (export "future-read" (func $future-read))
        (export "future-write" (func $future-write))
        (export "future-drop-readable" (func $future-drop-readable))
        (export "future-drop-writable" (func $future-drop-writable))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "roundtrip") (result s32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("future.read/write should execute");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[cfg(feature = "component-gated-feature-async")]
#[tokio::test]
async fn component_runtime_executes_stream_read_write_payloads() {
    let bytes = compile_component(
        r#"
(component
  (type $stream-u32 (stream u32))
  (core module $libc
    (memory (export "memory") 1)
  )
  (core instance $libc (instantiate $libc))
  (core func $stream-new
    (canon stream.new $stream-u32)
  )
  (core func $stream-read
    (canon stream.read $stream-u32 (memory $libc "memory"))
  )
  (core func $stream-write
    (canon stream.write $stream-u32 (memory $libc "memory"))
  )
  (core func $stream-drop-readable
    (canon stream.drop-readable $stream-u32)
  )
  (core func $stream-drop-writable
    (canon stream.drop-writable $stream-u32)
  )
  (core module $caller
    (import "" "stream-new" (func $stream-new (result i64)))
    (import "" "stream-read" (func $stream-read (param i32 i32 i32) (result i32)))
    (import "" "stream-write" (func $stream-write (param i32 i32 i32) (result i32)))
    (import "" "stream-drop-readable" (func $stream-drop-readable (param i32)))
    (import "" "stream-drop-writable" (func $stream-drop-writable (param i32)))
    (import "env" "memory" (memory 1))
    (func (export "roundtrip") (result i32)
      (local $stream i64)
      (local $readable i32)
      (local $writable i32)
      (local.set $stream (call $stream-new))
      (local.set $readable (i32.wrap_i64 (local.get $stream)))
      (local.set $writable
        (i32.wrap_i64 (i64.shr_u (local.get $stream) (i64.const 32))))
      (i32.store (i32.const 16) (i32.const 10))
      (i32.store (i32.const 20) (i32.const 32))
      (if (i32.ne
          (call $stream-write (local.get $writable) (i32.const 16) (i32.const 2))
          (i32.const 32))
        (then unreachable))
      (if (i32.ne
          (call $stream-read (local.get $readable) (i32.const 32) (i32.const 2))
          (i32.const 32))
        (then unreachable))
      (call $stream-drop-readable (local.get $readable))
      (call $stream-drop-writable (local.get $writable))
      (i32.add (i32.load (i32.const 32)) (i32.load (i32.const 36)))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "stream-new" (func $stream-new))
        (export "stream-read" (func $stream-read))
        (export "stream-write" (func $stream-write))
        (export "stream-drop-readable" (func $stream-drop-readable))
        (export "stream-drop-writable" (func $stream-drop-writable))
      ))
      (with "env" (instance
        (export "memory" (memory $libc "memory"))
      ))
    )
  )
  (func (export "roundtrip") (result s32)
    (canon lift (core func $caller "roundtrip"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "roundtrip", &[])
        .await
        .expect("stream.read/write should execute");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[tokio::test]
async fn component_runtime_calls_registered_core_export() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "core-add" (func (type 0)))
  (export "add" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let registry = Registry::new();
    let core = instantiate_wat(
        r#"
    (module
      (func (export "core_add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
    "#,
        &store,
        &registry,
    )
    .await;

    let mut linker = ComponentLinker::new();
    linker.register_export_core("add", core, "core_add");

    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("core call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_can_lower_registered_core_imports_back_into_core() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "core-add" (func $core-add (type 0)))
  (core func $core-add-lower
    (canon lower (func $core-add))
  )
  (core module $caller
    (import "" "core-add" (func $core-add (param i32 i32) (result i32)))
    (func (export "call-add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      call $core-add)
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "core-add" (func $core-add-lower))
      ))
    )
  )
  (func (export "call-add") (param "lhs" s32) (param "rhs" s32) (result s32)
    (canon lift (core func $caller "call-add"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let registry = Registry::new();
    let core = instantiate_wat(
        r#"
    (module
      (func (export "core_add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
    "#,
        &store,
        &registry,
    )
    .await;

    let mut linker = ComponentLinker::new();
    linker.register_import_core("core-add", core, "core_add");

    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "call-add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[tokio::test]
async fn component_runtime_nested_component_instantiation_succeeds() {
    let bytes = compile_component(
        r#"
(component
  (type
    (component
      (type (func (result u32)))
      (import "x" (func (type 0)))
    )
  )
  (import "b" (component (type 0)))
  (component
    (type
      (component
        (type (func (result u32)))
        (import "x" (func (type 0)))
      )
    )
    (import "a" (component (type 0)))
  )
  (instance (instantiate 1 (with "a" (component 0))))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();

    let _instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
}

#[tokio::test]
async fn component_runtime_inline_instance_export_instantiation_succeeds() {
    let bytes = compile_component(
        r#"
(component
  (component)
  (instance (instantiate 0))
  (instance (export "b" (instance 0)))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();

    let _instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
}

#[tokio::test]
async fn component_runtime_sync_wrapper_registration_still_works() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "host-add" (func (type 0)))
  (export "add" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    linker.register_export("add", |_store, args| {
        if args.len() != 2 {
            return Err(ComponentError::InvalidArgument(
                "add expects exactly 2 arguments".to_owned(),
            ));
        }
        let lhs = args[0]
            .as_i32()
            .ok_or_else(|| ComponentError::InvalidArgument("arg[0] must be i32".to_owned()))?;
        let rhs = args[1]
            .as_i32()
            .ok_or_else(|| ComponentError::InvalidArgument("arg[1] must be i32".to_owned()))?;
        Ok(vec![ComponentValue::I32(lhs + rhs)])
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(1), ComponentValue::I32(2)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(3)]);
}

#[tokio::test]
async fn component_runtime_canon_lower_writes_indirect_results_into_caller_area() {
    let bytes = compile_component(
        r#"
(component
  (import "host" (func $host (result string)))
  (core module $libc
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 8))
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
  (core func $host-lower
    (canon lower (func $host) (memory $libc "memory") (realloc (func $libc "realloc")))
  )
  (core module $caller
    (import "" "host" (func $host (param i32)))
    (func (export "call-host") (result i32)
      (call $host (i32.const 0))
      (i32.const 0)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "host" (func $host-lower))
      ))
    )
  )
  (func (export "call-host") (result string)
    (canon lift (core func $caller "call-host") (memory $libc "memory"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert!(
            args.is_empty(),
            "host should not receive component arguments"
        );
        Ok(vec![ComponentValue::String("hello".to_owned())])
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "call-host", &[])
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::String("hello".to_owned())]);
}

#[tokio::test]
async fn component_runtime_canon_lower_reads_packed_u8_lists_from_guest_memory() {
    let bytes = compile_component(
        r#"
(component
  (import "host" (func $host (param "bytes" (list u8))))
  (core module $guest
    (memory (export "memory") 1)
    (data (i32.const 8) "Hello, world!\n")
  )
  (core instance $guest (instantiate $guest))
  (core func $host-lower
    (canon lower (func $host) (memory $guest "memory"))
  )
  (core module $caller
    (import "" "host" (func $host (param i32 i32)))
    (func (export "call-host")
      (call $host (i32.const 8) (i32.const 14))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "host" (func $host-lower))
      ))
    )
  )
  (func (export "call-host")
    (canon lift (core func $caller "call-host") (memory $guest "memory"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert_eq!(
            args,
            &[ComponentValue::List(
                b"Hello, world!\n"
                    .iter()
                    .copied()
                    .map(ComponentValue::U8)
                    .collect()
            )]
        );
        Ok(Vec::new())
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    instance
        .call(&store, "call-host", &[])
        .await
        .expect("call should succeed");
}

#[tokio::test]
async fn component_runtime_fixup_populates_shim_table_for_host_lowerings() {
    let bytes = compile_component(
        r#"
(component
  (import "host" (func $host (param "value" u32) (result u32)))
  (core module $shim
    (type $thunk (func (param i32) (result i32)))
    (table (export "imports") 1 1 funcref)
    (func (export "wrapper") (param i32) (result i32)
      (local.get 0)
      (i32.const 0)
      (call_indirect (type $thunk))
    )
  )
  (core instance $shim (instantiate $shim))
  (core func $host-lower
    (canon lower (func $host))
  )
  (core instance $fixup-args
    (export "imports" (table $shim "imports"))
    (export "host" (func $host-lower))
  )
  (core module $fixup
    (type $thunk (func (param i32) (result i32)))
    (import "" "host" (func $host (type $thunk)))
    (import "" "imports" (table 1 1 funcref))
    (elem (i32.const 0) func $host)
  )
  (core instance $fixup
    (instantiate $fixup
      (with "" (instance $fixup-args))
    )
  )
  (func (export "call") (param "value" u32) (result u32)
    (canon lift (core func $shim "wrapper"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert_eq!(args, &[ComponentValue::U32(7)]);
        Ok(vec![ComponentValue::U32(42)])
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "call", &[ComponentValue::U32(7)])
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

#[tokio::test]
async fn component_runtime_calls_post_return_after_lift() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (memory (export "mem") 1)
    (global $count (mut i32) (i32.const 0))
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 8))
      (i32.store (i32.const 4) (i32.const 1))
      (i32.store8 (i32.const 8) (i32.const 97))
      (i32.const 0)
    )
    (func (export "p") (param i32)
      (global.set $count (i32.add (global.get $count) (i32.const 1)))
    )
    (func (export "count") (result i32)
      (global.get $count)
    )
  )
  (core instance $i (instantiate $m))
  (func (export "str") (result string)
    (canon lift (core func $i "f") (memory $i "mem") (post-return (func $i "p")))
  )
  (func (export "count") (result u32)
    (canon lift (core func $i "count"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "str", &[])
        .await
        .expect("call should succeed");
    assert_eq!(result, vec![ComponentValue::String("a".to_owned())]);

    let count = instance
        .call(&store, "count", &[])
        .await
        .expect("count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(1)]);
}

#[tokio::test]
async fn component_runtime_surfaces_post_return_traps() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (memory (export "mem") 1)
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 8))
      (i32.store (i32.const 4) (i32.const 1))
      (i32.store8 (i32.const 8) (i32.const 97))
      (i32.const 0)
    )
    (func (export "p") (param i32)
      unreachable
    )
  )
  (core instance $i (instantiate $m))
  (func (export "str") (result string)
    (canon lift (core func $i "f") (memory $i "mem") (post-return (func $i "p")))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&store, "str", &[])
        .await
        .expect_err("post-return trap should surface");
    assert!(matches!(error, ComponentError::Trap(message) if message.contains("unreachable")));
}

#[tokio::test]
async fn component_runtime_surfaces_resource_drop_destructor_traps() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (func (export "dtor") (param i32)
      unreachable
    )
  )
  (core instance $m (instantiate $m))
  (type $r (resource (rep i32) (dtor (func $m "dtor"))))
  (core func $new (canon resource.new $r))
  (core func $drop (canon resource.drop $r))
  (core module $caller
    (import "" "new" (func $new (param i32) (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "run")
      (call $drop (call $new (i32.const 7)))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "new" (func $new))
        (export "drop" (func $drop))
      ))
    )
  )
  (func (export "run")
    (canon lift (core func $caller "run"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("resource drop trap should surface");
    assert!(matches!(error, ComponentError::Trap(message) if message.contains("unreachable")));
}
