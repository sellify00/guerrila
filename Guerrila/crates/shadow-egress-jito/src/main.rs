// G6 – shadow-egress-jito binary
//
// Dedicated Jito egress proxy process.
// Opens per-worker egress rings, drains them, frames bundles, and sends
// via persistent Searcher API sessions.
//
// Startup contract:
//   - Opens (not creates) all shared-memory resources via ProxyResources::open().
//   - Fails closed on any open or version/segment-ID mismatch.
//   - Ring creation is bootstrap's job (shadow-core startup).
//
// Constraints:
//   - No BundleState telemetry parsing (PM owns that).
//   - No public TPU/QUIC fallback.
//   - Publishes coarse Jito lane health to /dev/shm/shadow_egress_health.
//   - No second drop/release queue; proxy-side failures go into the
//     existing NACK ring only.

use clap::Parser;
use shadow_egress_jito::config::{AuthMode, JitoEgressConfig};
use shadow_egress_jito::proxy::{JitoProxy, ProxyResources};
use shadow_egress_jito::route_mux_session::RouteMuxSession;
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(
    name = "shadow-egress-jito",
    about = "Shadow Engine G6 Jito egress proxy"
)]
struct Cli {
    /// Jito block engine gRPC endpoint.
    #[arg(long, default_value = "https://mainnet.block-engine.jito.wtf")]
    block_engine_url: String,

    /// Number of money workers whose egress rings to drain.
    #[arg(long, default_value_t = 4)]
    money_workers: u32,

    /// Auth mode: "no_auth" or path to a Solana JSON keypair file.
    #[arg(long, default_value = "no_auth")]
    auth_mode: String,

    /// Enable TLS for the gRPC channel.
    #[arg(long, default_value_t = true)]
    tls_enabled: bool,

    /// Consecutive failures before degrading the Jito lane.
    #[arg(long, default_value_t = 3)]
    degrade_threshold: u32,

    /// Consecutive failures before marking the Jito lane down.
    #[arg(long, default_value_t = 10)]
    down_threshold: u32,

    /// Consecutive success batches required to recover from degraded/down.
    #[arg(long, default_value_t = 2)]
    recover_batches: u32,
}

fn main() {
    let cli = Cli::parse();

    let auth_mode = if cli.auth_mode == "no_auth" {
        AuthMode::NoAuth
    } else {
        AuthMode::AuthKeypair(PathBuf::from(&cli.auth_mode))
    };

    let config = JitoEgressConfig {
        block_engine_url: cli.block_engine_url.clone(),
        money_workers: cli.money_workers,
        auth_mode,
        tls_enabled: cli.tls_enabled,
        degrade_failure_threshold: cli.degrade_threshold,
        down_failure_threshold: cli.down_threshold,
        hysteresis_recover_batches: cli.recover_batches,
        degraded_hysteresis_batches: 2,
        ..JitoEgressConfig::default()
    };

    println!("shadow-egress-jito starting");
    println!("  block_engine_url: {}", config.block_engine_url);
    println!("  money_workers:    {}", config.money_workers);
    println!("  auth_mode:        {:?}", config.auth_mode);
    println!("  tls_enabled:      {}", config.tls_enabled);
    println!();

    // Open all shared-memory resources.  Fail closed on any error.
    let resources = match ProxyResources::open(&config) {
        Ok(r) => {
            println!(
                "  opened {} egress ring(s), NACK ring, health segment",
                r.egress_rings.len()
            );
            r
        }
        Err(e) => {
            eprintln!("FATAL  startup contract violated: {e}");
            process::exit(1);
        }
    };

    // RouteMuxSession internally uses the real GrpcSearcherSession for Jito
    // bundle traffic while allowing Sender/Public-RPC envelopes to share the
    // same proxy loop when configured.
    let session = RouteMuxSession::new(config.clone());

    println!();
    println!("shadow-egress-jito entering run loop");

    let mut proxy = JitoProxy::new(config, session);
    proxy.run(&resources);
}
