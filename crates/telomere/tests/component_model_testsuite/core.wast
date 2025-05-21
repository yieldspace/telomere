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
