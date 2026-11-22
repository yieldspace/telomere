;; Trap-diagnostics conformance fixture: call_indirect through a null table entry.
(module $table_uninitialized
  (type $v (func))
  (table 1 funcref)
  (func $run (export "run") i32.const 0 call_indirect (type $v)))
