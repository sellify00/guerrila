````md
# Shadow Execution Rule — Ingest to Egress

## 0. Global rule

Shadow does not choose architecture by ideology.
Shadow chooses the fastest profitable path that can be proven, reconciled, and repeated.

Habitat discovery is now an explicit layer ahead of strategy conversion:

```text
external market universe
-> Shadow Habitat Registry
-> focused HUNT/WATCH no-send windows
-> strategy-specific proof
-> tiny-live proposal
```

Volatility is a habitat filter, not a trade signal. The registry decides where Shadow should spend windows; Shadow strategy still decides whether a habitat is useful for DirectArb, Backrun, post-shock, housekeeping, or only saved-loss research.

Formal habitat standard:

```text
habitat score chooses where to look
readiness score decides whether Shadow can convert
```

DirectArb also has a hard operating split now:

```text
broad discovery
-> classify out-of-scope rows
-> update registry / Pool Book
-> no live-track claim

bounded hunt
-> Pool Book selected habitats only
-> strict readiness gates
-> Run78B only if a fresh candidate appears
```

Promotion rules:

- `HUNT_LIVE_TRACK`: usable-now instrument exists with exact quote/min_out, PM-ready, policy-clean, account/inventory ready, and private-egress no-send proof.
- `HUNT_RESEARCH`: focused windows beat the broad baseline, but usable-now execution is still zero.
- `WATCH`: signal exists, blocker is specific, and more focused evidence is still warranted.
- `TRAINING`: useful for learning or saved-loss calibration, not a monetization target.
- `DENY`: policy-bad, dust, unsupported, no visibility, or repeated non-conversion.

Hard rules:

- No live send without numeric quote + min_out.
- No live send without PM exact reserve.
- No live send without route-specific EV after fee/tip.
- No live send from stale target transactions.
- No source is promoted to live globally; promotion is per strategy.
- Tier0 must not perform network I/O, RPC calls, gRPC waits, or dynamic inference.

---

# 1. Source / ingest roles

## AF_XDP zero-copy

Role:

- Private-grade hot trigger.
- Raw packet / target freshness source.
- Required for live DirectArb for now.
- Required for true Backrun target freshness unless Helius preprocessed/Shred Delivery becomes available.

Use for:

- DirectArb live trigger.
- Backrun target transaction capture.
- Fast post-shock variants if timing-sensitive.
- Benchmark against provider feeds.

Process:

```text
shadow-ingest-afxdp
-> normalized ingress record
-> shadow-tier0
-> quote / policy / PM / template
-> egress
````

Current status:

```text
Keep for DirectArb.
Use for Backrun target freshness, but AF_XDP-only projection is not green yet.
Do not rely on long same-eno1 leases on .15.
```

---

## LaserStream processed

Role:

* Continuous source.
* Research/no-send source.
* Enrichment and backfill.
* Backrun/post-shock discovery source.
* Not private-grade live source by default.

Use for:

* Backrun no-send discovery.
* Post-shock / continuation / rebalancing research.
* State enrichment.
* Recurrence/markout training.
* Source comparison.
* ShadowLite feasibility.

Process:

```text
shadow-ingest-laserstream
-> source=laserstream / PublicGeyser
-> worker / recorder / research sink
-> no-send / analysis / strategy proof
```

Current status:

```text
LaserStream Backrun no-send = proven.
LaserStream DirectArb live = not proven.
LaserStream Drop = suppression/saved-loss only.
Processed LaserStream is too late for true ordered live Backrun freshness.
```

---

## LaserStream preprocessed / Shred Delivery

Role:

* Would be ideal for Backrun target freshness without AF_XDP.
* Not available on current plan.

Use only if enabled:

```text
source=laserstream_preprocessed
-> target tx bytes before landing
-> bundle [target tx, our tx, tip]
```

Current status:

```text
Blocked: PermissionDenied / Unsupported plan type.
Do not use until Helius enables preprocessed access.
```

---

## Webhooks

Role:

* Receipt/reconciliation layer.
* Not hot ingest.

Use for:

* Landed tx confirmation.
* Wallet/inventory change alerts.
* PM external witness.
* UI event feed.

Never use for:

* DirectArb trigger.
* Backrun trigger.
* Latency edge.

---

# 2. Strategy routing rules

## DirectArb

Primary ingest:

```text
AF_XDP
```

Secondary ingest:

```text
LaserStream = observe/no-send only until proven
```

Required live gates:

```text
source=afxdp
owner_mode=directarb
route usable
numeric quote present
min_out_raw > 0
candidate_ev_positive=true
PM can lock exact input
input notional within cap
projection sane
inventory ready
output account ready
template ready
```

Preferred egress:

```text
helius_sender_dual for single-tx DirectArb
jito/harmonic bundle if protection/ordering needed
tpu_quic_public as proof/fallback
```

Process:

```text
AF_XDP
-> Tier0 DirectArb scorer
-> quote/min_out
-> PM reserve
-> template/sign
-> Helius Sender or bundle egress
-> status/reconcile
```

Current status:

```text
AF_XDP DirectArb = leading live-trigger path.
Run77C private egress = green.
Strict DirectArb = DIRECTARB_STRUCTURAL_HUNT_READY inside the bounded battlefield.
Run78B stays closed until a fresh candidate selects.
Current blocker is market / EV / candidate availability, not architecture.
```

Important interpretation:

```text
Unbounded search may still surface ATA, policy, unsupported-input, or dust-exit blockers.
Those rows are discovery noise unless the readiness filter admits them.
They do not reopen the DirectArb structural certificate.
```

---

## Backrun

Primary live target source for now:

```text
AF_XDP target trigger
```

Research/discovery source:

```text
LaserStream processed
```

Future source if enabled:

```text
LaserStream preprocessed
```

Required true Backrun gates:

```text
target tx bytes available
target not landed/finalized
target blockhash valid
target tx first in bundle
our tx second in bundle
tip present
numeric quote/min_out
PM reserve
net EV after tip positive
provider auth present
bundle status path present
```

Preferred egress:

```text
jito_bundle / harmonic_bundle via Helius
```

Process:

```text
AF_XDP target capture
-> local target relationship + projected post-target state
-> backrun tx build
-> PM reserve
-> bundle [target, our_tx, tip]
-> Helius/Jito/Harmonic egress
-> bundle status polling
-> PM reconcile
```

Processed LaserStream may join after or alongside this flow for enrichment,
accounting, recurrence, and markout, but it must not be required before the
freshness-critical bundle eligibility decision.

Current status:

```text
LaserStream Backrun no-send = proven.
Bundle artifact = proven.
PM reserve/release = proven.
Helius Jito auth = green.
Backrun structural certificate = 21_afbrh_backrun_structural_hunt_readiness.sh
Backrun status = BACKRUN_STRUCTURAL_HUNT_READY.
AF_XDP-only target entity / projection / quote / PM / bundle shape = structurally green.
Processed LaserStream hybrid freshness = too late for freshness-critical ordered Backrun.
AFBR1H immediate bundle-ready batch = SOURCE_LATE_DOMINANT, 10/10 valid samples SOURCE_LATE.
Live blocked by immediate target freshness / source timing, not provider auth, PM, target entity, projection, or bundle envelope shape.
```

Latest AFBR1H no-submit distribution:

```text
script = 21_afbr1h_immediate_bundle_ready_freshness_batch.sh
valid_afbr1h_sample_count = 10
freshness_class_counts = {"SOURCE_LATE": 10}
fresh_at_bundle_ready_count = 0
status_unknown_count = 0
probe_nonzero_count = 0
unsafe_live_activity_count = 0
target_seen_to_bundle_ready_us_p50 = 148
target_seen_to_bundle_ready_us_p90 = 16017
target_seen_to_bundle_ready_us_p99 = 30562
bundle_ready_to_status_start_us_p50 = 997
bundle_ready_to_status_start_us_p90 = 1377
bundle_ready_to_status_start_us_p99 = 1587
monitor_seen_to_status_start_us_p50 = 37
provider_sim_latency_us_p50 = 67213
sendBundle_called = false
wire_send = 0
live_landed = 0
```

AFBR1H decision:

```text
Backrun architecture is green.
.15 AF_XDP Backrun is source/fresh-target gated in this distribution.
Do not reopen target entity, projection, PM, provider auth, or bundle shape unless AFBR-H regresses.
Do not run AFBR2, LS12, or live proof from SOURCE_LATE_DOMINANT.
Next serious Backrun source work is earlier target availability: Helius preprocessed / Shred Delivery entitlement, or another earlier target-source topology.
```

Structural readiness rule:

```text
Backrun is structurally HUNT-ready only while AFBR-H can prove:
  target tx bytes available
  target freshness known and bundle-eligible
  target relationship populated
  projected post-target pool state computed locally
  expected_out/min_out/net EV after tip positive
  PM reserve/release green
  bundle [target, our tx, tip] built in no-submit mode
  provider auth/status/simulate path green
```

---

## Post-shock / Continuation / Rebalancing

Primary source:

```text
LaserStream processed
```

Fast variant source:

```text
AF_XDP if timing-sensitive
```

Required gates before live:

```text
entry rule proven
exit rule proven
horizon defined
max adverse excursion bounded
route usable
numeric quote/min_out
EV positive after fee/tip/slippage
PM reserve
inventory/account/template ready
```

Preferred egress:

```text
helius_sender_dual for single-tx entry/exit
jito/harmonic bundle if atomic/order protection needed
```

Process:

```text
LaserStream continuous tape
-> post-shock classifier / owner-mode arbiter
-> continuation/rebalancing rule
-> no-send proof
-> PM reserve
-> private egress
```

Current status (latest supervisor truth first; older trace below is retained for audit):

```text
66RG certificate reconciliation:
  final_verdict = REBALANCING_PARKED_CERTIFICATE_AMENDED
  66rb_status = REBALANCING_EXECUTION_PLUMBING_READY
  66rb_meaning = execution_plumbing_structural_ready
  rebalancing_rows_seen = 3997
  exact_positive_rows = 91
  quote_min_out_ready_rows = 324
  entry_rule_ready_rows = 3451
  exit_rule_ready_rows = 2916
  pm_ready_rows = 72
  account_ready_rows = 47
  inline_ata_possible_rows = 9
  template_ready_rows = 69
  sender_no_send_ready_rows = 12
  bundle_no_submit_ready_rows = 9
  roundtrip_positive_rows = 91
  structural_ready_rows = 21
  execution_plumbing_ready_rows = 21
  owner_mode_match_rebalancing_rows = 0
  pm_owner_lock_ready_rows = 0
  restoration_pressure_measured_rows = 0
  false_restoration_gate_input_ready_rows = 0
  proposal_eligible_rebalancing_rows = 0
  architecture_blocker_counts = {}
  send_transaction_called = false
  sendBundle_called = false
  wire_send = 0
  live_landed = 0

Interpretation:
  21 rows passed structural execution plumbing.
  0 rows passed Rebalancing owner-mode eligibility.
  This is no-send/no-submit evidence only.
  Next step is 66RG2 owner-selector instrumentation repair, not a proposal package.
```

Historical Run65/66 trace:

```text
Run65 corrected 5m epoch shows this is the dominant modeled surface.
Route access is not the current blocker in simulation:
  private_route_executable_count=3829
  blocked_private_route_count=0
Actual no-send scheduler:
  would_fire_count=44
  captured_ev_sol_equivalent=6.414265985
  missed_ev_sol_equivalent=438.053171931
  would_fire_housekeeping=31
  would_fire_rebalancing=12
  would_fire_drop=1
Current blockers:
  account_ata_missing=2956
  not_held_input=917
  quote_missing=33
Fresh follow-up:
  Run65C account/ATA policy-liquidity audit:
    precreate_action_allowed_now=0
    top AuRo... row = PARK_NO_PRECREATE
  Run65C held-input overlay audit:
    SIMULATE_ONLY=21
    simulate_only_expected_would_fire_delta=212
    simulate_only_strict_ev_sol_equivalent=0.001902811
  Run66 rebalancing rule proof:
    rebalancing_rows_seen=1660
    rebalancing_rule_candidates=1116
    rebalancing_positive_after_cost=1368
    original rebalancing_numeric_quote_min_out=0
  Run66Q quote/min_out preservation:
    rebalancing_numeric_quote_min_out=397
    quote_match_mode=slot_pool_mints_amount_unique
    exact_quote_request_id_match=not proven
    net_ev_after_fee_tip_positive=0 under matched raw fee/tip
  Run66Q2 exact quote identity / route-cost EV:
    rebalancing_numeric_quote_min_out=397
    unique_safe_quote_match_count=397
    net_ev_after_fee_tip_positive_count=397 under configured route-cost model
    quote_request_id_match_count=0
    exact_quote_match_count=0
    matched_rollout_net_ev_after_fee_tip_positive_count=0
  Run66 quote-overlay rerun:
    rebalancing_quote_ready=406
    rebalancing_numeric_quote_min_out=397
    rebalancing_would_fire=0
  Run66A account/ATA planner:
    precreate_allowed_group_count=1
    Q2-linked top_ata_review_decision=ONE_ATA_PLAN_REVIEW
    precreate_candidate_net_ev_after_fee_tip_positive_rows=2
    planner_only_no_create
  Run66C continuation proof:
    continuation_rows_seen=175
    continuation_rule_candidates=175
    would_fire_continuation=0
    blocker=inventory_not_ready
  Run66C-Q continuation quote/min_out overlay:
    continuation_numeric_quote_min_out=116
    continuation_min_out_positive=116
    continuation_net_ev_after_fee_tip_positive=116
    exact_quote_match_count=0
  Run66X exact quote identity bridge audit:
    script_added=66x_exact_quote_identity_bridge_audit.sh
    row_fields=quote_request_id/ingress_unix_ns/template_id/candidate_id/owner_mode_match
    outputs=run66x_exact_quote_identity_bridge_rows.jsonl + summary json
    q2_cq_now_emit=first_exact_identity_blocker_counts
    host_result_exact_quote_match_count=499
    host_result_ingress_identity_match_count=499
    host_result_quote_request_id_match_count=0
    host_result_matched_rollout_positive_count=99
  Run66Q2 bridge2 rebalancing:
    rebalancing_numeric_quote_min_out=409
    exact_quote_match_count=387
    matched_rollout_net_ev_after_fee_tip_positive_count=59
  Run66C-Q bridge2 continuation:
    continuation_numeric_quote_min_out=125
    exact_quote_match_count=112
    matched_rollout_net_ev_after_fee_tip_positive_count=40
  Run66R exact-positive readiness overlay:
    exact_positive_q2_rows=44
    baseline_would_fire=0
    synthetic_ata_overlay_would_fire=39
    combined_synthetic_readiness_would_fire=39
    baseline_blockers={"account_ata_missing":39,"entry_rule_missing":1,"exit_rule_missing":4}
    hydk_synthetic_ata_would_fire_delta=0
    hydk_review_result=SYNTHETIC_ATA_OVERLAY_FAIL_NOT_ATA_ONLY
  Run66R-ATA mint planner:
    script=66r_ata_only_convertible_planner.sh
    artifact=66r_ata_only_convertible_planner_20260524_5m_bridge2
    ata_only_convertible_rows=39
    ata_only_convertible_group_count=8
    precreate_review_group_count=0
    precreate_review_row_delta=0
    decision_counts={"REJECT_POLICY":8}
    top_mint=4D8qUHm334fxqeTauPvF8gQ7fYgrD4Mpmb1Wy6ftUSWR
    top_policy_verdict=metadata_only_token2022_requires_explicit_allow
    top_exit_quote_ok=false
    top_exit_dust=true
    no_ata_creation
  Run66T Token-2022 explicit-allow research:
    script=66t_token2022_explicit_allow_research.sh
    artifact=66t_token2022_explicit_allow_research_20260524_5m_bridge2
    mint_group_count=8
    explicit_allow_possible_count=7
    explicit_allow_candidate_count=0
    decision_counts={"REJECT_AUTHORITY_RISK":1,"REJECT_DUST_EXIT":3,"REJECT_RENT_NEGATIVE":4}
    metadata_only_token2022_authority_clean_groups=7
    hard_extension_risk_groups=0
    top_candidate=8FSVipViijpT4aE86e58PYRd73bQdZtyzng4FT2THSTT
    top_decision=REJECT_RENT_NEGATIVE
    top_full_band_exit_usdc=22.684588
    top_rent_adjusted_ev_lamports=-1816179
    no_explicit_allow
    no_ata_creation
  Run66U repeatability aggregate:
    scripts=66u_run_repeatability_chain.sh + 66u_ata_repeatability_aggregate.sh
    artifact=66u_ata_repeatability_aggregate_20260524_3x5m_retry3_final
    source_epoch_count=3
    source_row_count=15
    mint_group_count=5
    decision_counts={"REJECT_DUST_EXIT":1,"REJECT_RENT_NEGATIVE":4}
    one_ata_plan_review_count=0
    top_mint=8FSVipViijpT4aE86e58PYRd73bQdZtyzng4FT2THSTT
    top_symbol=DROPEE
    top_epoch_seen_count=3
    top_total_synthetic_ata_delta_across_epochs=19
    top_full_band_exit_min_usdc=15.924327
    top_full_band_exit_median_usdc=23.770404
    top_total_rent_adjusted_ev_lamports=-5336272
    top_decision_after_repeatability=REJECT_RENT_NEGATIVE
    no_ata_creation
    no_explicit_allow
    no_acquisition
  Run66H housekeeping audit:
    housekeeping_rows_seen=646
    housekeeping_would_fire=0
    blocker=account_ata_missing
  Run65B-SIM1:
    top_3_overlay_mints=PLAN_ONLY_SIGNED_SWAP_SIM_NOT_RUN
    simulation_ready_count=0
    acquisition_allowed=false
    live_send_allowed=false
  Run66C continuation readiness across 3 fresh samples:
    continuation_rows_seen=330
    continuation_numeric_quote_min_out=243
    continuation_net_ev_after_fee_tip_positive=242
    exact_quote_match_count=226
    matched_rollout_net_ev_after_fee_tip_positive_count=66
    exact_positive_continuation_rows=63
    baseline_would_fire=0
    synthetic_ata_overlay_would_fire=0
    synthetic_inventory_overlay_would_fire=58
    ata_only_convertible_exact_positive_rows=0
    inventory_only_convertible_exact_positive_rows=58
    interpretation=continuation is inventory-blocked, not ATA/rent blocked
  Run65B-SIM2 signed swap simulation:
    script=65b_sim2_signed_swap_simulation.sh
    artifact=65b_sim2_signed_swap_simulation_20260524_5m
    row_count=3
    swap_tx_built_count=3
    swap_tx_signed_for_simulation_count=3
    simulate_transaction_called_count=3
    signed_swap_simulation_passed_count=3
    simulation_ready_count=3
    send_transaction_called=false
    wire_send=0
    live_landed=0
    acquisition_allowed=false
  Run66CI continuation held-input planner:
    script=66ci_continuation_held_input_inventory_planner.sh
    artifact=66ci_continuation_held_input_inventory_planner_20260524_3x5m
    source_inventory_only_rows=58
    mint_group_count=12
    inventory_sim_review_count=0
    decision_counts={"REJECT_DUST_EXIT":2,"REJECT_LOW_EV":3,"REJECT_POLICY":7}
    wire_send=0
    live_landed=0
  Run65B-SIM3 lifecycle review:
    script=65b_sim3_inventory_lifecycle_review.sh
    artifact=65b_sim3_inventory_lifecycle_review_20260524_5m
    row_count=3
    lifecycle_review_count=3
    signed_swap_simulation_passed_count=3
    auto_flatten_available_count=3
    PM_reserve_sim_ok_count=0
    top_mint=vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump
    top_expected_unlocked_would_fire_delta=61
    top_max_inventory_at_risk_usdc=7.55475554
    top_roundtrip_loss_usdc=0.02612854
    top_net_after_inventory_carry_cost_usdc=0.12078146
    acquisition_allowed=false
    wire_send=0
    live_landed=0
  Run65B-SIM4 PM inventory reserve review:
    script=65b_sim4_pm_inventory_reserve_review.sh
    artifact=65b_sim4_pm_inventory_reserve_review_20260524_5m
    row_count=3
    pm_reserve_review_ready_count=3
    pm_reserve_input_ok_count=3
    pm_reserve_fee_ok_count=3
    pm_reserve_tip_ok_count=3
    pm_reserve_inventory_band_ok_count=3
    pm_release_on_no_send_ok_count=3
    pm_pending_after_zero_count=3
    reserve_blocker_counts={"none":3}
    release_blocker_counts={"none":3}
    vRse_required_inventory_raw=286475131
    vRse_pm_pending_after=0
    acquisition_allowed=false
    live_send_allowed=false
    wire_send=0
    live_landed=0
  Run65B-SIM5 vRse one-mint synthetic replay:
    script=65b_sim5_one_mint_conversion_replay.sh
    artifact=65b_sim5_one_mint_conversion_replay_20260524_5m
    target_mint=vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump
    status=run65b_sim5_one_mint_conversion_replay_blocked_no_vrse_continuation_rows
    baseline_would_fire=0
    synthetic_vRse_would_fire=61
    would_fire_delta=61
    continuation_would_fire_delta=0
    rebalancing_would_fire_delta=0
    directarb_would_fire_delta=0
    housekeeping_would_fire_delta=0
    drop_would_fire_delta=61
    exact_quote_match_count=0
    numeric_quote_min_out_count=0
    min_out_positive_count=0
    matched_rollout_net_ev_after_fee_tip_positive_count=0
    entry_quote_ok=true
    exit_quote_ok=true
    auto_flatten_available=true
    flatten_simulation_passed=false
    pm_reserve_ok=true
    pm_release_ok=true
    pm_pending_after=0
    decision=SYNTHETIC_REPLAY_BLOCKED
    first_replay_blocker=blocked_no_vrse_continuation_rows
    send_transaction_called=false
    sendBundle_called=false
    wire_send=0
    live_landed=0
  Run65B-SIM5 owner-gated top-3 reruns:
    script=65b_sim5_one_mint_conversion_replay.sh
    owner_mode_gate_added=true
    allowed_classifications=ACQUISITION_REVIEW_ELIGIBLE,DROP_ONLY_RESEARCH,WRONG_OWNER_MODE,NO_EXACT_QUOTE_PROOF,NO_MIN_OUT_PROOF,NO_MATCHED_ROLLOUT_EV
    vRse_artifact=65b_sim5_one_mint_conversion_replay_20260524_5m_vRse_ownergate
    vRse_decision=DROP_ONLY_RESEARCH
    vRse_continuation_would_fire_delta=0
    vRse_drop_would_fire_delta=61
    AeVz_artifact=65b_sim5_one_mint_conversion_replay_20260524_5m_AeVz
    AeVz_decision=DROP_ONLY_RESEARCH
    AeVz_continuation_would_fire_delta=0
    AeVz_drop_would_fire_delta=13
    2ACE_artifact=65b_sim5_one_mint_conversion_replay_20260524_5m_2ACE
    2ACE_decision=DROP_ONLY_RESEARCH
    2ACE_continuation_would_fire_delta=0
    2ACE_drop_would_fire_delta=8
    pm_reserve_ok=true_for_all_top3
    pm_release_ok=true_for_all_top3
    send_transaction_called=false
    sendBundle_called=false
    wire_send=0
    live_landed=0
  Run66V inline ATA lifecycle model:
    script=66v_inline_ata_lifecycle_model.sh
    artifact=66v_inline_ata_lifecycle_model_20260524_5m_bridge2
    source_exact_positive_synthetic_ata_rows=39
    row_count=39
    decision_counts={"EXPLICIT_ALLOW_REQUIRED_REVIEW_ONLY":27,"REJECT_INLINE_ATA_EV_NEGATIVE":11,"REJECT_POLICY_OR_CREATION_RISK":1}
    inline_ata_creatable_count=38
    inline_ata_template_ready_count=38
    pm_reserve_input_ok_count=39
    pm_reserve_rent_ok_count=39
    net_ev_with_ata_burned_positive_count=0
    net_ev_with_ata_recovered_positive_count=28
    inline_ata_ev_positive_count=27
    policy_actionable_now_count=0
    explicit_allow_required_count=38
    hard_policy_risk_count=1
    top_mint_out=AuRoVyXAcMueM1FP9owQYdWeQ8YCMXK8xjXoY9it7PdZ
    top_net_ev_with_ata_burned=-1505220
    top_net_ev_with_ata_recovered=534060
    top_policy_verdict=metadata_only_token2022_requires_explicit_allow
    top_policy_actionable_now=false
    wire_send=0
    live_landed=0
    ata_created=false
  Run66CI2 owner-aligned continuation instrument search:
    script=66ci2_owner_aligned_continuation_instrument_search.sh
    artifact=66ci2_owner_aligned_continuation_instrument_search_20260524_3x5m
    exact_positive_continuation_rows=63
    owner_aligned_inventory_only_rows=58
    instrument_group_count=12
    decision_counts={"REJECT_DUST_EXIT":2,"REJECT_LOW_EV":3,"REJECT_POLICY":7}
    continuation_instrument_candidate_count=0
    top_decision=REJECT_POLICY
    top_policy_verdict=unknown_token_program
    top_exit_quote_ok=false
    top_exit_dust=true
    acquisition_allowed=false
    wire_send=0
    live_landed=0
  Run66V policy-clean inline ATA scan:
    script=66v_policy_clean_inline_ata_scan.sh
    artifact=66v_policy_clean_inline_ata_scan_20260524_5m_bridge2
    source_row_count=39
    policy_clean_row_count=0
    policy_clean_inline_ata_candidate_count=0
    wire_send=0
    live_landed=0
    ata_created=false
  Run66CORE core-inventory-only post-shock scan:
    script=66core_core_inventory_postshock_scan.sh
    artifact=66core_core_inventory_postshock_scan_20260524_3x5m
    source_row_count=107
    scanned_row_count=107
    core_inventory_pair_count=1
    core_exact_positive_count=1
    core_inventory_would_fire=0
    core_exact_quote_match_count=1
    core_min_out_positive_count=1
    core_matched_rollout_ev_positive_count=1
    core_pm_reserve_ok_count=1
    core_owner_counts={"rebalancing":1}
    first_core_inventory_blocker_counts={"account_ata_missing":1}
    top_pair=WSOL->USDT
    top_pool=3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF
    top_venue=raydium_clmm
    wire_send=0
    live_landed=0
    sendBundle_called=false
    ata_created=false
  Run66EA existing-account-only post-shock scan:
    script=66ea_existing_account_postshock_scan.sh
    artifact=66ea_existing_account_postshock_scan_20260524_3x5m
    source_row_count=107
    exact_positive_postshock_rows=107
    existing_account_would_fire=0
    existing_account_exact_quote_match_count=107
    existing_account_min_out_positive_count=107
    existing_account_pm_reserve_ok_count=44
    existing_account_account_ready_count=63
    existing_account_inventory_ready_count=44
    owner_counts={"continuation":63,"rebalancing":44}
    first_existing_account_blocker_counts={"account_ata_missing":39,"entry_rule_missing":1,"exit_rule_missing":4,"inventory_not_ready":63}
    wire_send=0
    live_landed=0
    sendBundle_called=false
    ata_created=false
  Run66V2 policy-clean inline one-shot scan:
    script=66v2_policy_clean_inline_one_shot.sh
    artifact=66v2_policy_clean_inline_one_shot_20260524_5m_bridge2
    source_row_count=39
    policy_clean_row_count=0
    policy_clean_inline_one_shot_count=0
    wire_send=0
    live_landed=0
    sendBundle_called=false
    ata_created=false
  Run66CORE-USDT plan-only review:
    script=66core_usdt_plan_only_review.sh
    artifact=66core_usdt_plan_only_review_20260524_3x5m
    usdt_core_row_count=1
    usdt_ata_present=false
    token_program_id=TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    ata_rent_lamports=2039280
    core_ata_plan_review_count=0
    park_low_ev_or_one_off_count=1
    top_pool=3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF
    top_venue=raydium_clmm
    top_exact_quote_match=true
    top_min_out_raw=18785531
    top_matched_rollout_net_ev_after_fee_tip_raw=79889
    top_net_ev_after_route_and_ata_burned_raw=-1959391
    top_synthetic_usdt_ata_would_fire_delta=1
    top_repeat_observed_count=1
    top_pm_reserve_sim_ok=true
    top_decision=PARK_LOW_EV_OR_ONE_OFF
    wire_send=0
    live_landed=0
    ata_created=false
  Run77B wrapper repair:
    local_host_wrapper_state=fixed
    SHADOW_RUN77B_TPU_QUIC_WORKERS default=2
    required_quic_workers>=2
    blocked_quic_workers_zero=true
    blocked_quic_workers_too_low_for_1=true
    native_lease_armed=false
  Run77B native no-send acceptance:
    selector_path_restored_into_g1_results=true
    prehydrated_replay_tx_loaded=1
    replay_hydration_before_xdp=true
    replay_hydration_rpc_during_lease=false
    grpc_source_grade_temp_normalized_to_external_geyser=true
    tpu_quic_candidate_seen=1
    tpu_quic_would_enqueue=1
    tpu_quic_ring_enqueue_ok=1
    tpu_quic_sender_dequeue_count=1
    tpu_quic_no_send_accept_count=1
    tpu_wire_send_count=0
    live_landed=0
    cleanup_verified=true
    agave_health_after_cleanup=ok
    status=run52a_tpu_conversion_ready
    doctrine_note=proof_only_private_lanes_remain_primary_egress
Current conversion blockers:
  quote_request_id_bridge_technical_debt
  Token-2022 metadata-only explicit-allow is possible for repeated groups but not action-approved
  ATA rent amortization remains negative on 4 repeated groups
  exit dust remains on 1 repeated group
  repeatability is now proven for the surface, not for action safety
  exit_rule_missing=425
  entry_rule_missing=119
  inventory_not_ready=12
  quote_missing=8
  broad_continuation_inventory_policy_dust_low_ev
  PM synthetic inventory reserve/release is green for the top 3 65B mints, but only in no-send simulation
  vRse_one_mint_replay_is_drop_overlay_not_continuation
  65B_top3_owner_gated_replays_are_drop_only_research
  Run66V_inline_ata_recovered_rent_positive_but_policy_actionable_now_zero
  owner_aligned_continuation_instrument_candidate_count_zero
  policy_clean_inline_ata_candidate_count_zero
  core_inventory_pair_exists_but_account_ata_missing
  core_usdt_row_parks_low_ev_or_one_off
  existing_account_postshock_would_fire_zero
  policy_clean_inline_one_shot_zero
  Run66W_template_nosend_not_opened
  explicit_inventory_caps_ttl_flatten_policy_not_approved
  held_input_inventory_for_continuation_and_65B_remains acquisition-gated
Not live yet.
```

---

## Drop / Flatten

Drop:

```text
suppression / saved-loss only
no egress by default
```

Flatten:

```text
egress only if explicit inventory-exit action is selected
```

Current status:

```text
Drop is not a money egress lane.
Use for risk control and saved-loss analytics.
```

---

# 3. Egress rules

## Helius Sender

Use for:

```text
single-transaction DirectArb
single-transaction post-shock entry/exit
fast private send / staked + Jito path
```

Required:

```text
priority fee instruction
tip instruction
route-specific EV after fee/tip
PM reserve
status polling
```

Route name:

```text
helius_sender_dual
```

Not for:

```text
true target->our_tx ordered backrun bundle
```

---

## Jito / Harmonic bundle via Helius

Use for:

```text
Backrun
atomic multi-tx flow
ordered target -> our tx -> tip
bundle-protected execution
```

Required:

```text
target tx bytes if true backrun
bundle tx count <= 5
tip present
auth/API key present
status API reachable
PM reserve/reconcile
```

Route names:

```text
jito_bundle
harmonic_bundle
```

Current first passing provider:

```text
jito_via_helius
```

---

## TPU/QUIC public

Use for:

```text
mechanical proof
fallback
public route comparison
short no-send tests
```

Not preferred for economics if private lanes dominate.

---

# 4. Source promotion policy

## Live source permissions

```text
DirectArb live:
  AF_XDP only for now

Backrun live:
  AF_XDP target trigger
  or LaserStream preprocessed only if enabled/proven

Continuation/Rebalancing live:
  not yet
  LaserStream can become live only after no-send + rule proof

Drop:
  never live egress by default
```

## LaserStream current permissions

```text
Processed LaserStream:
  observe
  no-send
  enrichment
  backrun research
  post-shock research

Processed LaserStream live:
  disabled

Preprocessed LaserStream live:
  unavailable until Helius entitlement exists
```

---

# 5. PM / accounting rules

No candidate may be live unless:

```text
PM exact input reserve succeeds
fee balance available
tip balance available
priority fee accounted
output account exists
min_out present
net EV positive after route-specific costs
PM release/reconcile path proven
```

Sender no-send without PM reserve:

```text
research-only
not execution no-send
not live eligible
```

Bundle no-send without PM reserve:

```text
invalid
```

---

# 6. Current operating doctrine

```text
DirectArb:
  AF_XDP trigger
  private egress preferred
  live gate waits for fresh eligible candidate

Backrun:
  LaserStream processed proves discovery/no-send
  AF_XDP needed for fresh target bytes
  AF_XDP local projection is structurally green; source timing is the live blocker
  private bundle egress via Helius/Jito

Post-shock:
  LaserStream-first research
  build executable rule before live

Drop:
  suppression only

AF_XDP/local Agave:
  benchmark and hot trigger
  not long-window production on .15

LaserStream:
  continuous source
  Backrun/Post-shock research
  possible ShadowLite base after more proof

Egress:
  Helius Sender + Jito/Harmonic remain primary monetization routes
```

```
```
