;; Trap-diagnostics conformance fixture: integer remainder by zero.
(module $rem_zero
  (func $run (export "run") i32.const 1 i32.const 0 i32.rem_s drop))
