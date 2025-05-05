(component
    (type (enum "a" "b"))
)

(assert_invalid
    (component
        (type (enum ))
    )
    "enum has variants at least one"
)

(assert_invalid
    (component
        (type (enum "a" "a"))
    )
    "enum variant name is redundant defined"
)
