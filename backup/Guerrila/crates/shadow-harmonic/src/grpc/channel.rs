// gRPC channel creation with optional TLS.
//
// Same pattern as shadow-egress-jito — tonic with webpki roots.
// Used once per regional endpoint during connect.

use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

/// Create a tonic `Channel` to a Harmonic regional endpoint.
///
/// - `url`: gRPC endpoint (e.g. `https://fra.harmonic.xyz`)
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
