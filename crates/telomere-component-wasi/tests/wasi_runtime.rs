use std::future::Future;
use std::time::Duration;
use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};
use telomere_component_wasi::{add_to_linker_async, add_to_linker_sync, preview3, WasiState};

fn compile_component() -> Vec<u8> {
    compile_component_with_wasi_version("0.2.6")
}

fn compile_component_with_wasi_version(version: &str) -> Vec<u8> {
    let text = r#"
(component
  (type $env
    (instance
      (export "get-environment" (func (result (list (tuple string string)))))
      (export "get-arguments" (func (result (list string))))
      (export "initial-cwd" (func (result (option string))))
    )
  )
  (import "wasi:cli/environment@0.2.6" (instance $environment (type $env)))
  (alias export $environment "get-environment" (func $get-environment))
  (alias export $environment "get-arguments" (func $get-arguments))
  (alias export $environment "initial-cwd" (func $initial-cwd))

  (type $random
    (instance
      (export "get-random-u64" (func (result u64)))
    )
  )
  (import "wasi:random/random@0.2.6" (instance $random-instance (type $random)))
  (alias export $random-instance "get-random-u64" (func $get-random-u64))

  (type $insecure-random
    (instance
      (export "get-insecure-random-u64" (func (result u64)))
    )
  )
  (import "wasi:random/insecure@0.2.6" (instance $insecure-random-instance (type $insecure-random)))
  (alias export $insecure-random-instance "get-insecure-random-u64" (func $get-insecure-random-u64))

  (export "get-environment" (func $get-environment))
  (export "get-arguments" (func $get-arguments))
  (export "initial-cwd" (func $initial-cwd))
  (export "get-random-u64" (func $get-random-u64))
  (export "get-insecure-random-u64" (func $get-insecure-random-u64))
)
"#;
    wat::parse_str(text.replace("@0.2.6", &format!("@{version}")))
        .expect("component wat must parse")
}

fn compile_preview2_poll_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (import "pollable" (type $pollable (sub resource)))
  (type $io-poll
    (instance
      (export "pollable" (type (eq $pollable)))
      (export "[method]pollable.block" (func (param "self" (borrow $pollable))))
      (export "poll" (func (param "in" (list (borrow $pollable))) (result (list u32))))
    )
  )
  (import "wasi:io/poll@0.2.6" (instance $io-poll-instance (type $io-poll)))
  (alias export $io-poll-instance "[method]pollable.block" (func $pollable-block))
  (alias export $io-poll-instance "poll" (func $poll))

  (type $monotonic
    (instance
      (export "subscribe-duration" (func (param "when" u64) (result (own $pollable))))
    )
  )
  (import "wasi:clocks/monotonic-clock@0.2.6" (instance $monotonic-instance (type $monotonic)))
  (alias export $monotonic-instance "subscribe-duration" (func $subscribe-duration))

  (export "subscribe-duration" (func $subscribe-duration))
  (export "pollable-block" (func $pollable-block))
  (export "poll" (func $poll))
)
"#,
    )
    .expect("preview2 poll component wat must parse")
}

fn compile_preview3_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $env
    (instance
      (export "get-environment" (func (result (list (tuple string string)))))
      (export "get-arguments" (func (result (list string))))
      (export "initial-cwd" (func (result (option string))))
    )
  )
  (import "wasi:cli/environment@0.3.0-rc-2026-03-15" (instance $environment (type $env)))
  (alias export $environment "get-environment" (func $get-environment))
  (alias export $environment "get-arguments" (func $get-arguments))
  (alias export $environment "initial-cwd" (func $initial-cwd))

  (import "error" (type $error (sub resource)))
  (import "pollable" (type $pollable (sub resource)))
  (import "input-stream" (type $input-stream (sub resource)))
  (import "output-stream" (type $output-stream (sub resource)))
  (import "descriptor" (type $descriptor (sub resource)))
  (import "directory-entry-stream" (type $directory-entry-stream (sub resource)))
  (import "tcp-socket" (type $tcp-socket (sub resource)))
  (import "udp-socket" (type $udp-socket (sub resource)))
  (type $stream-error (variant (case "last-operation-failed" (own $error)) (case "closed")))
  (type $descriptor-type (enum "unknown" "block-device" "character-device" "directory" "fifo" "symbolic-link" "regular-file" "socket"))
  (type $descriptor-flags (flags "read" "write" "file-integrity-sync" "data-integrity-sync" "requested-write-sync" "mutate-directory"))
  (type $path-flags (flags "symlink-follow"))
  (type $open-flags (flags "create" "directory" "exclusive" "truncate"))
  (type $filesystem-error-code
    (enum
      "access" "would-block" "already" "bad-descriptor" "busy" "deadlock" "quota" "exist"
      "file-too-large" "illegal-byte-sequence" "in-progress" "interrupted" "invalid" "io"
      "is-directory" "loop" "too-many-links" "message-size" "name-too-long" "no-device"
      "no-entry" "no-lock" "insufficient-memory" "insufficient-space" "not-directory"
      "not-empty" "not-recoverable" "unsupported" "no-tty" "no-such-device" "overflow"
      "not-permitted" "pipe" "read-only" "invalid-seek" "text-file-busy" "cross-device"
    )
  )
  (type $directory-entry (record (field "type" $descriptor-type) (field "name" string)))
  (type $optional-directory-entry (option $directory-entry))
  (type $ip-address-family (enum "ipv4" "ipv6"))
  (type $ipv4-address (tuple u8 u8 u8 u8))
  (type $ipv6-address (tuple u16 u16 u16 u16 u16 u16 u16 u16))
  (type $ip-address (variant (case "ipv4" $ipv4-address) (case "ipv6" $ipv6-address)))
  (type $ip-address-list (list $ip-address))
  (type $socket-error-code (enum "not-supported"))
  (type $ip-name-error-code (enum "invalid-argument" "name-unresolvable"))
  (type $io-poll
    (instance
      (export "pollable" (type (eq $pollable)))
      (export "[method]pollable.ready" (func (param "self" (borrow $pollable)) (result bool)))
      (export "[method]pollable.block" (func (param "self" (borrow $pollable))))
      (export "poll" (func (param "in" (list (borrow $pollable))) (result (list u32))))
    )
  )
  (import "wasi:io/poll@0.2.8" (instance $io-poll-instance (type $io-poll)))
  (alias export $io-poll-instance "[method]pollable.ready" (func $pollable-ready))
  (alias export $io-poll-instance "[method]pollable.block" (func $pollable-block))
  (alias export $io-poll-instance "poll" (func $poll))

  (type $stdin
    (instance
      (export "get-stdin" (func (result (own $input-stream))))
    )
  )
  (import "wasi:cli/stdin@0.3.0-rc-2026-03-15" (instance $stdin-instance (type $stdin)))
  (alias export $stdin-instance "get-stdin" (func $get-stdin))

  (type $stdout
    (instance
      (export "get-stdout" (func (result (own $output-stream))))
    )
  )
  (import "wasi:cli/stdout@0.3.0-rc-2026-03-15" (instance $stdout-instance (type $stdout)))
  (alias export $stdout-instance "get-stdout" (func $get-stdout))

  (type $streams
    (instance
      (export "stream-error" (type (eq $stream-error)))
      (export "input-stream" (type (eq $input-stream)))
      (export "output-stream" (type (eq $output-stream)))
      (export "[method]input-stream.read" (func (param "self" (borrow $input-stream)) (param "len" u64) (result (result (list u8) (error $stream-error)))))
      (export "[method]input-stream.subscribe" (func (param "self" (borrow $input-stream)) (result (own $pollable))))
      (export "[method]output-stream.check-write" (func (param "self" (borrow $output-stream)) (result (result u64 (error $stream-error)))))
      (export "[method]output-stream.write" (func (param "self" (borrow $output-stream)) (param "contents" (list u8)) (result (result (error $stream-error)))))
      (export "[method]output-stream.subscribe" (func (param "self" (borrow $output-stream)) (result (own $pollable))))
    )
  )
  (import "wasi:io/streams@0.2.8" (instance $streams-instance (type $streams)))
  (alias export $streams-instance "[method]input-stream.read" (func $input-read))
  (alias export $streams-instance "[method]input-stream.subscribe" (func $input-subscribe))
  (alias export $streams-instance "[method]output-stream.check-write" (func $output-check-write))
  (alias export $streams-instance "[method]output-stream.write" (func $output-write))
  (alias export $streams-instance "[method]output-stream.subscribe" (func $output-subscribe))

  (type $preopens
    (instance
      (export "get-directories" (func (result (list (tuple (own $descriptor) string)))))
    )
  )
  (import "wasi:filesystem/preopens@0.3.0-rc-2026-03-15" (instance $preopens-instance (type $preopens)))
  (alias export $preopens-instance "get-directories" (func $get-directories))

  (type $filesystem-types
    (instance
      (export "descriptor" (type (eq $descriptor)))
      (export "descriptor-type" (type (eq $descriptor-type)))
      (export "descriptor-flags" (type (eq $descriptor-flags)))
      (export "path-flags" (type (eq $path-flags)))
      (export "open-flags" (type (eq $open-flags)))
      (export "error-code" (type (eq $filesystem-error-code)))
      (export "directory-entry" (type (eq $directory-entry)))
      (export "optional-directory-entry" (type (eq $optional-directory-entry)))
      (export "directory-entry-stream" (type (eq $directory-entry-stream)))
      (export "[method]descriptor.get-type" (func (param "self" (borrow $descriptor)) (result (result $descriptor-type (error $filesystem-error-code)))))
      (export "[method]descriptor.get-flags" (func (param "self" (borrow $descriptor)) (result (result $descriptor-flags (error $filesystem-error-code)))))
      (export "[method]descriptor.open-at" (func
        (param "self" (borrow $descriptor))
        (param "path-flags" $path-flags)
        (param "path" string)
        (param "open-flags" $open-flags)
        (param "flags" $descriptor-flags)
        (result (result (own $descriptor) (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.read" (func
        (param "self" (borrow $descriptor))
        (param "length" u64)
        (param "offset" u64)
        (result (result (tuple (list u8) bool) (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.write" (func
        (param "self" (borrow $descriptor))
        (param "buffer" (list u8))
        (param "offset" u64)
        (result (result u64 (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.set-size" (func
        (param "self" (borrow $descriptor))
        (param "size" u64)
        (result (result (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.create-directory-at" (func
        (param "self" (borrow $descriptor))
        (param "path" string)
        (result (result (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.remove-directory-at" (func
        (param "self" (borrow $descriptor))
        (param "path" string)
        (result (result (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.unlink-file-at" (func
        (param "self" (borrow $descriptor))
        (param "path" string)
        (result (result (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.rename-at" (func
        (param "self" (borrow $descriptor))
        (param "old-path" string)
        (param "new-descriptor" (borrow $descriptor))
        (param "new-path" string)
        (result (result (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.read-via-stream" (func
        (param "self" (borrow $descriptor))
        (param "offset" u64)
        (result (result (own $input-stream) (error $filesystem-error-code)))
      ))
      (export "[method]descriptor.read-directory" (func
        (param "self" (borrow $descriptor))
        (result (result (own $directory-entry-stream) (error $filesystem-error-code)))
      ))
      (export "[method]directory-entry-stream.read-directory-entry" (func
        (param "self" (borrow $directory-entry-stream))
        (result (result $optional-directory-entry (error $filesystem-error-code)))
      ))
    )
  )
  (import "wasi:filesystem/types@0.3.0-rc-2026-03-15" (instance $filesystem-types-instance (type $filesystem-types)))
  (alias export $filesystem-types-instance "[method]descriptor.get-type" (func $descriptor-get-type))
  (alias export $filesystem-types-instance "[method]descriptor.get-flags" (func $descriptor-get-flags))
  (alias export $filesystem-types-instance "[method]descriptor.open-at" (func $descriptor-open-at))
  (alias export $filesystem-types-instance "[method]descriptor.read" (func $descriptor-read))
  (alias export $filesystem-types-instance "[method]descriptor.write" (func $descriptor-write))
  (alias export $filesystem-types-instance "[method]descriptor.set-size" (func $descriptor-set-size))
  (alias export $filesystem-types-instance "[method]descriptor.create-directory-at" (func $descriptor-create-directory-at))
  (alias export $filesystem-types-instance "[method]descriptor.remove-directory-at" (func $descriptor-remove-directory-at))
  (alias export $filesystem-types-instance "[method]descriptor.unlink-file-at" (func $descriptor-unlink-file-at))
  (alias export $filesystem-types-instance "[method]descriptor.rename-at" (func $descriptor-rename-at))
  (alias export $filesystem-types-instance "[method]descriptor.read-via-stream" (func $descriptor-read-via-stream))
  (alias export $filesystem-types-instance "[method]descriptor.read-directory" (func $descriptor-read-directory))
  (alias export $filesystem-types-instance "[method]directory-entry-stream.read-directory-entry" (func $directory-entry-stream-read-directory-entry))

  (type $sockets-types
    (instance
      (export "tcp-socket" (type (eq $tcp-socket)))
      (export "udp-socket" (type (eq $udp-socket)))
      (export "ip-address-family" (type (eq $ip-address-family)))
      (export "error-code" (type (eq $socket-error-code)))
      (export "[static]tcp-socket.create" (func (param "address-family" $ip-address-family) (result (result (own $tcp-socket) (error $socket-error-code)))))
      (export "[method]tcp-socket.get-address-family" (func (param "self" (borrow $tcp-socket)) (result $ip-address-family)))
      (export "[method]tcp-socket.set-listen-backlog-size" (func (param "self" (borrow $tcp-socket)) (param "value" u64) (result (result (error $socket-error-code)))))
      (export "[static]udp-socket.create" (func (param "address-family" $ip-address-family) (result (result (own $udp-socket) (error $socket-error-code)))))
      (export "[method]udp-socket.get-address-family" (func (param "self" (borrow $udp-socket)) (result $ip-address-family)))
      (export "[method]udp-socket.set-receive-buffer-size" (func (param "self" (borrow $udp-socket)) (param "value" u64) (result (result (error $socket-error-code)))))
    )
  )
  (import "wasi:sockets/types@0.3.0-rc-2026-03-15" (instance $sockets-types-instance (type $sockets-types)))
  (alias export $sockets-types-instance "[static]tcp-socket.create" (func $create-tcp-socket))
  (alias export $sockets-types-instance "[method]tcp-socket.get-address-family" (func $tcp-address-family))
  (alias export $sockets-types-instance "[method]tcp-socket.set-listen-backlog-size" (func $tcp-set-listen-backlog-size))
  (alias export $sockets-types-instance "[static]udp-socket.create" (func $create-udp-socket))
  (alias export $sockets-types-instance "[method]udp-socket.get-address-family" (func $udp-address-family))
  (alias export $sockets-types-instance "[method]udp-socket.set-receive-buffer-size" (func $udp-set-receive-buffer-size))

  (type $ip-name-lookup
    (instance
      (export "ip-address" (type (eq $ip-address)))
      (export "ip-address-list" (type (eq $ip-address-list)))
      (export "error-code" (type (eq $ip-name-error-code)))
      (export "resolve-addresses" (func (param "name" string) (result (result $ip-address-list (error $ip-name-error-code)))))
    )
  )
  (import "wasi:sockets/ip-name-lookup@0.3.0-rc-2026-03-15" (instance $ip-name-lookup-instance (type $ip-name-lookup)))
  (alias export $ip-name-lookup-instance "resolve-addresses" (func $resolve-addresses))

  (type $random
    (instance
      (export "get-random-u64" (func (result u64)))
    )
  )
  (import "wasi:random/random@0.3.0-rc-2026-03-15" (instance $random-instance (type $random)))
  (alias export $random-instance "get-random-u64" (func $get-random-u64))

  (type $insecure-random
    (instance
      (export "get-insecure-random-u64" (func (result u64)))
    )
  )
  (import "wasi:random/insecure@0.3.0-rc-2026-03-15" (instance $insecure-random-instance (type $insecure-random)))
  (alias export $insecure-random-instance "get-insecure-random-u64" (func $get-insecure-random-u64))

  (type $datetime (record (field "seconds" u64) (field "nanoseconds" u32)))
  (export "datetime" (type $datetime))
  (type $wall
    (instance
      (export "datetime" (type (eq $datetime)))
      (export "now" (func (result $datetime)))
      (export "resolution" (func (result $datetime)))
    )
  )
  (import "wasi:clocks/wall-clock@0.3.0-rc-2026-03-15" (instance $wall-instance (type $wall)))
  (alias export $wall-instance "now" (func $wall-now))
  (alias export $wall-instance "resolution" (func $wall-resolution))

  (type $monotonic
    (instance
      (export "now" (func (result u64)))
      (export "resolution" (func (result u64)))
      (export "get-resolution" (func (result u64)))
      (export "wait-until" (func (param "when" u64)))
      (export "wait-for" (func (param "duration" u64)))
      (export "subscribe-duration" (func (param "when" u64) (result (own $pollable))))
    )
  )
  (import "wasi:clocks/monotonic-clock@0.3.0-rc-2026-03-15" (instance $monotonic-instance (type $monotonic)))
  (alias export $monotonic-instance "now" (func $monotonic-now))
  (alias export $monotonic-instance "resolution" (func $monotonic-resolution))
  (alias export $monotonic-instance "get-resolution" (func $monotonic-get-resolution))
  (alias export $monotonic-instance "wait-until" (func $monotonic-wait-until))
  (alias export $monotonic-instance "wait-for" (func $monotonic-wait-for))
  (alias export $monotonic-instance "subscribe-duration" (func $subscribe-duration))

  (export "get-environment" (func $get-environment))
  (export "get-arguments" (func $get-arguments))
  (export "initial-cwd" (func $initial-cwd))
  (export "get-stdin" (func $get-stdin))
  (export "get-stdout" (func $get-stdout))
  (export "input-read" (func $input-read))
  (export "input-subscribe" (func $input-subscribe))
  (export "output-check-write" (func $output-check-write))
  (export "output-write" (func $output-write))
  (export "output-subscribe" (func $output-subscribe))
  (export "get-directories" (func $get-directories))
  (export "descriptor-get-type" (func $descriptor-get-type))
  (export "descriptor-get-flags" (func $descriptor-get-flags))
  (export "descriptor-open-at" (func $descriptor-open-at))
  (export "descriptor-read" (func $descriptor-read))
  (export "descriptor-write" (func $descriptor-write))
  (export "descriptor-set-size" (func $descriptor-set-size))
  (export "descriptor-create-directory-at" (func $descriptor-create-directory-at))
  (export "descriptor-remove-directory-at" (func $descriptor-remove-directory-at))
  (export "descriptor-unlink-file-at" (func $descriptor-unlink-file-at))
  (export "descriptor-rename-at" (func $descriptor-rename-at))
  (export "descriptor-read-via-stream" (func $descriptor-read-via-stream))
  (export "descriptor-read-directory" (func $descriptor-read-directory))
  (export "directory-entry-stream-read-directory-entry" (func $directory-entry-stream-read-directory-entry))
  (export "create-tcp-socket" (func $create-tcp-socket))
  (export "create-udp-socket" (func $create-udp-socket))
  (export "tcp-address-family" (func $tcp-address-family))
  (export "tcp-set-listen-backlog-size" (func $tcp-set-listen-backlog-size))
  (export "udp-address-family" (func $udp-address-family))
  (export "udp-set-receive-buffer-size" (func $udp-set-receive-buffer-size))
  (export "resolve-addresses" (func $resolve-addresses))
  (export "get-random-u64" (func $get-random-u64))
  (export "get-insecure-random-u64" (func $get-insecure-random-u64))
  (export "wall-now" (func $wall-now))
  (export "wall-resolution" (func $wall-resolution))
  (export "monotonic-now" (func $monotonic-now))
  (export "monotonic-resolution" (func $monotonic-resolution))
  (export "monotonic-get-resolution" (func $monotonic-get-resolution))
  (export "monotonic-wait-until" (func $monotonic-wait-until))
  (export "monotonic-wait-for" (func $monotonic-wait-for))
  (export "subscribe-duration" (func $subscribe-duration))
  (export "pollable-ready" (func $pollable-ready))
  (export "pollable-block" (func $pollable-block))
  (export "poll" (func $poll))
)
"#,
    )
    .expect("component wat must parse")
}

fn compile_preview3_http_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $http
    (instance
      (export "placeholder" (func))
    )
  )
  (import "wasi:http/types@0.3.0-rc-2026-03-15" (instance $http-instance (type $http)))
  (alias export $http-instance "placeholder" (func $placeholder))
  (export "placeholder" (func $placeholder))
)
"#,
    )
    .expect("component wat must parse")
}

fn compile_preview3_exit_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $exit
    (instance
      (export "exit" (func (param "status" (result))))
    )
  )
  (import "wasi:cli/exit@0.3.0-rc-2026-03-15" (instance $exit-instance (type $exit)))
  (alias export $exit-instance "exit" (func $exit))
  (export "exit" (func $exit))
)
"#,
    )
    .expect("component wat must parse")
}

fn compile_preview3_stdio_stream_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $stdio-error-code (enum "io" "illegal-byte-sequence" "pipe"))
  (type $stdio-result (result (error $stdio-error-code)))
  (type $stdio-future (future $stdio-result))
  (type $stdio-read-result (tuple (stream u8) $stdio-future))

  (type $stdin
    (instance
      (export "error-code" (type (eq $stdio-error-code)))
      (export "stdio-result" (type (eq $stdio-result)))
      (export "stdio-future" (type (eq $stdio-future)))
      (export "stdio-read-result" (type (eq $stdio-read-result)))
      (export "read-via-stream" (func (result $stdio-read-result)))
    )
  )
  (import "wasi:cli/stdin@0.3.0-rc-2026-03-15" (instance $stdin-instance (type $stdin)))
  (alias export $stdin-instance "read-via-stream" (func $read-via-stream))

  (type $stdout
    (instance
      (export "error-code" (type (eq $stdio-error-code)))
      (export "stdio-result" (type (eq $stdio-result)))
      (export "stdio-future" (type (eq $stdio-future)))
      (export "write-via-stream" (func (param "data" (stream u8)) (result $stdio-future)))
    )
  )
  (import "wasi:cli/stdout@0.3.0-rc-2026-03-15" (instance $stdout-instance (type $stdout)))
  (alias export $stdout-instance "write-via-stream" (func $stdout-write-via-stream))

  (type $stderr
    (instance
      (export "error-code" (type (eq $stdio-error-code)))
      (export "stdio-result" (type (eq $stdio-result)))
      (export "stdio-future" (type (eq $stdio-future)))
      (export "write-via-stream" (func (param "data" (stream u8)) (result $stdio-future)))
    )
  )
  (import "wasi:cli/stderr@0.3.0-rc-2026-03-15" (instance $stderr-instance (type $stderr)))
  (alias export $stderr-instance "write-via-stream" (func $stderr-write-via-stream))

  (export "read-via-stream" (func $read-via-stream))
  (export "stdout-write-via-stream" (func $stdout-write-via-stream))
  (export "stderr-write-via-stream" (func $stderr-write-via-stream))
)
"#,
    )
    .expect("component wat must parse")
}

fn build_state() -> WasiState {
    WasiState::builder()
        .args(["guest", "one"])
        .env([("FOO", "BAR"), ("BAZ", "QUX")])
        .stdin(b"abc".to_vec())
        .preopen_dir(".", "sandbox")
        .random_seed(0x1234_5678_u64)
        .build()
}

fn temp_sandbox() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "telomere-component-wasi-runtime-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("temp sandbox should be created");
    path
}

fn expected_random(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn result_own_handle(value: &ComponentValue, context: &str) -> u32 {
    match value {
        ComponentValue::Result {
            ok: Some(value),
            err: None,
        } => match value.as_ref() {
            ComponentValue::Own(handle) => *handle,
            other => panic!("expected own {context} handle, got {other:?}"),
        },
        other => panic!("expected {context} success result, got {other:?}"),
    }
}

async fn expect_pending_then_values<F>(future: F, context: &str) -> Vec<ComponentValue>
where
    F: Future<Output = Result<Vec<ComponentValue>, telomere_component::ComponentError>>,
{
    tokio::pin!(future);
    tokio::select! {
        result = &mut future => panic!("{context} completed without suspension: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(2)) => {}
    }
    tokio::time::timeout(Duration::from_secs(1), &mut future)
        .await
        .unwrap_or_else(|_| panic!("{context} did not resume"))
        .unwrap_or_else(|error| panic!("{context} failed after suspension: {error}"))
}

fn assert_snapshot(
    secure_random: &ComponentValue,
    insecure_random: &ComponentValue,
    args: &[ComponentValue],
    env: &[ComponentValue],
    cwd: &ComponentValue,
) {
    assert!(matches!(secure_random, ComponentValue::U64(_)));
    assert_eq!(
        insecure_random,
        &ComponentValue::U64(expected_random(0x1234_5678_u64))
    );
    assert_eq!(
        args,
        &[
            ComponentValue::String("guest".to_owned()),
            ComponentValue::String("one".to_owned())
        ]
    );
    assert_eq!(env.len(), 2);
    assert!(env.iter().any(|entry| {
        matches!(
            entry,
            ComponentValue::Tuple(values)
                if values
                    == &[
                        ComponentValue::String("BAZ".to_owned()),
                        ComponentValue::String("QUX".to_owned()),
                    ]
        )
    }));
    assert!(env.iter().any(|entry| {
        matches!(
            entry,
            ComponentValue::Tuple(values)
                if values
                    == &[
                        ComponentValue::String("FOO".to_owned()),
                        ComponentValue::String("BAR".to_owned()),
                    ]
        )
    }));
    assert_eq!(
        cwd,
        &ComponentValue::Option(Some(Box::new(ComponentValue::String("sandbox".to_owned()))))
    );
}

async fn run_runtime(
    add: fn(&mut ComponentLinker, WasiState) -> Result<(), telomere_component::ComponentError>,
) {
    let bytes = compile_component();
    run_runtime_bytes(add, bytes).await;
}

async fn run_runtime_bytes(
    add: fn(&mut ComponentLinker, WasiState) -> Result<(), telomere_component::ComponentError>,
    bytes: Vec<u8>,
) {
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    add(&mut linker, build_state()).expect("wasi linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let random = instance
        .call(&store, "get-random-u64", &[])
        .await
        .expect("random call should succeed");
    let insecure_random = instance
        .call(&store, "get-insecure-random-u64", &[])
        .await
        .expect("insecure random call should succeed");
    let args = instance
        .call(&store, "get-arguments", &[])
        .await
        .expect("arguments call should succeed");
    let env = instance
        .call(&store, "get-environment", &[])
        .await
        .expect("environment call should succeed");
    let cwd = instance
        .call(&store, "initial-cwd", &[])
        .await
        .expect("cwd call should succeed");

    assert_snapshot(
        &random[0],
        &insecure_random[0],
        &match &args[0] {
            ComponentValue::List(values) => values.clone(),
            other => panic!("expected list, got {other:?}"),
        },
        &match &env[0] {
            ComponentValue::List(values) => values.clone(),
            other => panic!("expected list, got {other:?}"),
        },
        &cwd[0],
    );
}

#[tokio::test]
async fn wasi_linker_sync_supports_environment_and_random() {
    run_runtime(add_to_linker_sync).await;
}

#[tokio::test]
async fn wasi_linker_async_supports_environment_and_random() {
    run_runtime(add_to_linker_async).await;
}

#[tokio::test]
async fn wasi_linker_async_resolves_semver_compatible_preview2_imports() {
    run_runtime_bytes(
        add_to_linker_async,
        compile_component_with_wasi_version("0.2.0"),
    )
    .await;
}

#[tokio::test]
async fn wasi_preview2_poll_waits_on_shared_substrate_timer() {
    let bytes = compile_preview2_poll_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    add_to_linker_async(&mut linker, build_state())
        .expect("wasi linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let pollable = instance
        .call(
            &store,
            "subscribe-duration",
            &[ComponentValue::U64(30_000_000)],
        )
        .await
        .expect("preview2 monotonic subscribe should succeed");
    let pollable_handle = match pollable.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected preview2 pollable handle, got {other:?}"),
    };

    let poll_args = vec![ComponentValue::List(vec![ComponentValue::Borrow(
        pollable_handle,
    )])];
    let poll = expect_pending_then_values(
        instance.call(&store, "poll", &poll_args),
        "preview2 poll timer",
    )
    .await;
    assert_eq!(
        poll,
        vec![ComponentValue::List(vec![ComponentValue::U32(0)])]
    );

    let block_pollable = instance
        .call(
            &store,
            "subscribe-duration",
            &[ComponentValue::U64(30_000_000)],
        )
        .await
        .expect("preview2 block subscribe should succeed");
    let block_handle = match block_pollable.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected preview2 block pollable handle, got {other:?}"),
    };
    let block_args = vec![ComponentValue::Borrow(block_handle)];
    let block = expect_pending_then_values(
        instance.call(&store, "pollable-block", &block_args),
        "preview2 pollable.block timer",
    )
    .await;
    assert!(block.is_empty());
}

#[tokio::test]
async fn wasi_preview3_linker_async_supports_environment_and_random_snapshot() {
    let bytes = compile_preview3_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let state = build_state();
    let mut linker = ComponentLinker::new();
    preview3::add_to_linker_async(&mut linker, state.clone())
        .expect("preview3 linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let random = instance
        .call(&store, "get-random-u64", &[])
        .await
        .expect("random call should succeed");
    let insecure_random = instance
        .call(&store, "get-insecure-random-u64", &[])
        .await
        .expect("insecure random call should succeed");
    let args = instance
        .call(&store, "get-arguments", &[])
        .await
        .expect("arguments call should succeed");
    let env = instance
        .call(&store, "get-environment", &[])
        .await
        .expect("environment call should succeed");
    let cwd = instance
        .call(&store, "initial-cwd", &[])
        .await
        .expect("cwd call should succeed");
    let wall_now = instance
        .call(&store, "wall-now", &[])
        .await
        .expect("wall clock call should succeed");
    let wall_resolution = instance
        .call(&store, "wall-resolution", &[])
        .await
        .expect("wall clock resolution should succeed");
    let monotonic_now = instance
        .call(&store, "monotonic-now", &[])
        .await
        .expect("monotonic clock call should succeed");
    let monotonic_resolution = instance
        .call(&store, "monotonic-resolution", &[])
        .await
        .expect("monotonic clock resolution should succeed");
    let monotonic_get_resolution = instance
        .call(&store, "monotonic-get-resolution", &[])
        .await
        .expect("P3 monotonic get-resolution should succeed");
    instance
        .call(&store, "monotonic-wait-for", &[ComponentValue::U64(0)])
        .await
        .expect("P3 monotonic wait-for zero duration should be ready");
    let now_for_wait = match monotonic_now.as_slice() {
        [ComponentValue::U64(value)] => *value,
        other => panic!("expected monotonic now result, got {other:?}"),
    };
    instance
        .call(
            &store,
            "monotonic-wait-until",
            &[ComponentValue::U64(now_for_wait)],
        )
        .await
        .expect("P3 monotonic wait-until past/current deadline should be ready");
    let stdin = instance
        .call(&store, "get-stdin", &[])
        .await
        .expect("get-stdin should succeed");
    let stdin_handle = match stdin.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected input stream handle, got {other:?}"),
    };
    let input_read = instance
        .call(
            &store,
            "input-read",
            &[ComponentValue::Borrow(stdin_handle), ComponentValue::U64(2)],
        )
        .await
        .expect("input read should succeed");
    let stdout = instance
        .call(&store, "get-stdout", &[])
        .await
        .expect("get-stdout should succeed");
    let stdout_handle = match stdout.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected output stream handle, got {other:?}"),
    };
    let output_check_write = instance
        .call(
            &store,
            "output-check-write",
            &[ComponentValue::Borrow(stdout_handle)],
        )
        .await
        .expect("output check-write should succeed");
    let output_write = instance
        .call(
            &store,
            "output-write",
            &[
                ComponentValue::Borrow(stdout_handle),
                ComponentValue::List(vec![ComponentValue::U8(b'h'), ComponentValue::U8(b'i')]),
            ],
        )
        .await
        .expect("output write should succeed");
    let preopens = instance
        .call(&store, "get-directories", &[])
        .await
        .expect("filesystem preopens should succeed");
    let preopen_descriptor = match &preopens[0] {
        ComponentValue::List(entries) => entries
            .iter()
            .find_map(|entry| match entry {
                ComponentValue::Tuple(values)
                    if values.len() == 2
                        && values[1] == ComponentValue::String("sandbox".to_owned()) =>
                {
                    match values[0] {
                        ComponentValue::Own(handle) => Some(handle),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("sandbox preopen descriptor should be present"),
        other => panic!("expected preopen list, got {other:?}"),
    };
    let descriptor_get_type = instance
        .call(
            &store,
            "descriptor-get-type",
            &[ComponentValue::Borrow(preopen_descriptor)],
        )
        .await
        .expect("descriptor get-type should succeed");
    let descriptor_get_flags = instance
        .call(
            &store,
            "descriptor-get-flags",
            &[ComponentValue::Borrow(preopen_descriptor)],
        )
        .await
        .expect("descriptor get-flags should succeed");
    let opened = instance
        .call(
            &store,
            "descriptor-open-at",
            &[
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::Flags(Vec::new()),
                ComponentValue::String("Cargo.toml".to_owned()),
                ComponentValue::Flags(Vec::new()),
                ComponentValue::Flags(vec!["read".to_owned()]),
            ],
        )
        .await
        .expect("descriptor open-at should succeed");
    let opened_descriptor = match &opened[0] {
        ComponentValue::Result {
            ok: Some(value),
            err: None,
        } => match value.as_ref() {
            ComponentValue::Own(handle) => *handle,
            other => panic!("expected opened descriptor handle, got {other:?}"),
        },
        other => panic!("expected open-at success, got {other:?}"),
    };
    let descriptor_read = instance
        .call(
            &store,
            "descriptor-read",
            &[
                ComponentValue::Borrow(opened_descriptor),
                ComponentValue::U64(1),
                ComponentValue::U64(0),
            ],
        )
        .await
        .expect("descriptor read should succeed");
    let stream = instance
        .call(
            &store,
            "descriptor-read-via-stream",
            &[
                ComponentValue::Borrow(opened_descriptor),
                ComponentValue::U64(0),
            ],
        )
        .await
        .expect("descriptor read-via-stream should succeed");
    let file_stream = match &stream[0] {
        ComponentValue::Result {
            ok: Some(value),
            err: None,
        } => match value.as_ref() {
            ComponentValue::Own(handle) => *handle,
            other => panic!("expected file input stream handle, got {other:?}"),
        },
        other => panic!("expected read-via-stream success, got {other:?}"),
    };
    let file_stream_read = instance
        .call(
            &store,
            "input-read",
            &[ComponentValue::Borrow(file_stream), ComponentValue::U64(1)],
        )
        .await
        .expect("file stream read should succeed");
    let directory_stream = instance
        .call(
            &store,
            "descriptor-read-directory",
            &[ComponentValue::Borrow(preopen_descriptor)],
        )
        .await
        .expect("descriptor read-directory should succeed");
    let directory_stream_handle = match &directory_stream[0] {
        ComponentValue::Result {
            ok: Some(value),
            err: None,
        } => match value.as_ref() {
            ComponentValue::Own(handle) => *handle,
            other => panic!("expected directory entry stream handle, got {other:?}"),
        },
        other => panic!("expected read-directory success, got {other:?}"),
    };
    let directory_entry = instance
        .call(
            &store,
            "directory-entry-stream-read-directory-entry",
            &[ComponentValue::Borrow(directory_stream_handle)],
        )
        .await
        .expect("directory-entry-stream read should succeed");
    let tcp_socket = instance
        .call(
            &store,
            "create-tcp-socket",
            &[ComponentValue::Enum("ipv4".to_owned())],
        )
        .await
        .expect("tcp socket resource allocation should succeed");
    let udp_socket = instance
        .call(
            &store,
            "create-udp-socket",
            &[ComponentValue::Enum("ipv4".to_owned())],
        )
        .await
        .expect("udp socket resource allocation should succeed");
    let tcp_socket_handle = result_own_handle(&tcp_socket[0], "tcp socket");
    let udp_socket_handle = result_own_handle(&udp_socket[0], "udp socket");
    let tcp_address_family = instance
        .call(
            &store,
            "tcp-address-family",
            &[ComponentValue::Borrow(tcp_socket_handle)],
        )
        .await
        .expect("tcp address-family should report the local socket family");
    let tcp_set_backlog = instance
        .call(
            &store,
            "tcp-set-listen-backlog-size",
            &[
                ComponentValue::Borrow(tcp_socket_handle),
                ComponentValue::U64(1),
            ],
        )
        .await
        .expect("unsupported tcp option should return a WASI error result");
    let udp_address_family = instance
        .call(
            &store,
            "udp-address-family",
            &[ComponentValue::Borrow(udp_socket_handle)],
        )
        .await
        .expect("udp address-family should report the local socket family");
    let udp_set_receive_buffer = instance
        .call(
            &store,
            "udp-set-receive-buffer-size",
            &[
                ComponentValue::Borrow(udp_socket_handle),
                ComponentValue::U64(4096),
            ],
        )
        .await
        .expect("unsupported udp option should return a WASI error result");
    let resolved_addresses = instance
        .call(
            &store,
            "resolve-addresses",
            &[ComponentValue::String("127.0.0.1".to_owned())],
        )
        .await
        .expect("literal IP lookup should succeed without DNS");
    let pollable = instance
        .call(&store, "subscribe-duration", &[ComponentValue::U64(0)])
        .await
        .expect("monotonic subscribe should succeed");
    let pollable_handle = match pollable.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected own pollable handle, got {other:?}"),
    };
    let pollable_ready = instance
        .call(
            &store,
            "pollable-ready",
            &[ComponentValue::Borrow(pollable_handle)],
        )
        .await
        .expect("pollable ready call should succeed");
    let poll = instance
        .call(
            &store,
            "poll",
            &[ComponentValue::List(vec![ComponentValue::Borrow(
                pollable_handle,
            )])],
        )
        .await
        .expect("poll should return the ready pollable index");

    assert_snapshot(
        &random[0],
        &insecure_random[0],
        &match &args[0] {
            ComponentValue::List(values) => values.clone(),
            other => panic!("expected list, got {other:?}"),
        },
        &match &env[0] {
            ComponentValue::List(values) => values.clone(),
            other => panic!("expected list, got {other:?}"),
        },
        &cwd[0],
    );
    assert!(
        matches!(&wall_now[0], ComponentValue::Record(fields) if fields.iter().any(|(name, _)| name == "seconds"))
    );
    assert!(
        matches!(&wall_resolution[0], ComponentValue::Record(fields) if fields.iter().any(|(name, value)| name == "nanoseconds" && value == &ComponentValue::U32(1)))
    );
    assert!(matches!(&monotonic_now[0], ComponentValue::U64(_)));
    assert_eq!(monotonic_resolution[0], ComponentValue::U64(1));
    assert_eq!(monotonic_get_resolution[0], ComponentValue::U64(1));
    assert_eq!(
        input_read[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::List(vec![
                ComponentValue::U8(b'a'),
                ComponentValue::U8(b'b')
            ]))),
            err: None
        }
    );
    assert_eq!(
        output_check_write[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::U64(4096))),
            err: None
        }
    );
    assert_eq!(
        output_write[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            err: None
        }
    );
    assert_eq!(state.stdout(), b"hi");
    assert_eq!(
        descriptor_get_type[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Enum("directory".to_owned()))),
            err: None
        }
    );
    assert_eq!(
        descriptor_get_flags[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Flags(vec!["read".to_owned()]))),
            err: None
        }
    );
    assert_eq!(
        descriptor_read[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(vec![
                ComponentValue::List(vec![ComponentValue::U8(b'[')]),
                ComponentValue::Bool(false),
            ]))),
            err: None
        }
    );
    assert_eq!(
        file_stream_read[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::List(vec![ComponentValue::U8(
                b'['
            )]))),
            err: None
        }
    );
    assert!(
        matches!(
            &directory_entry[0],
            ComponentValue::Result {
                ok: Some(value),
                err: None,
            } if matches!(
                value.as_ref(),
                ComponentValue::Option(Some(entry))
                    if matches!(entry.as_ref(), ComponentValue::Record(fields) if fields.iter().any(|(name, value)| name == "name" && matches!(value, ComponentValue::String(_))))
            )
        ),
        "unexpected directory entry: {directory_entry:?}"
    );
    assert!(
        matches!(
            &tcp_socket[0],
            ComponentValue::Result {
                ok: Some(value),
                err: None,
            } if matches!(value.as_ref(), ComponentValue::Own(_))
        ),
        "unexpected tcp socket result: {tcp_socket:?}"
    );
    assert!(
        matches!(
            &udp_socket[0],
            ComponentValue::Result {
                ok: Some(value),
                err: None,
            } if matches!(value.as_ref(), ComponentValue::Own(_))
        ),
        "unexpected udp socket result: {udp_socket:?}"
    );
    assert_eq!(
        tcp_address_family[0],
        ComponentValue::Enum("ipv4".to_owned())
    );
    assert_eq!(
        udp_address_family[0],
        ComponentValue::Enum("ipv4".to_owned())
    );
    assert_eq!(
        tcp_set_backlog[0],
        ComponentValue::Result {
            ok: None,
            err: Some(Box::new(ComponentValue::Enum("not-supported".to_owned())))
        }
    );
    assert_eq!(
        udp_set_receive_buffer[0],
        ComponentValue::Result {
            ok: None,
            err: Some(Box::new(ComponentValue::Enum("not-supported".to_owned())))
        }
    );
    let resolved_list = match &resolved_addresses[0] {
        ComponentValue::Result {
            ok: Some(value),
            err: None,
        } => match value.as_ref() {
            ComponentValue::List(addresses) => addresses,
            other => panic!("expected address list, got {other:?}"),
        },
        other => panic!("expected successful address result, got {other:?}"),
    };
    assert!(
        matches!(
            resolved_list.as_slice(),
            [ComponentValue::Variant { case, value: Some(payload) }]
                if case == "ipv4"
                    && matches!(
                        payload.as_ref(),
                        ComponentValue::Tuple(bytes)
                            if bytes
                                == &vec![
                                    ComponentValue::U8(127),
                                    ComponentValue::U8(0),
                                    ComponentValue::U8(0),
                                    ComponentValue::U8(1),
                                ]
                    )
        ),
        "unexpected literal IP lookup result: {resolved_addresses:?}"
    );
    assert!(
        matches!(
            &preopens[0],
            ComponentValue::List(entries)
                if entries.iter().any(|entry| matches!(
                    entry,
                    ComponentValue::Tuple(values)
                        if values.len() == 2
                            && matches!(values[0], ComponentValue::Own(_))
                            && values[1] == ComponentValue::String("sandbox".to_owned())
                ))
        ),
        "unexpected preopens: {preopens:?}"
    );
    assert_eq!(pollable_ready[0], ComponentValue::Bool(true));
    assert_eq!(poll[0], ComponentValue::List(vec![ComponentValue::U32(0)]));
}

#[tokio::test]
async fn wasi_preview3_poll_and_wait_for_suspend_then_resume() {
    let bytes = compile_preview3_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let state = build_state();
    let mut linker = ComponentLinker::new();
    preview3::add_to_linker_async(&mut linker, state)
        .expect("preview3 linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let wait_args = vec![ComponentValue::U64(30_000_000)];
    let wait = expect_pending_then_values(
        instance.call(&store, "monotonic-wait-for", &wait_args),
        "preview3 monotonic wait-for",
    )
    .await;
    assert!(wait.is_empty());

    let pollable = instance
        .call(
            &store,
            "subscribe-duration",
            &[ComponentValue::U64(30_000_000)],
        )
        .await
        .expect("preview3 monotonic subscribe should succeed");
    let pollable_handle = match pollable.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected preview3 pollable handle, got {other:?}"),
    };
    let poll_args = vec![ComponentValue::List(vec![ComponentValue::Borrow(
        pollable_handle,
    )])];
    let poll = expect_pending_then_values(
        instance.call(&store, "poll", &poll_args),
        "preview3 poll timer",
    )
    .await;
    assert_eq!(
        poll,
        vec![ComponentValue::List(vec![ComponentValue::U32(0)])]
    );

    let block_pollable = instance
        .call(
            &store,
            "subscribe-duration",
            &[ComponentValue::U64(30_000_000)],
        )
        .await
        .expect("preview3 block subscribe should succeed");
    let block_handle = match block_pollable.as_slice() {
        [ComponentValue::Own(handle)] => *handle,
        other => panic!("expected preview3 block pollable handle, got {other:?}"),
    };
    let block_args = vec![ComponentValue::Borrow(block_handle)];
    let block = expect_pending_then_values(
        instance.call(&store, "pollable-block", &block_args),
        "preview3 pollable.block timer",
    )
    .await;
    assert!(block.is_empty());
}

#[tokio::test]
async fn wasi_preview3_filesystem_mutates_read_write_preopen() {
    let dir = temp_sandbox();
    let bytes = compile_preview3_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let state = WasiState::builder()
        .preopen_dir_read_write(&dir, "sandbox")
        .build();
    let mut linker = ComponentLinker::new();
    preview3::add_to_linker_async(&mut linker, state)
        .expect("preview3 linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let preopens = instance
        .call(&store, "get-directories", &[])
        .await
        .expect("filesystem preopens should succeed");
    let preopen_descriptor = match &preopens[0] {
        ComponentValue::List(entries) => match &entries[0] {
            ComponentValue::Tuple(values) => match &values[0] {
                ComponentValue::Own(handle) => *handle,
                other => panic!("expected preopen descriptor, got {other:?}"),
            },
            other => panic!("expected preopen tuple, got {other:?}"),
        },
        other => panic!("expected preopen list, got {other:?}"),
    };

    let mkdir = instance
        .call(
            &store,
            "descriptor-create-directory-at",
            &[
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::String("created".to_owned()),
            ],
        )
        .await
        .expect("create-directory-at should return a component result");
    assert_eq!(
        mkdir[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            err: None
        }
    );
    assert!(dir.join("created").is_dir());

    let opened = instance
        .call(
            &store,
            "descriptor-open-at",
            &[
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::Flags(Vec::new()),
                ComponentValue::String("created/file.txt".to_owned()),
                ComponentValue::Flags(vec!["create".to_owned()]),
                ComponentValue::Flags(vec!["read".to_owned(), "write".to_owned()]),
            ],
        )
        .await
        .expect("open-at create should return a component result");
    let descriptor = match &opened[0] {
        ComponentValue::Result {
            ok: Some(value),
            err: None,
        } => match value.as_ref() {
            ComponentValue::Own(handle) => *handle,
            other => panic!("expected opened descriptor, got {other:?}"),
        },
        other => panic!("expected open-at success, got {other:?}"),
    };

    let write = instance
        .call(
            &store,
            "descriptor-write",
            &[
                ComponentValue::Borrow(descriptor),
                ComponentValue::List(vec![
                    ComponentValue::U8(b'o'),
                    ComponentValue::U8(b'k'),
                    ComponentValue::U8(b'\n'),
                ]),
                ComponentValue::U64(0),
            ],
        )
        .await
        .expect("descriptor write should return a component result");
    assert_eq!(
        write[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::U64(3))),
            err: None
        }
    );

    let read = instance
        .call(
            &store,
            "descriptor-read",
            &[
                ComponentValue::Borrow(descriptor),
                ComponentValue::U64(8),
                ComponentValue::U64(0),
            ],
        )
        .await
        .expect("descriptor read should return written bytes");
    assert_eq!(
        read[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(vec![
                ComponentValue::List(vec![
                    ComponentValue::U8(b'o'),
                    ComponentValue::U8(b'k'),
                    ComponentValue::U8(b'\n'),
                ]),
                ComponentValue::Bool(true),
            ]))),
            err: None
        }
    );

    let set_size = instance
        .call(
            &store,
            "descriptor-set-size",
            &[ComponentValue::Borrow(descriptor), ComponentValue::U64(2)],
        )
        .await
        .expect("descriptor set-size should return a component result");
    assert_eq!(
        set_size[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            err: None
        }
    );
    assert_eq!(
        std::fs::read(dir.join("created/file.txt")).expect("truncated file should be readable"),
        b"ok"
    );

    let rename = instance
        .call(
            &store,
            "descriptor-rename-at",
            &[
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::String("created/file.txt".to_owned()),
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::String("created/renamed.txt".to_owned()),
            ],
        )
        .await
        .expect("rename-at should return a component result");
    assert_eq!(
        rename[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            err: None
        }
    );
    assert!(!dir.join("created/file.txt").exists());
    assert_eq!(
        std::fs::read(dir.join("created/renamed.txt")).expect("renamed file should be readable"),
        b"ok"
    );

    let unlink = instance
        .call(
            &store,
            "descriptor-unlink-file-at",
            &[
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::String("created/renamed.txt".to_owned()),
            ],
        )
        .await
        .expect("unlink-file-at should return a component result");
    assert_eq!(
        unlink[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            err: None
        }
    );
    assert!(!dir.join("created/file.txt").exists());

    let rmdir = instance
        .call(
            &store,
            "descriptor-remove-directory-at",
            &[
                ComponentValue::Borrow(preopen_descriptor),
                ComponentValue::String("created".to_owned()),
            ],
        )
        .await
        .expect("remove-directory-at should return a component result");
    assert_eq!(
        rmdir[0],
        ComponentValue::Result {
            ok: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            err: None
        }
    );
    assert!(!dir.join("created").exists());

    std::fs::remove_dir_all(dir).expect("temp sandbox should be removed");
}

#[tokio::test]
async fn wasi_preview3_http_imports_fail_closed() {
    let bytes = compile_preview3_http_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    preview3::add_to_linker_async(&mut linker, build_state())
        .expect("preview3 linker registration should succeed");

    let store = telomere::Store::new();
    let error = match engine.instantiate(&program, &store, &linker).await {
        Ok(_) => panic!("HTTP is outside the initial Preview 3 provider scope"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("wasi:http/types@0.3.0-rc-2026-03-15"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn wasi_preview3_exit_records_status_and_traps() {
    let bytes = compile_preview3_exit_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let state = build_state();
    let mut linker = ComponentLinker::new();
    preview3::add_to_linker_async(&mut linker, state.clone())
        .expect("preview3 linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(
            &store,
            "exit",
            &[ComponentValue::Result {
                ok: None,
                err: Some(Box::new(ComponentValue::Tuple(Vec::new()))),
            }],
        )
        .await
        .expect_err("exit should trap after recording status");

    assert!(
        error.to_string().contains("wasi cli exit(1)"),
        "unexpected error: {error}"
    );
    assert_eq!(state.exit_code(), Some(1));
}

#[tokio::test]
async fn wasi_preview3_stdio_uses_stream_and_future_handles() {
    let bytes = compile_preview3_stdio_stream_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let state = build_state();
    let mut linker = ComponentLinker::new();
    preview3::add_to_linker_async(&mut linker, state.clone())
        .expect("preview3 linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let read = instance
        .call(&store, "read-via-stream", &[])
        .await
        .expect("read-via-stream should succeed");
    let (stream, future) = match read.as_slice() {
        [ComponentValue::Tuple(values)] => match values.as_slice() {
            [ComponentValue::Stream(stream), ComponentValue::Future(future)] => (*stream, *future),
            other => panic!("expected stream/future tuple, got {other:?}"),
        },
        other => panic!("expected read-via-stream tuple result, got {other:?}"),
    };
    assert_ne!(stream, 0);
    assert_ne!(future, 0);

    let write = instance
        .call(
            &store,
            "stdout-write-via-stream",
            &[ComponentValue::Stream(stream)],
        )
        .await
        .expect("write-via-stream should accept the stream handle");
    let write_future = match write.as_slice() {
        [ComponentValue::Future(handle)] => *handle,
        other => panic!("expected write future handle, got {other:?}"),
    };
    assert_ne!(write_future, 0);
    assert_eq!(state.stdout(), b"abc");

    let stderr_read = instance
        .call(&store, "read-via-stream", &[])
        .await
        .expect("second read-via-stream should succeed");
    let stderr_stream = match stderr_read.as_slice() {
        [ComponentValue::Tuple(values)] => match values.as_slice() {
            [ComponentValue::Stream(stream), ComponentValue::Future(_)] => *stream,
            other => panic!("expected stream/future tuple, got {other:?}"),
        },
        other => panic!("expected read-via-stream tuple result, got {other:?}"),
    };
    instance
        .call(
            &store,
            "stderr-write-via-stream",
            &[ComponentValue::Stream(stderr_stream)],
        )
        .await
        .expect("stderr write-via-stream should accept the stream handle");
    assert_eq!(state.stderr(), b"abc");
}
