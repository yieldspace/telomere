;; fixture-mode: host-reentry
;; Trap-diagnostics conformance fixture (structural): host frame mid-chain.
;; Func 0 is a placeholder the runner links to a host function. $outer calls
;; it; the host function re-enters the "inner" export, which traps, and the
;; host propagates the failure, so the published trace for the outer call is
;; [host frame, $outer frame]. The inner guest frame belongs to the separate
;; reentrant stack and is intentionally not stitched in.
(module $host_frame_chain
  (func $host)
  (func $outer (export "run") call $host)
  (func $inner (export "inner") unreachable))
