// gRPC channel creation with optional TLS.
//
// Adapted from the jito-labs/searcher-examples `create_grpc_channel` pattern.
// Uses tonic's built-in TLS with webpki roots (no system cert dependency).

use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

/// Create a tonic `Channel` to the Jito block engine.
///
/// - `url`: gRPC endpoint (e.g. `https://mainnet.block-engine.jito.wtf`)
/// - `tls_enabled`: force TLS even if scheme is not `https`
/// - `connect_timeout_ms`: TCP + TLS handshake deadline
pub async fn create_channel(
    url: &str,
    tls_enabled: bool,
    connect_timeout_ms: u64,
) -> Result<Channel, tonic::transport::Error> {
    let mut endpoint = Endpoint::from_shared(url.to_string())?
        .connect_timeout(Duration::from_millis(connect_timeout_ms))
        .tcp_keepalive(Some(Duration::from_secs(30)));

    if tls_enabled || url.starts_with("https") {
        let tls = ClientTlsConfig::new().with_webpki_roots();
        endpoint = endpoint.tls_config(tls)?;
    }

    endpoint.connect().await
}
