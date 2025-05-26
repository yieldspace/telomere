(component
    (type
        (instance
        )
    )
)

(component
    (type
        (instance
            (type (tuple string string))
        )
    )
)

(component
    (type
        (instance
            (type (func (param "i" s32)))
            (export "export-func" (func (type 0)))
        )
    )
    (import "inst" (instance (type 0)))
    (alias export 0 "export-func" (func))
)
(assert_invalid
    (component
        (type
            (instance
                (type (func (param "i" s32)))
                (export "x" (func (type 0)))
            )
        )
        (component 
            (type
                (instance
                    (type (func (param "i" s32)))
                    (export "x" (func (type 0)))
                    (export "y" (func (type 0)))
                )
            )
            (import "a" (instance (type 0)))
        )
        (import "inst" (instance (type 0)))
        (instance (instantiate 0 (with "a" (instance 0))))
    )
  "export count mismatch"
)
;; FIXME: contextがいる。このPRのスコープではない。
;;(assert_invalid
;;    (component
;;        (type
;;            (instance
;;                (type (resource (rep i32)))
;;            )
;;        )
;;    )
;;    "resource type cannot use in instance type"
;;)