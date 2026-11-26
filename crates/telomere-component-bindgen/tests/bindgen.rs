use std::rc::Rc;

use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, Store};

telomere_component_bindgen::bindgen!({
    inline: r#"
        package ex:demo;

        world demo {
            record payload {
                value: u32,
            }

            enum status {
                ready,
                done,
            }

            flags modes {
                fast,
                slow,
            }

            import bump: func(value: u32) -> u32;

            import math: interface {
                double: func(value: u32) -> u32;
            }

            export guest: interface {
                run: func(value: u32) -> u32;
            }

            export ping: func(status: status, mode: modes) -> payload;
        }
    "#,
    world: "demo",
    module: "bindings"
});

fn compile_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $bump-t (func (param "value" u32) (result u32)))
  (import "bump" (func $bump (type $bump-t)))

  (type $math-t (func (param "value" u32) (result u32)))
  (type $math-i (instance (export "double" (func (type $math-t)))))
  (import "math" (instance $math (type $math-i)))
  (alias export $math "double" (func $double))

  (type $status (enum "ready" "done"))
  (export $status' "status" (type $status))
  (type $modes (flags "fast" "slow"))
  (export $modes' "modes" (type $modes))
  (type $payload (record (field "value" u32)))
  (export $payload' "payload" (type $payload))

  (core func $bump-lower (canon lower (func $bump)))
  (core func $double-lower (canon lower (func $double)))

  (core module $guest-m
    (import "" "double" (func $double (param i32) (result i32)))
    (func (export "run") (param i32) (result i32)
      local.get 0
      call $double)
  )
  (core instance $guest-core
    (instantiate $guest-m
      (with "" (instance
        (export "double" (func $double-lower))
      ))
    )
  )
  (type $guest-run-t (func (param "value" u32) (result u32)))
  (func $guest-run (type $guest-run-t)
    (canon lift (core func $guest-core "run")))
  (instance $guest
    (export "run" (func $guest-run))
  )
  (export "guest" (instance $guest))

  (core module $ping-m
    (import "" "bump" (func $bump (param i32) (result i32)))
    (func (export "ping") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add
      call $bump)
  )
  (core instance $ping-core
    (instantiate $ping-m
      (with "" (instance
        (export "bump" (func $bump-lower))
      ))
    )
  )
  (type $ping-t (func (param "status" $status') (param "mode" $modes') (result $payload')))
  (func (export "ping") (type $ping-t)
    (canon lift (core func $ping-core "ping")))
)
"#,
    )
    .expect("component wat must parse")
}

struct RootHost;

impl bindings::Imports for RootHost {
    fn bump(&self, _store: &Store, value: u32) -> Result<u32, ComponentError> {
        Ok(value + 10)
    }
}

struct MathHost;

impl bindings::imports::math::Host for MathHost {
    fn double(&self, _store: &Store, value: u32) -> Result<u32, ComponentError> {
        Ok(value * 2)
    }
}

telomere_component_bindgen::bindgen!({
    inline: r#"
        package ex:partial;

        world partial {
            import math: interface {
                double: func(value: u32) -> u32;
                triple: func(value: u32) -> u32;
            }

            export run: func(value: u32) -> u32;
        }
    "#,
    world: "partial",
    module: "partial_bindings",
    exclude: {
        "math": ["triple"],
    }
});

struct PartialMathHost;

impl partial_bindings::imports::math::Host for PartialMathHost {
    fn double(&self, _store: &Store, value: u32) -> Result<u32, ComponentError> {
        Ok(value * 2)
    }
}

fn compile_partial_component(export_name: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"
(component
  (type $math-t (func (param "value" u32) (result u32)))
  (type $math-i (instance (export "{export_name}" (func (type $math-t)))))
  (import "math" (instance $math (type $math-i)))
  (alias export $math "{export_name}" (func $imported))

  (core func $imported-lower (canon lower (func $imported)))

  (core module $guest-m
    (import "" "f" (func $f (param i32) (result i32)))
    (func (export "run") (param i32) (result i32)
      local.get 0
      call $f)
  )
  (core instance $guest-core
    (instantiate $guest-m
      (with "" (instance
        (export "f" (func $imported-lower))
      ))
    )
  )
  (type $run-t (func (param "value" u32) (result u32)))
  (func (export "run") (type $run-t)
    (canon lift (core func $guest-core "run")))
)
"#
    ))
    .expect("partial component wat must parse")
}

#[tokio::test]
async fn bindgen_excluded_function_is_not_registered() {
    let engine = ComponentEngine::new();
    let store = telomere::Store::new();

    let mut linker = ComponentLinker::new();
    partial_bindings::imports::math::add_to_linker(&mut linker, Rc::new(PartialMathHost));

    // The retained function links and runs.
    let bytes = compile_partial_component("double");
    let program = engine.compile(&bytes).expect("compile should succeed");
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate with a retained function should succeed");
    let result = partial_bindings::Exports::new(instance)
        .run(&store, 21)
        .await
        .expect("run should succeed");
    assert_eq!(result, 42);

    // The excluded function is absent from the linker instance, so a guest
    // importing it fails at instantiation instead of trapping on first call.
    let bytes = compile_partial_component("triple");
    let program = engine.compile(&bytes).expect("compile should succeed");
    let error = match engine.instantiate(&program, &store, &linker).await {
        Ok(_) => panic!("instantiate with an excluded function should fail"),
        Err(error) => error,
    };
    assert!(
        format!("{error:?}").contains("triple"),
        "link error should name the withheld function, got {error:?}"
    );
}

#[tokio::test]
async fn bindgen_supports_root_and_interface_bindings() {
    let bytes = compile_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    bindings::add_root_imports_to_linker(&mut linker, Rc::new(RootHost));
    bindings::imports::math::add_to_linker(&mut linker, Rc::new(MathHost));

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let exports = bindings::Exports::new(instance);
    let payload = exports
        .ping(
            &store,
            bindings::Status::Ready,
            bindings::Modes {
                fast: true,
                slow: false,
            },
        )
        .await
        .expect("root export should succeed");
    assert_eq!(payload.value, 11);

    let guest = exports.guest();
    let doubled = guest
        .run(&store, 21)
        .await
        .expect("interface export should succeed");
    assert_eq!(doubled, 42);
}
