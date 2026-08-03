# Core Wasm Metering and Cancellation

## Status and Scope

Telomere provides opt-in, Store-scoped metering for the interpreter. It gives an
embedder a finite fuel budget and a cancellation handle for untrusted or
otherwise bounded guest execution.

Metering is disabled by default. A Store without metering exposes no metering
handle, so fuel and cancellation cannot be configured accidentally on a Store
where they would have no effect.

```rust
use telomere::{MeteringConfig, RuntimeConfig, Store};

let mut runtime_config = RuntimeConfig::default();
runtime_config.metering.enabled = true;
runtime_config.metering.initial_fuel = Some(100_000);

let store = Store::new_with_runtime_config(runtime_config);
let meter = store.metering().expect("metering was enabled for this Store");
```

`MeteringHandle` is `Clone + Send + Sync`. It is the single control surface for
the Store:

- `set_fuel(n)` replaces the fuel limit with `n`; `u64::MAX` selects unlimited
  fuel;
- `fuel_remaining()` returns `Some(n)` for a finite limit and `None` for an
  unlimited limit;
- `fuel_consumed()` returns the Store's cumulative charged fuel units;
- `interrupt()`, `is_interrupted()`, and `reset_interrupt()` control the
  cancellation flag.

The budget is shared by every module invocation that uses the same Store. It is
not a per-module or per-function limit.

## Fuel Unit

An ordinary fuel unit is one **checkpoint**, not one WebAssembly instruction.
The interpreter charges checkpoints at:

- Wasm `loop` back-edges;
- `return_call` and `return_call_indirect` tail-call transitions; and
- each iteration of selected fused or internal runtime loops whose trip count
  is derived from guest state and which can otherwise run without returning to
  the dispatcher.

Native bulk operations are the explicit exception: they precharge a
proportional number of fuel units before they start. The calculation and its
atomicity boundary are specified in
[Bulk Work and Resource Bounds](#bulk-work-and-resource-bounds).

An ordinary direct `call` or `call_indirect` is deliberately not a checkpoint.
Fuel therefore does not count ordinary call depth. That does not make ordinary
recursion unbounded: each execution has a fixed 128 KiB runtime stack, and a
new frame must fit its parameters, locals, and call-frame metadata before it is
installed. If it does not fit, the invocation returns `VMResult::StackOverflow`.
The resulting maximum call depth is not one public number; it depends on the
size of each frame.

This has important consequences:

- fuel is not compatible with Wasmtime fuel and is not a stable cross-version
  instruction-counting unit;
- a single checkpoint may cover more than one Wasm instruction, so fuel alone
  is not a wall-clock limit;
- time limits should use a watchdog that calls `interrupt()` in addition to a
  finite fuel budget.

The implementation renews ordinary checkpoint fuel in chunks. The current
chunk size is 4,096 checkpoints. It is a responsiveness and accounting bound,
not a public promise that guest work takes a fixed amount of CPU time; a single
native bulk precharge can intentionally exceed that chunk size.

## Bulk Work and Resource Bounds

The native forms of `memory.copy`, `memory.fill`, `memory.init`,
`memory.grow`, `table.copy`, `table.fill`, and `table.grow` have one admission
checkpoint. At that checkpoint they precharge their whole operation before
mutating guest state. The total charge, including that admission checkpoint,
uses a 4,096-byte granule:

```text
memory.copy / memory.fill / memory.init: 1 + (len_bytes >> 12)
memory.grow:                            1 + (delta_pages << 4)
table.copy / table.fill / table.grow:   1 + (len_elements >> 10)
```

The table formula uses `u32` reference elements, so 1,024 elements occupy one
4,096-byte fuel granule. This is a fuel debit, not a loop of 4 KiB
guest-visible partial operations. A debit can be greater than
`CHECKPOINT_CHUNK`; that is an intentional exception to the ordinary
reservation size. A negative `table.grow` delta follows its ordinary `-1`
result before entering this admission path.

If a finite Store has less fuel than the required precharge, it consumes all
fuel still available, performs none of the operation, and returns
`VMResult::FuelExhausted`. The admission precharge is also not refunded when a
later native bounds or capacity check rejects the operation.

After a native bulk operation has begun, cancellation does not stop it midway:
it reaches its normal result, including any normal trap, before the next
cancellation observation. Together with cancellation at every metered
checkpoint, the worst-case cancellation response is one checkpoint plus at
most one maximum native bulk operation. This is a bounded-work contract, not a
fixed wall-clock promise.

Shared-memory `memory.atomic.notify` is different from guest-sized native bulk
work. Its queue length is bounded by the number of guest threads concurrently
blocked in `atomic.wait`, and only the embedder can create those threads. The
guest-supplied `count` therefore does not control the queue-scan bound; in
particular, the conventional `u32::MAX` "wake all" request is not charged as
billions of operations. After releasing the wait-queue lock, a metered
interpreter charges `1 + woken` checkpoints for the work that completed. If
that postcharge exhausts fuel or observes cancellation, the waiters have
already been notified and the operation is not rolled back.

An indexed `memory.copy` uses that native admission path only when its source
and destination resolve to the same underlying linear memory. A cross-memory
copy is intentionally different: it validates both complete ranges before
writing, then uses a checkpoint callback for every 4 KiB chunk rather than one
whole-operation precharge. Fuel exhaustion or cancellation can therefore
return after part of the destination range has been written; cross-memory copy
is not all-or-nothing with respect to interruption. `table.init` likewise has
a checkpoint per element of its guest-length initialization loop, and can
leave already initialized elements in the destination table when interrupted.

The maximum size of a memory operation is constrained by the participating
linear memories. At instantiation, the effective maximum of each linear memory
is capped by `RuntimeConfig::memory.max_memory_pages` (the per-linear-memory
ceiling introduced by the #125 memory-limit work), in addition to any smaller
module-declared maximum. This is not a Store-wide or host-process memory cap.

Tables have a different limit model. The current table type is `Limits { min:
u32, max: Option<u32> }`; `table.grow` enforces the declared maximum only when
`max` is `Some`. There is no `RuntimeConfig` table ceiling. For a table with no
declared maximum, host allocation and address-space availability, rather than
metering or `max_memory_pages`, provide the practical bound.

## Fuel Accounting

`initial_fuel: Some(n)` starts a Store with a finite budget. `initial_fuel:
None` and `initial_fuel: Some(u64::MAX)` both mean unlimited fuel. Unlimited
fuel still enables checkpoint accounting and cancellation; in that mode
`fuel_remaining()` returns `None`.

`fuel_consumed()` is monotonic from Store construction and is not reset by
`set_fuel()`. It counts checkpoint charges and native-bulk precharges even with
unlimited fuel. That makes unlimited mode useful for calibration: run a
representative workload, record the `fuel_consumed()` delta, then choose a
finite limit with an appropriate margin for production.

```rust
let before = meter.fuel_consumed();
// Run a representative module invocation here.
let fuel_used = meter.fuel_consumed() - before;
meter.set_fuel(fuel_used.saturating_mul(2));
```

While an invocation is running, the runtime holds one private ordinary-
checkpoint reservation. Both `fuel_remaining()` and `fuel_consumed()` can
therefore read up to one checkpoint chunk **lower** than the true value. They
are exact when the Store is idle.

The conservation rule is intentionally limited to a finite, same-epoch budget.
If a finite epoch begins while the Store is idle, let `F` be the configured
fuel and `C` the value of `fuel_consumed()` at that boundary. Until the next
`set_fuel()`, the following is true while execution is active:

```text
F - fuel_remaining() - (fuel_consumed() - C) is in [0, CHECKPOINT_CHUNK]
```

At an idle boundary the value is zero. Do not apply this equation across a
concurrent `set_fuel()` call: changing epochs deliberately discards the old
reservation's unused credit rather than crediting it into the new limit.

Calling `set_fuel(n)` while guest code is running is allowed but approximate.
The already granted old chunk remains runnable, so guest code can execute up to
`n + CHECKPOINT_CHUNK - 1` further fuel units after the call. This includes a
native bulk precharge. For an exact budget, set fuel between invocations. To
stop work rather than replace a future budget, use `interrupt()`.

If exhaustion and cancellation are both observed at the same renewal point,
Telomere returns `VMResult::FuelExhausted`. Fuel exhaustion takes precedence
because it is deterministic; cancellation is timing dependent.

## Interruption Results

Fuel exhaustion and watchdog cancellation are payload-free `VMResult` variants:
`VMResult::FuelExhausted` and `VMResult::Cancelled`. Match those variants when
the caller handles a completed execution result directly.

`InterruptReason` remains the small shared reason type for APIs that need to
translate between the two representations. `VMResult::interrupt_reason()`
returns the corresponding reason, if any, and
`InterruptReason::into_vm_result()` constructs the matching payload-free
`VMResult` variant.

## Cancellation and Watchdogs

Cancellation is cooperative at checkpoints, but does not require guest
cooperation. A watchdog may run on a different host thread:

```rust
use std::time::Duration;
use telomere::VMResult;

let watchdog_meter = meter.clone();
let watchdog = std::thread::spawn(move || {
    std::thread::sleep(Duration::from_secs(1));
    watchdog_meter.interrupt();
});

let result = /* run_module_function(&instance, &store, "entry", &args).await */;
watchdog.join().expect("watchdog must not panic");

match result {
    VMResult::Cancelled => {}
    other => panic!("expected cancellation, got {other:?}"),
}
```

Every metered checkpoint performs a Relaxed load of the cancellation flag. A
request is therefore observed by the next checkpoint, at most one checkpoint
after it races a just-completed load. A native bulk operation that has already
passed its admission checkpoint runs to completion first. Shared-memory
`atomic.notify` likewise finishes scanning and waking its current wait queue
before its postcharge observes cancellation. The worst-case response is one
checkpoint plus one of: a maximum native bulk operation, or one
`atomic.notify` over the current wait queue. The latter queue is bounded by the
number of guest threads concurrently blocked in `atomic.wait`, which is set by
host thread creation and cannot grow from guest data alone. Cross-memory copy
and `table.init` retain their smaller chunk or element checkpoints and can
expose partial writes as described above.

Cancellation still cannot interrupt a host function that is blocked in I/O, a
lock, or another host call; that host call must have its own timeout or
cancellation policy. Once it returns to guest execution, the next metering
checkpoint observes the flag.

`MeteringHandle::interrupt()` stops guest code that is executing at a metered
checkpoint. It does not wake guest code parked in a `memory.atomic.wait` with
no timeout: metering bounds execution, not waiting. A guest can park only when
its embedder has provided shared memory, and the embedder owns guest-thread
creation. An embedder executing untrusted code must therefore either withhold
shared memory and threads or allow only waits with finite timeouts.

`reset_interrupt()` clears the flag for a later invocation. It does not restore
fuel or resume an invocation that has already returned an interruption result.

## Static Loop-Coverage Check

`crates/telomere/tests/vm_docs.rs` is a fail-closed change detector for the
explicit Rust loop syntax in `src/runtime/vm.rs`, the files directly under
`src/runtime/vm/`, `src/common/store.rs`, and `src/common/memory.rs`. It
requires every added `loop`, `while`, or `for` body in that source set to have
a direct `vm_checkpoint!` or a reasoned allow-list entry; it also performs a
conservative token scan of `macro_rules!` definitions and invocations.

This is intentionally narrower than a proof that every execution path is
metered. It does not analyze call graphs, iterator or library internals,
implicitly repeated primitives, code outside that source set, or host calls.
The runtime checkpoint contract above, targeted regressions, and review of
those other paths remain necessary.

## JIT Boundary

Metered execution is interpreter-only in this release. If both
`RuntimeConfig::metering.enabled` and `RuntimeConfig::jit.enabled` are set,
Store construction normalizes `jit.enabled` to `false`; the effective value is
visible through `store.runtime_config()`. The Store will not populate or use its
JIT cache while metering is enabled.

This is a safety boundary rather than a performance fallback. The experimental
JIT remains unsuitable for untrusted guests, and JIT checkpointing is deferred
to a separate design.
