# SH0 Report

Implemented SH0 source-grade plumbing so G12 evidence can distinguish:

- `external_geyser`
- `local_validator_geyser`
- real private-grade packet ingress `afxdp`

## What changed

- Added explicit ingress source constants for `external_geyser` and `local_validator_geyser`.
- Added `system.proxies.ingress.grpc_source_grade` with default `external_geyser`.
- Removed the hardcoded public-Geyser source from gRPC ingest worker startup.
- Kept external/local Geyser on non-private routing semantics.
- Carried the raw `signal_source` byte through worker intents, no-send assembly, rollout shadow events, PM rollout stats, and G12 gate export/report.
- Added source-split worker diagnostics and rollout counters.

## Truth rules enforced by SH0

- `external_geyser` != private-grade
- `local_validator_geyser` != private-grade
- `afxdp` = private-grade only when it is actual AF_XDP ingress

## QC follow-up closed

PM rollout private-grade accounting is now **AF_XDP-only**. Non-AF_XDP sources such as:

- `jito_searcher_api`
- `private_mempool`
- `harmonic_searcher_api`

no longer increment:

- `observe_candidates_private_grade`
- `rollout_private_grade_admissible_opportunities`

## Main counters now available

- `worker_signal_external_geyser`
- `worker_signal_local_validator_geyser`
- `worker_signal_afxdp`
- `observe_candidates_external_geyser`
- `observe_candidates_local_validator_geyser`
- `observe_candidates_private_grade`
- `rollout_private_grade_admissible_opportunities`
- `rollout_geyser_shadow_opportunities`

## Focused verification run

```text
cargo test -p shadow-ingest
cargo test -p shadow-ipc
cargo test -p shadow-core rollout
cargo test -p shadow-tier0 source
cargo test -p shadow-tier0 g12
```

Result: passed after the SH0 patch set.

## Files packaged here

This folder contains the core SH0 review surface in flattened form so QC can be done without jumping through crate directories.
