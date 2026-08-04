;; A dense floating-point loop.  The two exact binary fractions total one per
;; iteration, so `run(n)` returns n and remains self-validating at the CLI.
(module
  (func (export "run") (param $iterations i32) (result i32)
    (local $index i32)
    (local $accumulator f64)

    (block $done
      (loop $loop
        local.get $index
        local.get $iterations
        i32.ge_u
        br_if $done

        local.get $accumulator
        f64.const 0.25
        f64.add
        f64.const 0.75
        f64.add
        local.set $accumulator

        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      )
    )

    local.get $accumulator
    i32.trunc_f64_s
  )
)
