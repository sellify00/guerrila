// Re-export from shadow-core — canonical location for shared policy logic.
// shadow-oracle crate re-exports so existing tests and telemetry continue to
// reference `crate::target_filter::*`.
pub use shadow_core::target_filter::*;
