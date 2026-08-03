(component
  (type $add (func (param "lhs" s32) (param "rhs" s32) (result s32)))

  (core module $core-add
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add)
  )
  (core instance $core-add-instance (instantiate $core-add))

  (func (export "add") (type $add)
    (canon lift (core func $core-add-instance "add"))
  )
)
