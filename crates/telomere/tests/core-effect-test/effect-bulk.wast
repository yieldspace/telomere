(module (memory 1)  (
  func (export "test") (result i32) 
    (i32.store (i32.const 0) (i32.const 1))
    (i32.store (i32.const 1) (i32.const 2))
    (memory.copy
      (i32.const 2)
      (i32.const 0)
      (i32.const 2)
    )
    (i32.load (i32.const 3))
  )
)
(assert_return (invoke "test") (i32.const 2))