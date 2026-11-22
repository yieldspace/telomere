;; Trap-diagnostics conformance fixture: integer division overflow (INT_MIN / -1).
(module $div_overflow
  (func $run (export "run") i32.const -2147483648 i32.const -1 i32.div_s drop))
