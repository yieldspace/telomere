;; Trap-diagnostics conformance fixture: stack overflow by direct self-recursion.
;; The trace is truncated: innermost 48 frames plus outermost 16 are retained.
(module $stack_overflow
  (func $run (export "run") call $run))
