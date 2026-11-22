//! Regression tests for #167: the component API must be drivable from any
//! executor an embedder might choose, including `futures::executor::block_on`.
//!
//! The component runtime used to call `futures::executor::block_on`
//! internally while instantiating core modules. `futures`' `block_on` takes
//! a thread-local enter guard, so driving the public API from an outer
//! `futures::executor::block_on` panicked with
//! `EnterError: cannot execute LocalPool executor from within another executor`.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

const ADD_COMPONENT_WAT: &str = r#"
(component
  (type $add (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (core module $core-add
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add)
  )
  (core instance $core-add-instance (instantiate $core-add))
  (func (export "add") (type $add)
    (canon lift (core func $core-add-instance "add"))
  )
)
"#;

async fn instantiate_and_add() -> Vec<ComponentValue> {
    let bytes = wat::parse_str(ADD_COMPONENT_WAT).expect("component WAT must be valid");
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("component must compile");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("component must instantiate");
    instance
        .call(
            &store,
            "add",
            &[ComponentValue::S32(20), ComponentValue::S32(22)],
        )
        .await
        .expect("component call must succeed")
}

fn assert_add_result(result: Vec<ComponentValue>) {
    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[test]
fn futures_block_on_drives_component_api() {
    assert_add_result(futures::executor::block_on(instantiate_and_add()));
}

/// A minimal hand-written executor: poll in a loop, park the thread while
/// pending. This is the least capable driver an embedder might write, and the
/// public API must work under it.
fn park_block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn hand_written_executor_drives_component_api() {
    assert_add_result(park_block_on(instantiate_and_add()));
}

#[test]
fn tokio_block_on_drives_component_api() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("tokio runtime builds");
    assert_add_result(runtime.block_on(instantiate_and_add()));
}
