;; empty component
(component
    (component)
    (instance (instantiate 0))
)

;; inline function import
(component
    (type (func (result u32)))
    (import "b" (func (type 0)))
    (component
        (type (func (result u32)))
        (import "a" (func (type 0)))
    )
    (instance (instantiate 0 (with "a" (func 0))))
)
