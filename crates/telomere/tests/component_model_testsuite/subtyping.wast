(component
    (component
        (type (resource (rep i32)))
        (export "y" (type 0) (type (eq 0)))
     )
    (instance (instantiate 0))
    (component
        (type $TI (instance (export "y" (type (sub resource)))))
        (import "instance" (instance (type $TI)))
        (alias export 0 "y" (type $R))
        (import "x" (type (eq $R)))
    )
    (alias export 0 "y" (type $R))
    (instance (instantiate 1 (with "x" (type $R)) (with "instance" (instance 0))))
)