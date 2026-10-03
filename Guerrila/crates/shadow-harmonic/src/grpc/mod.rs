// G7 – Harmonic gRPC client plumbing.
//
// Vendored proto types + minimal auth/channel helpers for the Harmonic API.
// Mirrors the structure of shadow-egress-jito/src/grpc/ but adapted for
// Harmonic's regional multi-endpoint model.

pub mod auth;
pub mod channel;
pub mod proto;
pub mod service;
