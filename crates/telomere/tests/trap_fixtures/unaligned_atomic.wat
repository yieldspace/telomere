;; fixture-requires: threads
;; Trap-diagnostics conformance fixture: unaligned atomic load (4-byte load at address 1).
(module $unaligned_atomic
  (memory 1 1 shared)
  (func $run (export "run") i32.const 1 i32.atomic.load drop))
