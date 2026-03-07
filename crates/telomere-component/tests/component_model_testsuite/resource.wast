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
