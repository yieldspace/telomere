;; Record
(component
    (type (record (field "a" s32) (field "b" s32)))
)

;; Record with no fields
(assert_invalid
    (component
        (type (record ))
    )
    "record has fields at least one"
)

;; Record with redundant name
(assert_invalid
    (component
        (type (record (field "a" s32) (field "a" s32)))
    )
    "record field name is redundant defined"
)

;; Variant
(component
    (type (variant (case "some" s32) (case "none")))
)

;; Variant with no cases
(assert_invalid
    (component
        (type (variant ))
    )
    "variant has cases at least one"
)

;; Enum
(component
    (type (enum "a" "b"))
)

;; Enum with no variants
(assert_invalid
    (component
        (type (enum ))
    )
    "enum has variants at least one"
)

;; Enum with redundant name
(assert_invalid
    (component
        (type (enum "a" "a"))
    )
    "enum variant name is redundant defined"
)

;; Flags
(component
    (type (flags "a" "b"))
)

;; Flags with redundant name
(assert_invalid
    (component
        (type (flags "a" "a"))
    )
    "flags variant name is redundant defined"
)

;; Enum with more than 32 variants
(assert_invalid
    (component
        (type (flags "a" "b" "c" "d" "e" "f" "g" "h" "i" "j" "k" "l" "m" "n" "o" "p" "q" "r" "s" "t" "u" "v" "w" "x" "y" "z" "aa" "bb" "cc" "dd" "ee" "ff" "gg" "hh" "ii"))
    )
    "flags variant name is too many"
)

;; List
(component
    (type (list s32))
    (type (list 0))
)