# SH7-QC - PM No-Send Evidence Pack

This folder captures the current **SH7-A / G12 software continuation** phase:

- scope: PM final no-send execution evidence
- latest checkpoint commit: `6278258` (`Add SH7-A controlled no-send proof`)
- current proof status: controlled software proof is green; host SH7-A observe-only/no-send proof is green for external-Geyser shadow admission and final no-send publication

This remains an **observe-only / no-send software proof**:

- no SH6 timing comparator
- no SH8 AF_XDP packet-ingress claim
- no `r1_tiny`
- no live send path
- no public RPC or public execution fallback
- no relabeling of `external_geyser` or `local_validator_geyser` as private-grade

## Purpose

SH7-A exists to close the old G12 blocker where workers publish route-opportunity shadow
events, but PM still reports:

```text
rollout_would_send = 0
rollout_live_send_suppressed = 0
micro_market_observations = 0
```

The goal is to prove the final no-send execution chain:

```text
observe candidate exists
-> worker builds/suppresses no-send execution event
-> PM consumes final no-send event
-> rollout_would_send > 0
-> rollout_live_send_suppressed > 0
-> live_landed = 0
-> promote_allowed = false
```

## File map

- `README.md` - reusable SH7-A scope, acceptance target, diagnostics, and evidence map.
- `SH7_REPORT.md` - execution report for the current SH7-A diagnostic state.
- `evidence\README.md` - evidence index for host R0 rerun bundles.
- `evidence\sh7a-current-host-summary.txt` - latest current-host SH7-A diagnostic summary.
- `evidence\sh7a-controlled-software-summary.txt` - controlled software-isolate proof for the final no-send path.
- `evidence\sh7a-host-candidate-substrate-summary.txt` - host candidate-substrate recovery and latest SH7-A no-send split.
- `evidence\sh7b-micro-market-proof-summary.txt` - SH7-B host micro-market proof extracted from the green SH7-A artifact.
- `evidence\sh7c-shadowguard-green-*` - SH7-C ShadowGuard deploy, smoke, preflight, and gate proof.
- `scrutiny\` - flattened source copies, patch, and sanitized host diagnostics for QC of the current SH7-A Geyser shadow no-send proof.

## Current diagnostic counters

The SH7-A diagnostics slice added these counters so the next host run can distinguish where the
no-send path stops:

```text
worker_rollout_would_send_events_published
worker_rollout_live_send_suppressed_events_published
rollout_events_observed
rollout_last_no_send_reason
rollout_last_drop_reason
```

Interpretation:

- `worker_rollout_would_send_events_published = 0` means tier0 did not publish final no-send events.
- `worker_rollout_would_send_events_published > 0` with `rollout_would_send = 0` means PM did not consume or classify them.
- `rollout_events_observed > 0` with wrong last reason/drop codes means PM consumed events but the event classification is wrong.

## Acceptance target

SH7-A is green only when an observe-only/no-send run proves:

```text
observe_candidates > 0
worker_shadow_events_published > 0
worker_rollout_would_send_events_published > 0
worker_rollout_live_send_suppressed_events_published > 0
rollout_events_observed > 0
rollout_would_send > 0
rollout_live_send_suppressed > 0
live_landed = 0
worker_health_drops = 0
promote_allowed = false
```

Promotion must remain false until the remaining G12 gates are also green, including micro-market
uplift and ShadowGuard ops proof.

## Current status

SH7-A is **green for the observe-only/no-send software policy proof**. Earlier host work first repaired the external G12 state substrate:

```text
bootstrap_rpc_program_accounts_returned{program=raydium_pool} = 705140
bootstrap_pool_decode_ok = 705140
bootstrap_pool_store_inserted = 705140
bootstrap_pool_published = 262140
bootstrap_mint_store_inserted = 700158
bootstrap_mint_published = 699873
bootstrap_vault_store_inserted = 1410250
```

That made a candidate-producing host run possible. The previous diagnostics run showed candidates
dying before `process_intent`, at route resolution:

```text
sample_slots = 396
worker_records_seen = 26
worker_pool_state_hit = 16
observe_candidates = 16
worker_shadow_events_published = 32
worker_candidates_entered_process_intent = 0
worker_candidates_dropped_before_process_intent = 16
worker_route_resolved_drop = 16
worker_route_reason_public_target = 16
worker_rollout_would_send_events_published = 0
worker_rollout_live_send_suppressed_events_published = 0
rollout_events_observed = 16
rollout_would_send = 0
rollout_live_send_suppressed = 0
rollout_last_drop_reason = 1
micro_market_observations = 0
worker_health_drops = 0
live_landed = 0
promote_allowed = false
```

The earlier current-host local-validator rerun proved local-validator Geyser records reached Tier0,
but they did not become observe candidates:

```text
worker_records_seen = 10461
worker_signal_local_validator_geyser = 10461
worker_pool_state_hit = 0
observe_candidates = 0
worker_shadow_events_published = 0
worker_rollout_would_send_events_published = 0
rollout_would_send = 0
live_landed = 0
promote_allowed = false
```

That earlier local-validator run remains useful as source-grade evidence, but it was superseded for
SH7-A no-send proof by the later external-Geyser candidate-producing host runs.

The latest host run adds a narrow observe-only Geyser no-send shadow route and keeps live safety
closed:

```text
sample_slots = 285
worker_signal_external_geyser = 16
worker_pool_state_hit = 4
observe_candidates = 12
worker_geyser_shadow_no_send_admitted = 4
worker_candidates_entered_process_intent = 4
worker_process_intent_ok = 4
worker_no_send_backend_called = 4
worker_final_no_send_event_published = 4
worker_rollout_would_send_events_published = 4
worker_rollout_live_send_suppressed_events_published = 4
rollout_events_observed = 12
rollout_would_send = 4
rollout_live_send_suppressed = 4
observe_candidates_private_grade = 0
rollout_private_grade_admissible_opportunities = 0
worker_health_drops = 0
live_landed = 0
promote_allowed = false
```

The controlled software isolate is also green: a supported fixture proves candidate routing, final
no-send event publication, and PM ledger classification locally. This remains **software/observe-only
proof only** and does not approve SH6 timing, SH8 AF_XDP, `r1_tiny`, or any live send path.

SH7-B micro-market proof is closed from the same accepted host artifact. Final no-send events now
carry micro-market counterfactual fields into PM and the G12 gate export:

```text
micro_market_observations = 4
micro_market_uplift_ok = true
micro_market_banked_retained_ev_lamports = 2850446
micro_market_flat_baseline_retained_ev_lamports = 2850446
micro_market_delta_lamports = 0
live_landed = 0
promote_allowed = false
```

The zero delta means the observed banked strategy matched the flat baseline in this window; it is
still a valid SH7-B green result because the gate requires real observations and non-negative uplift.

SH7-C ShadowGuard ops proof is now green on the new `.57` host. The funded host-independent deployer
deployed ShadowGuard program `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW` on mainnet-beta, Jito
and Harmonic templates reference that program ID, the Raydium V4 guard vault simulation proves both
pass and abort behavior without sending a live transaction, and the G12 gate report records
`shadow_guard_mainnet_deployed=true`, `shadow_guard_template_program_id_matches=true`,
`live_landed=0`, and `promote_allowed=false`.

## Do not start from this gate

Do not use SH7-A diagnostics as approval for:

- SH6 timing comparator
- SH8 AF_XDP packet capture
- `r1_tiny`
- live send path
- public RPC exposure
- public execution fallback

