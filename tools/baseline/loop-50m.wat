;; A linear i32 load/add/store workload.  The harness supplies the count; the
;; published baseline uses 50,000,000 iterations for the #190 sensitivity probe.
(module
  (memory 1)

  (func (export "run") (param $iterations i32) (result i32)
    (local $index i32)

    (block $done
      (loop $loop
        local.get $index
        local.get $iterations
        i32.ge_u
        br_if $done

        i32.const 0
        i32.const 0
        i32.load
        i32.const 1
        i32.add
        i32.store

        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      )
    )

    i32.const 0
    i32.load
  )
)
