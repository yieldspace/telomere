;; empty component
(component
    (component)
    (instance (instantiate 0))
)

;; function import
(component
    (type (func (result u32)))
    (import "b" (func (type 0)))
    (component
        (type (func (result u32)))
        (import "a" (func (type 0)))
    )
    (instance (instantiate 0 (with "a" (func 0))))
)
;; component import
(component
    (type 
        (component
            (type (func (result u32)))
            (import "x" (func (type 0)))
        )
    )
    (import "b" (component (type 0)))
    (component
        (type 
            (component
                (type (func (result u32)))
                (import "x" (func (type 0)))
            )
        )
        (import "a" (component (type 0)))
    )
    (instance (instantiate 1 (with "a" (component 0))))
)

(component
    (type 
        (component
            (type (func (result u32)))
            (import "x" (func (type 0)))
        )
    )
    (import "b" (component (type 0)))
    (component
        (type 
            (component
                (type (func (result u32)))
                (import "x" (func (type 0)))
                (import "y" (func (type 0)))
            )
        )
        (import "a" (component (type 0)))
    )
    (instance (instantiate 1 (with "a" (component 0))))
)

(assert_invalid
    (component
        (type 
            (component
                (type (func (result u32)))
                (import "x" (func (type 0)))
                (import "y" (func (type 0)))
            )
        ) ;; type 0
        (import "b" (component (type 0))) ;; component 0
        (component
            (type 
                (component
                    (type (func (result u32)))
                    (import "x" (func (type 0)))
                )
            )
            (import "a" (component (type 0)))
        ) ;; component 1
        (instance (instantiate 1 (with "a" (component 0))))
    )
    "type mismatch"
)
(component
    (type 
        (component
            (type (func (result u32)))
            (export "x" (func (type 0)))
        )
    )
    (import "b" (component (type 0)))
    (component
        (type 
            (component
                (type (func (result u32)))
                (export "x" (func (type 0)))
            )
        )
        (import "a" (component (type 0)))
    )
    (instance (instantiate 1 (with "a" (component 0))))
)

(component definition
    (import "a" (core module $m))
    (core instance $a (instantiate $m))
)

(component
    (component
        (import "a" (func $i))
        (import "b" (component $c (import "a" (func))))
        (instance (instantiate $c (with "a" (func $i))))
    )
)

(assert_invalid
    (component
        (core instance (instantiate 0))
    )
    "unknown module"
)

(assert_invalid
    (component
        (instance (instantiate 0))
    )
    "unknown component"
)

(assert_invalid
    (component
        (import "a" (component $m
            (import "a" (func))
        ))
        (instance (instantiate $m))
    )
    "missing import named `a`"
)
(assert_invalid
    (component
        (type 
            (component
                (type (func (result u32)))
                (export "x" (func (type 0)))
            )
        )
        (import "b" (component (type 0)))
        (component
            (type 
                (component
                    (type (func (result u32)))
                    (export "x" (func (type 0)))
                    (export "y" (func (type 0)))
                )
            )
            (import "a" (component (type 0)))
        )
        (instance (instantiate 1 (with "a" (component 0))))
    )
    "type mismatch"
)
(component
    (type 
        (component
            (type (func (result u32)))
            (export "x" (func (type 0)))
            (export "y" (func (type 0)))
        )
    )
    (import "b" (component (type 0)))
    (component
        (type 
            (component
                (type (func (result u32)))
                (export "x" (func (type 0)))
            )
        )
        (import "a" (component (type 0)))
    )
    (instance (instantiate 1 (with "a" (component 0))))
)
