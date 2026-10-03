// Hand-written tonic service clients for the Harmonic API.
//
// Same RPC paths as Jito — Harmonic endpoints speak the same proto.
// Only the RPCs needed by G7 are defined:
//   - AuthServiceClient: GenerateAuthChallenge, GenerateAuthTokens, RefreshAccessToken
//   - SearcherServiceClient: SendBundle only

use tonic::codegen::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use super::proto::{auth, searcher};

// ── AuthServiceClient ───────────────────────────────────────────────────────

const AUTH_CHALLENGE_PATH: &str = "/auth.AuthService/GenerateAuthChallenge";
const AUTH_TOKENS_PATH: &str = "/auth.AuthService/GenerateAuthTokens";
const AUTH_REFRESH_PATH: &str = "/auth.AuthService/RefreshAccessToken";

#[derive(Clone)]
pub struct AuthServiceClient {
    inner: tonic::client::Grpc<Channel>,
}

impl AuthServiceClient {
    pub fn new(channel: Channel) -> Self {
        Self {
            inner: tonic::client::Grpc::new(channel),
        }
    }

    pub async fn generate_auth_challenge(
        &mut self,
        request: Request<auth::GenerateAuthChallengeRequest>,
    ) -> Result<Response<auth::GenerateAuthChallengeResponse>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        self.inner
            .unary(
                request,
                AUTH_CHALLENGE_PATH.parse().unwrap(),
                tonic::codec::ProstCodec::default(),
            )
            .await
    }

    pub async fn generate_auth_tokens(
        &mut self,
        request: Request<auth::GenerateAuthTokensRequest>,
    ) -> Result<Response<auth::GenerateAuthTokensResponse>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        self.inner
            .unary(
                request,
                AUTH_TOKENS_PATH.parse().unwrap(),
                tonic::codec::ProstCodec::default(),
            )
            .await
    }

    pub async fn refresh_access_token(
        &mut self,
        request: Request<auth::RefreshAccessTokenRequest>,
    ) -> Result<Response<auth::RefreshAccessTokenResponse>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        self.inner
            .unary(
                request,
                AUTH_REFRESH_PATH.parse().unwrap(),
                tonic::codec::ProstCodec::default(),
            )
            .await
    }
}

// ── SearcherServiceClient ───────────────────────────────────────────────────

const SEND_BUNDLE_PATH: &str = "/searcher.SearcherService/SendBundle";

#[derive(Clone)]
pub struct SearcherServiceClient<I: Interceptor> {
    inner: tonic::client::Grpc<InterceptedService<Channel, I>>,
}

impl<I: Interceptor + Clone> SearcherServiceClient<I> {
    pub fn new(channel: Channel, interceptor: I) -> Self {
        Self {
            inner: tonic::client::Grpc::new(InterceptedService::new(channel, interceptor)),
        }
    }

    pub async fn send_bundle(
        &mut self,
        request: Request<searcher::SendBundleRequest>,
    ) -> Result<Response<searcher::SendBundleResponse>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        self.inner
            .unary(
                request,
                SEND_BUNDLE_PATH.parse().unwrap(),
                tonic::codec::ProstCodec::default(),
            )
            .await
    }
}

/// Unauthenticated searcher client (for NoAuth mode or testing).
#[derive(Clone)]
pub struct SearcherServiceClientNoAuth {
    inner: tonic::client::Grpc<Channel>,
}

impl SearcherServiceClientNoAuth {
    pub fn new(channel: Channel) -> Self {
        Self {
            inner: tonic::client::Grpc::new(channel),
        }
    }

    pub async fn send_bundle(
        &mut self,
        request: Request<searcher::SendBundleRequest>,
    ) -> Result<Response<searcher::SendBundleResponse>, Status> {
        self.inner
            .ready()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        self.inner
            .unary(
                request,
                SEND_BUNDLE_PATH.parse().unwrap(),
                tonic::codec::ProstCodec::default(),
            )
            .await
    }
}
