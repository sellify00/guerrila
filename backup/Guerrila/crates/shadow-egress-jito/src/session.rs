// G6 – Jito Searcher session management
//
// Maintains a persistent (but non-blocking) session to the Jito block-engine
// Searcher API.  The proxy owns reconnect logic; workers never see session state.
//
// Design constraints:
//   - reconnect MUST NOT block the dequeue loop; bundles arriving during
//     reconnect are NACKed rather than queued.
//   - no BundleState parsing here (PM owns that).
//   - no public TPU/QUIC fallback.

use std::path::PathBuf;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Session state machine
// ---------------------------------------------------------------------------

/// Coarse session state visible to the proxy loop.
///
/// State machine:
///   Down ──► Connecting ──► Up ──► Degraded
///     ▲          │            │        │
///     │          ▼            ▼        │
///     └──────── (fail) ◄─────────────┘
///
/// Rules:
///   - `Down` → `Connecting`: poll() initiates async reconnect.
///   - `Connecting` → `Up`: handshake + auth completed.
///   - `Connecting` → `Down`: handshake/auth failed.
///   - `Up` → `Degraded`: intermittent send failures accumulate.
///   - `Up` → `Down`: connection lost.
///   - `Degraded` → `Up`: sustained successes.
///   - `Degraded` → `Down`: too many failures or connection lost.
///
/// The proxy send path uses `can_send()`: only `Up` and `Degraded` allow
/// transmission.  `Down` and `Connecting` trigger fast NACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Connection dead, not attempting reconnect yet.
    Down,
    /// Async reconnect / handshake in progress; not authenticated.
    Connecting,
    /// Authenticated, ready to send.
    Up,
    /// Authenticated but experiencing intermittent failures.
    Degraded,
}

/// Abstraction over a Jito Searcher API session.
///
/// The real implementation will use the Jito gRPC SDK.
/// Tests inject a mock via this trait.
///
/// Contract:
///   - Sessions are long-lived and persistent.
///   - `poll()` drives async reconnect; the send loop never blocks on handshakes.
///   - `can_send()` gates transmission: only `Up` / `Degraded` allow sends.
///   - Per-send handshakes are forbidden; auth is done during `Connecting`.
pub trait SearcherSession {
    /// Current coarse state.
    fn state(&self) -> SessionState;

    /// Whether the session is in a state that permits sending.
    ///
    /// Returns `true` for `Up` and `Degraded`; `false` for `Down` and
    /// `Connecting`.  The proxy uses this to fast-NACK envelopes when
    /// no authenticated session is available.
    fn can_send(&self) -> bool {
        matches!(self.state(), SessionState::Up | SessionState::Degraded)
    }

    /// Attempt to send a bundle.
    ///
    /// G6 framing: each `BundleRequest` contains exactly one transaction.
    /// One envelope → one bundle → one send call.  No batching.
    ///
    /// Returns `Ok(())` if the bundle was accepted by the send path
    /// (not necessarily landed — that's BundleState, which PM handles).
    ///
    /// Returns `Err(SendError)` on any local or network failure that
    /// means the bundle was NOT transmitted.  The proxy treats every
    /// `Err` as a NACK trigger.
    ///
    /// Callers MUST check `can_send()` before calling; implementations
    /// may panic or return `NotConnected` if called in a non-sendable state.
    ///
    /// Returns the bundle UUID from the block engine on success.
    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError>;

    /// Drive reconnect / keepalive state forward.
    ///
    /// Called by the proxy loop on every iteration.  Must never block.
    /// Implementations that need async I/O should poll internal futures here.
    ///
    /// Typical state transitions driven by poll():
    ///   - `Down` → `Connecting` (initiate reconnect)
    ///   - `Connecting` → `Up` (handshake complete)
    ///   - `Connecting` → `Down` (handshake failed, backoff)
    fn poll(&mut self);
}

// ---------------------------------------------------------------------------
// Bundle request (one-tx-per-bundle framing)
// ---------------------------------------------------------------------------

/// A single Jito bundle request: exactly one transaction, one bundle.
///
/// G6 first-pass framing: each `SignedTxEnvelope` becomes one `BundleRequest`
/// containing one transaction.  No batching, no coalescing, no multi-envelope
/// packing.  The proxy constructs this from `envelope.raw_bytes[..envelope.len]`
/// and hands it to the session for submission.
///
/// The real Jito Searcher API `SendBundleRequest` wraps a `Bundle` containing
/// a list of `Packet` (each = raw tx bytes).  For G6, every bundle has exactly
/// one packet.  Multi-tx bundles are a later optimisation if ever needed.
pub struct BundleRequest<'a> {
    /// Raw signed transaction bytes — exactly one transaction.
    /// Sent verbatim; the proxy must not mutate these post-sign.
    pub tx_bytes: &'a [u8],
}

impl<'a> BundleRequest<'a> {
    /// Wrap a single transaction's raw bytes into a bundle request.
    #[inline]
    pub fn single_tx(tx_bytes: &'a [u8]) -> Self {
        BundleRequest { tx_bytes }
    }
}

/// Errors from a send attempt.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("session not connected")]
    NotConnected,
    #[error("send timed out")]
    Timeout,
    #[error("transport error: {0}")]
    Transport(String),
    #[error("server rejected bundle: {0}")]
    Rejected(String),
}

// ---------------------------------------------------------------------------
// Stub session (for integration scaffolding + tests)
// ---------------------------------------------------------------------------

/// A stub session that is always `Down`.
///
/// Used during initial scaffolding before the real Jito gRPC SDK is wired.
/// Every envelope is fast-NACKed through the proxy failure path.
pub struct StubSession;

impl SearcherSession for StubSession {
    fn state(&self) -> SessionState {
        SessionState::Down
    }

    fn send_bundle(&mut self, _request: &BundleRequest<'_>) -> Result<String, SendError> {
        Err(SendError::NotConnected)
    }

    fn poll(&mut self) {
        // no-op — stub never transitions out of Down.
    }
}

// ---------------------------------------------------------------------------
// GrpcSearcherSession — real Jito Searcher API client
// ---------------------------------------------------------------------------

use crate::config::{AuthMode, JitoEgressConfig};
use crate::grpc::auth::{self, AuthInterceptor, AuthToken};
use crate::grpc::proto::{bundle, packet, searcher, shared};
use crate::grpc::service::{AuthServiceClient, SearcherServiceClient, SearcherServiceClientNoAuth};

use ed25519_dalek::SigningKey;
use tonic::transport::Channel;

/// Result of an async connect + auth attempt, sent back from the worker
/// thread via `std::sync::mpsc`.  The proxy thread never blocks on
/// channel creation or auth RPCs — it spawns them and polls for completion.
pub(crate) enum ConnectResult {
    /// Auth mode: channel opened, challenge-response completed.
    Auth {
        channel: Channel,
        token: AuthToken,
        refresh_token_value: String,
        token_expires_at: Instant,
    },
    /// NoAuth mode: channel opened, no challenge-response needed.
    NoAuth { channel: Channel },
    /// Connect or auth failed.
    Failed(String),
}

/// Internal connectivity state for the gRPC session.
enum GrpcInner {
    /// No channel established.  `backoff_until` tracks exponential delay.
    Disconnected {
        backoff_until: Option<Instant>,
        current_backoff_ms: u64,
    },
    /// Connect + auth spawned on the runtime worker thread; waiting for result.
    /// `poll()` calls `try_recv()` — never blocks.
    Connecting {
        result_rx: std::sync::mpsc::Receiver<ConnectResult>,
        backoff_ms: u64,
    },
    /// Authenticated (or unauthenticated), ready to send.
    Ready(ReadyState),
}

/// Active session state — either authenticated or unauthenticated.
enum ReadyState {
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

/// Real Jito Searcher API session implementing `SearcherSession`.
///
/// Bridges the async tonic gRPC client into the proxy's synchronous
/// `poll()` / `send_bundle()` contract.  The runtime has a single
/// worker thread that drives connect/auth/refresh futures in the
/// background; `poll()` only does non-blocking `try_recv()` checks.
///
/// Auth flow (when `auth_mode = AuthKeypair`):
///   1. `poll()` in `Disconnected` → spawns connect+auth → `Connecting`
///   2. `poll()` in `Connecting` → `try_recv()` → on success → `Up`
///   3. `send_bundle()` in `Ready` → `block_on(client.send_bundle(...))`
///   4. `poll()` in `Ready` → spawns token refresh if nearing expiry
///
/// NoAuth mode skips auth steps.
///
/// `send_bundle()` is the only call that blocks the drain loop, bounded
/// by `send_timeout_ms`.  Connect, auth, and token refresh never block.
pub struct GrpcSearcherSession {
    state: SessionState,
    inner: GrpcInner,
    config: JitoEgressConfig,
    keypair: Option<SigningKey>,
    runtime: tokio::runtime::Runtime,
    /// Consecutive send failures (drives Degraded transitions).
    consecutive_failures: u32,
    /// Consecutive successes (drives recovery from Degraded).
    consecutive_successes: u32,
    /// In-flight token refresh: if Some, a refresh is running on the
    /// worker thread and we poll for the result non-blockingly.
    pending_refresh: Option<std::sync::mpsc::Receiver<Option<RefreshResult>>>,
}

/// Token refresh result sent back from the worker thread.
pub(crate) struct RefreshResult {
    pub(crate) new_value: String,
    pub(crate) expires_at: Instant,
}

impl GrpcSearcherSession {
    /// Create a new gRPC session from the egress config.
    ///
    /// Reads the keypair from disk if `auth_mode = AuthKeypair(path)`.
    /// Panics at startup if the keypair file is missing or malformed —
    /// this is fail-closed by design.
    ///
    /// Pre-warms the session: the constructor immediately spawns the
    /// connect + auth flow on the runtime worker thread, so the proxy
    /// starts in `Connecting` and auth proceeds in the background while
    /// the proxy drain loop begins.  The constructor itself never blocks
    /// on network I/O.
    pub fn new(config: JitoEgressConfig) -> Self {
        let keypair = match &config.auth_mode {
            AuthMode::NoAuth => None,
            AuthMode::AuthKeypair(path) => Some(load_keypair(path)),
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for gRPC session");

        // Pre-warm: spawn connect+auth immediately so auth proceeds in
        // background while the proxy drain loop starts.  Never blocks.
        let rx = spawn_connect(&runtime, &config, keypair.clone());
        let backoff_ms = config.reconnect_initial_backoff_ms;

        GrpcSearcherSession {
            state: SessionState::Connecting,
            inner: GrpcInner::Connecting {
                result_rx: rx,
                backoff_ms,
            },
            config,
            keypair,
            runtime,
            consecutive_failures: 0,
            consecutive_successes: 0,
            pending_refresh: None,
        }
    }

    /// Transition to Down with exponential backoff.
    fn go_down(&mut self, current_backoff_ms: u64) {
        let capped = current_backoff_ms.min(self.config.reconnect_max_backoff_ms);
        self.go_down_with_delay(
            capped,
            (capped * 2).min(self.config.reconnect_max_backoff_ms),
        );
    }

    /// Transition to Down with an explicit delay.
    ///
    /// Server-directed auth throttles must be respected even when they exceed
    /// the local reconnect cap; the cap only applies to our own exponential
    /// reconnect schedule.
    fn go_down_with_delay(&mut self, delay_ms: u64, next_backoff_ms: u64) {
        self.state = SessionState::Down;
        self.inner = GrpcInner::Disconnected {
            backoff_until: Some(Instant::now() + Duration::from_millis(delay_ms)),
            current_backoff_ms: next_backoff_ms.min(self.config.reconnect_max_backoff_ms),
        };
        self.consecutive_successes = 0;
    }

    /// Drive Disconnected → Connecting: spawn connect+auth on the worker thread.
    ///
    /// Non-blocking: checks backoff timer, spawns async work, returns immediately.
    fn poll_disconnected(&mut self, backoff_until: Option<Instant>, current_backoff_ms: u64) {
        // Respect backoff timer.
        if let Some(until) = backoff_until {
            if Instant::now() < until {
                self.inner = GrpcInner::Disconnected {
                    backoff_until: Some(until),
                    current_backoff_ms,
                };
                return;
            }
        }

        let rx = spawn_connect(&self.runtime, &self.config, self.keypair.clone());
        self.state = SessionState::Connecting;
        self.inner = GrpcInner::Connecting {
            result_rx: rx,
            backoff_ms: current_backoff_ms,
        };
    }

    /// Drive Connecting → Ready (or back to Down): non-blocking check
    /// via `try_recv()` on the channel from the worker thread.
    fn poll_connecting(
        &mut self,
        result_rx: std::sync::mpsc::Receiver<ConnectResult>,
        backoff_ms: u64,
    ) {
        match result_rx.try_recv() {
            Ok(ConnectResult::Auth {
                channel,
                token,
                refresh_token_value,
                token_expires_at,
            }) => {
                let interceptor = AuthInterceptor::new(token.clone());
                let client = SearcherServiceClient::new(channel, interceptor);
                self.state = SessionState::Up;
                self.inner = GrpcInner::Ready(ReadyState::Authenticated {
                    client,
                    token,
                    refresh_token_value,
                    token_expires_at,
                });
                self.consecutive_failures = 0;
                self.consecutive_successes = 0;
                eprintln!("jito-session: authenticated and sendable");
            }
            Ok(ConnectResult::NoAuth { channel }) => {
                let client = SearcherServiceClientNoAuth::new(channel);
                self.state = SessionState::Up;
                self.inner = GrpcInner::Ready(ReadyState::Unauthenticated { client });
                self.consecutive_failures = 0;
                self.consecutive_successes = 0;
                eprintln!("jito-session: connected without auth and sendable");
            }
            Ok(ConnectResult::Failed(reason)) => {
                eprintln!("jito-session: connect/auth failed: {reason}");
                let delay_ms = auth_failure_backoff_ms(
                    &reason,
                    backoff_ms,
                    self.config.reconnect_max_backoff_ms,
                );
                let next_backoff_ms = (backoff_ms * 2).min(self.config.reconnect_max_backoff_ms);
                self.go_down_with_delay(delay_ms, next_backoff_ms);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Still in flight — put the receiver back, stay Connecting.
                self.inner = GrpcInner::Connecting {
                    result_rx,
                    backoff_ms,
                };
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                eprintln!("jito-session: connect worker dropped before returning a result");
                // Worker thread panicked or dropped sender — treat as failure.
                self.go_down(backoff_ms);
            }
        }
    }

    /// In Ready state: if the access token is nearing expiry, spawn a
    /// non-blocking refresh on the worker thread.  If a refresh is
    /// already in flight, check for its result.
    fn poll_ready_refresh(&mut self) {
        // First, check for in-flight refresh results.
        if let Some(rx) = self.pending_refresh.take() {
            match rx.try_recv() {
                Ok(Some(result)) => {
                    // Apply the refreshed token.
                    if let GrpcInner::Ready(ReadyState::Authenticated {
                        token,
                        token_expires_at,
                        ..
                    }) = &mut self.inner
                    {
                        token.set(result.new_value);
                        *token_expires_at = result.expires_at;
                        // NOTE: refresh_token_value is intentionally NOT updated here.
                        // RefreshResult.new_value is the new *access* token, not a new
                        // refresh token.  The Jito auth API returns only a refreshed
                        // access token; the original refresh credential stays valid.
                    }
                    // Refresh complete — pending_refresh stays None.
                }
                Ok(None) => {
                    // Refresh failed — let token expire naturally.
                    // Next poll after expiry will reconnect from scratch.
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Still in flight — keep waiting.
                    self.pending_refresh = Some(rx);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Worker dropped — treat as refresh failure.
                }
            }
        }

        // Check if we need to start a new refresh.
        let (needs_refresh, refresh_token_clone) = match &self.inner {
            GrpcInner::Ready(ReadyState::Authenticated {
                token_expires_at,
                refresh_token_value,
                ..
            }) => {
                let nearing_expiry = Instant::now() + Duration::from_secs(30) >= *token_expires_at;
                (nearing_expiry, Some(refresh_token_value.clone()))
            }
            _ => (false, None),
        };

        if needs_refresh {
            if let Some(refresh_token) = refresh_token_clone {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let url = self.config.block_engine_url.clone();
                let tls = self.config.tls_enabled;
                let timeout_ms = self.config.connect_timeout_ms;
                self.runtime.spawn(async move {
                    let result = run_refresh(url, tls, timeout_ms, refresh_token).await;
                    let _ = tx.send(result);
                });
                self.pending_refresh = Some(rx);
            }
        }
    }

    fn record_send_success(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes += 1;
        if self.state == SessionState::Degraded
            && self.consecutive_successes >= self.config.hysteresis_recover_batches
        {
            self.state = SessionState::Up;
        }
    }

    fn record_send_failure(&mut self) {
        self.consecutive_successes = 0;
        self.consecutive_failures += 1;
        match self.state {
            SessionState::Up => {
                if self.consecutive_failures >= self.config.degrade_failure_threshold {
                    self.state = SessionState::Degraded;
                }
            }
            SessionState::Degraded => {
                if self.consecutive_failures >= self.config.down_failure_threshold {
                    self.go_down(self.config.reconnect_initial_backoff_ms);
                }
            }
            _ => {}
        }
    }
}

impl SearcherSession for GrpcSearcherSession {
    fn state(&self) -> SessionState {
        self.state
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        let grpc_request = build_send_bundle_request(request.tx_bytes);
        let timeout = Duration::from_millis(self.config.send_timeout_ms);

        let result = match &mut self.inner {
            GrpcInner::Ready(ReadyState::Authenticated { client, .. }) => {
                self.runtime.block_on(async {
                    tokio::time::timeout(
                        timeout,
                        client.send_bundle(tonic::Request::new(grpc_request)),
                    )
                    .await
                })
            }
            GrpcInner::Ready(ReadyState::Unauthenticated { client }) => {
                self.runtime.block_on(async {
                    tokio::time::timeout(
                        timeout,
                        client.send_bundle(tonic::Request::new(grpc_request)),
                    )
                    .await
                })
            }
            _ => {
                return Err(SendError::NotConnected);
            }
        };

        match result {
            Ok(Ok(resp)) => {
                self.record_send_success();
                Ok(resp.into_inner().uuid)
            }
            Ok(Err(status)) => {
                self.record_send_failure();
                Err(classify_status(status))
            }
            Err(_elapsed) => {
                self.record_send_failure();
                Err(SendError::Timeout)
            }
        }
    }

    fn poll(&mut self) {
        // Check for in-flight token refresh results (non-blocking).
        self.poll_ready_refresh();

        match std::mem::replace(
            &mut self.inner,
            GrpcInner::Disconnected {
                backoff_until: None,
                current_backoff_ms: self.config.reconnect_initial_backoff_ms,
            },
        ) {
            GrpcInner::Disconnected {
                backoff_until,
                current_backoff_ms,
            } => {
                self.poll_disconnected(backoff_until, current_backoff_ms);
            }
            GrpcInner::Connecting {
                result_rx,
                backoff_ms,
            } => {
                self.poll_connecting(result_rx, backoff_ms);
            }
            ready @ GrpcInner::Ready(_) => {
                // Put it back — refresh was already polled above.
                self.inner = ready;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Classify a tonic `Status` into `SendError::Rejected` vs `SendError::Transport`.
///
/// Server-side rejections (the bundle was received but the server refused it)
/// are semantically different from transport failures (the bundle may not have
/// reached the server at all).  Both are local failures that trigger NACK, but
/// the distinction matters for diagnostics and health accounting.
///
/// Rejected codes (server received the call and refused it):
///   3  = invalid argument
///   6  = already exists
///   7  = permission denied
///   9  = failed precondition
///   16 = unauthenticated
///
/// Transport codes (everything else): unavailable, internal, unknown,
/// deadline exceeded, cancelled, etc.
fn classify_status(status: tonic::Status) -> SendError {
    match status.code() as i32 {
        3 | 6 | 7 | 9 | 16 => SendError::Rejected(status.to_string()),
        _ => SendError::Transport(status.to_string()),
    }
}

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

/// Spawn the full connect + auth flow on the runtime's worker thread.
///
/// Returns a `Receiver` that the caller polls non-blockingly via `try_recv()`.
/// The spawned task does: create_channel → (optionally) run_auth_flow → send result.
/// The caller (proxy drain thread) never blocks on any of this.
fn spawn_connect(
    runtime: &tokio::runtime::Runtime,
    config: &JitoEgressConfig,
    keypair: Option<SigningKey>,
) -> std::sync::mpsc::Receiver<ConnectResult> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let url = config.block_engine_url.clone();
    let tls = config.tls_enabled;
    let timeout_ms = config.connect_timeout_ms;

    runtime.spawn(async move {
        // 1. Open channel.
        let channel = match crate::grpc::channel::create_channel(&url, tls, timeout_ms).await {
            Ok(ch) => ch,
            Err(err) => {
                let _ = tx.send(ConnectResult::Failed(format!("channel: {err}")));
                return;
            }
        };

        // 2. Auth or NoAuth.
        match keypair {
            None => {
                let _ = tx.send(ConnectResult::NoAuth { channel });
            }
            Some(kp) => match run_auth_flow(channel.clone(), &kp).await {
                Ok((token, refresh_token_value, token_expires_at)) => {
                    let _ = tx.send(ConnectResult::Auth {
                        channel,
                        token,
                        refresh_token_value,
                        token_expires_at,
                    });
                }
                Err(err) => {
                    let _ = tx.send(ConnectResult::Failed(format!("auth: {err}")));
                }
            },
        }
    });

    rx
}

/// Run a token refresh RPC on a freshly opened channel.
///
/// Opens a new channel for the refresh to avoid borrowing the session's
/// existing channel across the thread boundary.  Returns `Some(result)`
/// on success, `None` on failure.
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

/// Run the full Jito auth challenge-response flow.
///
/// Returns `(AuthToken, refresh_token_value, access_token_expiry)`.
async fn run_auth_flow(
    channel: Channel,
    keypair: &SigningKey,
) -> Result<(AuthToken, String, Instant), tonic::Status> {
    let mut auth_client = AuthServiceClient::new(channel);
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    // 1. Request challenge.
    let challenge_req = auth::challenge_request(&pubkey_bytes);
    let challenge_resp = auth_client
        .generate_auth_challenge(tonic::Request::new(challenge_req))
        .await?;
    let challenge = challenge_resp.into_inner().challenge;

    // 2. Sign challenge.
    let signed = auth::sign_challenge(keypair, &challenge);

    // 3. Exchange signed challenge for tokens.
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

    // Compute expiry as Instant.
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

/// Build a `SendBundleRequest` wrapping a single transaction's raw bytes.
///
/// G6 framing: one tx → one Packet → one Bundle → one SendBundleRequest.
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
///
/// The file contains a JSON array of 64 u8 values: 32 bytes secret + 32 bytes public.
/// Panics if the file is missing or malformed — fail-closed at startup.
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
// Test-only GrpcSearcherSession helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
impl GrpcSearcherSession {
    /// Create a GrpcSearcherSession in `Connecting` state with a pre-supplied
    /// receiver.  Tests send `ConnectResult` variants into the paired sender
    /// to drive state transitions without hitting the network.
    pub(crate) fn new_test_connecting(
        config: JitoEgressConfig,
        result_rx: std::sync::mpsc::Receiver<ConnectResult>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for test session");
        let backoff_ms = config.reconnect_initial_backoff_ms;
        GrpcSearcherSession {
            state: SessionState::Connecting,
            inner: GrpcInner::Connecting {
                result_rx,
                backoff_ms,
            },
            config,
            keypair: None,
            runtime,
            consecutive_failures: 0,
            consecutive_successes: 0,
            pending_refresh: None,
        }
    }

    /// Directly set the session state for unit testing state-machine logic.
    pub(crate) fn set_state_for_test(&mut self, state: SessionState) {
        self.state = state;
    }

    /// Expose `record_send_success()` for unit testing hysteresis recovery.
    pub(crate) fn test_record_send_success(&mut self) {
        self.record_send_success();
    }

    /// Expose `record_send_failure()` for unit testing degradation thresholds.
    pub(crate) fn test_record_send_failure(&mut self) {
        self.record_send_failure();
    }

    /// Read consecutive_successes counter for assertions.
    pub(crate) fn consecutive_successes(&self) -> u32 {
        self.consecutive_successes
    }

    /// Read consecutive_failures counter for assertions.
    pub(crate) fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Read remaining disconnected backoff for assertions.
    pub(crate) fn disconnected_backoff_remaining_ms(&self) -> Option<u64> {
        match &self.inner {
            GrpcInner::Disconnected {
                backoff_until: Some(until),
                ..
            } => Some(until.saturating_duration_since(Instant::now()).as_millis() as u64),
            _ => None,
        }
    }

    /// Inject a pending refresh receiver for testing the refresh path.
    pub(crate) fn set_pending_refresh_for_test(
        &mut self,
        rx: std::sync::mpsc::Receiver<Option<RefreshResult>>,
    ) {
        self.pending_refresh = Some(rx);
    }

    /// Read the stored refresh_token_value from the inner Ready state.
    /// Returns `None` if not in Authenticated state.
    pub(crate) fn refresh_token_value(&self) -> Option<&str> {
        match &self.inner {
            GrpcInner::Ready(ReadyState::Authenticated {
                refresh_token_value,
                ..
            }) => Some(refresh_token_value.as_str()),
            _ => None,
        }
    }

    /// Read the current bearer token from the inner Ready state.
    /// Returns `None` if not in Authenticated state.
    pub(crate) fn bearer_token_value(&self) -> Option<String> {
        match &self.inner {
            GrpcInner::Ready(ReadyState::Authenticated { token, .. }) => {
                Some(token.0.lock().unwrap().clone())
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Test-only controllable session
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockSession {
    pub session_state: SessionState,
    pub send_result: Option<Result<(), SendError>>,
    pub sent_count: u32,
}

/// A mock session that captures the exact bytes passed to `send_bundle`.
///
/// Used to verify the proxy sends `raw_bytes[..len]` verbatim — no patching,
/// no rebuilding, no mutation of the signed transaction.
#[cfg(test)]
pub struct CapturingSession {
    pub captured: Vec<Vec<u8>>,
}

#[cfg(test)]
impl MockSession {
    /// Session in `Up` state — sends succeed unless `send_result` overrides.
    pub fn connected() -> Self {
        Self {
            session_state: SessionState::Up,
            send_result: None,
            sent_count: 0,
        }
    }

    /// Session in `Down` state — `can_send()` returns false.
    pub fn disconnected() -> Self {
        Self {
            session_state: SessionState::Down,
            send_result: None,
            sent_count: 0,
        }
    }

    /// Session in `Connecting` state — `can_send()` returns false.
    pub fn connecting() -> Self {
        Self {
            session_state: SessionState::Connecting,
            send_result: None,
            sent_count: 0,
        }
    }

    /// Session in `Degraded` state — `can_send()` returns true.
    pub fn degraded() -> Self {
        Self {
            session_state: SessionState::Degraded,
            send_result: None,
            sent_count: 0,
        }
    }

    /// Session in `Up` state but every send returns the given error.
    pub fn with_failure(err: SendError) -> Self {
        Self {
            session_state: SessionState::Up,
            send_result: Some(Err(err)),
            sent_count: 0,
        }
    }
}

#[cfg(test)]
impl SearcherSession for MockSession {
    fn state(&self) -> SessionState {
        self.session_state
    }

    fn send_bundle(&mut self, _request: &BundleRequest<'_>) -> Result<String, SendError> {
        if !self.can_send() {
            return Err(SendError::NotConnected);
        }
        self.sent_count += 1;
        match &self.send_result {
            Some(Err(SendError::NotConnected)) => Err(SendError::NotConnected),
            Some(Err(SendError::Timeout)) => Err(SendError::Timeout),
            Some(Err(SendError::Transport(message))) => Err(SendError::Transport(message.clone())),
            Some(Err(SendError::Rejected(message))) => Err(SendError::Rejected(message.clone())),
            _ => Ok(String::new()),
        }
    }

    fn poll(&mut self) {
        // no-op for tests
    }
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
impl SearcherSession for CapturingSession {
    fn state(&self) -> SessionState {
        SessionState::Up
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        self.captured.push(request.tx_bytes.to_vec());
        Ok(String::new())
    }

    fn poll(&mut self) {
        // no-op for tests
    }
}
