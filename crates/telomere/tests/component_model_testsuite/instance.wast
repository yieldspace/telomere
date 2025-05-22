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
    "type mismatch"
)
