;; Trap-diagnostics conformance fixture: out-of-bounds store (address == memory size).
(module $oob_store
  (memory 1)
  (func $run (export "run") i32.const 65536 i32.const 1 i32.store))
