use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TacticFamily {
    Atba,
    ApexPartial,
    PlainWithinCap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteFamily {
    Jito,
    Harmonic,
}

/// A fixed-width composite key determining which intents compete against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DominanceKey {
    pub pool_index: u32,
    pub next_scheduled_slot: u64,
    pub direction: u8,
    pub tactic_family: TacticFamily,
    pub route_family: RouteFamily,
}

/// The state of the current incumbent winner, restricted purely to this worker thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct DominanceIncumbent {
    pub active: bool,
    pub base_state_unix_ns: u64,
    pub key: Option<DominanceKey>,
    pub retained_lcb_lamports: u64,
}

thread_local! {
    /// Zero-allocation, thread-local incumbent tracker strictly forbidding cross-worker locks.
    static INCUMBENT: RefCell<DominanceIncumbent> = RefCell::new(DominanceIncumbent::default());
}

/// The outcome of the dominance gate evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DominanceResult {
    /// Beat the incumbent or incomparable (different base state).
    Pass,
    /// Did not beat the incumbent enough, but engine is in observe-only mode.
    WouldDropObserveOnly,
    /// Did not beat the incumbent enough, engine is live => candidate suppressed.
    DropLive,
}

/// Evaluates a candidate execution against the thread-local incumbent.
#[inline(always)]
pub fn evaluate_and_update_dominance<F>(
    observe_only: bool,
    base_state_unix_ns: u64,
    key: DominanceKey,
    candidate_retained_lcb: u64,
    min_incremental_retained_lcb_lamports: u64,
    mut on_would_drop_log: F,
) -> DominanceResult
where
    F: FnMut(),
{
    INCUMBENT.with(|incumbent_cell| {
        let mut incumbent = incumbent_cell.borrow_mut();

        // 1. If the base state shifted, the incumbent is void. Pass and overwrite.
        if incumbent.base_state_unix_ns != base_state_unix_ns {
            *incumbent = DominanceIncumbent {
                active: true,
                base_state_unix_ns,
                key: Some(key),
                retained_lcb_lamports: candidate_retained_lcb,
            };
            return DominanceResult::Pass;
        }

        // 2. If the competitive key differs entirely (e.g. different pool or route), pass.
        // Ambiguous comparison, no suppression.
        if incumbent.key != Some(key) {
            *incumbent = DominanceIncumbent {
                active: true,
                base_state_unix_ns,
                key: Some(key),
                retained_lcb_lamports: candidate_retained_lcb,
            };
            return DominanceResult::Pass;
        }

        // 3. Same base state, same competitive key -> direct comparison.
        let threshold = incumbent
            .retained_lcb_lamports
            .saturating_add(min_incremental_retained_lcb_lamports);

        if candidate_retained_lcb >= threshold {
            // Beat the incumbent. Claim the slot.
            incumbent.retained_lcb_lamports = candidate_retained_lcb;
            DominanceResult::Pass
        } else {
            // Failed to beat incumbent.
            if observe_only {
                on_would_drop_log();
                DominanceResult::WouldDropObserveOnly
            } else {
                DominanceResult::DropLive
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_key() -> DominanceKey {
        DominanceKey {
            pool_index: 7,
            next_scheduled_slot: 1000,
            direction: 0,
            tactic_family: TacticFamily::Atba,
            route_family: RouteFamily::Jito,
        }
    }

    // Reset thread-local before each test
    fn reset_incumbent() {
        INCUMBENT.with(|i| {
            *i.borrow_mut() = DominanceIncumbent::default();
        });
    }

    #[test]
    fn test_dominance_base_state_shift_overwrites() {
        reset_incumbent();
        // First play
        let mut dropped = false;
        let r1 = evaluate_and_update_dominance(false, 100, base_key(), 500, 10, || dropped = true);
        assert_eq!(r1, DominanceResult::Pass);

        // State shifted to 101, even with lower LCB it passes and overwrites
        let r2 = evaluate_and_update_dominance(false, 101, base_key(), 10, 10, || dropped = true);
        assert_eq!(r2, DominanceResult::Pass);
    }

    #[test]
    fn test_dominance_live_suppression_on_insufficient_bid() {
        reset_incumbent();
        evaluate_and_update_dominance(false, 100, base_key(), 500, 10, || {});

        // Bid 508. Incumbent is 500, threshold is 510. Fails live.
        let r = evaluate_and_update_dominance(false, 100, base_key(), 508, 10, || {});
        assert_eq!(r, DominanceResult::DropLive);
    }

    #[test]
    fn test_dominance_observe_only_log() {
        reset_incumbent();
        evaluate_and_update_dominance(true, 100, base_key(), 500, 10, || {});

        let mut dropped = false;
        // Bid 508. Observe only. Returns WouldDrop, but triggers closure.
        let r = evaluate_and_update_dominance(true, 100, base_key(), 508, 10, || dropped = true);
        assert_eq!(r, DominanceResult::WouldDropObserveOnly);
        assert!(dropped);
    }

    #[test]
    fn test_dominance_key_ambiguity_passes() {
        reset_incumbent();
        evaluate_and_update_dominance(false, 100, base_key(), 500, 10, || {});

        let mut alt_key = base_key();
        alt_key.pool_index = 8; // Different pool

        let r = evaluate_and_update_dominance(false, 100, alt_key, 10, 10, || {});
        assert_eq!(r, DominanceResult::Pass); // Did not collide
    }
}

#[cfg(test)]
pub fn reset_incumbent_for_test() {
    INCUMBENT.with(|i| {
        *i.borrow_mut() = DominanceIncumbent::default();
    });
}

pub fn reset_incumbent_for_diagnostics() {
    INCUMBENT.with(|i| {
        *i.borrow_mut() = DominanceIncumbent::default();
    });
}
