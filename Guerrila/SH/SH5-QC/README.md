# SH5-QC - Local Validator Geyser Ingest Pack

This folder captures the current **SH5-lite / SH5-A-B** state on the constrained replacement host:

- host: `m4-metal-large-tyo-1`
- ssh: current constrained replacement host
- scope: Shadow observe-only ingest from localhost Yellowstone on top of the SH4-lite validator

This remains an **observe-only ingest proof**:

- no AF_XDP
- no private-grade relabeling
- no `r1_tiny`
- no live send path
- SH5-B is closed only as a controlled software proof; no host/mainnet candidate proof yet

## Purpose

SH5-A exists to prove that Shadow can consume the local validator Yellowstone stream as `local_validator_geyser`, normalize transaction updates, enqueue `IngressRecord`s into worker feed rings, and move:

```text
worker_signal_local_validator_geyser > 0
```

before any candidate-generation or live-path work is considered.

SH5-B then closes the controlled supported-fixture path: a gRPC-emitted
`local_validator_geyser` record must survive the normal decode/emission path,
reach money-worker observe-only routing, increment candidate counters, and still
avoid private-grade or live-send permission.

## File map

- `README.md` - reusable SH5-A scope, acceptance target, and evidence map.
- `SH5_REPORT.md` - live execution report for the current `.55` SH5-A run plus SH5-B software-proof closure.
- `evidence\2026-04-28-sh5a-producer-path-blocked\` - archived repo copy of the pre-patch blocked-state artifact bundle.
- `evidence\2026-04-28-sh5a-source-probe-green\` - archived repo copy of the SH5-A green source-probe bundle.

## On-host evidence paths

Historical blocked-state evidence:

```text
~/shadow-host-audit/sh5-local-validator-geyser/20260428_145715-after-grpc-diag
```

Latest source-probe pass:

```text
~/shadow-host-audit/sh5-local-validator-geyser/20260428_160610-source-probe
```

Key files in each bundle:

- `g12_summary.txt`
- `grpc_path_grep.txt`
- `sh5a-tier0.log`
- `status.txt`

Archived repo copy of the blocked-state bundle:

```text
SH\SH5-QC\evidence\2026-04-28-sh5a-producer-path-blocked\
```

Archived repo copy of the green source-probe bundle:

```text
SH\SH5-QC\evidence\2026-04-28-sh5a-source-probe-green\
```

## Acceptance target

SH5-A is green only when all of the following are true:

- `worker_records_seen > 0`
- `worker_signal_local_validator_geyser > 0`
- `worker_signal_afxdp = 0`
- `observe_candidates_private_grade = 0`
- `rollout_private_grade_admissible_opportunities = 0`
- `live_landed = 0`

Candidate counters are not required for SH5-A. Those belong to later SH5-B work.

## Current status

SH5-A is **green / source-proof passed** on the constrained `.55` host.

The latest `.55` source-probe rerun proved:

- `grpc_connected = 1`
- `grpc_subscribe_sent = 1`
- `grpc_updates_transaction = 9154`
- `grpc_normalize_ok = 9154`
- `grpc_pipeline_drop_unsupported_program = 0`
- `grpc_feed_enqueue_ok = 9125`
- `grpc_feed_enqueue_err = 0`
- `worker_records_seen = 9106`
- `worker_signal_local_validator_geyser = 9106`
- `worker_signal_afxdp = 0`
- `observe_candidates_private_grade = 0`
- `rollout_private_grade_admissible_opportunities = 0`
- `live_landed = 0`

This is the intended SH5-A truth shape: local Yellowstone records now reach worker-visible
`IngressRecord`s as `local_validator_geyser`, while private-grade and live counters remain at
zero.

On the host run itself, SH5-B remained deferred:

- `observe_candidates_local_validator_geyser = 0`
- `rollout_geyser_shadow_opportunities = 0`

That stayed correct for SH5-A because the `.55` rerun was only a source-proof gate.

SH5-B is now closed separately as a controlled software proof via
`money_worker::tests::sh5_b_grpc_local_validator_fixture_reaches_observe_candidate_split`.
That test proves a gRPC-emitted supported `local_validator_geyser` record yields:

- `worker_records_seen = 1`
- `worker_signal_local_validator_geyser = 1`
- `observe_candidates_local_validator_geyser = 1`
- `rollout_geyser_shadow_opportunities = 1`
- `observe_candidates_private_grade = 0`
- `rollout_private_grade_admissible_opportunities = 0`
- `would_send = 0`
- `live_send_suppressed = 0`

This closes SH5-B as software proof only. It does not authorize SH6, SH8,
`r1_tiny`, or any live promotion, and a stronger host/mainnet replay remains a
later strengthening step if needed.
