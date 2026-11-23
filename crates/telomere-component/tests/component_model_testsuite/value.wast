;; Component-model value index space (sort 0x02, value section 0x0c):
;; conformance cases for #241. The toolchain's WAT has no value syntax, so
;; every case is a hand-encoded `(component binary ...)` quote.

;; Defined values of several shapes, exported from the value index space.
(component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\07\2b\04\72\02\01\61\79\01\62\73\70\7d\6e\03\04\72\65\61\64\05\77\72\69\74\65\04\65\78\65\63\71\02\03\6f\66\66\00\00\02\6f\6e\01\7b\00"   ;; type section
    "\0c\23\07\79\02\ac\02\73\06\05\68\65\6c\6c\6f\00\03\07\01\78\01\04\03\01\02\03\02\01\05\03\03\01\e8\07\7f\01\01"   ;; value section
    "\0b\55\07\00\05\76\2d\75\33\32\02\00\00\00\08\76\2d\73\74\72\69\6e\67\02\01\00\00\08\76\2d\72\65\63\6f\72\64\02\02\00\00\06\76\2d\6c\69\73\74\02\03\00\00\07\76\2d\66\6c\61\67\73\02\04\00\00\09\76\2d\76\61\72\69\61\6e\74\02\05\00\00\06\76\2d\62\6f\6f\6c\02\06\00"   ;; export section
)

;; A value import that is re-exported unchanged.
(component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\0a\06\01\00\01\76\02\79"   ;; import section
    "\0b\07\01\00\01\76\02\00\00"   ;; export section
)

;; A defined value passed as an instantiate argument and re-exported
;; through an alias of the instance's value export.
(component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\04\19\00\61\73\6d\0d\00\01\00\0a\06\01\00\01\76\02\79\0b\07\01\00\01\76\02\00\00"   ;; component section (inner)
    "\0c\04\01\79\01\2a"   ;; value section
    "\05\08\01\00\00\01\01\76\02\00"   ;; instance section
    "\06\06\01\02\00\00\01\76"   ;; alias section
    "\0b\07\01\00\01\76\02\01\00"   ;; export section
)

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\0c\06\01\79\03\2a\00\00"   ;; value section
  )
  "value encoding does not match its declared length")

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\0c\04\01\7f\01\02"   ;; value section
  )
  "invalid bool value byte: 2")

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\07\09\01\71\01\03\6f\66\66\00\00"   ;; type section
    "\0c\04\01\00\01\03"   ;; value section
  )
  "variant case index 3 is out of range")

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\07\06\02\3f\7f\00\69\00"   ;; type section
    "\0c\04\01\01\01\00"   ;; value section
  )
  "defined values cannot contain resource handles")

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\07\06\02\3f\7f\00\68\00"   ;; type section
    "\0a\06\01\00\01\76\02\01"   ;; import section
  )
  "value types cannot contain borrowed resource handles")

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\0b\07\01\00\01\76\02\05\00"   ;; export section
  )
  "type idx not found 5")

(assert_invalid
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\0c\04\01\79\01\01"   ;; value section
    "\0b\09\01\00\01\76\02\00\01\02\73"   ;; export section
  )
  "prim valtype mismatch")

(assert_malformed
  (component binary
    "\00asm" "\0d\00\01\00"   ;; component header
    "\0c\04\01\79\0a\80"   ;; value section (truncated)
  )
  "failed to fill whole buffer")
