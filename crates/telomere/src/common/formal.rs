#![allow(dead_code, unexpected_cfgs)]

#[cfg(verus_keep_ghost)]
use vstd::multiset::Multiset;
use vstd::{map::Map, prelude::*, seq::Seq};

verus! {

pub type Address = nat;
pub type MemoryId = nat;
pub type WaiterId = nat;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemoryHandleView {
    Local(MemoryId),
    Shared(MemoryId),
}

pub struct LocalRefView {
    pub local_top: nat,
    pub local_size: nat,
}

pub struct FrameView {
    pub return_pc: int,
    pub instance: nat,
    pub default_memory: Option<MemoryHandleView>,
    pub prev_local: LocalRefView,
}

pub struct StackView {
    pub bytes: Seq<u8>,
    pub top: nat,
    pub frame_stack: Seq<FrameView>,
    pub active_local: LocalRefView,
}

pub struct LinearMemoryView {
    pub bytes: Seq<u8>,
    pub current_pages: nat,
    pub max_pages: nat,
    pub shared: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    Waiting,
    Notified,
    TimedOut,
}

pub struct WaiterView {
    pub waiter_id: WaiterId,
    pub address: Address,
    pub state: WaitState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WaitTicketTok {
    pub waiter_id: WaiterId,
    pub address: Address,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WakeTok {
    pub waiter_id: WaiterId,
    pub address: Address,
}

pub struct SharedMemoryProtocol {
    pub memory: LinearMemoryView,
    pub wait_queues: Map<Address, Seq<WaiterId>>,
    pub waiters: Map<WaiterId, WaitState>,
    pub next_waiter_id: WaiterId,
}

pub struct ExecContextToken {
    pub current_frame: FrameView,
    pub caller_frame: Option<FrameView>,
    pub cont_addr: nat,
    pub task_id: nat,
}

pub open spec fn optional_memory_handle_view_from_raw(
    present: bool,
    shared: bool,
    raw: nat,
) -> Option<MemoryHandleView> {
    if !present {
        None
    } else if shared {
        Some(MemoryHandleView::Shared(raw))
    } else {
        Some(MemoryHandleView::Local(raw))
    }
}

pub open spec fn local_reference_view_from_parts(local_top: nat, local_size: nat) -> LocalRefView {
    LocalRefView {
        local_top,
        local_size,
    }
}

pub open spec fn zero_bytes(len: nat) -> Seq<u8> {
    Seq::new(len as nat, |i: int| 0u8)
}

pub open spec fn spec_write_range(data: Seq<u8>, start: int, bytes: Seq<u8>) -> Seq<u8> {
    Seq::new(
        data.len(),
        |i: int| {
            if start <= i && i < start + bytes.len() {
                bytes[i - start]
            } else {
                data[i]
            }
        },
    )
}

pub open spec fn spec_fill_range(data: Seq<u8>, start: int, len: int, value: u8) -> Seq<u8> {
    Seq::new(
        data.len(),
        |i: int| {
            if start <= i && i < start + len {
                value
            } else {
                data[i]
            }
        },
    )
}

pub open spec fn spec_copy_within_range(data: Seq<u8>, dst: int, src: int, len: int) -> Seq<u8> {
    spec_write_range(data, dst, data.subrange(src, src + len))
}

pub closed spec fn spec_le_u16(bytes: Seq<u8>) -> u16
    recommends
        bytes.len() == 2,
{
    (bytes[0] as u16) | ((bytes[1] as u16) << 8)
}

pub closed spec fn spec_le_u32(bytes: Seq<u8>) -> u32
    recommends
        bytes.len() == 4,
{
    (bytes[0] as u32)
        | ((bytes[1] as u32) << 8)
        | ((bytes[2] as u32) << 16)
        | ((bytes[3] as u32) << 24)
}

pub closed spec fn spec_le_u64(bytes: Seq<u8>) -> u64
    recommends
        bytes.len() == 8,
{
    (bytes[0] as u64)
        | ((bytes[1] as u64) << 8)
        | ((bytes[2] as u64) << 16)
        | ((bytes[3] as u64) << 24)
        | ((bytes[4] as u64) << 32)
        | ((bytes[5] as u64) << 40)
        | ((bytes[6] as u64) << 48)
        | ((bytes[7] as u64) << 56)
}

pub closed spec fn spec_atomic_cmpxchg_u8(old: u8, expected: u8, value: u8) -> u8 {
    if old == expected { value } else { old }
}

pub closed spec fn spec_atomic_cmpxchg_u16(old: u16, expected: u16, value: u16) -> u16 {
    if old == expected { value } else { old }
}

pub closed spec fn spec_atomic_cmpxchg_u32(old: u32, expected: u32, value: u32) -> u32 {
    if old == expected { value } else { old }
}

pub closed spec fn spec_atomic_cmpxchg_u64(old: u64, expected: u64, value: u64) -> u64 {
    if old == expected { value } else { old }
}

pub open spec fn spec_seq_concat(left: Seq<u8>, right: Seq<u8>) -> Seq<u8> {
    Seq::new(
        left.len() + right.len(),
        |i: int| if i < left.len() { left[i] } else { right[i - left.len()] },
    )
}

pub open spec fn stack_push_bytes(view: StackView, bytes: Seq<u8>) -> StackView
    recommends
        view.top == view.bytes.len(),
{
    let new_bytes = spec_seq_concat(view.bytes, bytes);
    StackView {
        bytes: new_bytes,
        top: view.top + bytes.len(),
        frame_stack: view.frame_stack,
        active_local: view.active_local,
    }
}

pub open spec fn stack_pop_bytes(view: StackView, len: nat) -> StackView
    recommends
        len <= view.top,
        view.top == view.bytes.len(),
{
    let new_top = (view.top - len) as nat;
    StackView {
        bytes: view.bytes.subrange(0, new_top as int),
        top: new_top,
        frame_stack: view.frame_stack,
        active_local: view.active_local,
    }
}

pub open spec fn stack_enter_frame(
    view: StackView,
    frame: FrameView,
    local_top: nat,
    local_size: nat,
) -> StackView
    recommends
        view.top == view.bytes.len(),
        local_top <= view.top,
{
    StackView {
        bytes: view.bytes,
        top: view.top,
        frame_stack: view.frame_stack.push(frame),
        active_local: LocalRefView { local_top, local_size },
    }
}

pub open spec fn stack_return_to_caller(
    view: StackView,
    caller: LocalRefView,
    caller_frame: Option<FrameView>,
    new_top: nat,
) -> StackView
    recommends
        new_top <= view.top,
        view.frame_stack.len() > 0,
{
    let frame_stack = if view.frame_stack.len() > 1 {
        view.frame_stack.subrange(0, (view.frame_stack.len() - 1) as int)
    } else {
        Seq::empty()
    };
    let frame_stack = match caller_frame {
        Some(frame) if frame_stack.len() == 0 => seq![frame],
        _ => frame_stack,
    };
    StackView {
        bytes: view.bytes.subrange(0, new_top as int),
        top: new_top,
        frame_stack,
        active_local: caller,
    }
}

pub open spec fn stack_tail_call(
    view: StackView,
    callee: FrameView,
    param_size: nat,
    local_size: nat,
) -> StackView
    recommends
        param_size <= view.top,
        view.frame_stack.len() > 0,
{
    let preserved = view.bytes.subrange((view.top - param_size) as int, view.top as int);
    let prefix = view.bytes.subrange(0, view.active_local.local_top as int);
    let new_bytes = spec_seq_concat(prefix, preserved);
    let new_top = view.active_local.local_top + param_size;
    let new_frame_stack =
        view.frame_stack.subrange(0, (view.frame_stack.len() - 1) as int).push(callee);
    StackView {
        bytes: new_bytes,
        top: new_top + local_size,
        frame_stack: new_frame_stack,
        active_local: LocalRefView {
            local_top: view.active_local.local_top,
            local_size: param_size + local_size,
        },
    }
}

pub open spec fn stack_function_call(
    view: StackView,
    frame: FrameView,
    param_size: nat,
    local_size: nat,
    frame_metadata: Seq<u8>,
) -> StackView
    recommends
        param_size <= view.top,
        view.top == view.bytes.len(),
{
    let local_top = (view.top - param_size) as nat;
    let new_bytes = spec_seq_concat(
        view.bytes,
        spec_seq_concat(zero_bytes(local_size), frame_metadata),
    );
    StackView {
        bytes: new_bytes,
        top: view.top + local_size + frame_metadata.len(),
        frame_stack: view.frame_stack.push(frame),
        active_local: LocalRefView {
            local_top,
            local_size: param_size + local_size + frame_metadata.len(),
        },
    }
}

pub open spec fn stack_function_return(
    view: StackView,
    caller: LocalRefView,
    return_size: nat,
) -> StackView
    recommends
        return_size <= view.top,
        view.top == view.bytes.len(),
        return_size <= view.active_local.local_size,
        view.frame_stack.len() > 0,
{
    let result_bytes = view.bytes.subrange((view.top - return_size) as int, view.top as int);
    let new_bytes = spec_seq_concat(
        view.bytes.subrange(0, view.active_local.local_top as int),
        result_bytes,
    );
    StackView {
        bytes: new_bytes,
        top: view.active_local.local_top + return_size,
        frame_stack: view.frame_stack.subrange(0, (view.frame_stack.len() - 1) as int),
        active_local: caller,
    }
}

pub open spec fn stack_function_return_in_place(
    view: StackView,
    caller: LocalRefView,
    return_size: nat,
) -> StackView
    recommends
        view.top == view.bytes.len(),
        return_size <= view.active_local.local_size,
        view.frame_stack.len() > 0,
{
    let new_top = view.active_local.local_top + return_size;
    StackView {
        bytes: view.bytes.subrange(0, new_top as int),
        top: new_top,
        frame_stack: view.frame_stack.subrange(0, (view.frame_stack.len() - 1) as int),
        active_local: caller,
    }
}

pub open spec fn stack_function_return_call(
    view: StackView,
    callee: FrameView,
    param_size: nat,
    local_size: nat,
    frame_metadata: Seq<u8>,
) -> StackView
    recommends
        param_size <= view.top,
        view.top == view.bytes.len(),
        view.frame_stack.len() > 0,
{
    let local_top = view.active_local.local_top;
    let preserved = view.bytes.subrange((view.top - param_size) as int, view.top as int);
    let new_bytes = spec_seq_concat(
        view.bytes.subrange(0, local_top as int),
        spec_seq_concat(
            preserved,
            spec_seq_concat(zero_bytes(local_size), frame_metadata),
        ),
    );
    let new_frame_stack =
        view.frame_stack.subrange(0, (view.frame_stack.len() - 1) as int).push(callee);
    StackView {
        bytes: new_bytes,
        top: local_top + param_size + local_size + frame_metadata.len(),
        frame_stack: new_frame_stack,
        active_local: LocalRefView {
            local_top,
            local_size: param_size + local_size + frame_metadata.len(),
        },
    }
}

pub open spec fn stack_block_return(
    view: StackView,
    block_stack_top: nat,
    return_size: nat,
) -> StackView
    recommends
        return_size <= view.top,
        view.top == view.bytes.len(),
{
    let dst = view.active_local.local_top + view.active_local.local_size + block_stack_top;
    let prefix = view.bytes.subrange(0, dst as int);
    let result_bytes = view.bytes.subrange((view.top - return_size) as int, view.top as int);
    let new_bytes = spec_seq_concat(prefix, result_bytes);
    StackView {
        bytes: new_bytes,
        top: dst + return_size,
        frame_stack: view.frame_stack,
        active_local: view.active_local,
    }
}

pub open spec fn linear_write_bytes(
    view: LinearMemoryView,
    start: int,
    bytes: Seq<u8>,
) -> LinearMemoryView {
    LinearMemoryView {
        bytes: spec_write_range(view.bytes, start, bytes),
        current_pages: view.current_pages,
        max_pages: view.max_pages,
        shared: view.shared,
    }
}

pub open spec fn linear_fill_bytes(
    view: LinearMemoryView,
    start: int,
    len: int,
    value: u8,
) -> LinearMemoryView {
    LinearMemoryView {
        bytes: spec_fill_range(view.bytes, start, len, value),
        current_pages: view.current_pages,
        max_pages: view.max_pages,
        shared: view.shared,
    }
}

pub open spec fn linear_copy_bytes(
    view: LinearMemoryView,
    dst: int,
    src: int,
    len: int,
) -> LinearMemoryView {
    LinearMemoryView {
        bytes: spec_copy_within_range(view.bytes, dst, src, len),
        current_pages: view.current_pages,
        max_pages: view.max_pages,
        shared: view.shared,
    }
}

pub open spec fn linear_grow(
    view: LinearMemoryView,
    page_delta: nat,
    zeroed: Seq<u8>,
) -> LinearMemoryView
    recommends
        zeroed.len() == page_delta * 65536,
        forall|i: int| 0 <= i < zeroed.len() ==> zeroed[i] == 0u8,
{
    LinearMemoryView {
        bytes: spec_seq_concat(view.bytes, zeroed),
        current_pages: view.current_pages + page_delta,
        max_pages: view.max_pages,
        shared: view.shared,
    }
}

pub open spec fn linear_atomic_cmpxchg_u8(
    view: LinearMemoryView,
    start: int,
    expected: u8,
    value: u8,
) -> LinearMemoryView
    recommends
        0 <= start,
        start + 1 <= view.bytes.len(),
{
    let old = view.bytes[start];
    linear_write_bytes(view, start, seq![spec_atomic_cmpxchg_u8(old, expected, value)])
}

pub open spec fn linear_atomic_cmpxchg_u16(
    view: LinearMemoryView,
    start: int,
    old_bytes: Seq<u8>,
    expected: u16,
    value_bytes: Seq<u8>,
) -> LinearMemoryView
    recommends
        old_bytes.len() == 2,
        value_bytes.len() == 2,
        0 <= start,
        start + 2 <= view.bytes.len(),
{
    linear_write_bytes(
        view,
        start,
        if spec_atomic_cmpxchg_u16(spec_le_u16(old_bytes), expected, spec_le_u16(value_bytes))
            == spec_le_u16(value_bytes)
        {
            value_bytes
        } else {
            old_bytes
        },
    )
}

pub open spec fn linear_atomic_cmpxchg_u32(
    view: LinearMemoryView,
    start: int,
    old_bytes: Seq<u8>,
    expected: u32,
    value_bytes: Seq<u8>,
) -> LinearMemoryView
    recommends
        old_bytes.len() == 4,
        value_bytes.len() == 4,
        0 <= start,
        start + 4 <= view.bytes.len(),
{
    linear_write_bytes(
        view,
        start,
        if spec_atomic_cmpxchg_u32(spec_le_u32(old_bytes), expected, spec_le_u32(value_bytes))
            == spec_le_u32(value_bytes)
        {
            value_bytes
        } else {
            old_bytes
        },
    )
}

pub open spec fn linear_atomic_cmpxchg_u64(
    view: LinearMemoryView,
    start: int,
    old_bytes: Seq<u8>,
    expected: u64,
    value_bytes: Seq<u8>,
) -> LinearMemoryView
    recommends
        old_bytes.len() == 8,
        value_bytes.len() == 8,
        0 <= start,
        start + 8 <= view.bytes.len(),
{
    linear_write_bytes(
        view,
        start,
        if spec_atomic_cmpxchg_u64(spec_le_u64(old_bytes), expected, spec_le_u64(value_bytes))
            == spec_le_u64(value_bytes)
        {
            value_bytes
        } else {
            old_bytes
        },
    )
}

pub open spec fn shared_wait_queue(
    protocol: SharedMemoryProtocol,
    address: Address,
) -> Seq<WaiterId> {
    if protocol.wait_queues.dom().contains(address) {
        protocol.wait_queues[address]
    } else {
        Seq::empty()
    }
}

pub open spec fn shared_queue_contains(queue: Seq<WaiterId>, waiter_id: WaiterId) -> bool {
    exists|i: int| 0 <= i < queue.len() && queue[i] == waiter_id
}

pub open spec fn shared_queue_update(
    wait_queues: Map<Address, Seq<WaiterId>>,
    address: Address,
    queue: Seq<WaiterId>,
) -> Map<Address, Seq<WaiterId>> {
    if queue.len() == 0 {
        wait_queues.remove(address)
    } else {
        wait_queues.insert(address, queue)
    }
}

pub open spec fn shared_queue_remove(queue: Seq<WaiterId>, waiter_id: WaiterId) -> Seq<WaiterId>
    decreases queue.len(),
{
    if queue.len() == 0 {
        queue
    } else if queue[0] == waiter_id {
        queue.subrange(1, queue.len() as int)
    } else {
        seq![queue[0]].add(shared_queue_remove(
            queue.subrange(1, queue.len() as int),
            waiter_id,
        ))
    }
}

pub closed spec fn shared_notify_count(queue_len: nat, count: u32) -> nat {
    if queue_len < count as nat {
        queue_len
    } else {
        count as nat
    }
}

pub open spec fn shared_notify_prefix(queue: Seq<WaiterId>, count: u32) -> Seq<WaiterId>
    recommends
        shared_notify_count(queue.len() as nat, count) <= queue.len(),
{
    queue.subrange(0, shared_notify_count(queue.len() as nat, count) as int)
}

pub open spec fn shared_notify_suffix(queue: Seq<WaiterId>, count: u32) -> Seq<WaiterId>
    recommends
        shared_notify_count(queue.len() as nat, count) <= queue.len(),
{
    queue.subrange(
        shared_notify_count(queue.len() as nat, count) as int,
        queue.len() as int,
    )
}

pub open spec fn shared_waiters_with_state(
    waiters: Map<WaiterId, WaitState>,
    queue: Seq<WaiterId>,
    state: WaitState,
) -> Map<WaiterId, WaitState> {
    Map::new(
        |waiter_id: WaiterId| waiters.dom().contains(waiter_id),
        |waiter_id: WaiterId| {
            if shared_queue_contains(queue, waiter_id) {
                state
            } else {
                waiters[waiter_id]
            }
        },
    )
}

pub open spec fn shared_wake_tokens(queue: Seq<WaiterId>, address: Address) -> Seq<WakeTok> {
    Seq::new(
        queue.len(),
        |i: int| WakeTok {
            waiter_id: queue[i],
            address,
        },
    )
}

pub open spec fn shared_register_wait(
    protocol: SharedMemoryProtocol,
    address: Address,
) -> (SharedMemoryProtocol, WaitTicketTok) {
    let waiter_id = protocol.next_waiter_id;
    let queue = shared_wait_queue(protocol, address);
    (
        SharedMemoryProtocol {
            memory: protocol.memory,
            wait_queues: protocol.wait_queues.insert(address, queue.push(waiter_id)),
            waiters: protocol.waiters.insert(waiter_id, WaitState::Waiting),
            next_waiter_id: waiter_id + 1,
        },
        WaitTicketTok { waiter_id, address },
    )
}

pub open spec fn shared_timeout_wait(
    protocol: SharedMemoryProtocol,
    ticket: WaitTicketTok,
) -> SharedMemoryProtocol {
    let queue = shared_wait_queue(protocol, ticket.address);
    SharedMemoryProtocol {
        memory: protocol.memory,
        wait_queues: shared_queue_update(
            protocol.wait_queues,
            ticket.address,
            shared_queue_remove(queue, ticket.waiter_id),
        ),
        waiters: protocol.waiters.insert(ticket.waiter_id, WaitState::TimedOut),
        next_waiter_id: protocol.next_waiter_id,
    }
}

pub open spec fn shared_notify_wait(
    protocol: SharedMemoryProtocol,
    ticket: WaitTicketTok,
) -> (SharedMemoryProtocol, WakeTok) {
    let queue = shared_wait_queue(protocol, ticket.address);
    (
        SharedMemoryProtocol {
            memory: protocol.memory,
            wait_queues: shared_queue_update(
                protocol.wait_queues,
                ticket.address,
                shared_queue_remove(queue, ticket.waiter_id),
            ),
            waiters: protocol.waiters.insert(ticket.waiter_id, WaitState::Notified),
            next_waiter_id: protocol.next_waiter_id,
        },
        WakeTok {
            waiter_id: ticket.waiter_id,
            address: ticket.address,
        },
    )
}

pub open spec fn shared_notify_waiters(
    protocol: SharedMemoryProtocol,
    address: Address,
    count: u32,
) -> (SharedMemoryProtocol, Seq<WakeTok>) {
    let queue = shared_wait_queue(protocol, address);
    let notified = shared_notify_prefix(queue, count);
    (
        SharedMemoryProtocol {
            memory: protocol.memory,
            wait_queues: shared_queue_update(
                protocol.wait_queues,
                address,
                shared_notify_suffix(queue, count),
            ),
            waiters: shared_waiters_with_state(protocol.waiters, notified, WaitState::Notified),
            next_waiter_id: protocol.next_waiter_id,
        },
        shared_wake_tokens(notified, address),
    )
}

pub closed spec fn wait_result_not_equal_code() -> int {
    1
}

pub closed spec fn wait_result_ok_code() -> int {
    0
}

pub closed spec fn wait_result_timed_out_code() -> int {
    2
}

pub open spec fn exec_context_token(
    current_frame: FrameView,
    caller_frame: Option<FrameView>,
    cont_addr: nat,
    task_id: nat,
) -> ExecContextToken {
    ExecContextToken {
        current_frame,
        caller_frame,
        cont_addr,
        task_id,
    }
}

pub proof fn lemma_stack_push_updates_top(view: StackView, bytes: Seq<u8>)
    requires
        view.top == view.bytes.len(),
    ensures
        stack_push_bytes(view, bytes).top == view.top + bytes.len(),
{
}

pub proof fn lemma_linear_write_preserves_page_metadata(
    view: LinearMemoryView,
    start: int,
    bytes: Seq<u8>,
)
    ensures
        linear_write_bytes(view, start, bytes).current_pages == view.current_pages,
        linear_write_bytes(view, start, bytes).max_pages == view.max_pages,
        linear_write_bytes(view, start, bytes).shared == view.shared,
{
}

pub proof fn lemma_stack_function_call_updates_top(
    view: StackView,
    frame: FrameView,
    param_size: nat,
    local_size: nat,
    frame_metadata: Seq<u8>,
)
    requires
        param_size <= view.top,
        view.top == view.bytes.len(),
    ensures
        stack_function_call(view, frame, param_size, local_size, frame_metadata).top
            == view.top + local_size + frame_metadata.len(),
{
}

pub proof fn lemma_stack_function_return_restores_top(
    view: StackView,
    caller: LocalRefView,
    return_size: nat,
)
    requires
        return_size <= view.top,
        view.top == view.bytes.len(),
        return_size <= view.active_local.local_size,
        view.frame_stack.len() > 0,
    ensures
        stack_function_return(view, caller, return_size).top
            == view.active_local.local_top + return_size,
{
}

pub proof fn lemma_stack_block_return_updates_top(
    view: StackView,
    block_stack_top: nat,
    return_size: nat,
)
    requires
        return_size <= view.top,
        view.top == view.bytes.len(),
    ensures
        stack_block_return(view, block_stack_top, return_size).top
            == view.active_local.local_top + view.active_local.local_size + block_stack_top
                + return_size,
{
}

pub proof fn lemma_shared_register_wait_advances_ticket(
    protocol: SharedMemoryProtocol,
    address: Address,
)
    ensures
        shared_register_wait(protocol, address).0.next_waiter_id == protocol.next_waiter_id + 1,
        shared_register_wait(protocol, address).1.waiter_id == protocol.next_waiter_id,
        shared_register_wait(protocol, address).1.address == address,
{
}

} // verus!

#[cfg(verus_keep_ghost)]
use verus_state_machines_macros::tokenized_state_machine;

#[cfg(verus_keep_ghost)]
tokenized_state_machine!(SharedMemoryProtocolToks {
    fields {
        #[sharding(variable)]
        pub bytes: Seq<u8>,

        #[sharding(variable)]
        pub pages: nat,

        #[sharding(constant)]
        pub max_pages: nat,

        #[sharding(constant)]
        pub shared: bool,

        #[sharding(variable)]
        pub wait_queues: Map<Address, Seq<WaiterId>>,

        #[sharding(variable)]
        pub waiters: Map<WaiterId, WaitState>,

        #[sharding(variable)]
        pub next_waiter_id: WaiterId,

        #[sharding(option)]
        pub shared_mem_tok: Option<()>,

        #[sharding(multiset)]
        pub wait_ticket_tok: Multiset<WaitTicketTok>,

        #[sharding(multiset)]
        pub wake_tok: Multiset<WakeTok>,
    }

    init!{
        initialize(memory: LinearMemoryView) {
            init bytes = memory.bytes;
            init pages = memory.current_pages;
            init max_pages = memory.max_pages;
            init shared = memory.shared;
            init wait_queues = Map::empty();
            init waiters = Map::empty();
            init next_waiter_id = 0;
            init shared_mem_tok = Some(());
            init wait_ticket_tok = Multiset::empty();
            init wake_tok = Multiset::empty();
        }
    }

    transition!{
        register_wait(address: Address) {
            let waiter_id = pre.next_waiter_id;
            let queue = if pre.wait_queues.dom().contains(address) {
                pre.wait_queues[address]
            } else {
                Seq::empty()
            };
            let ticket = WaitTicketTok { waiter_id, address };

            update wait_queues = pre.wait_queues.insert(address, queue.push(waiter_id));
            update waiters = pre.waiters.insert(waiter_id, WaitState::Waiting);
            update next_waiter_id = waiter_id + 1;
            add wait_ticket_tok += { ticket };
        }
    }

    transition!{
        timeout_wait(ticket: WaitTicketTok) {
            require(pre.waiters.dom().contains(ticket.waiter_id));
            require(pre.waiters[ticket.waiter_id] == WaitState::Waiting);

            let queue = if pre.wait_queues.dom().contains(ticket.address) {
                pre.wait_queues[ticket.address]
            } else {
                Seq::empty()
            };

            update wait_queues = shared_queue_update(
                pre.wait_queues,
                ticket.address,
                shared_queue_remove(queue, ticket.waiter_id),
            );
            update waiters = pre.waiters.insert(ticket.waiter_id, WaitState::TimedOut);
        }
    }

    transition!{
        notify_wait(ticket: WaitTicketTok) {
            require(pre.waiters.dom().contains(ticket.waiter_id));
            require(pre.waiters[ticket.waiter_id] == WaitState::Waiting);

            let queue = if pre.wait_queues.dom().contains(ticket.address) {
                pre.wait_queues[ticket.address]
            } else {
                Seq::empty()
            };
            let wake = WakeTok {
                waiter_id: ticket.waiter_id,
                address: ticket.address,
            };

            update wait_queues = shared_queue_update(
                pre.wait_queues,
                ticket.address,
                shared_queue_remove(queue, ticket.waiter_id),
            );
            update waiters = pre.waiters.insert(ticket.waiter_id, WaitState::Notified);
            remove wait_ticket_tok -= { ticket };
            add wake_tok += { wake };
        }
    }

    transition!{
        consume_timed_out(ticket: WaitTicketTok) {
            require(pre.waiters.dom().contains(ticket.waiter_id));
            require(pre.waiters[ticket.waiter_id] == WaitState::TimedOut);
            remove wait_ticket_tok -= { ticket };
        }
    }

    transition!{
        consume_wake(wake: WakeTok) {
            remove wake_tok -= { wake };
        }
    }

    transition!{
        atomic_store(start: int, value_bytes: Seq<u8>) {
            require(0 <= start);
            require(start + value_bytes.len() <= pre.bytes.len());
            update bytes = spec_write_range(pre.bytes, start, value_bytes);
        }
    }

    transition!{
        atomic_rmw_u32(start: int, value_bytes: Seq<u8>) {
            require(0 <= start);
            require(value_bytes.len() == 4);
            require(start + 4 <= pre.bytes.len());
            update bytes = spec_write_range(pre.bytes, start, value_bytes);
        }
    }

    transition!{
        atomic_cmpxchg_u32(start: int, expected: u32, value_bytes: Seq<u8>) {
            require(0 <= start);
            require(value_bytes.len() == 4);
            require(start + 4 <= pre.bytes.len());

            let old_bytes = pre.bytes.subrange(start, start + 4);
            update bytes =
                if spec_atomic_cmpxchg_u32(
                    spec_le_u32(old_bytes),
                    expected,
                    spec_le_u32(value_bytes),
                ) == spec_le_u32(value_bytes) {
                    spec_write_range(pre.bytes, start, value_bytes)
                } else {
                    pre.bytes
                };
        }
    }
});
