(component
    (core module $M
        (func (export "dtor") (param i32))
        (func (export "bad-dtor") (param i64))
    )
    (core instance $m (instantiate $M))
    (type (resource (rep i32)))
    (type (resource (rep i32) (dtor (func $m "dtor"))))
)

(assert_invalid
    (component
        (core module $M
            (func (export "bad-dtor") (param i64))
        )
        (core instance $m (instantiate $M))
        (type (resource (rep i32) (dtor (func $m "bad-dtor"))))
    )
    "resource destructor function type is not correct"
)

(component
    (type $x (resource (rep i32)))
    (core func (canon resource.new $x))
    (core func (canon resource.rep $x))
    (core func (canon resource.drop $x))
)

(component definition
    (import "x" (type $x (sub resource)))
    (core func (canon resource.drop $x))
)

(assert_invalid
    (component
        (type $x (resource (rep i64)))
    )
    "resources can only be represented by `i32`"
)

(assert_invalid
    (component
        (type $t u8)
        (type $x (borrow $t))
    )
    "not a resource type"
)

(assert_invalid
    (component
        (import "x" (type $x (sub resource)))
        (core func (canon resource.new $x))
    )
    "not a local resource"
)
