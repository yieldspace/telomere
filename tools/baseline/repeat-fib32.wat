;; A call-heavy workload with a linear repeat-count parameter.  `run(n)` makes
;; exactly n calls to fixed fib(32), then validates the accumulated result.
(module
  (func $fib (param $n i32) (result i32)
    local.get $n
    i32.const 2
    i32.lt_s
    if (result i32)
      local.get $n
    else
      local.get $n
      i32.const 1
      i32.sub
      call $fib
      local.get $n
      i32.const 2
      i32.sub
      call $fib
      i32.add
    end
  )

  (func (export "run") (param $repeat_count i32) (result i32)
    (local $index i32)
    (local $sum i32)

    (block $done
      (loop $loop
        local.get $index
        local.get $repeat_count
        i32.ge_u
        br_if $done

        local.get $sum
        i32.const 32
        call $fib
        i32.add
        local.set $sum

        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      )
    )

    ;; fib(32) = 2,178,309.  Returning -1 on a mismatch keeps the workload
    ;; self-validating while allowing the command-level expectation to be n.
    local.get $sum
    local.get $repeat_count
    i32.const 2178309
    i32.mul
    i32.ne
    if (result i32)
      i32.const -1
    else
      local.get $repeat_count
    end
  )
)
