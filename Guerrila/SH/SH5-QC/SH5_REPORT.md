# SH5 Report - Local Validator Geyser Ingest and Observe-Candidate Proof

## Scope

This report records the current **SH5-lite / SH5-A-B** state on the replayed constrained host:

- host: `m4-metal-large-tyo-1`
- ssh: current constrained replacement host
- primary NIC: `eno1`

Goal:

```text
current SH4-lite localhost Yellowstone
-> point Shadow at http://127.0.0.1:10000
-> keep grpc_source_grade = local_validator_geyser
-> stay observe-only / no-send
-> prove worker_signal_local_validator_geyser > 0
-> keep AF_XDP/private-grade and live-send counters at 0
```

## Why SH5-A is separate

SH4-lite only proved that the local validator and Yellowstone plugin were healthy on `.55`. SH5-A is the first stage where Shadow itself must prove that a localhost Yellowstone transaction stream can become worker-visible `IngressRecord`s with the correct non-private source labeling.

This began as a source-grade ingest gate. SH5-B is now also closed locally as a controlled
supported-fixture software proof, but that does not upgrade the constrained host result into a
stronger mainnet/local-validator replay proof.

## Work performed on `.55`

### 1. Replayed the real SH5 source-grade tree

The initial SH5 bundle on `.55` lagged the local working tree and did not include the newer SH0/SH5 source-grade plumbing. The current host clone was repaired by syncing the required local runtime/config files, then rebuilding the coupled crates so the host actually carried:

- `grpc_source_grade`
- `local_validator_geyser`
- `grpc_source_probe_mode: unsupported_program_noop`
- source-split worker counters
- source-split G12 exports

### 2. Brought up the required local Shadow substrate

The current SH5-A host posture includes:

- SH4 localhost Yellowstone still serving `127.0.0.1:10000`
- `shadow-state` publishing required shared-memory segments
- `shadow-oracle` publishing tip LUT segments
- `shadow-tier0` running in observe-only / no-send posture

This proved the remaining blocker was no longer missing substrate or startup readiness.

### 3. Narrowed the failure with SH5 gRPC-stage diagnostics

The latest Shadow patch added explicit SH5 counters for:

- gRPC connection and subscribe success
- total and transaction update counts
- normalization success/error counts
- feed enqueue success/full/error counts
- `local_validator_geyser` source-carry counts

It also removed the stale Raydium account filter from the Tier0 subscribe request so SH5-A matched the known-good transactions-only Yellowstone client shape.

### 4. Added the gated source-proof path

The final SH5-A patch added a fail-closed observe-only source-probe mode for:

```text
grpc_source_grade = local_validator_geyser
grpc_source_probe_mode = unsupported_program_noop
rollout.mode = observe_only
rollout.no_send_backend = true
shred_capture_workers = 0
```

In that posture, normalized local Yellowstone transactions that would previously die at
`UnsupportedProgram` are converted into no-op source-proof records. They increment worker ingest
and source counters, then stop before candidate/build/send/private-grade accounting.

This source-probe mode is intentionally observe-only and no-op:

- allowed only for `local_validator_geyser`
- allowed only under observe-only / no-send conditions
- does not create strategy candidates
- does not increment private-grade counters
- does not permit live send
- does not relabel Geyser as AF_XDP

## Latest green result

The latest bounded `.55` rerun completed with normal timeout status for a bounded window:

```text
SH5A_RUN_STATUS=124
```

Observed G12 summary:

```text
grpc_connected=1
grpc_subscribe_sent=1
grpc_updates_transaction=9154
grpc_normalize_ok=9154
grpc_normalize_err=0
grpc_pipeline_drop_unsupported_program=0
grpc_feed_enqueue_ok=9125
grpc_feed_enqueue_full=0
grpc_feed_enqueue_err=0
worker_records_seen=9106
worker_signal_local_validator_geyser=9106
observe_candidates_local_validator_geyser=0
rollout_geyser_shadow_opportunities=0
worker_signal_external_geyser=0
worker_signal_afxdp=0
observe_candidates_private_grade=0
rollout_private_grade_admissible_opportunities=0
live_landed=0
```

## What the evidence proves

The source path is now complete:

```text
Yellowstone emits tx stream
-> Shadow connects and subscribes
-> Shadow receives transaction updates
-> Shadow normalizes transaction updates
-> source-probe mode emits no-op source-proof records
-> producer enqueues worker-visible IngressRecords
-> workers count local_validator_geyser records
```

The decisive SH5-A gate is now satisfied:

- `worker_records_seen > 0`
- `worker_signal_local_validator_geyser > 0`
- `worker_signal_afxdp = 0`
- `observe_candidates_private_grade = 0`
- `rollout_private_grade_admissible_opportunities = 0`
- `live_landed = 0`

At the same time, the `.55` host run itself still left SH5-B candidate counters at zero:

- `observe_candidates_local_validator_geyser = 0`
- `rollout_geyser_shadow_opportunities = 0`

That is acceptable for SH5-A because this run is proving source receipt, not candidate generation.

## SH5-B software proof

SH5-B is now closed locally through a controlled supported fixture rather than by pretending random
devnet traffic on `.55` was already a real candidate stream.

The new test
`money_worker::tests::sh5_b_grpc_local_validator_fixture_reaches_observe_candidate_split`
emits a supported gRPC record as `local_validator_geyser`, routes it through the same decode and
emission path, and then proves the money-worker observe-only split:

```text
worker_records_seen = 1
worker_signal_local_validator_geyser = 1
observe_candidates_local_validator_geyser = 1
rollout_geyser_shadow_opportunities = 1
observe_candidates_private_grade = 0
rollout_private_grade_admissible_opportunities = 0
would_send = 0
live_send_suppressed = 0
```

The proof outcome is still observe-only / no-send (`RouteReason::PublicTarget`), so the supported
local-validator candidate path is now covered without relabeling Geyser as private-grade and
without opening any live send path.

## On-host evidence paths

Historical blocked-state evidence:

```text
~/shadow-host-audit/sh5-local-validator-geyser/20260428_145715-after-grpc-diag
```

Current SH5-A green source-probe evidence:

```text
~/shadow-host-audit/sh5-local-validator-geyser/20260428_160610-source-probe
```

Key artifacts in each bundle:

- `g12_summary.txt`
- `grpc_path_grep.txt`
- `sh5a-tier0.log`
- `status.txt`

Archived repo copy:

```text
SH\SH5-QC\evidence\2026-04-28-sh5a-producer-path-blocked\
```

Archived repo copy of the green source-probe bundle:

```text
SH\SH5-QC\evidence\2026-04-28-sh5a-source-probe-green\
```

## SH5-A gate status

SH5-A is now **green / source-proof passed** because the required ingest gate is met:

```text
worker_signal_local_validator_geyser = 9106
worker_records_seen = 9106
```

The observe-only truth constraints also still hold:

- `worker_signal_afxdp = 0`
- `observe_candidates_private_grade = 0`
- `rollout_private_grade_admissible_opportunities = 0`
- `live_landed = 0`

## Remaining boundary

The next boundary is no longer SH5-B software proof. If stronger SolanaHost evidence is wanted
later, the next strengthening step is to repeat the SH5-B candidate path on a larger host with a
real local-validator stream.

Do not spend the next cycle on:

- SH6 comparator work
- SH8 AF_XDP
- `r1_tiny`
- any live send path
- any relabeling of local Geyser as private-grade

Do not treat this software proof as permission to start SH6, SH8, `r1_tiny`, or any live send
path.
