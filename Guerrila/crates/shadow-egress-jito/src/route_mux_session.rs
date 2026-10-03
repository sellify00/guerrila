use crate::config::JitoEgressConfig;
use crate::helius_sender_client::HeliusSenderSession;
use crate::json_rpc_client::JsonRpcSearcherSession;
use crate::public_rpc_client::PublicRpcSearcherSession;
use crate::session::{
    BundleRequest, GrpcSearcherSession, SearcherSession, SendError, SessionState,
};
use shadow_ipc::egress_ring::{
    PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER, PRIVATE_EGRESS_TRANSPORT_JITO_BUNDLE,
    PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC,
};

enum DefaultJitoSession {
    Grpc(GrpcSearcherSession),
    JsonRpc(JsonRpcSearcherSession),
}

impl DefaultJitoSession {
    fn state(&self) -> SessionState {
        match self {
            Self::Grpc(session) => session.state(),
            Self::JsonRpc(session) => session.state(),
        }
    }

    fn can_send(&self) -> bool {
        match self {
            Self::Grpc(session) => session.can_send(),
            Self::JsonRpc(session) => session.can_send(),
        }
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        match self {
            Self::Grpc(session) => session.send_bundle(request),
            Self::JsonRpc(session) => session.send_bundle(request),
        }
    }

    fn poll(&mut self) {
        match self {
            Self::Grpc(session) => session.poll(),
            Self::JsonRpc(session) => session.poll(),
        }
    }
}

/// Route-aware private-lane mux.
///
/// Keeps a single proxy thread and shared Jito-family ring contract while
/// allowing different envelopes to use different submit transports:
///   - Jito bundle/searcher path
///   - explicit public RPC scout path
///   - Helius Sender single-tx path
pub struct RouteMuxSession {
    default_jito: DefaultJitoSession,
    public_rpc: Option<PublicRpcSearcherSession>,
    helius_sender: Option<HeliusSenderSession>,
}

impl RouteMuxSession {
    pub fn new(config: JitoEgressConfig) -> Self {
        let default_jito = if config.json_rpc.enabled {
            DefaultJitoSession::JsonRpc(JsonRpcSearcherSession::new(config.clone()))
        } else {
            DefaultJitoSession::Grpc(GrpcSearcherSession::new(config.clone()))
        };
        let public_rpc = if config.public_rpc.enabled {
            Some(PublicRpcSearcherSession::new(config.clone()))
        } else {
            None
        };
        let helius_sender = if config.helius_sender.enabled {
            Some(HeliusSenderSession::new(config))
        } else {
            None
        };
        Self {
            default_jito,
            public_rpc,
            helius_sender,
        }
    }

    fn aggregate_state(&self) -> SessionState {
        let mut any_up = false;
        let mut any_degraded = false;
        let mut any_connecting = false;
        let states = [
            Some(self.default_jito.state()),
            self.public_rpc.as_ref().map(SearcherSession::state),
            self.helius_sender.as_ref().map(SearcherSession::state),
        ];
        for state in states.into_iter().flatten() {
            match state {
                SessionState::Up => any_up = true,
                SessionState::Degraded => any_degraded = true,
                SessionState::Connecting => any_connecting = true,
                SessionState::Down => {}
            }
        }
        if any_up {
            SessionState::Up
        } else if any_degraded {
            SessionState::Degraded
        } else if any_connecting {
            SessionState::Connecting
        } else {
            SessionState::Down
        }
    }
}

impl SearcherSession for RouteMuxSession {
    fn state(&self) -> SessionState {
        self.aggregate_state()
    }

    fn can_send(&self) -> bool {
        self.default_jito.can_send()
            || self
                .public_rpc
                .as_ref()
                .map(SearcherSession::can_send)
                .unwrap_or(false)
            || self
                .helius_sender
                .as_ref()
                .map(SearcherSession::can_send)
                .unwrap_or(false)
    }

    fn can_send_request(&self, request: &BundleRequest<'_>) -> bool {
        match request.transport_hint {
            PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC => self
                .public_rpc
                .as_ref()
                .map(SearcherSession::can_send)
                .unwrap_or(false),
            PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER => self
                .helius_sender
                .as_ref()
                .map(SearcherSession::can_send)
                .unwrap_or(false),
            PRIVATE_EGRESS_TRANSPORT_JITO_BUNDLE | _ => self.default_jito.can_send(),
        }
    }

    fn send_bundle(&mut self, request: &BundleRequest<'_>) -> Result<String, SendError> {
        match request.transport_hint {
            PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC => self
                .public_rpc
                .as_mut()
                .ok_or(SendError::NotConnected)?
                .send_bundle(request),
            PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER => self
                .helius_sender
                .as_mut()
                .ok_or(SendError::NotConnected)?
                .send_bundle(request),
            PRIVATE_EGRESS_TRANSPORT_JITO_BUNDLE | _ => self.default_jito.send_bundle(request),
        }
    }

    fn poll(&mut self) {
        self.default_jito.poll();
        if let Some(session) = self.public_rpc.as_mut() {
            session.poll();
        }
        if let Some(session) = self.helius_sender.as_mut() {
            session.poll();
        }
    }
}
