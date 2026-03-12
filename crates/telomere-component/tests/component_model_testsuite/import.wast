(component
    (type (func (result u32)))
    (import "foo-bar" (func (type 0)))
    (import "foo-BAR2" (func (type 0)))
    (import "hoge" (type $hoge (sub resource)))
    (import "[constructor]hoge" (func (result (own $hoge))))
    (import "[method]hoge.fuga" (func (param "self" (borrow $hoge))))
    (import "[static]hoge.foo" (func (type 0)))
    (import "url=<https://mycdn.com/my-component.wasm>" (func (type 0)))
    (import "url=<./other-component.wasm>,integrity=<sha256-X9ArH3k...>" (func (type 0)))
    (import "locked-dep=<my-registry:sqlite@1.2.3>,integrity=<sha256-H8BRh8j...>" (func (type 0)))
    (import "unlocked-dep=<my-registry:imagemagick@{>=1.0.0}>" (func (type 0)))
    (import "integrity=<sha256-Y3BsI4l...>" (func (type 0)))
)

(assert_invalid
    (component
        (type (func (result u32)))
        (import "foo" (func (type 0)))
        (import "FOO" (func (type 0)))
    )
    "Invalid import name: import is redundant defined"
)

(assert_invalid
    (component
        (type (func (result u32)))
        (import "foo-bar" (func (type 0)))
        (import "foo-BAR" (func (type 0)))
    )
    "Invalid import name: import is redundant defined"
)


(assert_invalid
    (component
        (type (func (result u32)))
        (import "a" (func (type 0)))
        (import "a" (func (type 0)))
    )
    "Invalid import name: import is redundant defined"
)

(component
    (import "a" (type $a (sub resource)))
    (import "[constructor]a" (func (result (own $a))))
)

(component
    (component
        (import "a" (func))
        (import "b" (instance))
        (import "c" (instance
            (export "a" (func))
        ))
        (import "d" (component
            (import "a" (core module))
            (export "b" (func))
        ))
    )
)

(assert_invalid
    (component
        (type $f (func))
        (import "a" (instance (type $f)))
    )
    "type index 0 is not an instance type"
)

(assert_invalid
    (component
        (core module
            (import "" "a" (func))
            (import "" "a" (func))
        )
    )
    "duplicate import name `:a`"
)

(component definition
    (import "wasi:http/types@1.0.0" (func))
    (import "a:b/c@1.2.3" (func))
)

(assert_invalid
    (component
        (import "wasi:http/types@" (func))
    )
    "empty string"
)
