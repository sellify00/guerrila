//! shadow-tier0 library crate.
//!
//! The primary entry point is the `main` binary (`src/main.rs`).
//! This library re-exports modules needed by auxiliary binaries
//! (`g4_ingress_seal`, `g4_shred_replay`).
//!
//! The main binary also declares these modules via its own `mod`
//! statements — they compile separately under each crate root.

pub mod afxdp_producer;
pub mod blocking_http_client;
pub mod boot_exec;
pub mod file_allowlist;
pub mod g4_seal_stats;
pub mod grpc_diag;
pub mod grpc_normalize;
pub mod grpc_producer;
pub mod ingress_control;
pub mod money_worker;
pub mod no_send_sink;
pub mod nonce_producer;
pub mod pool_mint_cache;
pub mod rollout_caps;
pub mod rollout_gate_export;
pub mod rpc_bundle_source;
pub mod rpc_sig_client;
pub mod scorecard_replay;
pub mod shm_alt_resolver;
pub mod shm_resolvers;
pub mod sig_dedupe;
pub mod source_arbiter_timing;
pub mod tail_latency;

#[cfg(target_os = "linux")]
pub mod xsk;

#[cfg(target_os = "linux")]
pub mod afxdp_runtime;

#[cfg(all(target_os = "linux", feature = "dpdk"))]
pub mod dpdk_producer;

#[cfg(all(target_os = "linux", feature = "dpdk"))]
pub mod dpdk_runtime;

/// Pipeline dependencies shared across all ingress threads.
///
/// Loaded once at boot; immutable and thread-safe.
pub struct PipelineDeps {
    pub allowlist: std::sync::Arc<dyn shadow_ingest::allowlist::AllowlistLookup + Send + Sync>,
    pub alt_resolver: std::sync::Arc<dyn shadow_ingest::alt_expand::AltResolver + Send + Sync>,
    pub mint_resolver: std::sync::Arc<dyn shadow_ingest::mint_resolve::MintResolver + Send + Sync>,
}
