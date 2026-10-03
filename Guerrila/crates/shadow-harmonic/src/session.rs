// G7 – Harmonic session abstraction
//
// Mirrors the SearcherSession trait from shadow-egress-jito, adapted for
// Harmonic's regional fanout model.
//
// GrpcHarmonicSession manages one gRPC connection per configured region.
// send_bundle fans out to all ready regions and aggregates results into
// a RegionalResult bitmask.  Reconnect, auth, and token refresh are
// non-blocking — only send_bundle blocks the drain loop, bounded by
// send_timeout_ms.

/// Session state shared across the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Down,
    Connecting,
    Up,
    Degraded,
}

/// Errors that `send_bundle` may return.
#[derive(Debug, Clone)]
pub enum SendError {
    /// No session is connected.
    NotConnected,
    /// RPC timed out.
    Timeout,
    /// Transport-level gRPC error.
    Transport(String),
    /// Server rejected the bundle.
    Rejected(String),
    /// All regions failed (Death Clamp condition).
    /// Carries the attempted region bitmask and first error code so the
    /// proxy can publish accurate EgressResult detail on the death-clamp path.
    AllRegionsFailed { region_mask: u8, err_code: u8 },
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::NotConnected => write!(f, "not connected"),
            SendError::Timeout => write!(f, "timeout"),
            SendError::Transport(s) => write!(f, "transport: {s}"),
            SendError::Rejected(s) => write!(f, "rejected: {s}"),
            SendError::AllRegionsFailed {
                region_mask,
                err_code,
            } => {
                write!(
                    f,
                    "all regions failed (mask=0x{region_mask:02x}, err={err_code})"
                )
            }
        }
    }
}

/// A bundle request carrying raw transaction bytes.
pub struct BundleRequest<'a> {
    pub tx_bytes: &'a [u8],
}

impl<'a> BundleRequest<'a> {
    pub fn single_tx(tx_bytes: &'a [u8]) -> Self {
        Self { tx_bytes }
    }
}

/// Regional fanout result from a single send attempt.
#[derive(Debug, Clone, Copy)]
pub struct RegionalResult {
    /// Bitmask of regions attempted.
    pub region_mask: u8,
    /// Bitmask of regions that acknowledged success.
    pub ok_mask: u8,
    /// Error code from first failing region (0 on full success).
    pub err_code: u8,
    /// Bundle UUID from the first successful region (16 raw bytes).
    /// All zeros if no UUID was captured.
    pub bundle_uuid: [u8; 16],
}

/// Session trait for Harmonic endpoints.
///
/// Implementations manage one or more regional gRPC connections.
/// The proxy calls `poll()` to drive reconnect, `can_send()` to gate
/// dequeue, and `send_bundle()` to transmit.
pub trait HarmonicSession {
    fn state(&self) -> SessionState;

    fn can_send(&self) -> bool {
        matches!(self.state(), SessionState::Up | SessionState::Degraded)
    }

    /// Send a bundle to all available regions. Returns regional outcome.
    fn send_bundle(&mut self, request: &BundleRequest) -> Result<RegionalResult, SendError>;

    /// Drive reconnect / keepalive. Must never block.
    fn poll(&mut self);
}

// ---------------------------------------------------------------------------
// StubSession — always Down, for testing fail-closed behavior
// ---------------------------------------------------------------------------

pub struct StubSession;

impl HarmonicSession for StubSession {
    fn state(&self) -> SessionState {
        SessionState::Down
    }

    fn send_bundle(&mut self, _request: &BundleRequest) -> Result<RegionalResult, SendError> {
        Err(SendError::NotConnected)
    }

    fn poll(&mut self) {}
}

// ---------------------------------------------------------------------------
// MockSession — controllable state for unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockSession {
    pub session_state: SessionState,
    pub failure: Option<SendError>,
    pub sent_count: u64,
    pub region_count: u8,
}

#[cfg(test)]
impl MockSession {
    pub fn connected() -> Self {
        Self {
            session_state: SessionState::Up,
            failure: None,
            sent_count: 0,
            region_count: 2,
        }
    }

    pub fn disconnected() -> Self {
        Self {
            session_state: SessionState::Down,
            failure: None,
            sent_count: 0,
            region_count: 2,
        }
    }

    pub fn connecting() -> Self {
        Self {
            session_state: SessionState::Connecting,
            failure: None,
            sent_count: 0,
            region_count: 2,
        }
    }

    pub fn degraded() -> Self {
        Self {
            session_state: SessionState::Degraded,
            failure: None,
            sent_count: 0,
            region_count: 2,
        }
    }

    pub fn with_failure(err: SendError) -> Self {
        Self {
            session_state: SessionState::Up,
            failure: Some(err),
            sent_count: 0,
            region_count: 2,
        }
    }
}

#[cfg(test)]
impl HarmonicSession for MockSession {
    fn state(&self) -> SessionState {
        self.session_state
    }

    fn send_bundle(&mut self, _request: &BundleRequest) -> Result<RegionalResult, SendError> {
        if let Some(ref err) = self.failure {
            return Err(err.clone());
        }
        self.sent_count += 1;
        let mask = (1u8 << self.region_count) - 1;
        Ok(RegionalResult {
            region_mask: mask,
            ok_mask: mask,
            err_code: 0,
            bundle_uuid: [0; 16],
        })
    }

    fn poll(&mut self) {}
}

// ---------------------------------------------------------------------------
// CapturingSession — captures sent bytes for immutability tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct CapturingSession {
    pub captured: Vec<Vec<u8>>,
}

#[cfg(test)]
impl CapturingSession {
    pub fn new() -> Self {
        Self {
            captured: Vec::new(),
        }
    }
}

#[cfg(test)]
impl HarmonicSession for CapturingSession {
    fn state(&self) -> SessionState {
        SessionState::Up
    }

    fn send_bundle(&mut self, request: &BundleRequest) -> Result<RegionalResult, SendError> {
        self.captured.push(request.tx_bytes.to_vec());
        Ok(RegionalResult {
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
            bundle_uuid: [0; 16],
        })
    }

    fn poll(&mut self) {}
}

// ---------------------------------------------------------------------------
// GrpcHarmonicSession — real multi-region gRPC client
// ---------------------------------------------------------------------------

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::{AuthMode, HarmonicEgressConfig};
use crate::grpc::auth::{self, AuthInterceptor, AuthToken};
use crate::grpc::proto::{bundle, packet, searcher, shared};
use crate::grpc::service::{AuthServiceClient, SearcherServiceClient, SearcherServiceClientNoAuth};

use ed25519_dalek::SigningKey;
use tonic::transport::Channel;

/// Parse a UUID string (with or without hyphens) into 16 raw bytes.
/// Returns all zeros on malformed input.
fn parse_uuid_to_bytes(s: &str) -> [u8; 16] {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return [0; 16];
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = match u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16) {
            Ok(b) => b,
            Err(_) => return [0; 16],
        };
    }
    out
}

/// Maximum number of regional endpoints supported.
pub const MAX_REGIONS: usize = 8;

/// Result of an async connect + auth attempt for one region.
pub(crate) enum RegionConnectResult {
    Auth {
        channel: Channel,
        token: AuthToken,
        refresh_token_value: String,
        token_expires_at: Instant,
    },
    NoAuth {
        channel: Channel,
    },
    Failed(String),
}

/// Active per-region session state.
enum RegionReadyState {
    Authenticated {
        client: SearcherServiceClient<AuthInterceptor>,
        token: AuthToken,
        refresh_token_value: String,
        token_expires_at: Instant,
    },
    Unauthenticated {
        client: SearcherServiceClientNoAuth,
    },
}

/// Internal connectivity state for one regional endpoint.
enum RegionInner {
    Disconnected {
        backoff_until: Option<Instant>,
        current_backoff_ms: u64,
    },
    Connecting {
        result_rx: std::sync::mpsc::Receiver<RegionConnectResult>,
        backoff_ms: u64,
    },
    Ready(RegionReadyState),
}

/// Per-region session managing one gRPC connection.
struct RegionSession {
    /// The endpoint URL for this region.
    endpoint: String,
    /// Region index (0-based) — used for bitmask computation.
    index: u8,
    inner: RegionInner,
    /// Coarse state for this region.
    state: SessionState,
    /// In-flight token refresh receiver (non-blocking poll).
    pending_refresh: Option<std::sync::mpsc::Receiver<Option<RefreshResult>>>,
}

/// Token refresh result.
pub(crate) struct RefreshResult {
    pub(crate) new_value: String,
    pub(crate) expires_at: Instant,
}

/// Real multi-region Harmonic session implementing `HarmonicSession`.
///
/// Manages one `RegionSession` per configured endpoint.  The aggregate
/// session state is derived from the best regional state:
///   - Any region Up → aggregate Up
///   - Any region Degraded (none Up) → aggregate Degraded
///   - Any region Connecting (none Up/Degraded) → aggregate Connecting
///   - All regions Down → aggregate Down
///
/// `send_bundle()` fans out to all Ready regions and aggregates results
/// into a `RegionalResult`.  If no regions are Ready, returns
/// `Err(SendError::NotConnected)`.  If all attempted regions fail,
/// returns `Err(SendError::AllRegionsFailed)`.
///
/// `poll()` drives reconnect/auth/refresh for all regions non-blockingly.
/// `send_bundle()` is the only call that blocks the drain loop, bounded
/// by `send_timeout_ms` per region (sequential fan-out).
pub struct GrpcHarmonicSession {
    regions: Vec<RegionSession>,
    config: HarmonicEgressConfig,
    keypair: Option<SigningKey>,
    runtime: tokio::runtime::Runtime,
    /// Consecutive aggregate send failures (drives Degraded transitions).
    consecutive_failures: u32,
    /// Consecutive aggregate send successes (drives recovery).
    consecutive_successes: u32,
}

impl GrpcHarmonicSession {
    /// Create a new multi-region gRPC session.
    ///
    /// Reads the keypair from disk if `auth_mode = AuthKeypair(path)`.
    /// Panics at startup on missing/malformed keypair — fail-closed.
    ///
    /// Pre-warms all regions: spawns connect+auth for each region
    /// immediately so auth proceeds in background while the drain loop
    /// starts.
    pub fn new(config: HarmonicEgressConfig) -> Self {
        let keypair = match &config.auth_mode {
            AuthMode::NoAuth => None,
            AuthMode::AuthKeypair(path) => Some(load_keypair(path)),
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for Harmonic gRPC session");

        let mut regions = Vec::with_capacity(config.region_endpoints.len());
        for (i, endpoint) in config.region_endpoints.iter().enumerate() {
            let rx = spawn_region_connect(
                &runtime,
                endpoint,
                config.tls_enabled,
                config.connect_timeout_ms,
                keypair.clone(),
            );
            regions.push(RegionSession {
                endpoint: endpoint.clone(),
                index: i as u8,
                inner: RegionInner::Connecting {
                    result_rx: rx,
                    backoff_ms: config.reconnect_initial_backoff_ms,
                },
                state: SessionState::Connecting,
                pending_refresh: None,
            });
        }

        GrpcHarmonicSession {
            regions,
            config,
            keypair,
            runtime,
            consecutive_failures: 0,
            consecutive_successes: 0,
        }
    }

    /// Derive aggregate session state from all regions.
    fn aggregate_state(&self) -> SessionState {
        let mut has_up = false;
        let mut has_degraded = false;
        let mut has_connecting = false;
        for r in &self.regions {
            match r.state {
                SessionState::Up => has_up = true,
                SessionState::Degraded => has_degraded = true,
                SessionState::Connecting => has_connecting = true,
                SessionState::Down => {}
            }
        }
        if has_up {
            SessionState::Up
        } else if has_degraded {
            SessionState::Degraded
        } else if has_connecting {
            SessionState::Connecting
        } else {
            SessionState::Down
        }
    }

    /// Transition a region to Down with exponential backoff.
    fn region_go_down(region: &mut RegionSession, current_backoff_ms: u64, max_backoff_ms: u64) {
        let capped = current_backoff_ms.min(max_backoff_ms);
        Self::region_go_down_with_delay(region, capped, (capped * 2).min(max_backoff_ms));
    }

    /// Transition a region to Down with an explicit delay.
    fn region_go_down_with_delay(region: &mut RegionSession, delay_ms: u64, next_backoff_ms: u64) {
        region.state = SessionState::Down;
        region.inner = RegionInner::Disconnected {
            backoff_until: Some(Instant::now() + Duration::from_millis(delay_ms)),
            current_backoff_ms: next_backoff_ms,
        };
    }

    /// Poll a single region's reconnect state machine. Non-blocking.
    fn poll_region(&self, region: &mut RegionSession) {
        // First: check for in-flight token refresh.
        Self::poll_region_refresh(region);

        let placeholder = RegionInner::Disconnected {
            backoff_until: None,
            current_backoff_ms: self.config.reconnect_initial_backoff_ms,
        };
        match std::mem::replace(&mut region.inner, placeholder) {
            RegionInner::Disconnected {
                backoff_until,
                current_backoff_ms,
            } => {
                if let Some(until) = backoff_until {
                    if Instant::now() < until {
                        // Still in backoff — put it back.
                        region.inner = RegionInner::Disconnected {
                            backoff_until: Some(until),
                            current_backoff_ms,
                        };
                        return;
                    }
                }
                // Spawn connect.
                let rx = spawn_region_connect(
                    &self.runtime,
                    &region.endpoint,
                    self.config.tls_enabled,
                    self.config.connect_timeout_ms,
                    self.keypair.clone(),
                );
                region.state = SessionState::Connecting;
                region.inner = RegionInner::Connecting {
                    result_rx: rx,
                    backoff_ms: current_backoff_ms,
                };
            }
            RegionInner::Connecting {
                result_rx,
                backoff_ms,
            } => {
                match result_rx.try_recv() {
                    Ok(RegionConnectResult::Auth {
                        channel,
                        token,
                        refresh_token_value,
                        token_expires_at,
                    }) => {
                        let interceptor = AuthInterceptor::new(token.clone());
                        let client = SearcherServiceClient::new(channel, interceptor);
                        region.state = SessionState::Up;
                        region.inner = RegionInner::Ready(RegionReadyState::Authenticated {
                            client,
                            token,
                            refresh_token_value,
                            token_expires_at,
                        });
                        eprintln!(
                            "harmonic-session[{}]: authenticated and sendable ({})",
                            region.index, region.endpoint
                        );
                    }
                    Ok(RegionConnectResult::NoAuth { channel }) => {
                        let client = SearcherServiceClientNoAuth::new(channel);
                        region.state = SessionState::Up;
                        region.inner =
                            RegionInner::Ready(RegionReadyState::Unauthenticated { client });
                        eprintln!(
                            "harmonic-session[{}]: connected without auth and sendable ({})",
                            region.index, region.endpoint
                        );
                    }
                    Ok(RegionConnectResult::Failed(reason)) => {
                        eprintln!(
                            "harmonic-session[{}]: connect/auth failed: {} ({})",
                            region.index, reason, region.endpoint
                        );
                        let delay_ms = auth_failure_backoff_ms(
                            &reason,
                            backoff_ms,
                            self.config.reconnect_max_backoff_ms,
                        );
                        let next_backoff_ms =
                            (backoff_ms * 2).min(self.config.reconnect_max_backoff_ms);
                        Self::region_go_down_with_delay(region, delay_ms, next_backoff_ms);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // Still connecting — put receiver back.
                        region.inner = RegionInner::Connecting {
                            result_rx,
                            backoff_ms,
                        };
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Self::region_go_down(
                            region,
                            backoff_ms,
                            self.config.reconnect_max_backoff_ms,
                        );
                    }
                }
            }
            ready @ RegionInner::Ready(_) => {
                // Already handled refresh above — put it back.
                region.inner = ready;
            }
        }
    }

    /// Non-blocking token refresh poll for a single region.
    fn poll_region_refresh(region: &mut RegionSession) {
        if let Some(rx) = region.pending_refresh.take() {
            match rx.try_recv() {
                Ok(Some(result)) => {
                    if let RegionInner::Ready(RegionReadyState::Authenticated {
                        token,
                        token_expires_at,
                        ..
                    }) = &mut region.inner
                    {
                        token.set(result.new_value);
                        *token_expires_at = result.expires_at;
                    }
                }
                Ok(None) => {
                    // Refresh failed — let token expire naturally.
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    region.pending_refresh = Some(rx);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Worker dropped.
                }
            }
        }
    }

    /// Check if a region needs a token refresh and spawn one if so.
    fn maybe_spawn_refresh(&self, region: &mut RegionSession) {
        if region.pending_refresh.is_some() {
            return;
        }
        let (needs_refresh, refresh_token_clone) = match &region.inner {
            RegionInner::Ready(RegionReadyState::Authenticated {
                token_expires_at,
                refresh_token_value,
                ..
            }) => {
                let nearing = Instant::now() + Duration::from_secs(30) >= *token_expires_at;
                (nearing, Some(refresh_token_value.clone()))
            }
            _ => (false, None),
        };
        if needs_refresh {
            if let Some(refresh_token) = refresh_token_clone {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let url = region.endpoint.clone();
                let tls = self.config.tls_enabled;
                let timeout_ms = self.config.connect_timeout_ms;
                self.runtime.spawn(async move {
                    let result = run_refresh(url, tls, timeout_ms, refresh_token).await;
                    let _ = tx.send(result);
                });
                region.pending_refresh = Some(rx);
            }
        }
    }

    fn record_aggregate_success(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes += 1;
    }

    fn record_aggregate_failure(&mut self) {
        self.consecutive_successes = 0;
        self.consecutive_failures += 1;
    }
}

impl HarmonicSession for GrpcHarmonicSession {
    fn state(&self) -> SessionState {
        self.aggregate_state()
    }

    fn send_bundle(&mut self, request: &BundleRequest) -> Result<RegionalResult, SendError> {
        let mut region_mask: u8 = 0;
        let mut ok_mask: u8 = 0;
        let mut first_err_code: u8 = 0;
        let mut any_attempted = false;
        let mut bundle_uuid: [u8; 16] = [0; 16];

        let timeout = Duration::from_millis(self.config.send_timeout_ms);

        for i in 0..self.regions.len() {
            let region = &mut self.regions[i];
            let bit = 1u8 << region.index;

            // Only attempt send on Ready regions.
            let is_ready = matches!(region.inner, RegionInner::Ready(_));
            if !is_ready {
                continue;
            }

            region_mask |= bit;
            any_attempted = true;

            let grpc_request = build_send_bundle_request(request.tx_bytes);

            let result = match &mut region.inner {
                RegionInner::Ready(RegionReadyState::Authenticated { client, .. }) => {
                    self.runtime.block_on(async {
                        tokio::time::timeout(
                            timeout,
                            client.send_bundle(tonic::Request::new(grpc_request)),
                        )
                        .await
                    })
                }
                RegionInner::Ready(RegionReadyState::Unauthenticated { client }) => {
                    self.runtime.block_on(async {
                        tokio::time::timeout(
                            timeout,
                            client.send_bundle(tonic::Request::new(grpc_request)),
                        )
                        .await
                    })
                }
                _ => unreachable!(), // guarded by is_ready check above
            };

            match result {
                Ok(Ok(resp)) => {
                    ok_mask |= bit;
                    // Capture UUID from the first successful region.
                    if bundle_uuid == [0; 16] {
                        bundle_uuid = parse_uuid_to_bytes(&resp.into_inner().uuid);
                    }
                }
                Ok(Err(status)) => {
                    let err = classify_status(status);
                    if first_err_code == 0 {
                        first_err_code = err;
                    }
                }
                Err(_elapsed) => {
                    if first_err_code == 0 {
                        first_err_code = ERR_TIMEOUT;
                    }
                }
            }
        }

        if !any_attempted {
            self.record_aggregate_failure();
            return Err(SendError::NotConnected);
        }

        if ok_mask == 0 {
            self.record_aggregate_failure();
            return Err(SendError::AllRegionsFailed {
                region_mask,
                err_code: first_err_code,
            });
        }

        self.record_aggregate_success();
        Ok(RegionalResult {
            region_mask,
            ok_mask,
            err_code: first_err_code,
            bundle_uuid,
        })
    }

    fn poll(&mut self) {
        // Drive reconnect/auth/refresh for every region.
        // We need to work around the borrow checker: take regions out,
        // poll each, put them back.
        let mut regions = std::mem::take(&mut self.regions);
        for region in &mut regions {
            self.poll_region(region);
            self.maybe_spawn_refresh(region);
        }
        self.regions = regions;
    }
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

const AUTH_REJECTED_BACKOFF_MS: u64 = 120_000;

fn auth_failure_backoff_ms(reason: &str, local_backoff_ms: u64, max_local_backoff_ms: u64) -> u64 {
    let local = local_backoff_ms.min(max_local_backoff_ms);
    let server_retry = parse_retry_after_ms(reason).unwrap_or(0);
    let auth_rejected = reason.contains("PermissionDenied") || reason.contains("not authorized");
    let auth_floor = if auth_rejected {
        AUTH_REJECTED_BACKOFF_MS
    } else {
        0
    };
    local.max(server_retry).max(auth_floor)
}

fn parse_retry_after_ms(reason: &str) -> Option<u64> {
    parse_digits_after_marker(reason, "Retry after ")
        .or_else(|| parse_digits_after_marker(reason, "x-wait-to-retry-ms\": \""))
}

fn parse_digits_after_marker(reason: &str, marker: &str) -> Option<u64> {
    let after = reason.split(marker).nth(1)?;
    let digits: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

/// Error codes for `RegionalResult.err_code`.
pub const ERR_TIMEOUT: u8 = 1;
pub const ERR_TRANSPORT: u8 = 2;
pub const ERR_REJECTED: u8 = 3;

/// Classify a tonic `Status` into an error code.
fn classify_status(status: tonic::Status) -> u8 {
    match status.code() as i32 {
        3 | 6 | 7 | 9 | 16 => ERR_REJECTED,
        _ => ERR_TRANSPORT,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spawn the connect + auth flow for one region on the runtime's worker thread.
fn spawn_region_connect(
    runtime: &tokio::runtime::Runtime,
    endpoint: &str,
    tls_enabled: bool,
    connect_timeout_ms: u64,
    keypair: Option<SigningKey>,
) -> std::sync::mpsc::Receiver<RegionConnectResult> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let url = endpoint.to_string();

    runtime.spawn(async move {
        let channel =
            match crate::grpc::channel::create_channel(&url, tls_enabled, connect_timeout_ms).await
            {
                Ok(ch) => ch,
                Err(err) => {
                    let _ = tx.send(RegionConnectResult::Failed(format!("channel: {err}")));
                    return;
                }
            };

        match keypair {
            None => {
                let _ = tx.send(RegionConnectResult::NoAuth { channel });
            }
            Some(kp) => match run_auth_flow(channel.clone(), &kp).await {
                Ok((token, refresh_token_value, token_expires_at)) => {
                    let _ = tx.send(RegionConnectResult::Auth {
                        channel,
                        token,
                        refresh_token_value,
                        token_expires_at,
                    });
                }
                Err(err) => {
                    let _ = tx.send(RegionConnectResult::Failed(format!("auth: {err}")));
                }
            },
        }
    });

    rx
}

/// Run the full auth challenge-response flow for a single region.
async fn run_auth_flow(
    channel: Channel,
    keypair: &SigningKey,
) -> Result<(AuthToken, String, Instant), tonic::Status> {
    let mut auth_client = AuthServiceClient::new(channel);
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    let challenge_req = auth::challenge_request(&pubkey_bytes);
    let challenge_resp = auth_client
        .generate_auth_challenge(tonic::Request::new(challenge_req))
        .await?;
    let challenge = challenge_resp.into_inner().challenge;

    let signed = auth::sign_challenge(keypair, &challenge);

    let tokens_req = auth::auth_tokens_request(challenge, &pubkey_bytes, signed);
    let tokens_resp = auth_client
        .generate_auth_tokens(tonic::Request::new(tokens_req))
        .await?;
    let tokens = tokens_resp.into_inner();

    let access = tokens
        .access_token
        .ok_or_else(|| tonic::Status::internal("no access_token in response"))?;
    let refresh = tokens
        .refresh_token
        .ok_or_else(|| tonic::Status::internal("no refresh_token in response"))?;

    let token = AuthToken::empty();
    token.set(access.value.clone());

    let expires_at = access
        .expires_at_utc
        .map(|ts| {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let secs_from_now = ts.seconds.saturating_sub(now_secs);
            Instant::now() + Duration::from_secs(secs_from_now.max(60) as u64)
        })
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(900));

    Ok((token, refresh.value, expires_at))
}

/// Run token refresh on a freshly opened channel.
async fn run_refresh(
    url: String,
    tls: bool,
    timeout_ms: u64,
    refresh_token: String,
) -> Option<RefreshResult> {
    let channel = crate::grpc::channel::create_channel(&url, tls, timeout_ms)
        .await
        .ok()?;
    let mut auth_client = AuthServiceClient::new(channel);
    let req = auth::refresh_request(refresh_token);
    let resp = auth_client
        .refresh_access_token(tonic::Request::new(req))
        .await
        .ok()?;
    let new_token = resp.into_inner().access_token?;
    let expires_at = new_token
        .expires_at_utc
        .map(|ts| {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let secs_from_now = ts.seconds.saturating_sub(now_secs);
            Instant::now() + Duration::from_secs(secs_from_now.max(60) as u64)
        })
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(900));

    Some(RefreshResult {
        new_value: new_token.value,
        expires_at,
    })
}

/// Build a `SendBundleRequest` wrapping a single transaction's raw bytes.
pub(crate) fn build_send_bundle_request(tx_bytes: &[u8]) -> searcher::SendBundleRequest {
    let pkt = packet::Packet {
        data: tx_bytes.to_vec(),
        meta: None,
    };
    let bundle = bundle::Bundle {
        header: Some(shared::Header {
            ts: Some(prost_types::Timestamp {
                seconds: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
        }),
        packets: vec![pkt],
    };
    searcher::SendBundleRequest {
        bundle: Some(bundle),
    }
}

/// Load an Ed25519 signing key from a Solana-style JSON keypair file.
fn load_keypair(path: &PathBuf) -> SigningKey {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read keypair file {}: {e}", path.display()));
    let bytes: Vec<u8> = serde_json::from_str(&data).unwrap_or_else(|e| {
        panic!(
            "failed to parse keypair file {} as JSON array of u8: {e}",
            path.display()
        )
    });
    if bytes.len() < 32 {
        panic!(
            "keypair file {} has {} bytes, need at least 32",
            path.display(),
            bytes.len()
        );
    }
    let secret: [u8; 32] = bytes[..32]
        .try_into()
        .expect("keypair secret key must be 32 bytes");
    SigningKey::from_bytes(&secret)
}

// ---------------------------------------------------------------------------
// Test-only GrpcHarmonicSession helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
impl GrpcHarmonicSession {
    /// Create a GrpcHarmonicSession with pre-supplied region connect receivers.
    /// Tests send `RegionConnectResult` into paired senders to drive transitions.
    pub(crate) fn new_test(
        config: HarmonicEgressConfig,
        region_rxs: Vec<std::sync::mpsc::Receiver<RegionConnectResult>>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for test session");

        let backoff_ms = config.reconnect_initial_backoff_ms;
        let mut regions = Vec::with_capacity(region_rxs.len());
        for (i, rx) in region_rxs.into_iter().enumerate() {
            let endpoint = config
                .region_endpoints
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("https://test-region-{i}.local"));
            regions.push(RegionSession {
                endpoint,
                index: i as u8,
                inner: RegionInner::Connecting {
                    result_rx: rx,
                    backoff_ms,
                },
                state: SessionState::Connecting,
                pending_refresh: None,
            });
        }

        GrpcHarmonicSession {
            regions,
            config,
            keypair: None,
            runtime,
            consecutive_failures: 0,
            consecutive_successes: 0,
        }
    }

    /// Number of regions.
    pub(crate) fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// State of a specific region.
    pub(crate) fn region_state(&self, idx: usize) -> SessionState {
        self.regions[idx].state
    }

    /// Consecutive aggregate failures counter.
    pub(crate) fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Consecutive aggregate successes counter.
    pub(crate) fn consecutive_successes(&self) -> u32 {
        self.consecutive_successes
    }
}
