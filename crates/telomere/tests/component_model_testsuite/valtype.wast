(component
    (type (list bool))
)

(component
    (type (list s32))
    (type (list 0))
)

(assert_invalid
    (component
        (type (func (param "a" (list s32))))
        (type (list 1))
    )
    "the typeidx of valtype must refer to defvaltype"
)
