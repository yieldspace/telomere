use super::WasiHost;
use crate::bindings::imports::{
    wasi_clocks_monotonic_clock as clocks_monotonic, wasi_clocks_wall_clock as clocks_wall,
};
use crate::bindings::types::{WasiClocksWallClockDatetime, WasiIoPollPollable};
use crate::state::PollableEntry;
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

pub(super) fn add_to_linker_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    clocks_monotonic::add_to_linker(linker, Rc::clone(&host));
    clocks_wall::add_to_linker(linker, host);
}

pub(super) fn add_to_linker_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    clocks_monotonic::add_to_linker_async(linker, Rc::clone(&host));
    clocks_wall::add_to_linker_async(linker, host);
}

impl WasiHost {
    pub(super) fn wall_clock_now(&self) -> WasiClocksWallClockDatetime {
        let now = (self.state.inner.borrow().wall_clock)();
        let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        WasiClocksWallClockDatetime {
            seconds: duration.as_secs(),
            nanoseconds: duration.subsec_nanos(),
        }
    }

    pub(super) fn wall_clock_resolution(&self) -> WasiClocksWallClockDatetime {
        WasiClocksWallClockDatetime {
            seconds: 0,
            nanoseconds: 1,
        }
    }

    pub(super) fn monotonic_now(&self) -> u64 {
        super::common::nanos_u64((self.state.inner.borrow().monotonic_clock)())
    }

    pub(super) fn monotonic_resolution(&self) -> u64 {
        1
    }

    pub(super) fn monotonic_subscribe_instant(&self, when: u64) -> WasiIoPollPollable {
        self.allocate_pollable(PollableEntry::MonotonicDeadline(Duration::from_nanos(when)))
    }

    pub(super) fn monotonic_subscribe_duration(&self, when: u64) -> WasiIoPollPollable {
        let deadline = (self.state.inner.borrow().monotonic_clock)() + Duration::from_nanos(when);
        self.allocate_pollable(PollableEntry::MonotonicDeadline(deadline))
    }
}

impl clocks_wall::Host for WasiHost {
    fn now(&self, _store: &Store) -> Result<WasiClocksWallClockDatetime, ComponentError> {
        Ok(self.wall_clock_now())
    }

    fn resolution(&self, _store: &Store) -> Result<WasiClocksWallClockDatetime, ComponentError> {
        Ok(self.wall_clock_resolution())
    }
}

impl clocks_wall::HostAsync for WasiHost {
    fn now<'a>(
        &'a self,
        _store: &'a Store,
    ) -> ComponentFuture<'a, Result<WasiClocksWallClockDatetime, ComponentError>> {
        Box::pin(async move { Ok(self.wall_clock_now()) })
    }

    fn resolution<'a>(
        &'a self,
        _store: &'a Store,
    ) -> ComponentFuture<'a, Result<WasiClocksWallClockDatetime, ComponentError>> {
        Box::pin(async move { Ok(self.wall_clock_resolution()) })
    }
}

impl clocks_monotonic::Host for WasiHost {
    fn now(&self, _store: &Store) -> Result<u64, ComponentError> {
        Ok(self.monotonic_now())
    }

    fn resolution(&self, _store: &Store) -> Result<u64, ComponentError> {
        Ok(self.monotonic_resolution())
    }

    fn subscribe_instant(
        &self,
        _store: &Store,
        when: u64,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        Ok(self.monotonic_subscribe_instant(when))
    }

    fn subscribe_duration(
        &self,
        _store: &Store,
        when: u64,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        Ok(self.monotonic_subscribe_duration(when))
    }
}

impl clocks_monotonic::HostAsync for WasiHost {
    fn now<'a>(&'a self, _store: &'a Store) -> ComponentFuture<'a, Result<u64, ComponentError>> {
        Box::pin(async move { Ok(self.monotonic_now()) })
    }

    fn resolution<'a>(
        &'a self,
        _store: &'a Store,
    ) -> ComponentFuture<'a, Result<u64, ComponentError>> {
        Box::pin(async move { Ok(self.monotonic_resolution()) })
    }

    fn subscribe_instant<'a>(
        &'a self,
        _store: &'a Store,
        when: u64,
    ) -> ComponentFuture<'a, Result<WasiIoPollPollable, ComponentError>> {
        Box::pin(async move { Ok(self.monotonic_subscribe_instant(when)) })
    }

    fn subscribe_duration<'a>(
        &'a self,
        _store: &'a Store,
        when: u64,
    ) -> ComponentFuture<'a, Result<WasiIoPollPollable, ComponentError>> {
        Box::pin(async move { Ok(self.monotonic_subscribe_duration(when)) })
    }
}
