;; Trap-diagnostics conformance fixture: call_indirect signature mismatch.
;; The table holds a (param i32) function; the call site declares (func).
(module $indirect_type_mismatch
  (type $v (func))
  (type $i (func (param i32)))
  (table 1 funcref)
  (elem (i32.const 0) func $f)
  (func $f (type $i))
  (func $run (export "run") i32.const 0 call_indirect (type $v)))
