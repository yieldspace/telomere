;; Trap-diagnostics conformance fixture: out-of-bounds load (address == memory size).
(module $oob_load
  (memory 1)
  (func $run (export "run") i32.const 65536 i32.load drop))
