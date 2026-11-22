;; Trap-diagnostics conformance fixture (structural): nine-frame call chain.
(module $deep_chain
  (func $run (export "run") call $f1)
  (func $f1 call $f2)
  (func $f2 call $f3)
  (func $f3 call $f4)
  (func $f4 call $f5)
  (func $f5 call $f6)
  (func $f6 call $f7)
  (func $f7 call $f8)
  (func $f8 unreachable))
