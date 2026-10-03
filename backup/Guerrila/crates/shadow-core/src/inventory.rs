use std::sync::atomic::{AtomicU64, Ordering};

/// Error indicating the requested intent size breaches the allowed inventory ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryReservationError {
    BreachedFence,
}

/// A lightweight reservation primitive supporting the strict zero-allocation logic.
pub struct InventoryFencer<'a> {
    pub current_allocation: &'a AtomicU64,
    pub absolute_ceiling: u64,
}

impl<'a> InventoryFencer<'a> {
    /// Attempts to securely reserve the amount of lamports for an intent.
    /// If the `fetch_add` causes the baseline to overflow `absolute_ceiling`,
    /// it unconditionally and instantaneously rolls back via `fetch_sub` and returns an error.
    #[inline(always)]
    pub fn reserve_intent(
        &self,
        trade_size_lamports: u64,
    ) -> Result<u64, InventoryReservationError> {
        let prev = self
            .current_allocation
            .fetch_add(trade_size_lamports, Ordering::AcqRel);
        let new_balance = prev.saturating_add(trade_size_lamports);

        if new_balance > self.absolute_ceiling {
            // Immediate rollback on breach to maintain non-stale availability.
            self.current_allocation
                .fetch_sub(trade_size_lamports, Ordering::AcqRel);
            Err(InventoryReservationError::BreachedFence)
        } else {
            // Success. Reconcile returning the reserved unscaled notional sizing.
            Ok(trade_size_lamports)
        }
    }

    /// Reverts a previously secured reservation, e.g. if egress serialization fails downstream.
    #[inline(always)]
    pub fn rollback_intent(&self, reserved_amount: u64) {
        self.current_allocation
            .fetch_sub(reserved_amount, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_reserve_success() {
        let alloc = AtomicU64::new(0);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: 1000,
        };

        // Reserving 500 under a 1000 fence
        let res = fencer.reserve_intent(500);
        assert_eq!(res.unwrap(), 500);
        assert_eq!(alloc.load(Ordering::SeqCst), 500);

        // Rolling back
        fencer.rollback_intent(500);
        assert_eq!(alloc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_inventory_reserve_breach_rollback() {
        let alloc = AtomicU64::new(800);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: 1000,
        };

        // Reserving 500 hits 1300, breaching 1000 fence
        let res = fencer.reserve_intent(500);
        assert_eq!(res.unwrap_err(), InventoryReservationError::BreachedFence);

        // Assert immediate exactly-once rollback down to 800
        assert_eq!(alloc.load(Ordering::SeqCst), 800);
    }
}
