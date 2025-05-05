(component
    (type (func (param "i" s32)))
    (import "func" (func (type 0)))
    (type (resource (rep i32)))
    (type (resource (rep i32) (dtor (func 0))))
)

(assert_invalid
    (component
        (type (func (param "i" u32)))
        (import "func" (func (type 0)))
        (type (resource (rep i32) (dtor (func 0))))
    )
    "resource destructor function type is not correct"
)
