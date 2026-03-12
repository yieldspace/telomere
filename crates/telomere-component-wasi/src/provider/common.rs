use std::time::Duration;

pub(super) fn next_handle(slot: &mut u32) -> u32 {
    let handle = *slot;
    *slot = slot.saturating_add(1);
    handle
}

pub(super) fn nanos_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
