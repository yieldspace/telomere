;; Trap-diagnostics conformance fixture: integer division by zero.
(module $div_zero
  (func $run (export "run") i32.const 1 i32.const 0 i32.div_s drop))
