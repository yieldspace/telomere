;; Trap-diagnostics conformance fixture (structural): tail-call chain.
;; $a return_calls $b, so $a reuses its frame slot and disappears from the
;; trace by design; the expected trace is [$b, $run] with no $a frame.
(module $tail_call_chain
  (func $run (export "run") call $a)
  (func $a return_call $b)
  (func $b unreachable))
