;; A minimal WASI 0.2 component command.
;;
;; It imports `wasi:cli/environment@0.2.6.get-arguments`, lowers it into a core
;; module through the canonical ABI (linear memory + realloc), and exports
;; `wasi:cli/run@0.2.6`. The run result is `ok` when the component received at
;; least one guest argument after argv[0], and `err` otherwise, so the outcome
;; is observable through the process exit status.
;;
;; This sample deliberately reports via its exit status so it stays focused on
;; argument lowering. Telomere can obtain and use a WASI `output-stream`, but
;; host-provided resources cannot yet be released with `canon resource.drop`.
;; See examples/README.md for that lifecycle constraint.
;;
;; Build:
;;   wasm-tools parse examples/wasi-component-args.wat -o examples/wasi-component-args.wasm
;;
;; Run:
;;   cargo run -- component examples/wasi-component-args.wasm -- one
(component
  (type $environment (instance
    (export "get-arguments" (func (result (list string))))
  ))
  (import "wasi:cli/environment@0.2.6" (instance $env (type $environment)))
  (alias export $env "get-arguments" (func $get-arguments))

  ;; A standalone memory module so that `canon lower` can reference a memory
  ;; without creating a cycle with the module that consumes the lowered import.
  (core module $memory-module
    (memory (export "memory") 1)
  )
  (core instance $memory-instance (instantiate $memory-module))
  (alias core export $memory-instance "memory" (core memory $memory))

  ;; Bump allocator used as the canonical ABI `realloc`. The bump pointer lives
  ;; at address 0 and the arena starts at 4096.
  (core module $allocator
    (import "env" "memory" (memory 1))

    (func $init
      (i32.store (i32.const 0) (i32.const 4096)))
    (start $init)

    (func (export "realloc")
      (param $old_ptr i32) (param $old_size i32)
      (param $align i32) (param $new_size i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr
        (i32.and
          (i32.add (i32.load (i32.const 0)) (i32.const 7))
          (i32.const -8)))
      (i32.store (i32.const 0) (i32.add (local.get $ptr) (local.get $new_size)))
      (local.get $ptr))
  )
  (core instance $allocator-instance (instantiate $allocator
    (with "env" (instance (export "memory" (memory $memory))))
  ))
  (alias core export $allocator-instance "realloc" (core func $realloc))

  (core func $get-arguments-core
    (canon lower (func $get-arguments) (memory $memory) (realloc $realloc)))

  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-arguments" (func $get-arguments (param i32)))

    ;; Returns 0 (`ok`) when argv holds more than just argv[0].
    (func (export "run") (result i32)
      ;; The canonical ABI writes `[ptr, len]` to the return pointer.
      (call $get-arguments (i32.const 2048))
      (if (result i32)
        (i32.gt_u (i32.load (i32.const 2052)) (i32.const 1))
        (then (i32.const 0))
        (else (i32.const 1))))
  )
  (core instance $main-instance (instantiate $main
    (with "env" (instance (export "memory" (memory $memory))))
    (with "wasi" (instance (export "get-arguments" (func $get-arguments-core))))
  ))

  (type $run-func (func (result (result))))
  (func $run (type $run-func) (canon lift (core func $main-instance "run")))
  (instance $run-instance (export "run" (func $run)))
  (export "wasi:cli/run@0.2.6" (instance $run-instance))
)
