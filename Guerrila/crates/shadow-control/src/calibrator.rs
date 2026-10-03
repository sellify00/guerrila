use crate::tripwire::TripwireOutcome;

#[derive(Debug, Clone, Copy, Default)]
pub struct CalibratorDecision {
    pub active: bool,
}

pub fn evaluate(tripwire: &TripwireOutcome) -> CalibratorDecision {
    CalibratorDecision {
        active: tripwire.requires_repair(),
    }
}
