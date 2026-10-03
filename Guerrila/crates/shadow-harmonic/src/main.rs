// G7 – shadow-egress-harmonic binary
//
// Dedicated Harmonic egress proxy process.
// Opens per-worker egress rings, drains them, frames bundles, and sends
// via persistent gRPC sessions to all configured Harmonic regions.
//
// Startup contract:
//   - Opens (not creates) all shared-memory resources via ProxyResources::open().
//   - Fails closed on any open or version/segment-ID mismatch.
//   - Ring creation is bootstrap's job (shadow-core startup).
//
// Constraints:
//   - Death Clamp: any send failure → drop + NACK; no TPU/public fallback.
//   - No BundleState telemetry parsing (PM owns that).
//   - Publishes coarse Harmonic lane health to /dev/shm/shadow_egress_health.
//   - Publishes fixed-width EgressResult to /dev/shm/shadow_egress_result.
//   - No second drop/release queue; proxy-side failures go into the
//     existing NACK ring only.

use clap::Parser;
use shadow_harmonic::config::{AuthMode, HarmonicEgressConfig};
use shadow_harmonic::proxy::{HarmonicProxy, ProxyResources};
use shadow_harmonic::session::GrpcHarmonicSession;
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(
    name = "shadow-egress-harmonic",
    about = "Shadow Engine G7 Harmonic egress proxy"
)]
struct Cli {
    /// Comma-separated Harmonic regional gRPC endpoints.
    #[arg(long, value_delimiter = ',')]
    region_endpoints: Vec<String>,

    /// Number of money workers whose egress rings to drain.
    #[arg(long, default_value_t = 4)]
    money_workers: u32,

    /// Auth mode: "no_auth" or path to a Solana JSON keypair file.
    #[arg(long, default_value = "no_auth")]
    auth_mode: String,

    /// Connection timeout per regional gRPC channel (ms).
    #[arg(long, default_value_t = 5000)]
    connect_timeout_ms: u64,

    /// Per-send timeout for SendBundle RPC (ms).
    #[arg(long, default_value_t = 3000)]
    send_timeout_ms: u64,

    /// Initial backoff (ms) before first reconnect attempt after a drop.
    #[arg(long, default_value_t = 250)]
    reconnect_initial_backoff_ms: u64,

    /// Maximum backoff (ms) cap for exponential reconnect jitter.
    #[arg(long, default_value_t = 10000)]
    reconnect_max_backoff_ms: u64,

    /// Enable TLS on all regional channels.
    #[arg(long, default_value_t = true)]
    tls_enabled: bool,

    /// Consecutive failures before degrading the Harmonic lane.
    #[arg(long, default_value_t = 3)]
    degrade_threshold: u32,

    /// Consecutive failures before marking the Harmonic lane down.
    #[arg(long, default_value_t = 10)]
    down_threshold: u32,

    /// Consecutive success batches required to recover from degraded/down.
    #[arg(long, default_value_t = 2)]
    recover_batches: u32,

    /// Hysteresis batches value published in the egress health segment.
    #[arg(long, default_value_t = 2)]
    degraded_hysteresis_batches: u16,
}

fn main() {
    let cli = Cli::parse();

    if cli.region_endpoints.is_empty() {
        eprintln!("FATAL  no --region-endpoints provided");
        process::exit(1);
    }

    let auth_mode = if cli.auth_mode == "no_auth" {
        AuthMode::NoAuth
    } else {
        AuthMode::AuthKeypair(PathBuf::from(&cli.auth_mode))
    };

    let config = HarmonicEgressConfig {
        region_endpoints: cli.region_endpoints.clone(),
        money_workers: cli.money_workers,
        auth_mode,
        connect_timeout_ms: cli.connect_timeout_ms,
        send_timeout_ms: cli.send_timeout_ms,
        reconnect_initial_backoff_ms: cli.reconnect_initial_backoff_ms,
        reconnect_max_backoff_ms: cli.reconnect_max_backoff_ms,
        tls_enabled: cli.tls_enabled,
        degrade_failure_threshold: cli.degrade_threshold,
        down_failure_threshold: cli.down_threshold,
        hysteresis_recover_batches: cli.recover_batches,
        degraded_hysteresis_batches: cli.degraded_hysteresis_batches,
    };

    eprintln!("shadow-egress-harmonic starting");
    eprintln!("  region_endpoints: {:?}", config.region_endpoints);
    eprintln!("  money_workers:    {}", config.money_workers);
    eprintln!("  auth_mode:        {:?}", config.auth_mode);
    eprintln!("  tls_enabled:      {}", config.tls_enabled);
    eprintln!(
        "  reconnect:        {}ms init, {}ms max",
        config.reconnect_initial_backoff_ms, config.reconnect_max_backoff_ms
    );
    eprintln!();

    // Open all shared-memory resources. Fail closed on any error.
    let resources = match ProxyResources::open(&config) {
        Ok(r) => {
            eprintln!(
                "  opened {} egress ring(s), NACK ring, health segment, result ring",
                r.egress_rings.len()
            );
            r
        }
        Err(e) => {
            eprintln!("FATAL  startup contract violated: {e}");
            process::exit(1);
        }
    };

    // Create the real multi-region gRPC session.
    // Pre-warms: spawns connect+auth for all regions immediately.
    let session = GrpcHarmonicSession::new(config.clone());

    eprintln!();
    eprintln!("shadow-egress-harmonic entering run loop");

    let mut proxy = HarmonicProxy::new(config, session);
    proxy.run(&resources);
}
