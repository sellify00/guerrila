// G6 – Jito gRPC client plumbing.
//
// Vendored proto types + minimal auth/channel helpers for the Searcher API.
// Only the SendBundle RPC path is exposed; no BundleResult/subscription
// flows are imported (PM owns off-chain outcome handling).

pub mod auth;
pub mod channel;
pub mod proto;
pub mod service;
