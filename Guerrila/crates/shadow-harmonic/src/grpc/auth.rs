// Harmonic API authentication: challenge-response + bearer token interceptor.
//
// Same flow as Jito (both use the same proto schema):
//   1. GenerateAuthChallenge(role=Searcher, pubkey)
//   2. Sign `pubkey || challenge` with Ed25519 keypair
//   3. GenerateAuthTokens(challenge, client_pubkey, signed_challenge)
//   4. Receive access_token + refresh_token
//   5. Inject access_token as `authorization: Bearer <token>` on every RPC
//   6. Periodically call RefreshAccessToken before the access_token expires

use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signer, SigningKey};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::{Request, Status};

use super::proto::auth;

// ── Bearer token interceptor ────────────────────────────────────────────────

/// Shared handle to the current access token.
#[derive(Clone)]
pub struct AuthToken(pub Arc<Mutex<String>>);

impl AuthToken {
    pub fn empty() -> Self {
        Self(Arc::new(Mutex::new(String::new())))
    }

    pub fn get(&self) -> String {
        self.0.lock().unwrap().clone()
    }

    pub fn set(&self, value: String) {
        *self.0.lock().unwrap() = value;
    }
}

/// Tonic interceptor that injects `authorization: Bearer <token>` on every
/// outbound RPC.
#[derive(Clone)]
pub struct AuthInterceptor {
    token: AuthToken,
}

impl AuthInterceptor {
    pub fn new(token: AuthToken) -> Self {
        Self { token }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        let value = self.token.get();
        if !value.is_empty() {
            let header: MetadataValue<Ascii> = format!("Bearer {value}")
                .parse()
                .map_err(|_| Status::internal("invalid bearer token characters"))?;
            req.metadata_mut().insert("authorization", header);
        }
        Ok(req)
    }
}

// ── Challenge-response helpers ──────────────────────────────────────────────

/// Signs the auth challenge: `sign(pubkey_bytes || challenge_string)`.
pub fn sign_challenge(keypair: &SigningKey, challenge: &str) -> Vec<u8> {
    let pubkey_bytes = keypair.verifying_key().to_bytes();
    let mut message = Vec::with_capacity(32 + challenge.len());
    message.extend_from_slice(&pubkey_bytes);
    message.extend_from_slice(challenge.as_bytes());
    let sig = keypair.sign(&message);
    sig.to_bytes().to_vec()
}

/// Build a `GenerateAuthChallengeRequest` for the Searcher role.
pub fn challenge_request(pubkey: &[u8; 32]) -> auth::GenerateAuthChallengeRequest {
    auth::GenerateAuthChallengeRequest {
        role: auth::Role::Searcher as i32,
        pubkey: pubkey.to_vec(),
    }
}

/// Build a `GenerateAuthTokensRequest` from a signed challenge.
pub fn auth_tokens_request(
    challenge: String,
    pubkey: &[u8; 32],
    signed_challenge: Vec<u8>,
) -> auth::GenerateAuthTokensRequest {
    auth::GenerateAuthTokensRequest {
        challenge,
        client_pubkey: pubkey.to_vec(),
        signed_challenge,
    }
}

/// Build a `RefreshAccessTokenRequest`.
pub fn refresh_request(refresh_token: String) -> auth::RefreshAccessTokenRequest {
    auth::RefreshAccessTokenRequest { refresh_token }
}
