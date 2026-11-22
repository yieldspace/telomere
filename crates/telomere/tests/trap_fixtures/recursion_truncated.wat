;; Trap-diagnostics conformance fixture (structural): truncated trace shape.
;; Mutual recursion overflows the stack; the retained trace keeps the
;; innermost 48 frames and the outermost 16, with an elided middle, and the
;; alternating ping/pong names make any renumbering visible.
(module $recursion_truncated
  (func $run (export "run") call $ping)
  (func $ping call $pong)
  (func $pong call $ping))
