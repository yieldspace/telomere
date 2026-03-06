(component
    (type (func (result u32)))
    (import "dummy" (func (type 0)))
    (export "foo-bar" (func 0))
    (export "foo-BAR2" (func 0))
)

(assert_invalid
    (component
        (type (func (result u32)))
        (import "dummy" (func (type 0)))
        (export "foo" (func  0))
        (export "FOO" (func 0))
    )
    "Invalid export name: export is redundant defined"
)

(assert_invalid
    (component
        (type (func (result u32)))
        (import "dummy" (func (type 0)))
        (export "foo-bar" (func 0))
        (export "foo-BAR" (func 0))
    )
    "Invalid export name: export is redundant defined"
)


(assert_invalid
    (component
        (type (func (result u32)))
        (import "dummy" (func (type 0)))
        (export "a" (func 0))
        (export "a" (func 0))
    )
    "Invalid export name: export is redundant defined"
)

;;(component
;;    (type (func (param "ptr" s32)))
;;    (export "a" (type (sub resource)))
;;    (export "[constructor]a" (func (type 0)))
;;)
