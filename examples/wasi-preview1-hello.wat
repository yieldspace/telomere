;; A minimal WASI preview1 command module.
;;
;; It writes a greeting to stdout and then echoes every argv entry it received,
;; one per line. It only uses the preview1 host functions that the telomere CLI
;; implements: args_sizes_get, args_get, fd_write, proc_exit.
;;
;; The point of the sample is the argv path, so it uses that path the way a real
;; guest has to: it checks the errno every preview1 call returns, and it uses
;; *both* results of args_sizes_get. argc sizes the pointer array and
;; argv_buf_size sizes the string buffer, so the buffer is placed immediately
;; after the pointer array and the two can never overlap however many arguments
;; the host passes. If the arguments do not fit in the current linear memory the
;; module grows it, and if the host refuses to grow it the module reports the
;; failure on stderr and exits non-zero instead of corrupting memory.
;;
;; Build:
;;   wasm-tools parse examples/wasi-preview1-hello.wat -o examples/wasi-preview1-hello.wasm
;;
;; Run:
;;   cargo run -- examples/wasi-preview1-hello.wasm -- one two
(module
  (import "wasi_snapshot_preview1" "args_sizes_get"
    (func $args_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_get"
    (func $args_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit"
    (func $proc_exit (param i32)))

  (memory (export "memory") 1)

  ;; 0   .. 8   : fd_write iovec
  ;; 16  .. 20  : fd_write nwritten
  ;; 32  .. 40  : args_sizes_get out params (argc, argv_buf_size)
  ;; 64  .. 100 : greeting
  ;; 128 .. 129 : newline
  ;; 160 .. 195 : error message
  ;; 256 ..     : argv pointer array (argc * 4 bytes), immediately followed by
  ;;              the argv string buffer (argv_buf_size bytes). Every static
  ;;              byte lives below 256, so this region can grow to whatever
  ;;              args_sizes_get reports without overwriting any of it.
  (data (i32.const 64) "hello from telomere (wasi preview1)\n")
  (data (i32.const 128) "\n")
  (data (i32.const 160) "argv does not fit in linear memory\n")

  (global $argv_ptrs i32 (i32.const 256))

  ;; Write `len` bytes starting at `ptr` to `fd`.
  (func $write (param $fd i32) (param $ptr i32) (param $len i32)
    (i32.store (i32.const 0) (local.get $ptr))
    (i32.store (i32.const 4) (local.get $len))
    (drop (call $fd_write
      (local.get $fd)
      (i32.const 0)   ;; iovs
      (i32.const 1)   ;; iovs_len
      (i32.const 16)  ;; nwritten out pointer
    )))

  ;; Report that argv cannot be laid out, and exit non-zero.
  (func $fail
    (call $write (i32.const 2) (i32.const 160) (i32.const 35))
    (call $proc_exit (i32.const 1)))

  ;; Make sure `bytes` addresses are backed by linear memory, growing it when
  ;; they are not. Exits through $fail if the host refuses to grow.
  (func $reserve (param $bytes i32)
    (local $pages i32)
    ;; ceil(bytes / 65536), computed without an addition that could wrap.
    (local.set $pages
      (i32.add
        (i32.shr_u (local.get $bytes) (i32.const 16))
        (i32.ne (i32.and (local.get $bytes) (i32.const 0xffff)) (i32.const 0))))
    (if (i32.gt_u (local.get $pages) (memory.size))
      (then
        (if (i32.eq
              (memory.grow (i32.sub (local.get $pages) (memory.size)))
              (i32.const -1))
          (then (call $fail))))))

  (func $strlen (param $ptr i32) (result i32)
    (local $len i32)
    (block $done
      (loop $scan
        (br_if $done
          (i32.eqz (i32.load8_u (i32.add (local.get $ptr) (local.get $len)))))
        (local.set $len (i32.add (local.get $len) (i32.const 1)))
        (br $scan)))
    (local.get $len))

  (func (export "_start")
    (local $argc i32)
    (local $argv_buf_size i32)
    (local $argv_buf i32)
    (local $needed i32)
    (local $index i32)
    (local $entry i32)

    (call $write (i32.const 1) (i32.const 64) (i32.const 36))

    ;; args_sizes_get returns an errno. A guest that drops it cannot tell a
    ;; successful call from one that wrote nothing into 32..40.
    (if (call $args_sizes_get (i32.const 32) (i32.const 36))
      (then (call $fail)))
    (local.set $argc (i32.load (i32.const 32)))
    (local.set $argv_buf_size (i32.load (i32.const 36)))

    ;; Keep argc * 4 from wrapping. 0x0fffffff arguments is far past any real
    ;; host limit, so refusing more of them is not a restriction in practice.
    (if (i32.gt_u (local.get $argc) (i32.const 0x0fffffff))
      (then (call $fail)))

    ;; The string buffer starts right after the pointer array, so its size is
    ;; what decides the total, not a fixed guess.
    (local.set $argv_buf
      (i32.add (global.get $argv_ptrs)
               (i32.mul (local.get $argc) (i32.const 4))))
    (local.set $needed
      (i32.add (local.get $argv_buf) (local.get $argv_buf_size)))
    (if (i32.lt_u (local.get $needed) (local.get $argv_buf))
      (then (call $fail)))
    (call $reserve (local.get $needed))

    (if (call $args_get (global.get $argv_ptrs) (local.get $argv_buf))
      (then (call $fail)))

    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $argc)))
        (local.set $entry
          (i32.load (i32.add (global.get $argv_ptrs)
                             (i32.mul (local.get $index) (i32.const 4)))))
        (call $write (i32.const 1)
                     (local.get $entry)
                     (call $strlen (local.get $entry)))
        (call $write (i32.const 1) (i32.const 128) (i32.const 1))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))

    (call $proc_exit (i32.const 0)))
)
