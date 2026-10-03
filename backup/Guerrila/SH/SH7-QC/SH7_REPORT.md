# SH7 Report - PM Final No-Send Evidence

## Scope

This report records the current **SH7-A / G12 software continuation** state.

Goal:

```text
observe candidate exists
-> worker publishes final no-send event
-> PM consumes final no-send event
-> rollout_would_send > 0
-> rollout_live_send_suppressed > 0
-> live_landed = 0
-> promote_allowed remains false
```

SH7-A is not validator work and does not unlock SH6. It is the PM/rollout accounting continuation
needed before micro-market and ShadowGuard acceptance can become meaningful.

## Starting blocker

The latest handoff state showed the old G12 blocker:

```text
worker_shadow_events_published > 0
rollout_would_send = 0
rollout_live_send_suppressed = 0
micro_market_observations = 0
promote_allowed = false
```

Interpretation:

- ingest, worker health, pool hits, latency, and route-opportunity publication were already moving;
- PM was still missing final no-send execution events;
- micro-market stayed downstream-blocked because it needs final no-send observations;
- ShadowGuard ops proof remained separately blocked by the missing deployer keypair.

## Work performed locally

### 1. Frozen SH5-B state

SH5-B was accepted as green only as a controlled software proof at:

```text
ac278c0 SH5-B observe candidate software proof
```

The local archive artifacts were moved under `.local-artifacts/`, and that folder is ignored by:

```text
8964546 Ignore local sync artifacts
```

### 2. Added SH7-A no-send visibility

The first SH7-A diagnostic patch landed at:

```text
13ebab0 Add SH7-A no-send diagnostics
```

It added worker-side final no-send publish counters:

```text
worker_rollout_would_send_events_published
worker_rollout_live_send_suppressed_events_published
```

It also added PM/gate-report context:

```text
rollout_events_observed
rollout_last_no_send_reason
rollout_last_drop_reason
```

These counters do not change routing semantics or live behavior. They only expose whether the final
no-send chain stops before worker publication, between worker publication and PM consumption, or
inside PM classification.

### 3. Kept gRPC/source truth intact

The SH7-A validation sweep also fixed the brittle gRPC normalize structural guard so it no longer
depends on rustfmt import ordering. This preserves the shared gRPC decode-path guard without making
`cargo fmt --check` fail.

## Validation performed

The local post-change sweep passed:

```text
cargo fmt --all --check
cargo test -p shadow-tier0 sh5_b -- --nocapture
cargo test -p shadow-tier0 local_validator_geyser -- --nocapture
cargo test -p shadow-tier0 grpc -- --nocapture
cargo test -p shadow-core rollout -- --nocapture
cargo test -p shadow-control rollout_gate -- --nocapture
cargo build -p shadow-tier0 --bin shadow-tier0
```

## Current-host R0 diagnostic

After the diagnostics landed, the current constrained local-validator/Geyser host was synced with
the latest runtime code and run in observe-only/no-send posture. The host checkout itself was stale,
so the runtime code was overlaid from the local branch and the host's dirty pre-sync state was saved
under `~/shadow-host-audit/pre-sh7a-sync/`.

The successful diagnostic artifact is:

```text
~/shadow-host-audit/sh7a-no-send/20260428_174559-tier0-ubuntu
```

The run used the local-validator source config and did not enable live send. The important counters
were:

```text
worker_records_seen = 10461
worker_signal_local_validator_geyser = 10461
worker_pool_state_hit = 0
observe_candidates = 0
observe_candidates_local_validator_geyser = 0
worker_shadow_events_published = 0
worker_rollout_would_send_events_published = 0
worker_rollout_live_send_suppressed_events_published = 0
rollout_events_observed = 0
rollout_would_send = 0
rollout_live_send_suppressed = 0
micro_market_observations = 0
worker_health_drops = 0
live_landed = 0
all_gates_green = false
promote_allowed = false
```

Interpretation:

- Tier0 is receiving local-validator Geyser records.
- Those records are not becoming pool-state hits or observe candidates in this host window.
- The final no-send bridge is not yet exercised by this host run because there is no candidate to
  suppress.
- `live_landed` stayed zero and `promote_allowed` stayed false.

This run therefore moves SH7-A from "host rerun pending" to "host rerun complete, blocked upstream
of candidate creation." The next useful SH7-A action is to restore a candidate-producing worker
universe on the current host, then rerun the same no-send counters.

## Candidate-substrate recovery attempt

The generic G12 configs had drifted behind the current fail-closed config schema. They were updated
so the current binary accepts them again:

```text
submit.routing.pre_sign_egress_admission_gate = true
system.proxies.health.publish_egress_health = true
system.proxies.health.degraded_hysteresis_batches = 2
state.geyser.harmonic_max_state_slot_lag = 1
state.geyser.require_live_transfer_fee_updates = true
state.clmm.publish_dist_to_tick_lamports = true
submit.harmonic.quarantine_scope = class_scoped_with_global_failsafe
submit.harmonic.tactic_class_count = 8
```

Static validation now passes for:

```text
config.guerrila.yaml
g12-qc/config.guerrila.yaml
g1-host-pack/config.guerrila.yaml
```

On the current host, an external G12 state rehydrate was attempted without using the SH5
local-validator config. The first attempt did not produce a usable candidate substrate: the state
service fetched the external pool/mint/ALT account sets but published a zero-count snapshot:

```text
pools = 0
mints = 0
vaults = 0
alts = 0
```

That means another host Tier0 run would still not be a valid SH7-A no-send evidence run. The
temporary external state/oracle pair was stopped, and the active host config was restored to the
local-validator posture.

The follow-up substrate diagnostic identified and fixed the state-side issue:

```text
RPC returned accounts: yes
pool decode rejected accounts: no
store inserted state: yes after bootstrap promotion
publisher wrote nonzero state: yes after bootstrap publish
```

The repaired state bootstrap now applies the bootstrap snapshot on `BootstrapComplete`, publishes the
dirty snapshot immediately, reports sanitized return/decode/store/publish counters, and avoids the
unbounded full ALT program scan in this Geyser/RPC candidate-substrate path. Latest host substrate
evidence:

```text
bootstrap_rpc_program_accounts_returned{program=raydium_pool} = 705140
bootstrap_pool_decode_ok = 705140
bootstrap_pool_decode_reject_short = 0
bootstrap_pool_decode_reject_status = 0
bootstrap_pool_decode_reject_other = 0
bootstrap_pool_store_inserted = 705140
bootstrap_pool_published = 262140
bootstrap_pool_deferred_missing_vaults = 15
bootstrap_mint_decode_ok = 700158
bootstrap_mint_store_inserted = 700158
bootstrap_mint_published = 699873
bootstrap_vault_decode_ok = 1410250
bootstrap_vault_store_inserted = 1410250
bootstrap_alt_accounts_returned = 0
bootstrap_alt_published = 0
bootstrap complete: pools = 705140, mints = 700158, vaults = 1410250, alts = 0
```

ALT full-program bootstrap remains intentionally skipped for this path; ALT state can accumulate from
live Geyser/demand hydration and is not required to prove the immediate pool-hit/no-send split.

## Controlled software isolate

Because the live candidate substrate is still not restored, SH7-A now has a controlled software
fixture that isolates the no-send accounting path:

```text
supported record
-> pool_state_hit
-> route/opportunity event
-> process_intent
-> no-send backend
-> final would_send/live_send_suppressed event
-> PM rollout ledger
```

The fixture also fixed the worker-side final no-send diagnostic counter path: final events emitted
through the no-send sink now increment:

```text
worker_rollout_would_send_events_published
worker_rollout_live_send_suppressed_events_published
```

Validation:

```text
cargo test -p shadow-tier0 sh7_a_controlled_supported_candidate_reaches_final_no_send_pm_ledger -- --nocapture
cargo test -p shadow-tier0 g12_observe_only_exhaust_scavenge_records_shadow_event -- --nocapture
```

Result: controlled software isolate green. This was later superseded by the host green run below.

## Host candidate-producing no-send run

After the substrate fix, the host was run with external G12 state/oracle shared memory, no AF_XDP
workers, observe-only/no-send Tier0, a throwaway local signer for no-send assembly, and Rosetta
disabled in the host candidate config because the raw archive belongs to the deferred AF_XDP/SH8
path. No live send path was enabled.

Artifact:

```text
~/shadow-host-audit/sh7a-no-send/20260428_192828-candidate-substrate
```

Counters:

```text
sample_slots = 494
worker_records_seen = 39
worker_pool_state_hit = 14
observe_candidates = 14
worker_shadow_events_published = 28
worker_rollout_would_send_events_published = 0
worker_rollout_live_send_suppressed_events_published = 0
rollout_events_observed = 14
rollout_would_send = 0
rollout_live_send_suppressed = 0
rollout_last_no_send_reason = 0
rollout_last_drop_reason = 1
micro_market_observations = 0
micro_market_uplift_ok = false
worker_health_drops = 0
live_landed = 0
all_gates_green = false
promote_allowed = false
```

Interpretation: host candidate substrate was recovered, and PM observed route-opportunity rollout
events, but Tier0 still did not publish final no-send execution events. The next patch therefore
targeted route classification before `process_intent`.

## SH7-A host green run

The accepted host artifact is:

```text
/home/ubuntu/shadow-host-audit/sh7a-no-send/20260428_210813-geyser-shadow-nosend-stale-ok
```

It proves the full observe-only/no-send path:

```text
external/local Geyser candidate
-> pool/state hit
-> observe candidate
-> gated Geyser shadow no-send admission
-> process_intent
-> no-send backend
-> final no-send event publish
-> PM rollout ledger counts would_send + live_send_suppressed
```

Key counters:

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

## SH7-B micro-market proof

The same accepted host artifact closes SH7-B because the final no-send event stream now carries
micro-market counterfactual fields into PM and the G12 gate export:

```text
rollout_would_send = 4
rollout_live_send_suppressed = 4
micro_market_observations = 4
micro_market_uplift_ok = true
micro_market_banked_retained_ev_lamports = 2850446
micro_market_flat_baseline_retained_ev_lamports = 2850446
micro_market_delta_lamports = 0
live_landed = 0
all_gates_green = false
promote_allowed = false
```

The micro-market delta is exactly zero in this window, so the banked class/index matched the flat
baseline retained EV. That is still green for SH7-B because the gate requires real observations and
non-negative uplift, not strictly positive uplift. Promotion remains false because ShadowGuard ops
proof is still missing.

## SH7 gate status

SH7-A and SH7-B are closed for observe-only/no-send software evidence:

```text
SH7-A final no-send proof: closed
SH7-B micro-market proof: closed
live_landed = 0
promote_allowed = false
```

## Expected diagnostic split

```text
worker_rollout_would_send_events_published = 0
-> tier0 is not reaching successful emit_prepared_no_send

worker_rollout_would_send_events_published > 0
and rollout_would_send = 0
-> PM is not consuming/classifying final no-send events

rollout_events_observed > 0
and rollout_last_no_send_reason/drop_reason wrong
-> PM consumed events but classification is wrong
```

## Remaining blockers

### SH7-C ShadowGuard ops proof

SH7-C is now green on the replacement `.57` host. Current host facts:

```text
/etc/shadow-engine/shadow-guard-deployer.json = present, root:root mode=600, funded host-independent deployer
solana CLI on PATH = present, solana-cli 3.1.14
execution.shadow_guard_program_id = NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW
guard vault fields = verified Raydium V4 token accounts
shadow-guard-program source = present and builds with cargo-build-sbf
deployer balance after deploy = 0.31427784 SOL
shadow_guard_mainnet_deployed = true
shadow_guard_template_program_id_matches = true
guard_vault_layout_smoke = true
live_landed = 0
promote_allowed = false
```

The preflight script was hardened to reject known placeholder and non-ShadowGuard IDs fail-closed,
including IDs equal to `execution.swap_program_id`, AddressLookupTable placeholders, token program
IDs, and sysvars. It now also records deployer keypair ownership/mode and fails if group/world bits
are present. It also invokes Solana account/program checks with the staged deployer keypair so root
does not need a separate default signer.

SH7-C green evidence:

```text
program_id = NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW
deploy_signature = 39ZxpfbxLk4eyrWT6DaSSBwWiVx3HCg9dCkyJLTf5bsn74haZ6vs6KbWTWMM4zHAERLa7J57YcckoZDgtFrnFFiX
shadow_guard_smoke_pass_ok = true
shadow_guard_smoke_abort_ok = true
shadow_guard_program_id_present = true
shadow_guard_mainnet_deployed = true
shadow_guard_template_program_id_matches = true
sample_slots = 285
observe_candidates = 12
live_landed = 0
promote_allowed = false
all_gates_green = false
```

The program keypair changed from the earlier `.55` prepared value because the `.55` host-local
program keypair was not preserved before reprovisioning. The `.57` proof supersedes that blocked
prepared ID. The funded deployer remained host-independent and was securely staged on `.57`.

### New-host continuity note

If `.57` is replaced later, SH7-C remains recoverable, but the host-local ops state must be
recreated. The repo contains the ShadowGuard code, tests, hardened preflight, template proof helper,
and green evidence. It does not contain private keypair JSON files or `/etc/shadow-engine` host
config.

Carry forward or recreate these facts on the new host:

```text
deployer_pubkey = 4iYqgdyuU8YE6FkZRq3195SZU7m5E5MkaNeaHayfoiZJ
deployed_program_id = NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW
guard_vault_in = HAULecjkcF2GHGSQ566yRBuwRoHxH24YGZs1n6B3QpAG
guard_vault_out = 9mo6Dhx8RhrwNqxCBGcfqEZzmGPGr4hz1mfTdW8tpsq7
last_known_shadow_guard_mainnet_deployed = true
last_known_shadow_guard_template_program_id_matches = true
last_known_live_landed = 0
last_known_promote_allowed = false
```

The exact deployed program ID can only be preserved for future upgrades if
`/etc/shadow-engine/shadow-guard-program-keypair.json` is securely restored and its pubkey matches
`NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`. If that keypair is not available, generate and deploy
a new program keypair, update active configs, and treat the old program ID as superseded. Do not
commit keypairs or deploy recovery material to git.

Minimum new-host proof loop:

```text
pull sh0-source-grade at 7b138ab or newer
install/verify Solana CLI + Rust/cargo/cargo-build-sbf
stage keypairs under /etc/shadow-engine with root:root mode=600
wire execution.shadow_guard_program_id and rollout.shadow_guard_program_id
wire real guard vault accounts
run 21_shadowguard_preflight.sh
verify ShadowGuard on mainnet-beta or deploy a replacement program ID
run guard abort/pass smoke
export G12 gate input/report
require shadow_guard_mainnet_deployed=true
require shadow_guard_template_program_id_matches=true
require live_landed=0
require promote_allowed=false
```

The broader G12 board still requires:

```text
ShadowGuard ops proof green: satisfied
promote_allowed = false until all gates are actually green
```

### G12-R0 long observe-only accumulation

After SH7-A/B/C were green, `.57` ran a longer observe-only/no-send G12-R0 accumulation:

```text
/home/ubuntu/shadow-host-audit/g12-r0-observe/20260429_084358-long-accumulation
```

Final counters:

```text
status = threshold_reached
elapsed = 7446
sample_slots = 18401
observe_candidates = 10335
rollout_would_send = 3743
rollout_live_send_suppressed = 3743
micro_market_observations = 3743
micro_market_uplift_ok = true
shadow_guard_mainnet_deployed = true
shadow_guard_template_program_id_matches = true
worker_health_drops = 0
live_landed = 0
promote_allowed = false
all_gates_green = false
```

This closes the long observe-only safety-soak objective: the run maintained no-send/live-suppressed
behavior, ShadowGuard truth, and micro-market observations while collecting more than the configured
sample/candidate volume targets.

Full G12 promotion remains correctly red because this run was intentionally Geyser-only and did not
prove private-grade ingress or worker latency:

```text
private_lane_admissibility_ok = false
observe_candidates_private_grade = 0
rollout_private_grade_admissible_opportunities = 0
latency_ok = false
worker_latency_proof_present = false
worker_signal_external_geyser = 8371
worker_signal_afxdp = 0
```

Interpretation:

```text
G12-R0 long safety soak: green
G12 full promotion threshold: not green
Reason: no private-grade ingress / latency proof in this Geyser-only accumulation run
```

## Non-unlocks

Do not treat SH7-A diagnostics as permission to start:

- SH6 timing comparator
- SH8 AF_XDP packet capture
- `r1_tiny`
- live send path
- public RPC exposure
- public execution fallback

