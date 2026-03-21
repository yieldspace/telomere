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

pub struct TableView {
    pub entries: Seq<u32>,
    pub max_len: Option<nat>,
}

pub struct GlobalView {
    pub bytes: Seq<u8>,
}

pub struct RefView {
    pub raw: u32,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrapCode {
    MemoryIndexOutOfRange,
    TableIndexOutOfRange,
    TableUninitialized,
    CallIndirectInvalidType,
    UnalignedAtomic,
    InvalidOperand,
    Unreachable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PendingCode {
    Wait,
    HostCall,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CoreOutcome {
    Continue,
    Trap(TrapCode),
    Pending(PendingCode),
}

pub type TableId = nat;
pub type GlobalId = nat;
pub type FunctionId = nat;
pub type SegmentId = nat;
pub type TypeId = nat;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemorySelector {
    CurrentDefault,
    Explicit(MemoryHandleView),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Wasm,
    Host,
}

pub struct FunctionView {
    pub instance: nat,
    pub entry_pc: nat,
    pub param_size: nat,
    pub local_size: nat,
    pub type_id: TypeId,
    pub default_memory: Option<MemoryHandleView>,
    pub kind: FunctionKind,
}

pub struct CoreStepState {
    pub stack: StackView,
    pub context: ExecContextToken,
    pub tables: Map<TableId, TableView>,
    pub globals: Map<GlobalId, GlobalView>,
    pub functions: Map<FunctionId, FunctionView>,
    pub local_memories: Map<MemoryId, LinearMemoryView>,
    pub shared_memories: Map<MemoryId, SharedMemoryProtocol>,
    pub data_segments: Map<SegmentId, Seq<u8>>,
    pub elem_segments: Map<SegmentId, Seq<u32>>,
    pub frame_metadata_len: nat,
}

pub enum NumericStep {
    PushConst { bytes: Seq<u8>, next_cont: nat },
    ReplaceTop {
        pop_len: nat,
        result_bytes: Seq<u8>,
        next_cont: nat,
    },
}

pub enum ControlStep {
    SetCont { cont_addr: nat },
    ConditionalCont { taken: bool, true_addr: nat, false_addr: nat },
    BlockReturn {
        block_stack_top: nat,
        return_size: nat,
        cont_addr: nat,
    },
    FunctionReturn { return_size: nat },
    Trap { code: TrapCode },
}

pub enum CallStep {
    Call {
        function_id: FunctionId,
        return_addr: nat,
        is_return_call: bool,
    },
    CallIndirect {
        table_id: TableId,
        elem_index: nat,
        expected_type_id: TypeId,
        return_addr: nat,
        is_return_call: bool,
    },
}

pub enum LocalStep {
    Drop { size: nat, next_cont: nat },
    Select {
        size: nat,
        cond: u32,
        next_cont: nat,
    },
    Get {
        local_addr: nat,
        size: nat,
        next_cont: nat,
    },
    Set {
        local_addr: nat,
        size: nat,
        next_cont: nat,
    },
    Tee {
        local_addr: nat,
        size: nat,
        next_cont: nat,
    },
}

pub enum GlobalStep {
    Get {
        global_id: GlobalId,
        next_cont: nat,
    },
    Set {
        global_id: GlobalId,
        next_cont: nat,
    },
}

pub enum TableStep {
    Get {
        table_id: TableId,
        index: nat,
        next_cont: nat,
    },
    Set {
        table_id: TableId,
        index: nat,
        value: u32,
        next_cont: nat,
    },
    Size {
        table_id: TableId,
        next_cont: nat,
    },
    Grow {
        table_id: TableId,
        len: nat,
        value: u32,
        next_cont: nat,
    },
    Fill {
        table_id: TableId,
        index: nat,
        len: nat,
        value: u32,
        next_cont: nat,
    },
    Copy {
        dst_table_id: TableId,
        src_table_id: TableId,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    Init {
        table_id: TableId,
        elem_segment_id: SegmentId,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    ElemDrop {
        elem_segment_id: SegmentId,
        next_cont: nat,
    },
}

pub enum RefStep {
    Null { next_cont: nat },
    IsNull { next_cont: nat },
    Func {
        function_id: FunctionId,
        next_cont: nat,
    },
}

pub enum MemoryLoadKind {
    Raw { load_width: nat },
    ZeroExtend { load_width: nat, result_width: nat },
    SignExtend { load_width: nat, result_width: nat },
}

pub enum MemoryStep {
    Load {
        selector: MemorySelector,
        start: nat,
        kind: MemoryLoadKind,
        next_cont: nat,
    },
    Store {
        selector: MemorySelector,
        start: nat,
        len: nat,
        next_cont: nat,
    },
    Size {
        selector: MemorySelector,
        next_cont: nat,
    },
    Grow {
        selector: MemorySelector,
        page_delta: nat,
        next_cont: nat,
    },
}

pub enum AtomicWaitKind {
    I32(u32),
    I64(u64),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AtomicCmpxchgExpected {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

#[allow(inconsistent_fields)]
pub enum AtomicStep {
    Notify {
        selector: MemorySelector,
        start: nat,
        count: u32,
        aligned: bool,
        next_cont: nat,
    },
    Wait {
        selector: MemorySelector,
        start: nat,
        expected: AtomicWaitKind,
        timeout_immediate: bool,
        aligned: bool,
        next_cont: nat,
    },
    Store {
        selector: MemorySelector,
        start: nat,
        bytes: Seq<u8>,
        aligned: bool,
        next_cont: nat,
    },
    Rmw {
        selector: MemorySelector,
        start: nat,
        result_bytes: Seq<u8>,
        write_bytes: Seq<u8>,
        aligned: bool,
        next_cont: nat,
    },
    Cmpxchg {
        selector: MemorySelector,
        start: nat,
        expected: AtomicCmpxchgExpected,
        value_bytes: Seq<u8>,
        aligned: bool,
        next_cont: nat,
    },
}

pub enum BulkMemoryStep {
    Init {
        selector: MemorySelector,
        data_segment_id: SegmentId,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    Copy {
        dst_selector: MemorySelector,
        src_selector: MemorySelector,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    Fill {
        selector: MemorySelector,
        start: nat,
        len: nat,
        value: u8,
        next_cont: nat,
    },
    DataDrop {
        data_segment_id: SegmentId,
        next_cont: nat,
    },
}

#[cfg(feature = "simd")]
pub enum SimdStep {
    ReplaceTop {
        pop_len: nat,
        result_bytes: Seq<u8>,
        next_cont: nat,
    },
    Load {
        selector: MemorySelector,
        start: nat,
        access_width: nat,
        result_bytes: Seq<u8>,
        next_cont: nat,
    },
    Store {
        selector: MemorySelector,
        start: nat,
        len: nat,
        next_cont: nat,
    },
}

pub enum CoreStepInstr {
    Numeric(NumericStep),
    Control(ControlStep),
    Call(CallStep),
    Local(LocalStep),
    Global(GlobalStep),
    Table(TableStep),
    Ref(RefStep),
    Memory(MemoryStep),
    Atomic(AtomicStep),
    BulkMemory(BulkMemoryStep),
    #[cfg(feature = "simd")]
    Simd(SimdStep),
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

pub open spec fn frame_view_from_parts(
    return_pc: int,
    instance: nat,
    default_memory_present: bool,
    default_memory_shared: bool,
    default_memory_raw: nat,
    prev_local_top: nat,
    prev_local_size: nat,
) -> FrameView {
    FrameView {
        return_pc,
        instance,
        default_memory: optional_memory_handle_view_from_raw(
            default_memory_present,
            default_memory_shared,
            default_memory_raw,
        ),
        prev_local: LocalRefView {
            local_top: prev_local_top,
            local_size: prev_local_size,
        },
    }
}

pub open spec fn stack_view_from_parts(
    bytes: Seq<u8>,
    top: nat,
    frame_stack: Seq<FrameView>,
    active_local_top: nat,
    active_local_size: nat,
) -> StackView {
    StackView {
        bytes,
        top,
        frame_stack,
        active_local: LocalRefView {
            local_top: active_local_top,
            local_size: active_local_size,
        },
    }
}

pub open spec fn linear_memory_view_from_parts(
    bytes: Seq<u8>,
    current_pages: nat,
    max_pages: nat,
    shared: bool,
) -> LinearMemoryView {
    LinearMemoryView {
        bytes,
        current_pages,
        max_pages,
        shared,
    }
}

pub open spec fn global_view_from_bytes(bytes: Seq<u8>) -> GlobalView {
    GlobalView { bytes }
}

pub open spec fn table_view_from_elements(elements: Seq<u32>) -> TableView {
    table_view_from_parts(elements, None)
}

pub open spec fn table_view_from_parts(elements: Seq<u32>, max_len: Option<nat>) -> TableView {
    TableView {
        entries: elements,
        max_len,
    }
}

pub open spec fn core_continue() -> CoreOutcome {
    CoreOutcome::Continue
}

pub open spec fn core_trap(code: TrapCode) -> CoreOutcome {
    CoreOutcome::Trap(code)
}

pub open spec fn core_pending(code: PendingCode) -> CoreOutcome {
    CoreOutcome::Pending(code)
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

pub closed spec fn spec_u32_to_le_bytes(value: u32) -> Seq<u8> {
    seq![
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
    ]
}

pub closed spec fn spec_u16_to_le_bytes(value: u16) -> Seq<u8> {
    seq![(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

pub closed spec fn spec_u64_to_le_bytes(value: u64) -> Seq<u8> {
    seq![
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
        ((value >> 32) & 0xff) as u8,
        ((value >> 40) & 0xff) as u8,
        ((value >> 48) & 0xff) as u8,
        ((value >> 56) & 0xff) as u8,
    ]
}

pub closed spec fn spec_u128_to_le_bytes(value: u128) -> Seq<u8> {
    seq![
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
        ((value >> 32) & 0xff) as u8,
        ((value >> 40) & 0xff) as u8,
        ((value >> 48) & 0xff) as u8,
        ((value >> 56) & 0xff) as u8,
        ((value >> 64) & 0xff) as u8,
        ((value >> 72) & 0xff) as u8,
        ((value >> 80) & 0xff) as u8,
        ((value >> 88) & 0xff) as u8,
        ((value >> 96) & 0xff) as u8,
        ((value >> 104) & 0xff) as u8,
        ((value >> 112) & 0xff) as u8,
        ((value >> 120) & 0xff) as u8,
    ]
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

pub open spec fn spec_repeat_byte(len: nat, value: u8) -> Seq<u8> {
    Seq::new(len, |i: int| value)
}

pub open spec fn spec_sign_fill_byte(bytes: Seq<u8>) -> u8
    recommends
        bytes.len() > 0,
{
    if bytes[bytes.len() - 1] & 0x80 == 0 {
        0u8
    } else {
        0xffu8
    }
}

pub open spec fn spec_zero_extend_bytes(bytes: Seq<u8>, result_width: nat) -> Seq<u8>
    recommends
        bytes.len() <= result_width,
{
    spec_seq_concat(bytes, zero_bytes((result_width as int - bytes.len()) as nat))
}

pub open spec fn spec_sign_extend_bytes(bytes: Seq<u8>, result_width: nat) -> Seq<u8>
    recommends
        bytes.len() > 0,
        bytes.len() <= result_width,
{
    spec_seq_concat(
        bytes,
        spec_repeat_byte(
            (result_width as int - bytes.len()) as nat,
            spec_sign_fill_byte(bytes),
        ),
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

pub open spec fn stack_drop_values(view: StackView, len: nat) -> StackView
    recommends
        len <= view.top,
        view.top == view.bytes.len(),
{
    stack_pop_bytes(view, len)
}

pub open spec fn stack_const_bytes(view: StackView, bytes: Seq<u8>) -> StackView
    recommends
        view.top == view.bytes.len(),
{
    stack_push_bytes(view, bytes)
}

pub open spec fn stack_unary_result(
    view: StackView,
    operand_width: nat,
    result_bytes: Seq<u8>,
) -> StackView
    recommends
        operand_width <= view.top,
        view.top == view.bytes.len(),
{
    stack_push_bytes(stack_pop_bytes(view, operand_width), result_bytes)
}

pub open spec fn stack_binary_result(
    view: StackView,
    operand_width: nat,
    result_bytes: Seq<u8>,
) -> StackView
    recommends
        operand_width * 2 <= view.top,
        view.top == view.bytes.len(),
{
    stack_push_bytes(stack_pop_bytes(view, operand_width * 2), result_bytes)
}

pub open spec fn stack_compare_result(
    view: StackView,
    operand_width: nat,
    result: u32,
) -> StackView
    recommends
        operand_width * 2 <= view.top,
        view.top == view.bytes.len(),
{
    stack_binary_result(view, operand_width, spec_u32_to_le_bytes(result))
}

pub open spec fn stack_select_bytes(
    view: StackView,
    value_width: nat,
    cond: u32,
) -> StackView
    recommends
        value_width * 2 + 4 <= view.top,
        view.top == view.bytes.len(),
{
    let cond_top = view.top - 4;
    let rhs_start = cond_top - value_width;
    let lhs_start = rhs_start - value_width;
    let chosen =
        if cond == 0 {
            view.bytes.subrange(rhs_start as int, cond_top as int)
        } else {
            view.bytes.subrange(lhs_start as int, rhs_start as int)
        };
    StackView {
        bytes: spec_seq_concat(view.bytes.subrange(0, lhs_start as int), chosen),
        top: (lhs_start + value_width) as nat,
        frame_stack: view.frame_stack,
        active_local: view.active_local,
    }
}

pub open spec fn stack_top_bytes(view: StackView, len: nat) -> Option<Seq<u8>> {
    if len <= view.top {
        Some(view.bytes.subrange((view.top - len) as int, view.top as int))
    } else {
        None
    }
}

pub open spec fn stack_local_bytes(view: StackView, local_addr: nat, size: nat) -> Option<Seq<u8>> {
    let start = view.active_local.local_top + local_addr;
    let end = start + size;
    if end <= view.active_local.local_top + view.active_local.local_size
        && end <= view.bytes.len() as nat
    {
        Some(view.bytes.subrange(start as int, end as int))
    } else {
        None
    }
}

pub open spec fn stack_local_get(
    view: StackView,
    local_addr: nat,
    size: nat,
) -> Option<StackView>
    recommends
        view.top == view.bytes.len(),
{
    match stack_local_bytes(view, local_addr, size) {
        Some(bytes) => Some(stack_push_bytes(view, bytes)),
        None => None,
    }
}

pub open spec fn stack_local_set(
    view: StackView,
    local_addr: nat,
    size: nat,
) -> Option<StackView>
    recommends
        view.top == view.bytes.len(),
{
    let start = view.active_local.local_top + local_addr;
    let end = start + size;
    match stack_top_bytes(view, size) {
        Some(bytes) if end <= view.active_local.local_top + view.active_local.local_size => {
            let replaced = spec_write_range(view.bytes, start as int, bytes);
            let new_top = (view.top - size) as nat;
            Some(StackView {
                bytes: replaced.subrange(0, new_top as int),
                top: new_top,
                frame_stack: view.frame_stack,
                active_local: view.active_local,
            })
        }
        _ => None,
    }
}

pub open spec fn stack_local_tee(
    view: StackView,
    local_addr: nat,
    size: nat,
) -> Option<StackView>
    recommends
        view.top == view.bytes.len(),
{
    let start = view.active_local.local_top + local_addr;
    let end = start + size;
    match stack_top_bytes(view, size) {
        Some(bytes) if end <= view.active_local.local_top + view.active_local.local_size => {
            Some(StackView {
                bytes: spec_write_range(view.bytes, start as int, bytes),
                top: view.top,
                frame_stack: view.frame_stack,
                active_local: view.active_local,
            })
        }
        _ => None,
    }
}

pub open spec fn stack_push_u32(view: StackView, value: u32) -> StackView
    recommends
        view.top == view.bytes.len(),
{
    stack_push_bytes(view, spec_u32_to_le_bytes(value))
}

pub open spec fn stack_push_u64(view: StackView, value: u64) -> StackView
    recommends
        view.top == view.bytes.len(),
{
    stack_push_bytes(view, spec_u64_to_le_bytes(value))
}

pub open spec fn stack_push_u128(view: StackView, value: u128) -> StackView
    recommends
        view.top == view.bytes.len(),
{
    stack_push_bytes(view, spec_u128_to_le_bytes(value))
}

pub open spec fn ref_null() -> RefView {
    RefView { raw: 0 }
}

pub closed spec fn ref_is_null_result(raw: u32) -> u32 {
    if raw == 0 { 1 } else { 0 }
}

pub open spec fn global_get_bytes(global: GlobalView) -> Seq<u8> {
    global.bytes
}

pub open spec fn global_set_bytes(global: GlobalView, bytes: Seq<u8>) -> GlobalView {
    GlobalView { bytes }
}

pub open spec fn table_get_result(table: TableView, idx: nat) -> Option<u32> {
    if idx < table.entries.len() as nat {
        Some(table.entries[idx as int])
    } else {
        None
    }
}

pub open spec fn table_set_result(table: TableView, idx: nat, value: u32) -> Option<TableView> {
    if idx < table.entries.len() as nat {
        Some(TableView {
            entries: Seq::new(
                table.entries.len(),
                |i: int| if i == idx as int { value } else { table.entries[i] },
            ),
            max_len: table.max_len,
        })
    } else {
        None
    }
}

pub open spec fn table_size_result(table: TableView) -> nat {
    table.entries.len() as nat
}

pub open spec fn table_grow_result(table: TableView, len: nat, value: u32) -> (TableView, int) {
    match table.max_len {
        Some(max_len) if table.entries.len() + len > max_len => (table, -1),
        _ => (
            TableView {
                entries: Seq::new(
                    table.entries.len() + len,
                    |i: int| if i < table.entries.len() { table.entries[i] } else { value },
                ),
                max_len: table.max_len,
            },
            table.entries.len() as int,
        ),
    }
}

pub open spec fn outcome_continue() -> CoreOutcome {
    CoreOutcome::Continue
}

pub open spec fn outcome_trap(code: TrapCode) -> CoreOutcome {
    CoreOutcome::Trap(code)
}

pub open spec fn outcome_pending(code: PendingCode) -> CoreOutcome {
    CoreOutcome::Pending(code)
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

pub open spec fn linear_read_bytes(
    view: LinearMemoryView,
    start: nat,
    len: nat,
) -> Option<Seq<u8>> {
    if start + len <= view.bytes.len() as nat {
        Some(view.bytes.subrange(start as int, (start + len) as int))
    } else {
        None
    }
}

pub open spec fn memory_load_result_bytes(
    raw_bytes: Seq<u8>,
    kind: MemoryLoadKind,
) -> Option<Seq<u8>> {
    match kind {
        MemoryLoadKind::Raw { load_width } => {
            if raw_bytes.len() == load_width {
                Some(raw_bytes)
            } else {
                None
            }
        }
        MemoryLoadKind::ZeroExtend {
            load_width,
            result_width,
        } => {
            if raw_bytes.len() == load_width && load_width <= result_width {
                Some(spec_zero_extend_bytes(raw_bytes, result_width))
            } else {
                None
            }
        }
        MemoryLoadKind::SignExtend {
            load_width,
            result_width,
        } => {
            if raw_bytes.len() == load_width && load_width <= result_width && load_width > 0 {
                Some(spec_sign_extend_bytes(raw_bytes, result_width))
            } else {
                None
            }
        }
    }
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

pub open spec fn shared_consume_timed_out(
    protocol: SharedMemoryProtocol,
    ticket: WaitTicketTok,
) -> SharedMemoryProtocol {
    SharedMemoryProtocol {
        memory: protocol.memory,
        wait_queues: protocol.wait_queues,
        waiters: protocol.waiters.remove(ticket.waiter_id),
        next_waiter_id: protocol.next_waiter_id,
    }
}

pub open spec fn shared_consume_wake(
    protocol: SharedMemoryProtocol,
    wake: WakeTok,
) -> SharedMemoryProtocol {
    SharedMemoryProtocol {
        memory: protocol.memory,
        wait_queues: protocol.wait_queues,
        waiters: protocol.waiters.remove(wake.waiter_id),
        next_waiter_id: protocol.next_waiter_id,
    }
}

pub open spec fn shared_atomic_store(
    protocol: SharedMemoryProtocol,
    start: int,
    value_bytes: Seq<u8>,
) -> SharedMemoryProtocol {
    SharedMemoryProtocol {
        memory: linear_write_bytes(protocol.memory, start, value_bytes),
        wait_queues: protocol.wait_queues,
        waiters: protocol.waiters,
        next_waiter_id: protocol.next_waiter_id,
    }
}

pub open spec fn shared_atomic_rmw(
    protocol: SharedMemoryProtocol,
    start: int,
    value_bytes: Seq<u8>,
) -> SharedMemoryProtocol {
    shared_atomic_store(protocol, start, value_bytes)
}

pub open spec fn shared_atomic_cmpxchg(
    protocol: SharedMemoryProtocol,
    start: int,
    expected: AtomicCmpxchgExpected,
    value_bytes: Seq<u8>,
) -> SharedMemoryProtocol
    recommends
        value_bytes.len() == atomic_cmpxchg_width(expected),
        0 <= start,
        start + value_bytes.len() <= protocol.memory.bytes.len(),
{
    let old_bytes = protocol.memory.bytes.subrange(start, start + value_bytes.len());
    SharedMemoryProtocol {
        memory: if old_bytes == atomic_expected_bytes(expected) {
            linear_write_bytes(protocol.memory, start, value_bytes)
        } else {
            protocol.memory
        },
        wait_queues: protocol.wait_queues,
        waiters: protocol.waiters,
        next_waiter_id: protocol.next_waiter_id,
    }
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

pub open spec fn frame_view_from_projection_parts(
    return_pc: nat,
    instance_raw: u32,
    default_memory_present: bool,
    default_memory_shared: bool,
    default_memory_raw: u32,
    prev_local_top: nat,
    prev_local_size: nat,
) -> FrameView {
    frame_view_from_parts(
        return_pc as int,
        instance_raw as nat,
        default_memory_present,
        default_memory_shared,
        default_memory_raw as nat,
        prev_local_top,
        prev_local_size,
    )
}

pub open spec fn exec_context_token_from_projection_parts(
    current_return_pc: nat,
    current_instance_raw: u32,
    current_default_memory_present: bool,
    current_default_memory_shared: bool,
    current_default_memory_raw: u32,
    current_prev_local_top: nat,
    current_prev_local_size: nat,
    caller_present: bool,
    caller_return_pc: nat,
    caller_instance_raw: u32,
    caller_default_memory_present: bool,
    caller_default_memory_shared: bool,
    caller_default_memory_raw: u32,
    caller_prev_local_top: nat,
    caller_prev_local_size: nat,
    cont_addr: nat,
    task_id: u32,
) -> ExecContextToken {
    exec_context_token(
        frame_view_from_projection_parts(
            current_return_pc,
            current_instance_raw,
            current_default_memory_present,
            current_default_memory_shared,
            current_default_memory_raw,
            current_prev_local_top,
            current_prev_local_size,
        ),
        if caller_present {
            Some(frame_view_from_projection_parts(
                caller_return_pc,
                caller_instance_raw,
                caller_default_memory_present,
                caller_default_memory_shared,
                caller_default_memory_raw,
                caller_prev_local_top,
                caller_prev_local_size,
            ))
        } else {
            None
        },
        cont_addr,
        task_id as nat,
    )
}

pub open spec fn core_step_state_with_stack(state: CoreStepState, stack: StackView) -> CoreStepState {
    CoreStepState {
        stack,
        context: state.context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_with_exec(
    state: CoreStepState,
    context: ExecContextToken,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_with_stack_exec(
    state: CoreStepState,
    stack: StackView,
    context: ExecContextToken,
) -> CoreStepState {
    CoreStepState {
        stack,
        context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_with_table(
    state: CoreStepState,
    table_id: TableId,
    table: TableView,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context: state.context,
        tables: state.tables.insert(table_id, table),
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_with_global(
    state: CoreStepState,
    global_id: GlobalId,
    global: GlobalView,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context: state.context,
        tables: state.tables,
        globals: state.globals.insert(global_id, global),
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_with_local_memory(
    state: CoreStepState,
    memory_id: MemoryId,
    memory: LinearMemoryView,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context: state.context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories.insert(memory_id, memory),
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_with_shared_protocol(
    state: CoreStepState,
    memory_id: MemoryId,
    protocol: SharedMemoryProtocol,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context: state.context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories.insert(memory_id, protocol),
        data_segments: state.data_segments,
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_without_data_segment(
    state: CoreStepState,
    data_segment_id: SegmentId,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context: state.context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments.remove(data_segment_id),
        elem_segments: state.elem_segments,
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn core_step_state_without_elem_segment(
    state: CoreStepState,
    elem_segment_id: SegmentId,
) -> CoreStepState {
    CoreStepState {
        stack: state.stack,
        context: state.context,
        tables: state.tables,
        globals: state.globals,
        functions: state.functions,
        local_memories: state.local_memories,
        shared_memories: state.shared_memories,
        data_segments: state.data_segments,
        elem_segments: state.elem_segments.remove(elem_segment_id),
        frame_metadata_len: state.frame_metadata_len,
    }
}

pub open spec fn context_with_cont(context: ExecContextToken, cont_addr: nat) -> ExecContextToken {
    ExecContextToken {
        current_frame: context.current_frame,
        caller_frame: context.caller_frame,
        cont_addr,
        task_id: context.task_id,
    }
}

pub open spec fn resolve_memory_handle(
    state: CoreStepState,
    selector: MemorySelector,
) -> Option<MemoryHandleView> {
    match selector {
        MemorySelector::CurrentDefault => state.context.current_frame.default_memory,
        MemorySelector::Explicit(handle) => Some(handle),
    }
}

pub open spec fn resolve_linear_memory(
    state: CoreStepState,
    selector: MemorySelector,
) -> Option<LinearMemoryView> {
    match resolve_memory_handle(state, selector) {
        Some(MemoryHandleView::Local(memory_id)) => {
            if state.local_memories.dom().contains(memory_id) {
                Some(state.local_memories[memory_id])
            } else {
                None
            }
        }
        Some(MemoryHandleView::Shared(memory_id)) => {
            if state.shared_memories.dom().contains(memory_id) {
                Some(state.shared_memories[memory_id].memory)
            } else {
                None
            }
        }
        None => None,
    }
}

pub open spec fn resolve_shared_protocol(
    state: CoreStepState,
    selector: MemorySelector,
) -> Option<SharedMemoryProtocol> {
    match resolve_memory_handle(state, selector) {
        Some(MemoryHandleView::Shared(memory_id)) => {
            if state.shared_memories.dom().contains(memory_id) {
                Some(state.shared_memories[memory_id])
            } else {
                None
            }
        }
        _ => None,
    }
}

pub open spec fn core_step_state_with_selected_memory(
    state: CoreStepState,
    selector: MemorySelector,
    memory: LinearMemoryView,
) -> Option<CoreStepState> {
    match resolve_memory_handle(state, selector) {
        Some(MemoryHandleView::Local(memory_id)) => {
            if state.local_memories.dom().contains(memory_id) {
                Some(core_step_state_with_local_memory(state, memory_id, memory))
            } else {
                None
            }
        }
        Some(MemoryHandleView::Shared(memory_id)) => {
            if state.shared_memories.dom().contains(memory_id) {
                let protocol = state.shared_memories[memory_id];
                Some(core_step_state_with_shared_protocol(
                    state,
                    memory_id,
                    SharedMemoryProtocol {
                        memory,
                        wait_queues: protocol.wait_queues,
                        waiters: protocol.waiters,
                        next_waiter_id: protocol.next_waiter_id,
                    },
                ))
            } else {
                None
            }
        }
        None => None,
    }
}

pub open spec fn table_write_range(
    table: TableView,
    start: nat,
    values: Seq<u32>,
) -> Option<TableView> {
    if start + values.len() <= table.entries.len() as nat {
        Some(TableView {
            entries: Seq::new(
                table.entries.len(),
                |i: int| {
                    if start as int <= i && i < start as int + values.len() {
                        values[i - start as int]
                    } else {
                        table.entries[i]
                    }
                },
            ),
            max_len: table.max_len,
        })
    } else {
        None
    }
}

pub open spec fn table_fill_result(
    table: TableView,
    start: nat,
    len: nat,
    value: u32,
) -> Option<TableView> {
    table_write_range(table, start, Seq::new(len, |i: int| value))
}

pub open spec fn atomic_cmpxchg_width(expected: AtomicCmpxchgExpected) -> nat {
    match expected {
        AtomicCmpxchgExpected::U8(_) => 1,
        AtomicCmpxchgExpected::U16(_) => 2,
        AtomicCmpxchgExpected::U32(_) => 4,
        AtomicCmpxchgExpected::U64(_) => 8,
    }
}

pub open spec fn atomic_expected_bytes(expected: AtomicCmpxchgExpected) -> Seq<u8> {
    match expected {
        AtomicCmpxchgExpected::U8(value) => seq![value],
        AtomicCmpxchgExpected::U16(value) => spec_u16_to_le_bytes(value),
        AtomicCmpxchgExpected::U32(value) => spec_u32_to_le_bytes(value),
        AtomicCmpxchgExpected::U64(value) => spec_u64_to_le_bytes(value),
    }
}

pub open spec fn atomic_wait_expected_bytes(expected: AtomicWaitKind) -> Seq<u8> {
    match expected {
        AtomicWaitKind::I32(value) => spec_u32_to_le_bytes(value),
        AtomicWaitKind::I64(value) => spec_u64_to_le_bytes(value),
    }
}

pub open spec fn function_frame_for_call(
    state: CoreStepState,
    function: FunctionView,
    return_addr: nat,
) -> FrameView {
    FrameView {
        return_pc: return_addr as int,
        instance: function.instance,
        default_memory: function.default_memory,
        prev_local: state.stack.active_local,
    }
}

pub open spec fn context_after_call(
    context: ExecContextToken,
    callee: FrameView,
    function: FunctionView,
    return_addr: nat,
    is_return_call: bool,
) -> ExecContextToken {
    ExecContextToken {
        current_frame: callee,
        caller_frame: if is_return_call {
            context.caller_frame
        } else {
            Some(context.current_frame)
        },
        cont_addr: if function.kind == FunctionKind::Wasm {
            function.entry_pc
        } else {
            return_addr
        },
        task_id: context.task_id,
    }
}

pub open spec fn context_after_function_return(context: ExecContextToken) -> ExecContextToken {
    ExecContextToken {
        current_frame: match context.caller_frame {
            Some(frame) => frame,
            None => context.current_frame,
        },
        caller_frame: None,
        cont_addr: context.current_frame.return_pc as nat,
        task_id: context.task_id,
    }
}

pub open spec fn core_step_continue_state(state: CoreStepState) -> (CoreStepState, CoreOutcome) {
    (state, core_continue())
}

pub open spec fn core_step_trap_state(
    state: CoreStepState,
    code: TrapCode,
) -> (CoreStepState, CoreOutcome) {
    (state, core_trap(code))
}

pub open spec fn core_step_pending_state(
    state: CoreStepState,
    code: PendingCode,
) -> (CoreStepState, CoreOutcome) {
    (state, core_pending(code))
}

pub open spec fn spec_step_numeric(
    state: CoreStepState,
    step: NumericStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        NumericStep::PushConst { bytes, next_cont } => {
            if state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_bytes(state.stack, bytes),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        NumericStep::ReplaceTop {
            pop_len,
            result_bytes,
            next_cont,
        } => {
            if pop_len <= state.stack.top && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_bytes(stack_pop_bytes(state.stack, pop_len), result_bytes),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
    }
}

pub open spec fn spec_step_control(
    state: CoreStepState,
    step: ControlStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        ControlStep::SetCont { cont_addr } => {
            core_step_continue_state(core_step_state_with_exec(
                state,
                context_with_cont(state.context, cont_addr),
            ))
        }
        ControlStep::ConditionalCont {
            taken,
            true_addr,
            false_addr,
        } => {
            if state.stack.top >= 4 && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_pop_bytes(state.stack, 4),
                    context_with_cont(
                        state.context,
                        if taken { true_addr } else { false_addr },
                    ),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        ControlStep::BlockReturn {
            block_stack_top,
            return_size,
            cont_addr,
        } => {
            if return_size <= state.stack.top && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_block_return(state.stack, block_stack_top, return_size),
                    context_with_cont(state.context, cont_addr),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        ControlStep::FunctionReturn { return_size } => {
            if return_size <= state.stack.top
                && state.stack.top == state.stack.bytes.len() as nat
                && return_size <= state.stack.active_local.local_size
                && state.stack.frame_stack.len() > 0
            {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_function_return(state.stack, state.context.current_frame.prev_local, return_size),
                    context_after_function_return(state.context),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        ControlStep::Trap { code } => core_step_trap_state(state, code),
    }
}

pub open spec fn spec_step_call_resolved(
    state: CoreStepState,
    function: FunctionView,
    function_id: FunctionId,
    return_addr: nat,
    is_return_call: bool,
) -> (CoreStepState, CoreOutcome) {
    let _ = function_id;
    if state.stack.top != state.stack.bytes.len() as nat {
        core_step_trap_state(state, TrapCode::InvalidOperand)
    } else {
        let callee = function_frame_for_call(state, function, return_addr);
        let stack =
            if is_return_call {
                stack_function_return_call(
                    state.stack,
                    callee,
                    function.param_size,
                    function.local_size,
                    zero_bytes(state.frame_metadata_len),
                )
            } else {
                stack_function_call(
                    state.stack,
                    callee,
                    function.param_size,
                    function.local_size,
                    zero_bytes(state.frame_metadata_len),
                )
            };
        let context =
            context_after_call(state.context, callee, function, return_addr, is_return_call);
        match function.kind {
            FunctionKind::Wasm => {
                core_step_continue_state(core_step_state_with_stack_exec(state, stack, context))
            }
            FunctionKind::Host => {
                core_step_pending_state(
                    core_step_state_with_stack_exec(state, stack, context),
                    PendingCode::HostCall,
                )
            }
        }
    }
}

pub open spec fn spec_step_call(
    state: CoreStepState,
    step: CallStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        CallStep::Call {
            function_id,
            return_addr,
            is_return_call,
        } => {
            if state.functions.dom().contains(function_id) {
                spec_step_call_resolved(
                    state,
                    state.functions[function_id],
                    function_id,
                    return_addr,
                    is_return_call,
                )
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        CallStep::CallIndirect {
            table_id,
            elem_index,
            expected_type_id,
            return_addr,
            is_return_call,
        } => {
            if state.tables.dom().contains(table_id) {
                match table_get_result(state.tables[table_id], elem_index) {
                    Some(raw) if raw == 0 => {
                        core_step_trap_state(state, TrapCode::TableUninitialized)
                    }
                    Some(raw) => {
                        let function_id = raw as nat;
                        if !state.functions.dom().contains(function_id) {
                            core_step_trap_state(state, TrapCode::InvalidOperand)
                        } else if state.functions[function_id].type_id != expected_type_id {
                            core_step_trap_state(state, TrapCode::CallIndirectInvalidType)
                        } else {
                            spec_step_call_resolved(
                                state,
                                state.functions[function_id],
                                function_id,
                                return_addr,
                                is_return_call,
                            )
                        }
                    }
                    None => core_step_trap_state(state, TrapCode::TableIndexOutOfRange),
                }
            } else {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            }
        }
    }
}

pub open spec fn spec_step_local(
    state: CoreStepState,
    step: LocalStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        LocalStep::Drop { size, next_cont } => {
            if size <= state.stack.top && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_drop_values(state.stack, size),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        LocalStep::Select {
            size,
            cond,
            next_cont,
        } => {
            if size * 2 + 4 <= state.stack.top && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_select_bytes(state.stack, size, cond),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        LocalStep::Get {
            local_addr,
            size,
            next_cont,
        } => {
            match stack_local_get(state.stack, local_addr, size) {
                Some(stack) => core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack,
                    context_with_cont(state.context, next_cont),
                )),
                None => core_step_trap_state(state, TrapCode::InvalidOperand),
            }
        }
        LocalStep::Set {
            local_addr,
            size,
            next_cont,
        } => {
            match stack_local_set(state.stack, local_addr, size) {
                Some(stack) => core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack,
                    context_with_cont(state.context, next_cont),
                )),
                None => core_step_trap_state(state, TrapCode::InvalidOperand),
            }
        }
        LocalStep::Tee {
            local_addr,
            size,
            next_cont,
        } => {
            match stack_local_tee(state.stack, local_addr, size) {
                Some(stack) => core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack,
                    context_with_cont(state.context, next_cont),
                )),
                None => core_step_trap_state(state, TrapCode::InvalidOperand),
            }
        }
    }
}

pub open spec fn spec_step_global(
    state: CoreStepState,
    step: GlobalStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        GlobalStep::Get {
            global_id,
            next_cont,
        } => {
            if state.globals.dom().contains(global_id) && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_bytes(state.stack, global_get_bytes(state.globals[global_id])),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        GlobalStep::Set {
            global_id,
            next_cont,
        } => {
            if state.globals.dom().contains(global_id) {
                let size = state.globals[global_id].bytes.len() as nat;
                match stack_top_bytes(state.stack, size) {
                    Some(bytes) => {
                        if state.stack.top == state.stack.bytes.len() as nat {
                            let stack = stack_pop_bytes(state.stack, size);
                            let next_state = core_step_state_with_global(
                                core_step_state_with_stack_exec(
                                    state,
                                    stack,
                                    context_with_cont(state.context, next_cont),
                                ),
                                global_id,
                                global_set_bytes(state.globals[global_id], bytes),
                            );
                            core_step_continue_state(next_state)
                        } else {
                            core_step_trap_state(state, TrapCode::InvalidOperand)
                        }
                    }
                    None => core_step_trap_state(state, TrapCode::InvalidOperand),
                }
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
    }
}

pub open spec fn spec_step_table(
    state: CoreStepState,
    step: TableStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        TableStep::Get {
            table_id,
            index,
            next_cont,
        } => {
            if state.tables.dom().contains(table_id) {
                match table_get_result(state.tables[table_id], index) {
                    Some(value) if state.stack.top == state.stack.bytes.len() as nat => {
                        core_step_continue_state(core_step_state_with_stack_exec(
                            state,
                            stack_push_u32(state.stack, value),
                            context_with_cont(state.context, next_cont),
                        ))
                    }
                    Some(_) => core_step_trap_state(state, TrapCode::InvalidOperand),
                    None => core_step_trap_state(state, TrapCode::TableIndexOutOfRange),
                }
            } else {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            }
        }
        TableStep::Set {
            table_id,
            index,
            value,
            next_cont,
        } => {
            if state.tables.dom().contains(table_id) {
                match table_set_result(state.tables[table_id], index, value) {
                    Some(table) => core_step_continue_state(core_step_state_with_exec(
                        core_step_state_with_table(state, table_id, table),
                        context_with_cont(state.context, next_cont),
                    )),
                    None => core_step_trap_state(state, TrapCode::TableIndexOutOfRange),
                }
            } else {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            }
        }
        TableStep::Size {
            table_id,
            next_cont,
        } => {
            if state.tables.dom().contains(table_id) && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_u32(state.stack, table_size_result(state.tables[table_id]) as u32),
                    context_with_cont(state.context, next_cont),
                ))
            } else if !state.tables.dom().contains(table_id) {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        TableStep::Grow {
            table_id,
            len,
            value,
            next_cont,
        } => {
            if !state.tables.dom().contains(table_id) {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            } else if state.stack.top != state.stack.bytes.len() as nat {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            } else {
                let grown = table_grow_result(state.tables[table_id], len, value);
                let result = grown.1;
                core_step_continue_state(core_step_state_with_stack_exec(
                    core_step_state_with_table(state, table_id, grown.0),
                    if result < 0 {
                        stack_push_u32(state.stack, 0xffff_ffffu32)
                    } else {
                        stack_push_u32(state.stack, result as u32)
                    },
                    context_with_cont(state.context, next_cont),
                ))
            }
        }
        TableStep::Fill {
            table_id,
            index,
            len,
            value,
            next_cont,
        } => {
            if state.tables.dom().contains(table_id) {
                match table_fill_result(state.tables[table_id], index, len, value) {
                    Some(table) => core_step_continue_state(core_step_state_with_exec(
                        core_step_state_with_table(state, table_id, table),
                        context_with_cont(state.context, next_cont),
                    )),
                    None => core_step_trap_state(state, TrapCode::TableIndexOutOfRange),
                }
            } else {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            }
        }
        TableStep::Copy {
            dst_table_id,
            src_table_id,
            dst,
            src,
            len,
            next_cont,
        } => {
            if state.tables.dom().contains(dst_table_id) && state.tables.dom().contains(src_table_id) {
                let src_table = state.tables[src_table_id];
                let dst_table = state.tables[dst_table_id];
                if src + len > src_table.entries.len() as nat
                    || dst + len > dst_table.entries.len() as nat
                {
                    core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
                } else {
                    let values = src_table.entries.subrange(src as int, (src + len) as int);
                    match table_write_range(dst_table, dst, values) {
                        Some(table) => core_step_continue_state(core_step_state_with_exec(
                            core_step_state_with_table(state, dst_table_id, table),
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::TableIndexOutOfRange),
                    }
                }
            } else {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            }
        }
        TableStep::Init {
            table_id,
            elem_segment_id,
            dst,
            src,
            len,
            next_cont,
        } => {
            if !state.tables.dom().contains(table_id) {
                core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
            } else if !state.elem_segments.dom().contains(elem_segment_id) {
                if len == 0 && src == 0 {
                    core_step_continue_state(core_step_state_with_exec(
                        state,
                        context_with_cont(state.context, next_cont),
                    ))
                } else {
                    core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
                }
            } else {
                let table = state.tables[table_id];
                let elems = state.elem_segments[elem_segment_id];
                if src + len > elems.len() as nat || dst + len > table.entries.len() as nat {
                    core_step_trap_state(state, TrapCode::TableIndexOutOfRange)
                } else {
                    match table_write_range(
                        table,
                        dst,
                        elems.subrange(src as int, (src + len) as int),
                    ) {
                        Some(next_table) => core_step_continue_state(core_step_state_with_exec(
                            core_step_state_with_table(state, table_id, next_table),
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::TableIndexOutOfRange),
                    }
                }
            }
        }
        TableStep::ElemDrop {
            elem_segment_id,
            next_cont,
        } => core_step_continue_state(core_step_state_with_exec(
            core_step_state_without_elem_segment(state, elem_segment_id),
            context_with_cont(state.context, next_cont),
        )),
    }
}

pub open spec fn spec_step_ref(
    state: CoreStepState,
    step: RefStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        RefStep::Null { next_cont } => {
            if state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_u32(state.stack, 0u32),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        RefStep::IsNull { next_cont } => match stack_top_bytes(state.stack, 4) {
            Some(bytes) if state.stack.top == state.stack.bytes.len() as nat => {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_u32(
                        stack_pop_bytes(state.stack, 4),
                        ref_is_null_result(spec_le_u32(bytes)),
                    ),
                    context_with_cont(state.context, next_cont),
                ))
            }
            _ => core_step_trap_state(state, TrapCode::InvalidOperand),
        },
        RefStep::Func {
            function_id,
            next_cont,
        } => {
            if state.functions.dom().contains(function_id) && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_u32(state.stack, function_id as u32),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
    }
}

pub open spec fn spec_step_memory(
    state: CoreStepState,
    step: MemoryStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        MemoryStep::Load {
            selector,
            start,
            kind,
            next_cont,
        } => match resolve_linear_memory(state, selector) {
            Some(memory) => {
                let load_width = match kind {
                    MemoryLoadKind::Raw { load_width } => load_width,
                    MemoryLoadKind::ZeroExtend { load_width, .. } => load_width,
                    MemoryLoadKind::SignExtend { load_width, .. } => load_width,
                };
                match linear_read_bytes(memory, start, load_width) {
                    Some(raw_bytes) => match memory_load_result_bytes(raw_bytes, kind) {
                        Some(result_bytes) if state.stack.top == state.stack.bytes.len() as nat => {
                            core_step_continue_state(core_step_state_with_stack_exec(
                                state,
                                stack_push_bytes(state.stack, result_bytes),
                                context_with_cont(state.context, next_cont),
                            ))
                        }
                        Some(_) => core_step_trap_state(state, TrapCode::InvalidOperand),
                        None => core_step_trap_state(state, TrapCode::InvalidOperand),
                    },
                    None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                }
            }
            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
        MemoryStep::Store {
            selector,
            start,
            len,
            next_cont,
        } => match (resolve_linear_memory(state, selector), stack_top_bytes(state.stack, len)) {
            (Some(memory), Some(bytes)) if state.stack.top == state.stack.bytes.len() as nat => {
                if start + len > memory.bytes.len() as nat {
                    core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                } else {
                    match core_step_state_with_selected_memory(
                        state,
                        selector,
                        linear_write_bytes(memory, start as int, bytes),
                    ) {
                        Some(next_state) => core_step_continue_state(core_step_state_with_stack_exec(
                            next_state,
                            stack_pop_bytes(state.stack, len),
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                    }
                }
            }
            (Some(_), Some(_)) => core_step_trap_state(state, TrapCode::InvalidOperand),
            _ => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
        MemoryStep::Size { selector, next_cont } => match resolve_linear_memory(state, selector) {
            Some(memory) if state.stack.top == state.stack.bytes.len() as nat => {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_u32(state.stack, memory.current_pages as u32),
                    context_with_cont(state.context, next_cont),
                ))
            }
            Some(_) => core_step_trap_state(state, TrapCode::InvalidOperand),
            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
        MemoryStep::Grow {
            selector,
            page_delta,
            next_cont,
        } => match resolve_linear_memory(state, selector) {
            Some(memory) if state.stack.top == state.stack.bytes.len() as nat => {
                if memory.current_pages + page_delta <= memory.max_pages {
                    match core_step_state_with_selected_memory(
                        state,
                        selector,
                        linear_grow(memory, page_delta, zero_bytes(page_delta * 65536)),
                    ) {
                        Some(next_state) => core_step_continue_state(core_step_state_with_stack_exec(
                            next_state,
                            stack_push_u32(state.stack, memory.current_pages as u32),
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                    }
                } else {
                    core_step_continue_state(core_step_state_with_stack_exec(
                        state,
                        stack_push_u32(state.stack, 0xffff_ffffu32),
                        context_with_cont(state.context, next_cont),
                    ))
                }
            }
            Some(_) => core_step_trap_state(state, TrapCode::InvalidOperand),
            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
    }
}

pub open spec fn spec_step_atomic(
    state: CoreStepState,
    step: AtomicStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        AtomicStep::Notify {
            selector,
            start,
            count,
            aligned,
            next_cont,
        } => {
            if !aligned {
                core_step_trap_state(state, TrapCode::UnalignedAtomic)
            } else {
                match resolve_memory_handle(state, selector) {
                    Some(MemoryHandleView::Local(_))
                        if state.stack.top == state.stack.bytes.len() as nat =>
                    {
                        core_step_continue_state(core_step_state_with_stack_exec(
                            state,
                            stack_push_u32(state.stack, 0u32),
                            context_with_cont(state.context, next_cont),
                        ))
                    }
                    Some(MemoryHandleView::Local(_)) => {
                        core_step_trap_state(state, TrapCode::InvalidOperand)
                    }
                    Some(MemoryHandleView::Shared(memory_id)) => {
                        if !state.shared_memories.dom().contains(memory_id) {
                            core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                        } else if state.stack.top != state.stack.bytes.len() as nat {
                            core_step_trap_state(state, TrapCode::InvalidOperand)
                        } else {
                            let notified =
                                shared_notify_waiters(state.shared_memories[memory_id], start, count);
                            core_step_continue_state(core_step_state_with_stack_exec(
                                core_step_state_with_shared_protocol(state, memory_id, notified.0),
                                stack_push_u32(state.stack, notified.1.len() as u32),
                                context_with_cont(state.context, next_cont),
                            ))
                        }
                    }
                    None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                }
            }
        }
        AtomicStep::Wait {
            selector,
            start,
            expected,
            timeout_immediate,
            aligned,
            next_cont,
        } => {
            if !aligned {
                core_step_trap_state(state, TrapCode::UnalignedAtomic)
            } else {
                match resolve_memory_handle(state, selector) {
                    Some(MemoryHandleView::Local(_)) => {
                        core_step_trap_state(state, TrapCode::InvalidOperand)
                    }
                    Some(MemoryHandleView::Shared(memory_id)) => {
                        if !state.shared_memories.dom().contains(memory_id) {
                            core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                        } else {
                            let protocol = state.shared_memories[memory_id];
                            let expected_bytes = atomic_wait_expected_bytes(expected);
                            match linear_read_bytes(protocol.memory, start, expected_bytes.len() as nat) {
                                Some(bytes) if bytes != expected_bytes => {
                                    if state.stack.top != state.stack.bytes.len() as nat {
                                        core_step_trap_state(state, TrapCode::InvalidOperand)
                                    } else {
                                        core_step_continue_state(core_step_state_with_stack_exec(
                                            state,
                                            stack_push_u32(
                                                state.stack,
                                                wait_result_not_equal_code() as u32,
                                            ),
                                            context_with_cont(state.context, next_cont),
                                        ))
                                    }
                                }
                                Some(_) if timeout_immediate => {
                                    if state.stack.top != state.stack.bytes.len() as nat {
                                        core_step_trap_state(state, TrapCode::InvalidOperand)
                                    } else {
                                        core_step_continue_state(core_step_state_with_stack_exec(
                                            state,
                                            stack_push_u32(
                                                state.stack,
                                                wait_result_timed_out_code() as u32,
                                            ),
                                            context_with_cont(state.context, next_cont),
                                        ))
                                    }
                                }
                                Some(_) => {
                                    let pending = shared_register_wait(protocol, start);
                                    core_step_pending_state(
                                        core_step_state_with_exec(
                                            core_step_state_with_shared_protocol(
                                                state,
                                                memory_id,
                                                pending.0,
                                            ),
                                            context_with_cont(state.context, next_cont),
                                        ),
                                        PendingCode::Wait,
                                    )
                                }
                                None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            }
                        }
                    }
                    None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                }
            }
        }
        AtomicStep::Store {
            selector,
            start,
            bytes,
            aligned,
            next_cont,
        } => {
            if !aligned {
                core_step_trap_state(state, TrapCode::UnalignedAtomic)
            } else {
                match resolve_memory_handle(state, selector) {
                    Some(MemoryHandleView::Local(_)) => {
                        match resolve_linear_memory(state, selector) {
                            Some(memory)
                                if start + bytes.len() as nat <= memory.bytes.len() as nat =>
                            {
                                match core_step_state_with_selected_memory(
                                    state,
                                    selector,
                                    linear_write_bytes(memory, start as int, bytes),
                                ) {
                                    Some(next_state) => core_step_continue_state(
                                        core_step_state_with_exec(
                                            next_state,
                                            context_with_cont(state.context, next_cont),
                                        ),
                                    ),
                                    None => {
                                        core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                                    }
                                }
                            }
                            Some(_) => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                        }
                    }
                    Some(MemoryHandleView::Shared(memory_id)) => {
                        match resolve_shared_protocol(state, selector) {
                            Some(protocol)
                                if start + bytes.len() as nat <= protocol.memory.bytes.len() as nat =>
                            {
                                core_step_continue_state(core_step_state_with_exec(
                                    core_step_state_with_shared_protocol(
                                        state,
                                        memory_id,
                                        shared_atomic_store(protocol, start as int, bytes),
                                    ),
                                    context_with_cont(state.context, next_cont),
                                ))
                            }
                            Some(_) => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                        }
                    }
                    None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                }
            }
        }
        AtomicStep::Rmw {
            selector,
            start,
            result_bytes,
            write_bytes,
            aligned,
            next_cont,
        } => {
            if !aligned {
                core_step_trap_state(state, TrapCode::UnalignedAtomic)
            } else if state.stack.top != state.stack.bytes.len() as nat {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            } else {
                match resolve_memory_handle(state, selector) {
                    Some(MemoryHandleView::Local(_)) => {
                        match resolve_linear_memory(state, selector) {
                            Some(memory)
                                if start + write_bytes.len() as nat <= memory.bytes.len() as nat =>
                            {
                                match core_step_state_with_selected_memory(
                                    state,
                                    selector,
                                    linear_write_bytes(memory, start as int, write_bytes),
                                ) {
                                    Some(next_state) => {
                                        core_step_continue_state(core_step_state_with_stack_exec(
                                            next_state,
                                            stack_push_bytes(state.stack, result_bytes),
                                            context_with_cont(state.context, next_cont),
                                        ))
                                    }
                                    None => {
                                        core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                                    }
                                }
                            }
                            Some(_) => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                        }
                    }
                    Some(MemoryHandleView::Shared(memory_id)) => {
                        match resolve_shared_protocol(state, selector) {
                            Some(protocol)
                                if start + write_bytes.len() as nat
                                    <= protocol.memory.bytes.len() as nat =>
                            {
                                core_step_continue_state(core_step_state_with_stack_exec(
                                    core_step_state_with_shared_protocol(
                                        state,
                                        memory_id,
                                        shared_atomic_rmw(protocol, start as int, write_bytes),
                                    ),
                                    stack_push_bytes(state.stack, result_bytes),
                                    context_with_cont(state.context, next_cont),
                                ))
                            }
                            Some(_) => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                        }
                    }
                    None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                }
            }
        }
        AtomicStep::Cmpxchg {
            selector,
            start,
            expected,
            value_bytes,
            aligned,
            next_cont,
        } => {
            if !aligned {
                core_step_trap_state(state, TrapCode::UnalignedAtomic)
            } else if state.stack.top != state.stack.bytes.len() as nat {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            } else {
                let width = atomic_cmpxchg_width(expected);
                if value_bytes.len() != width {
                    core_step_trap_state(state, TrapCode::InvalidOperand)
                } else {
                    match resolve_memory_handle(state, selector) {
                        Some(MemoryHandleView::Local(_)) => {
                            match resolve_linear_memory(state, selector) {
                                Some(memory) => match linear_read_bytes(memory, start, width) {
                                    Some(old_bytes) => {
                                        let updated =
                                            if old_bytes == atomic_expected_bytes(expected) {
                                                linear_write_bytes(memory, start as int, value_bytes)
                                            } else {
                                                memory
                                            };
                                        match core_step_state_with_selected_memory(
                                            state,
                                            selector,
                                            updated,
                                        ) {
                                            Some(next_state) => {
                                                core_step_continue_state(
                                                    core_step_state_with_stack_exec(
                                                        next_state,
                                                        stack_push_bytes(state.stack, old_bytes),
                                                        context_with_cont(
                                                            state.context,
                                                            next_cont,
                                                        ),
                                                    ),
                                                )
                                            }
                                            None => core_step_trap_state(
                                                state,
                                                TrapCode::MemoryIndexOutOfRange,
                                            ),
                                        }
                                    }
                                    None => {
                                        core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                                    }
                                },
                                None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            }
                        }
                        Some(MemoryHandleView::Shared(memory_id)) => {
                            match resolve_shared_protocol(state, selector) {
                                Some(protocol) => {
                                    match linear_read_bytes(protocol.memory, start, width) {
                                        Some(old_bytes) => {
                                            core_step_continue_state(core_step_state_with_stack_exec(
                                                core_step_state_with_shared_protocol(
                                                    state,
                                                    memory_id,
                                                    shared_atomic_cmpxchg(
                                                        protocol,
                                                        start as int,
                                                        expected,
                                                        value_bytes,
                                                    ),
                                                ),
                                                stack_push_bytes(state.stack, old_bytes),
                                                context_with_cont(state.context, next_cont),
                                            ))
                                        }
                                        None => {
                                            core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                                        }
                                    }
                                }
                                None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                            }
                        }
                        None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                    }
                }
            }
        }
    }
}

pub open spec fn spec_step_bulk_memory(
    state: CoreStepState,
    step: BulkMemoryStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        BulkMemoryStep::Init {
            selector,
            data_segment_id,
            dst,
            src,
            len,
            next_cont,
        } => {
            let copied =
                if state.data_segments.dom().contains(data_segment_id) {
                    let data = state.data_segments[data_segment_id];
                    if src + len <= data.len() as nat {
                        Some(data.subrange(src as int, (src + len) as int))
                    } else {
                        None
                    }
                } else if len == 0 && src == 0 {
                    Some(Seq::empty())
                } else {
                    None
                };
            match (resolve_linear_memory(state, selector), copied) {
                (Some(memory), Some(bytes))
                    if dst + bytes.len() as nat <= memory.bytes.len() as nat =>
                {
                    match core_step_state_with_selected_memory(
                        state,
                        selector,
                        linear_write_bytes(memory, dst as int, bytes),
                    ) {
                        Some(next_state) => core_step_continue_state(core_step_state_with_exec(
                            next_state,
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                    }
                }
                (Some(_), Some(_)) => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                _ => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
            }
        }
        BulkMemoryStep::Copy {
            dst_selector,
            src_selector,
            dst,
            src,
            len,
            next_cont,
        } => match (resolve_linear_memory(state, dst_selector), resolve_linear_memory(state, src_selector)) {
            (Some(dst_memory), Some(src_memory)) => match linear_read_bytes(src_memory, src, len) {
                Some(bytes) if dst + len <= dst_memory.bytes.len() as nat => {
                    match core_step_state_with_selected_memory(
                        state,
                        dst_selector,
                        linear_write_bytes(dst_memory, dst as int, bytes),
                    ) {
                        Some(next_state) => core_step_continue_state(core_step_state_with_exec(
                            next_state,
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                    }
                }
                _ => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
            },
            _ => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
        BulkMemoryStep::Fill {
            selector,
            start,
            len,
            value,
            next_cont,
        } => match resolve_linear_memory(state, selector) {
            Some(memory) if start + len <= memory.bytes.len() as nat => {
                match core_step_state_with_selected_memory(
                    state,
                    selector,
                    linear_fill_bytes(memory, start as int, len as int, value),
                ) {
                    Some(next_state) => core_step_continue_state(core_step_state_with_exec(
                        next_state,
                        context_with_cont(state.context, next_cont),
                    )),
                    None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                }
            }
            _ => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
        BulkMemoryStep::DataDrop {
            data_segment_id,
            next_cont,
        } => core_step_continue_state(core_step_state_with_exec(
            core_step_state_without_data_segment(state, data_segment_id),
            context_with_cont(state.context, next_cont),
        )),
    }
}

#[cfg(feature = "simd")]
pub open spec fn spec_step_simd(
    state: CoreStepState,
    step: SimdStep,
) -> (CoreStepState, CoreOutcome) {
    match step {
        SimdStep::ReplaceTop {
            pop_len,
            result_bytes,
            next_cont,
        } => {
            if pop_len <= state.stack.top && state.stack.top == state.stack.bytes.len() as nat {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_bytes(stack_pop_bytes(state.stack, pop_len), result_bytes),
                    context_with_cont(state.context, next_cont),
                ))
            } else {
                core_step_trap_state(state, TrapCode::InvalidOperand)
            }
        }
        SimdStep::Load {
            selector,
            start,
            access_width,
            result_bytes,
            next_cont,
        } => match resolve_linear_memory(state, selector) {
            Some(memory)
                if start + access_width <= memory.bytes.len() as nat
                    && state.stack.top == state.stack.bytes.len() as nat =>
            {
                core_step_continue_state(core_step_state_with_stack_exec(
                    state,
                    stack_push_bytes(state.stack, result_bytes),
                    context_with_cont(state.context, next_cont),
                ))
            }
            Some(_) => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
            None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        },
        SimdStep::Store {
            selector,
            start,
            len,
            next_cont,
        } => match (resolve_linear_memory(state, selector), stack_top_bytes(state.stack, len)) {
            (Some(memory), Some(bytes)) if state.stack.top == state.stack.bytes.len() as nat => {
                if start + len > memory.bytes.len() as nat {
                    core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange)
                } else {
                    match core_step_state_with_selected_memory(
                        state,
                        selector,
                        linear_write_bytes(memory, start as int, bytes),
                    ) {
                        Some(next_state) => core_step_continue_state(core_step_state_with_stack_exec(
                            next_state,
                            stack_pop_bytes(state.stack, len),
                            context_with_cont(state.context, next_cont),
                        )),
                        None => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
                    }
                }
            }
            _ => core_step_trap_state(state, TrapCode::MemoryIndexOutOfRange),
        }
    }
}

pub open spec fn spec_step(
    state: CoreStepState,
    instr: CoreStepInstr,
) -> (CoreStepState, CoreOutcome) {
    match instr {
        CoreStepInstr::Numeric(step) => spec_step_numeric(state, step),
        CoreStepInstr::Control(step) => spec_step_control(state, step),
        CoreStepInstr::Call(step) => spec_step_call(state, step),
        CoreStepInstr::Local(step) => spec_step_local(state, step),
        CoreStepInstr::Global(step) => spec_step_global(state, step),
        CoreStepInstr::Table(step) => spec_step_table(state, step),
        CoreStepInstr::Ref(step) => spec_step_ref(state, step),
        CoreStepInstr::Memory(step) => spec_step_memory(state, step),
        CoreStepInstr::Atomic(step) => spec_step_atomic(state, step),
        CoreStepInstr::BulkMemory(step) => spec_step_bulk_memory(state, step),
        #[cfg(feature = "simd")]
        CoreStepInstr::Simd(step) => spec_step_simd(state, step),
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

pub proof fn lemma_exec_context_projection_builder_preserves_fields(
    current_return_pc: nat,
    current_instance_raw: u32,
    current_default_memory_present: bool,
    current_default_memory_shared: bool,
    current_default_memory_raw: u32,
    current_prev_local_top: nat,
    current_prev_local_size: nat,
    caller_present: bool,
    caller_return_pc: nat,
    caller_instance_raw: u32,
    caller_default_memory_present: bool,
    caller_default_memory_shared: bool,
    caller_default_memory_raw: u32,
    caller_prev_local_top: nat,
    caller_prev_local_size: nat,
    cont_addr: nat,
    task_id: u32,
)
    ensures
        exec_context_token_from_projection_parts(
            current_return_pc,
            current_instance_raw,
            current_default_memory_present,
            current_default_memory_shared,
            current_default_memory_raw,
            current_prev_local_top,
            current_prev_local_size,
            caller_present,
            caller_return_pc,
            caller_instance_raw,
            caller_default_memory_present,
            caller_default_memory_shared,
            caller_default_memory_raw,
            caller_prev_local_top,
            caller_prev_local_size,
            cont_addr,
            task_id,
        ).current_frame == frame_view_from_projection_parts(
            current_return_pc,
            current_instance_raw,
            current_default_memory_present,
            current_default_memory_shared,
            current_default_memory_raw,
            current_prev_local_top,
            current_prev_local_size,
        ),
        exec_context_token_from_projection_parts(
            current_return_pc,
            current_instance_raw,
            current_default_memory_present,
            current_default_memory_shared,
            current_default_memory_raw,
            current_prev_local_top,
            current_prev_local_size,
            caller_present,
            caller_return_pc,
            caller_instance_raw,
            caller_default_memory_present,
            caller_default_memory_shared,
            caller_default_memory_raw,
            caller_prev_local_top,
            caller_prev_local_size,
            cont_addr,
            task_id,
        ).caller_frame == if caller_present {
            Some(frame_view_from_projection_parts(
                caller_return_pc,
                caller_instance_raw,
                caller_default_memory_present,
                caller_default_memory_shared,
                caller_default_memory_raw,
                caller_prev_local_top,
                caller_prev_local_size,
            ))
        } else {
            None
        },
        exec_context_token_from_projection_parts(
            current_return_pc,
            current_instance_raw,
            current_default_memory_present,
            current_default_memory_shared,
            current_default_memory_raw,
            current_prev_local_top,
            current_prev_local_size,
            caller_present,
            caller_return_pc,
            caller_instance_raw,
            caller_default_memory_present,
            caller_default_memory_shared,
            caller_default_memory_raw,
            caller_prev_local_top,
            caller_prev_local_size,
            cont_addr,
            task_id,
        ).cont_addr == cont_addr,
        exec_context_token_from_projection_parts(
            current_return_pc,
            current_instance_raw,
            current_default_memory_present,
            current_default_memory_shared,
            current_default_memory_raw,
            current_prev_local_top,
            current_prev_local_size,
            caller_present,
            caller_return_pc,
            caller_instance_raw,
            caller_default_memory_present,
            caller_default_memory_shared,
            caller_default_memory_raw,
            caller_prev_local_top,
            caller_prev_local_size,
            cont_addr,
            task_id,
        ).task_id == task_id as nat,
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
            init next_waiter_id = 1;
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
            update waiters = pre.waiters.remove(ticket.waiter_id);
        }
    }

    transition!{
        consume_wake(wake: WakeTok) {
            remove wake_tok -= { wake };
            update waiters = pre.waiters.remove(wake.waiter_id);
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
        atomic_rmw(start: int, value_bytes: Seq<u8>) {
            require(0 <= start);
            require(start + value_bytes.len() <= pre.bytes.len());
            update bytes = spec_write_range(pre.bytes, start, value_bytes);
        }
    }

    transition!{
        atomic_cmpxchg(start: int, expected_bytes: Seq<u8>, value_bytes: Seq<u8>) {
            require(0 <= start);
            require(expected_bytes.len() == value_bytes.len());
            require(start + value_bytes.len() <= pre.bytes.len());

            let old_bytes = pre.bytes.subrange(start, start + value_bytes.len());
            update bytes =
                if old_bytes == expected_bytes {
                    spec_write_range(pre.bytes, start, value_bytes)
                } else {
                    pre.bytes
                };
        }
    }
});
