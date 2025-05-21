(component
    (core module
    )
)

(component
    (core module
        (func (export "mod-main") (result i32)
            (i32.const 42)
        )
    )
    (core instance (instantiate 0))
)

(component
    (core module
        (func (export "mod-main") (result i32)
            (i32.const 42)
        )
    )
    (core instance (instantiate 0))
    (alias core export 0 "mod-main" (core func))
)
