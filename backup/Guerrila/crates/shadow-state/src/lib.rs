//! # shadow-state — off-core state daemon
//!
//! shadow-state is a separate daemon because workers must not fetch pools,
//! mints, ALTs, fees, or leader schedule over the network; those inputs have
//! to be maintained off the money cores and published through shared memory.
//!
//! ## G3 minimum contract
//!
//! 1. **Premium shared Geyser/RPC source** — subscribe to account updates and
//!    slot notifications via the shared Geyser gRPC endpoint.
//! 2. **Monotonic `snapshot_slot`** — every health publish carries
//!    `snapshot_slot` ≥ previous; workers gate on this for freshness.
//! 3. **Live Token-2022 transfer-fee streaming** — decode
//!    `TransferFeeConfig` extensions on every mint update so workers never
//!    use a stale fee schedule.
//! 4. **ALT snapshot** — maintain the decoded address lookup tables that
//!    ingress later consults for transaction compilation.
//! 5. **Leader schedule / class tags** — publish per-slot `EngineTag` and
//!    `leader_class_id` for routing decisions.
//!
//! ## Excluded from G3
//!
//! - AF_XDP / G4 ingress logic
//! - Harmonic proxy / G7
//! - Full CLMM tick arrays to workers
//! - Taylor replacing exact reserve state
//! - Hot-path ALT expansion or worker-side RPC

pub mod alt_hydrator;
pub mod clmm_quote;
pub mod clmm_quote_probe;
pub mod config;
pub mod decoder;
pub mod health;
pub mod hot_pool_hydrator;
pub mod leader_schedule;
pub mod publish;
pub mod source;
pub mod store;
pub mod trace;

#[cfg(test)]
mod tests;
