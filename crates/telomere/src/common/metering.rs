use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Number of checkpoints reserved by one cold-path metering grant.
pub const CHECKPOINT_CHUNK: u64 = 4096;

/// Configuration for Store-scoped guest-execution metering.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeteringConfig {
    /// Enables checkpoint metering and interruption for this store.
    pub enabled: bool,
    /// Initial finite fuel. `None` and `Some(u64::MAX)` both mean unlimited fuel.
    pub initial_fuel: Option<u64>,
}

#[derive(Debug)]
struct Ledger {
    limit: Option<u64>,
    consumed: u64,
    epoch: u64,
}

/// Shared state behind a metering handle.
///
/// `ledger` is deliberately a leaf lock: callers must not invoke VM or Store GC operations while
/// holding it. Grant, release, and `set_fuel` are cold paths, so keeping their multi-word update
/// in one critical section is preferable to a lock-free protocol with observable races.
#[derive(Debug)]
pub(crate) struct MeteringState {
    ledger: Mutex<Ledger>,
    interrupted: AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeteringGrant {
    pub(crate) reserved: u64,
    pub(crate) budget: u64,
    pub(crate) epoch: u64,
}

/// Cloneable handle for a Store with metering enabled.
///
/// It is `Send + Sync` and may be moved to a watchdog thread.
#[derive(Debug, Clone)]
pub struct MeteringHandle(Arc<MeteringState>);

impl MeteringHandle {
    pub(crate) fn new(initial_fuel: Option<u64>) -> Self {
        Self(Arc::new(MeteringState {
            ledger: Mutex::new(Ledger {
                limit: finite_limit(initial_fuel),
                consumed: 0,
                epoch: 0,
            }),
            interrupted: AtomicBool::new(false),
        }))
    }

    /// Replaces the fuel limit; `u64::MAX` switches to unlimited fuel.
    ///
    /// In-flight grants remain runnable, so after a concurrent finite `set_fuel(n)`, guest code
    /// can execute up to `n + CHECKPOINT_CHUNK - 1` further fuel units. Use [`Self::interrupt`]
    /// to request cancellation instead. This invalidates refunds from in-flight grants.
    pub fn set_fuel(&self, fuel: u64) {
        let mut ledger = self.0.ledger.lock();
        ledger.limit = finite_limit(Some(fuel));
        ledger.epoch = ledger.epoch.wrapping_add(1);
    }

    /// Returns the unreserved fuel limit, or `None` when fuel is unlimited.
    ///
    /// While guest execution is active, this is a lower bound that can be up to
    /// [`CHECKPOINT_CHUNK`] below the true remaining fuel. It is exact while idle.
    pub fn fuel_remaining(&self) -> Option<u64> {
        self.0.ledger.lock().limit
    }

    /// Returns cumulative charged fuel units since Store creation, including native bulk charges.
    ///
    /// This value is monotonic and is not reset by [`Self::set_fuel`]. While guest execution is
    /// active, it can be up to [`CHECKPOINT_CHUNK`] below the true charged fuel. It is exact while
    /// idle.
    pub fn fuel_consumed(&self) -> u64 {
        self.0.ledger.lock().consumed
    }

    /// Requests cancellation at the next metering checkpoint.
    pub fn interrupt(&self) {
        self.0.interrupted.store(true, Ordering::Relaxed);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_interrupted(&self) -> bool {
        self.0.interrupted.load(Ordering::Relaxed)
    }

    /// Clears a previously requested cancellation.
    pub fn reset_interrupt(&self) {
        self.0.interrupted.store(false, Ordering::Relaxed);
    }

    /// Charges the exhausted grant and refills the caller-owned checkpoint counters.
    ///
    /// Callers must take the hot `budget.checked_sub(1)` path themselves and invoke this only
    /// once that local budget reaches zero. Keeping the handle out of that hot path avoids a
    /// ledger lock per checkpoint. Callers that need the handle without extending its ownership
    /// can borrow it through `Store::metering_ref`.
    #[cold]
    #[inline(never)]
    pub(crate) fn refill_checkpoint_budget(
        &self,
        budget: &mut u64,
        reserved: &mut u64,
        budget_epoch: &mut u64,
    ) -> Result<(), InterruptReason> {
        let completed_reserved = std::mem::take(reserved);
        *budget = 0;
        let grant = self.grant(completed_reserved)?;
        *reserved = grant.reserved;
        *budget = grant.budget;
        *budget_epoch = grant.epoch;
        Ok(())
    }

    /// Charges `amount` checkpoints after the caller's current grant proved too small, then
    /// reserves the next chunk. This is deliberately one ledger critical section so a concurrent
    /// `set_fuel` cannot observe or create a partial charge.
    ///
    /// The caller must have already established `amount > *budget`. The current grant remains
    /// usable even when its epoch is stale; only its eventual refund is invalidated. When finite
    /// fuel cannot cover the requested amount, every available unit is consumed, the caller's
    /// counters are cleared, and the guest operation must not run.
    #[cold]
    #[inline(never)]
    pub(crate) fn charge_n(
        &self,
        budget: &mut u64,
        reserved: &mut u64,
        budget_epoch: &mut u64,
        amount: u64,
    ) -> Result<(), InterruptReason> {
        debug_assert!(amount > *budget);
        debug_assert!(*budget <= *reserved);

        let current_budget = *budget;
        let additional = amount - current_budget;
        let mut ledger = self.0.ledger.lock();

        if let Some(limit) = ledger.limit {
            if additional > limit {
                // Count both the completed portion and the remaining portion of the old grant,
                // then spend every unit that was available in the current ledger epoch. The
                // rejected bulk operation itself is not charged.
                ledger.consumed = ledger.consumed.saturating_add(*reserved);
                ledger.consumed = ledger.consumed.saturating_add(limit);
                ledger.limit = Some(0);
                *budget = 0;
                *reserved = 0;
                *budget_epoch = ledger.epoch;
                // Fuel wins over cancellation so a fixed finite budget remains deterministic.
                return Err(InterruptReason::FuelExhausted);
            }
        }

        if self.0.interrupted.load(Ordering::Relaxed) {
            return Err(InterruptReason::Cancelled);
        }

        // `reserved` includes the already-spent part of the current grant, which has not yet
        // been published to `consumed`. The remaining `additional` units are paid directly from
        // the ledger before the next reservation is made.
        ledger.consumed = ledger.consumed.saturating_add(*reserved);
        ledger.consumed = ledger.consumed.saturating_add(additional);
        if let Some(limit) = &mut ledger.limit {
            *limit -= additional;
        }

        let take = match ledger.limit {
            None => CHECKPOINT_CHUNK,
            Some(limit) => limit.min(CHECKPOINT_CHUNK),
        };
        if let Some(limit) = &mut ledger.limit {
            *limit -= take;
        }

        *reserved = take;
        *budget = take;
        *budget_epoch = ledger.epoch;
        Ok(())
    }

    /// Charges a completed grant and reserves the next one.
    pub(crate) fn grant(&self, completed_reserved: u64) -> Result<MeteringGrant, InterruptReason> {
        let mut ledger = self.0.ledger.lock();
        ledger.consumed = ledger.consumed.saturating_add(completed_reserved);

        let take = match ledger.limit {
            None => CHECKPOINT_CHUNK,
            Some(limit) => limit.min(CHECKPOINT_CHUNK),
        };
        if take == 0 {
            // Fuel wins over cancellation to keep a fixed fuel budget deterministic.
            return Err(InterruptReason::FuelExhausted);
        }
        if self.0.interrupted.load(Ordering::Relaxed) {
            return Err(InterruptReason::Cancelled);
        }
        if let Some(limit) = ledger.limit {
            ledger.limit = Some(limit - take);
        }

        Ok(MeteringGrant {
            reserved: take,
            // The checkpoint that entered this cold path has not been charged by the hot path.
            budget: take - 1,
            epoch: ledger.epoch,
        })
    }

    /// Charges used fuel and returns the unused part only when its grant still has this epoch.
    pub(crate) fn release(&self, reserved: u64, budget: u64, grant_epoch: u64) {
        debug_assert!(budget <= reserved);
        let mut ledger = self.0.ledger.lock();
        let spent = reserved.saturating_sub(budget);
        ledger.consumed = ledger.consumed.saturating_add(spent);
        if grant_epoch == ledger.epoch {
            if let Some(limit) = ledger.limit {
                ledger.limit = Some(limit.saturating_add(budget));
            }
        }
    }
}

fn finite_limit(fuel: Option<u64>) -> Option<u64> {
    fuel.filter(|fuel| *fuel != u64::MAX)
}

/// Reason a metered execution stopped.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptReason {
    /// The Store's finite fuel was exhausted.
    FuelExhausted,
    /// A watchdog requested cancellation through [`MeteringHandle::interrupt`].
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::{InterruptReason, MeteringHandle, CHECKPOINT_CHUNK};

    #[test]
    fn finite_same_epoch_grant_and_release_conserve_fuel() {
        let meter = MeteringHandle::new(Some(100));
        let grant = meter.grant(0).expect("fuel must grant a reservation");

        assert_eq!(grant.reserved, 100);
        assert_eq!(grant.budget, 99);
        meter.release(grant.reserved, 95, grant.epoch);

        assert_eq!(meter.fuel_remaining(), Some(95));
        assert_eq!(meter.fuel_consumed(), 5);
    }

    #[test]
    fn set_fuel_invalidates_old_refunds_without_losing_consumption() {
        let meter = MeteringHandle::new(Some(100));
        let grant = meter.grant(0).expect("fuel must grant a reservation");

        meter.set_fuel(10);
        meter.release(grant.reserved, 97, grant.epoch);

        assert_eq!(meter.fuel_remaining(), Some(10));
        assert_eq!(meter.fuel_consumed(), 3);
    }

    #[test]
    fn fuel_exhaustion_wins_over_cancellation() {
        let meter = MeteringHandle::new(Some(0));
        meter.interrupt();

        assert_eq!(meter.grant(0), Err(InterruptReason::FuelExhausted));
    }

    #[test]
    fn charge_n_pays_the_requested_work_then_reserves_the_remainder() {
        let meter = MeteringHandle::new(Some(100));
        let (mut budget, mut reserved, mut epoch) = (0, 0, 0);

        meter
            .charge_n(&mut budget, &mut reserved, &mut epoch, 10)
            .expect("fuel must cover the bulk charge");

        assert_eq!((budget, reserved), (90, 90));
        assert_eq!(meter.fuel_remaining(), Some(0));
        assert_eq!(meter.fuel_consumed(), 10);
        meter.release(reserved, budget, epoch);
        assert_eq!(meter.fuel_remaining(), Some(90));
        assert_eq!(meter.fuel_consumed(), 10);
    }

    #[test]
    fn charge_n_insufficient_fuel_consumes_every_available_unit() {
        let meter = MeteringHandle::new(Some(5));
        let (mut budget, mut reserved, mut epoch) = (0, 0, 0);

        assert_eq!(
            meter.charge_n(&mut budget, &mut reserved, &mut epoch, 6),
            Err(InterruptReason::FuelExhausted)
        );
        assert_eq!((budget, reserved), (0, 0));
        assert_eq!(meter.fuel_remaining(), Some(0));
        assert_eq!(meter.fuel_consumed(), 5);
    }

    #[test]
    fn charge_n_checks_cancellation_only_after_fuel_is_known_sufficient() {
        let meter = MeteringHandle::new(Some(5));
        let (mut budget, mut reserved, mut epoch) = (0, 0, 0);
        meter.interrupt();

        assert_eq!(
            meter.charge_n(&mut budget, &mut reserved, &mut epoch, 1),
            Err(InterruptReason::Cancelled)
        );
        assert_eq!((budget, reserved), (0, 0));
        assert_eq!(meter.fuel_remaining(), Some(5));
        assert_eq!(meter.fuel_consumed(), 0);

        assert_eq!(
            meter.charge_n(&mut budget, &mut reserved, &mut epoch, 6),
            Err(InterruptReason::FuelExhausted)
        );
        assert_eq!(meter.fuel_remaining(), Some(0));
        assert_eq!(meter.fuel_consumed(), 5);
    }

    #[test]
    fn charge_n_uses_a_stale_grant_but_only_refunds_the_new_epoch() {
        let meter = MeteringHandle::new(Some(10));
        let (mut budget, mut reserved, mut epoch) = (0, 0, 0);
        meter
            .refill_checkpoint_budget(&mut budget, &mut reserved, &mut epoch)
            .expect("initial grant must succeed");
        assert_eq!((budget, reserved), (9, 10));

        meter.set_fuel(5);
        meter
            .charge_n(&mut budget, &mut reserved, &mut epoch, 10)
            .expect("the stale grant plus new fuel must remain usable");

        assert_eq!((budget, reserved), (4, 4));
        assert_eq!(meter.fuel_remaining(), Some(0));
        assert_eq!(meter.fuel_consumed(), 11);
        meter.release(reserved, budget, epoch);
        assert_eq!(meter.fuel_remaining(), Some(4));
        assert_eq!(meter.fuel_consumed(), 11);
    }

    #[test]
    fn observation_gap_is_bounded_by_the_current_reservation() {
        let meter = MeteringHandle::new(Some(CHECKPOINT_CHUNK + 7));
        let (mut budget, mut reserved, mut epoch) = (0, 0, 0);
        meter
            .refill_checkpoint_budget(&mut budget, &mut reserved, &mut epoch)
            .expect("initial grant must succeed");

        let remaining = meter.fuel_remaining().expect("finite fuel expected");
        let consumed = meter.fuel_consumed();
        let gap = CHECKPOINT_CHUNK + 7 - remaining - consumed;
        assert_eq!(gap, reserved);
        assert!(gap <= CHECKPOINT_CHUNK);
    }
}
