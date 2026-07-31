;; A minimal WASI preview1 command module.
;;
;; It writes a greeting to stdout and then echoes every argv entry it received,
;; one per line. It only uses the preview1 host functions that the telomere CLI
;; implements: args_sizes_get, args_get, fd_write, proc_exit.
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

  ;; 0    .. 8    : iovec
  ;; 16   .. 20   : nwritten
  ;; 32   .. 40   : argc / argv_buf_size
  ;; 64   .. 256  : argv pointer array
  ;; 256  .. 1024 : argv string buffer
  ;; 1024 ..      : static strings
  (data (i32.const 1024) "hello from telomere (wasi preview1)\n")
  (data (i32.const 1088) "\n")

  ;; Write `len` bytes starting at `ptr` to stdout.
  (func $write (param $ptr i32) (param $len i32)
    (i32.store (i32.const 0) (local.get $ptr))
    (i32.store (i32.const 4) (local.get $len))
    (drop (call $fd_write
      (i32.const 1)   ;; fd = stdout
      (i32.const 0)   ;; iovs
      (i32.const 1)   ;; iovs_len
      (i32.const 16)  ;; nwritten out pointer
    )))

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
    (local $index i32)
    (local $entry i32)

    (call $write (i32.const 1024) (i32.const 36))

    (drop (call $args_sizes_get (i32.const 32) (i32.const 36)))
    (local.set $argc (i32.load (i32.const 32)))
    (drop (call $args_get (i32.const 64) (i32.const 256)))

    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $argc)))
        (local.set $entry
          (i32.load (i32.add (i32.const 64)
                             (i32.mul (local.get $index) (i32.const 4)))))
        (call $write (local.get $entry) (call $strlen (local.get $entry)))
        (call $write (i32.const 1088) (i32.const 1))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))

    (call $proc_exit (i32.const 0)))
)
