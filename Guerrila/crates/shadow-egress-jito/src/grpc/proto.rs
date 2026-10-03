// Hand-written prost message types vendored from jito-labs/mev-protos.
//
// Only the types required by the G6 Searcher API path are included:
//   - auth.proto   → challenge-response + token refresh
//   - searcher.proto → SendBundle RPC only
//   - bundle.proto → Bundle framing
//   - packet.proto → Packet (raw tx bytes)
//   - shared.proto → Header (timestamp wrapper)
//
// Deliberately excluded (PM owns off-chain outcome handling):
//   - BundleResult, Accepted, Rejected, Finalized, Processed, Dropped
//   - SubscribeBundleResults RPC
//   - All block_engine.proto / relayer.proto / shredstream.proto types

// ── shared ──────────────────────────────────────────────────────────────────

pub mod shared {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Header {
        #[prost(message, optional, tag = "1")]
        pub ts: ::core::option::Option<::prost_types::Timestamp>,
    }
}

// ── packet ──────────────────────────────────────────────────────────────────

pub mod packet {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Packet {
        #[prost(bytes = "vec", tag = "1")]
        pub data: ::prost::alloc::vec::Vec<u8>,
        #[prost(message, optional, tag = "2")]
        pub meta: ::core::option::Option<Meta>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Meta {
        #[prost(uint64, tag = "1")]
        pub size: u64,
        #[prost(string, tag = "2")]
        pub addr: ::prost::alloc::string::String,
        #[prost(uint32, tag = "3")]
        pub port: u32,
        #[prost(message, optional, tag = "4")]
        pub flags: ::core::option::Option<PacketFlags>,
        #[prost(uint64, tag = "5")]
        pub sender_stake: u64,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct PacketFlags {
        #[prost(bool, tag = "1")]
        pub discard: bool,
        #[prost(bool, tag = "2")]
        pub forwarded: bool,
        #[prost(bool, tag = "3")]
        pub repair: bool,
        #[prost(bool, tag = "4")]
        pub simple_vote_tx: bool,
        #[prost(bool, tag = "5")]
        pub tracer_packet: bool,
        #[prost(bool, tag = "6")]
        pub from_staked_node: bool,
    }
}

// ── bundle ──────────────────────────────────────────────────────────────────

pub mod bundle {
    use super::{packet, shared};

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Bundle {
        #[prost(message, optional, tag = "2")]
        pub header: ::core::option::Option<shared::Header>,
        #[prost(message, repeated, tag = "3")]
        pub packets: ::prost::alloc::vec::Vec<packet::Packet>,
    }
}

// ── searcher ────────────────────────────────────────────────────────────────

pub mod searcher {
    use super::bundle;

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct SendBundleRequest {
        #[prost(message, optional, tag = "1")]
        pub bundle: ::core::option::Option<bundle::Bundle>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct SendBundleResponse {
        #[prost(string, tag = "1")]
        pub uuid: ::prost::alloc::string::String,
    }
}

// ── auth ────────────────────────────────────────────────────────────────────

pub mod auth {
    /// Searcher = 1 in the proto enum.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Role {
        Relayer = 0,
        Searcher = 1,
        Validator = 2,
        ShredstreamSubscriber = 3,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct GenerateAuthChallengeRequest {
        #[prost(enumeration = "Role", tag = "1")]
        pub role: i32,
        /// 32-byte Ed25519 public key.
        #[prost(bytes = "vec", tag = "2")]
        pub pubkey: ::prost::alloc::vec::Vec<u8>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct GenerateAuthChallengeResponse {
        #[prost(string, tag = "1")]
        pub challenge: ::prost::alloc::string::String,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct GenerateAuthTokensRequest {
        #[prost(string, tag = "1")]
        pub challenge: ::prost::alloc::string::String,
        /// 32-byte Ed25519 public key (same as in the challenge request).
        #[prost(bytes = "vec", tag = "2")]
        pub client_pubkey: ::prost::alloc::vec::Vec<u8>,
        /// 64-byte Ed25519 signature of `pubkey || challenge`.
        #[prost(bytes = "vec", tag = "3")]
        pub signed_challenge: ::prost::alloc::vec::Vec<u8>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct GenerateAuthTokensResponse {
        #[prost(message, optional, tag = "1")]
        pub access_token: ::core::option::Option<Token>,
        #[prost(message, optional, tag = "2")]
        pub refresh_token: ::core::option::Option<Token>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Token {
        #[prost(string, tag = "1")]
        pub value: ::prost::alloc::string::String,
        #[prost(message, optional, tag = "2")]
        pub expires_at_utc: ::core::option::Option<::prost_types::Timestamp>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct RefreshAccessTokenRequest {
        #[prost(string, tag = "1")]
        pub refresh_token: ::prost::alloc::string::String,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct RefreshAccessTokenResponse {
        #[prost(message, optional, tag = "1")]
        pub access_token: ::core::option::Option<Token>,
    }
}
