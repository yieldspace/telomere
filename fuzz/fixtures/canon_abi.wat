;; Canonical ABI types used by `canon_lift_args` fuzzing.
;;
;; The adapter compiles this component for its type table only. It never
;; instantiates this component or executes any of its guest functions.
(component
  ;; Export aliases make the nominal types valid in the callable export surface.
  (type $numbers' (list u32))
  (export $numbers "numbers" (type $numbers'))
  (type $inner' (record
    (field "text" string)
    (field "count" u32)
  ))
  (export $inner "inner" (type $inner'))
  (type $nested' (record
    (field "head" $inner)
    (field "tail" $numbers)
  ))
  (export $nested "nested" (type $nested'))
  (type $choice' (variant
    (case "none")
    (case "text" string)
    (case "nested" $nested)
  ))
  (export $choice "choice" (type $choice'))
  (type $permissions' (flags
    "read" "write" "append" "create" "delete" "rename" "execute" "share"
    "admin" "audit" "sync" "lock" "unlock" "watch" "mount" "unmount"
    "snapshot" "restore"
  ))
  (export $permissions "permissions" (type $permissions'))

  (type $string-func (func (param "value" string)))
  (type $list-func (func (param "value" $numbers)))
  (type $nested-record-func (func (param "value" $nested)))
  (type $variant-func (func (param "value" $choice)))
  (type $flags-func (func (param "value" $permissions)))
  ;; 17 flattened `u32` values force the MAX_FLAT_PARAMS indirect branch.
  (type $indirect-func (func
    (param "p00" u32) (param "p01" u32) (param "p02" u32)
    (param "p03" u32) (param "p04" u32) (param "p05" u32)
    (param "p06" u32) (param "p07" u32) (param "p08" u32)
    (param "p09" u32) (param "p10" u32) (param "p11" u32)
    (param "p12" u32) (param "p13" u32) (param "p14" u32)
    (param "p15" u32) (param "p16" u32)
  ))

  ;; This core module only makes the component exports valid. It is not the
  ;; fuzz memory module and is never instantiated by the adapter.
  (core module $guest
    (memory (export "mem") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 0
    )
    (func (export "string") (param i32 i32) unreachable)
    (func (export "list") (param i32 i32) unreachable)
    (func (export "nested-record") (param i32 i32 i32 i32 i32) unreachable)
    (func (export "variant") (param i32 i32 i32 i32 i32 i32) unreachable)
    (func (export "flags") (param i32) unreachable)
    (func (export "indirect") (param i32) unreachable)
  )
  (core instance $guest-instance (instantiate $guest))

  (func $string (type $string-func)
    (canon lift (core func $guest-instance "string")
      (memory $guest-instance "mem")
      (realloc (func $guest-instance "realloc"))
    )
  )
  (func $list (type $list-func)
    (canon lift (core func $guest-instance "list")
      (memory $guest-instance "mem")
      (realloc (func $guest-instance "realloc"))
    )
  )
  (func $nested-record (type $nested-record-func)
    (canon lift (core func $guest-instance "nested-record")
      (memory $guest-instance "mem")
      (realloc (func $guest-instance "realloc"))
    )
  )
  (func $variant (type $variant-func)
    (canon lift (core func $guest-instance "variant")
      (memory $guest-instance "mem")
      (realloc (func $guest-instance "realloc"))
    )
  )
  (func $flags (type $flags-func)
    (canon lift (core func $guest-instance "flags"))
  )
  (func $indirect (type $indirect-func)
    (canon lift (core func $guest-instance "indirect")
      (memory $guest-instance "mem")
      (realloc (func $guest-instance "realloc"))
    )
  )

  (export "string" (func $string))
  (export "list" (func $list) (func (param "value" $numbers)))
  (export "nested-record" (func $nested-record) (func (param "value" $nested)))
  (export "variant" (func $variant) (func (param "value" $choice)))
  (export "flags" (func $flags) (func (param "value" $permissions)))
  (export "indirect" (func $indirect))
)
