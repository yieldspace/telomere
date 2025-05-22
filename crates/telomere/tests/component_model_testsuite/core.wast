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
        (memory (export "memory") 17)
        (global (export "global") (mut i32) i32.const 1048576)
        (table (export "table") 193 193 funcref)
    )
    (core instance (instantiate 0))
    (alias core export 0 "mod-main" (core func))
    (alias core export 0 "memory" (core memory))
    (alias core export 0 "global" (core global))
    (alias core export 0 "table" (core table))
)

(assert_invalid
    (component
        (core module
            (func (export "mod-main") (result i32)
                (i32.const 42)
            )
        )
        (core instance (instantiate 0))
        (alias core export 0 "mod-main" (core table))
    )
    "invalid sort type"
)

(assert_invalid
    (component
        (core module
            (func (export "mod-main") (result i32)
                (i32.const 42)
            )
        )
        (core instance (instantiate 0))
        (alias core export 0 "nod-main" (core func))
    )
    "import name not found"
)
