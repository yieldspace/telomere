(component
    (type (func (result u32)))
    (import "foo-bar" (func (type 0)))
    (import "foo-BAR2" (func (type 0)))
    (import "hoge" (type (sub resource)))
    (import "[constructor]hoge" (func (type 0)))
    (import "[method]hoge.fuga" (func (type 0)))
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
    (type (func (param "ptr" s32)))
    (import "a" (type (sub resource)))
    (import "[constructor]a" (func (type 0)))
)
