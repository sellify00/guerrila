# Shadow Engine V6.4-Guerrila — Handoff Status Memo

**Date:** 2026-05-22

**Purpose:** Bring the next agent up to speed after G12 observe-only safety proof, SH8 AF_XDP private-grade proof, AF_XDP-vs-Triton latency comparison, R1-FIX-B no-send proof, R1 tiny-live private-lane diagnostics, the R1-JITO-JSON no-send host proof, and the 2026-05-06 operator pivot to explicit public tiny-live.

**Doctrine update 2026-05-08:** Shadow Engine is now framed as a **micro-impulse propagation engine**, not a rare-shock backrun bot. Immediate Backrun remains a primitive, but the active research frontier is always-on post-impulse scoring across Continuation, Housekeeping, Rebalancing, Scout/Press/Kill, Flatten, and Drop. The speed advantage is AF_XDP/DPDK plus O(1) worker latency observing propagation before slower systems classify it. This loosens trigger admission and observation volume only; it does not loosen live-send caps, provenance, source-grade truth, route legality, ShadowGuard, inventory, fee floors, or PM reconciliation.


**Optimization-stage update 2026-05-09:** The project is now in **surface-to-profit optimization**, not raw-speed proof. The speed layer is credible enough to stop re-proving it as the main activity. Next work optimizes conversion: better niche coverage, better pool selection, better route-specific heuristics, owned/private source access, bounded inventory tolerance, willingness to take small ugly trades, and fast execution on neglected venues. Direct route-gap arbitrage is a capability probe, not the whole strategy. If direct arb does not yield enough clean net-after-fee gaps, Backrun, post-impulse, Housekeeping, Rebalancing, and Continuation remain active owner modes. The program should run continuously: ingest, classify, score owner modes, select one owner, and send only when the target clears clean per-run D-class gates.

**Speed-monetization update 2026-05-09:** Speed is now treated as a first-class economic variable. The next research question is not whether Shadow is fast, but where that speed creates exclusive positive windows. Speed Monetization P0 must compare clean candidates at Shadow latency against artificial delays and external-gRPC first-seen time, then report `ours_only_positive`, `gRPC_still_positive`, `speed_exclusive_positive_count`, and `speed_saved_loss_count`. Raw RX/classify speed still does not authorize send; only clean per-run net-after-fee evidence can.

**Habitat-registry doctrine update 2026-05-25:** Shadow must make money. Proofs are useful only if they move the system toward profitable, safe execution. The assistant or operator must not be a yes-man: ideas that improve monetization probability, remove a real blocker, or reduce live risk should be tested; ideas that create proof theater, repeated blocker loops, dead inventory, or live risk without evidence should be rejected or parked. Every branch must have a monetization thesis, a concrete acceptance gate, a kill or park condition, a next action if it passes, and a clear reason to stop if it fails. Habitat discoveries are now durable state, not disposable notes: the runtime registry is `/etc/shadow-engine/shadow_habitat_registry.json`, the append-only audit registry is `/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_registry.jsonl`, and the latest generated report lives under `/home/ubuntu/Guerrila/g1-host-pack/results/80_habitat_registry_latest/`. Volatility is a habitat filter, not a trade signal; formal selection now uses two separate scores where habitat score chooses where to look and readiness score decides whether Shadow can convert. Live execution remains gated by exact quote/min_out, route-specific EV after costs, PM reserve/release, source-grade truth, account/inventory readiness, and reconciliation.

**Launch R2 live milestone 2026-06-01:** classify the first Launch hot R2 live sample as `LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED`. This is a valid live-path milestone, not a clean profitable Launch result and not a strategy-duration test. Run root: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_live_20260601T061754Z`. The live signer path was repaired (`ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX`), late-follow Sender acquisition landed on mint `CLcqcknXPdYyFFA3w8JHvNkywZouKQrhv4B48avg5kdr`, rescue flatten landed, final inventory returned to `0`, and the daily ledger open exposure returned to `0` with status `LAUNCH_RESCUE_FLATTENED`. The corrected classification is `technical_status=LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN`, `strategy_status=NOT_A_STRATEGY_DURATION_TEST`, `pnl_status=SHORT_VALIDATION_COST_DOMINATED`, and `primary_blocker_hit=PUMPSWAP_DIRECT_SELL_ROUTE_SHAPE`. Cost autopsy: `gross_price_pnl_lamports=+135527`, `buy_sender_tip_lamports=-200000`, `sell_sender_tip_lamports=-200000`, `pump_account_setup_lamports=-2039280`, `native_fixed_drag_lamports=-2449280`, and `total_realized_pnl_lamports=-2313753`. Next gate is `R3_MONITORED_POSITION_PRIMARY_FLATTEN`: acquisition lands, the position enters `POSITION_OPEN` / `MONITORING`, entry value, peak value, drawdown, velocity, liquidity, slippage, and recoverable-value fields are populated, an adverse-move trigger fires, primary flatten simulation passes, primary flatten lands, rescue is not used, final inventory is zero, and ledger open exposure is zero.

**Launch R2-HOLD implementation update 2026-06-01:** Launch Continuation is now framed as early-entry momentum with fast risk exit, not a sub-5-second scalp. New scripts are `g1-host-pack/scripts/22_launch11_position_monitor_exit_sentinel.sh`, `g1-host-pack/scripts/22_launch12_hold_markout_research.sh`, and `g1-host-pack/scripts/22_launch13_r2_hold_live_execute.sh`. First R2-HOLD defaults are `hard_stop_bps=1000`, `trailing_stop_bps=1500`, `panic_drop_10s_bps=800`, and `max_hold_seconds=86400`; do not move to 2-3 day holds until monitor restart/recovery and PM state recovery are proven.

**Launch0Q quote-state hydration no-wire milestone 2026-06-02:** record the realtime coverage repair as `LAUNCH0Q_QUOTE_STATE_HYDRATION_NO_WIRE_PASS`, not as live-ready. Root: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z`. The Launch funnel now has a real no-wire bridge from `trigger_b -> quote_state -> exit_liquidity_guard -> entry_quality -> entry_edge -> fragility_absorption`, clearing the old dominant blocker `ENTRY_QUALITY_MISSING_QUOTE_STATE`. Final no-wire metrics: `unique_launch_states=566`, `trigger_b_rows=343`, `quote_state_ready_rows=166`, `exit_liquidity_guard_ready_rows=166`, `entry_quality_evaluated_rows=343`, `entry_quality_ready_rows=7`, `entry_edge_ready_rows=6`, `fragility_absorption_ready_rows=165`, `wire_send=0`, and `live_landed=0`. Current primary blockers are now `QUOTE_ROUTE_UNSUPPORTED` and `entry_quality_warmup`; next gate is `LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE` for unsupported-route taxonomy and warmup calibration before any further R3 live batch.

**Launch0R route-support / warmup reconcile milestone 2026-06-02:** record the diagnostic pass as `LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE_PASS`, still no-wire and not live-ready. Same root: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z`. The run preserved `wire_send=0`, `live_landed=0`, and `acceptance_pass=true`, then decomposed the old generic unsupported-route bucket: `QUOTE_ROUTE_UNSUPPORTED=333` is entirely `PUMP_BONDING_CURVE`, split as `PUMP_BONDING_CURVE_BUY=212` and `PUMP_BONDING_CURVE_SELL=121`. Warmup blockers are also route-attributed: `warmup_blocked_rows=249`, `PUMP_BONDING_CURVE=230`, `PUMPSWAP_AMM=19`. Current primary blocker is now `PUMP_BONDING_CURVE_QUOTE_SUPPORT`; secondary blocker is `WARMUP_CALIBRATION`. Next no-wire gate is `LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE`; do not resume R3 live and do not finalize warmup calibration until Pump bonding-curve buy/sell quote hydration exists and entry quality cannot pass without exit quote support.

**Launch0S Pump bonding-curve quote support milestone 2026-06-02:** record the no-wire pass as `LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE_PASS`. Same root: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z`. Safety stayed closed with `wire_send=0`, `live_landed=0`, and `acceptance_pass=true`. Pump bonding-curve quote hydration now has `PUMP_BONDING_CURVE_BUY quote rows=53`, `PUMP_BONDING_CURVE_SELL quote rows=38`, `pump_bonding_curve_ready_rows=44`, and `quote_route_unsupported_pump_bonding_curve_after=0`. The full quote-state path now reports `quote_state_ready_rows=205`, `entry_quality_ready_rows=19`, `entry_edge_ready_rows=11`, and `fragility_absorption_ready_rows=202`. Remaining Pump bonding-curve blockers are route-specific: `BONDING_CURVE_COMPLETE_OR_MIGRATED=171`, `BONDING_CURVE_STATE_MISSING=65`, `BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR=46`, `BONDING_CURVE_RESERVES_MISSING=6`, and `BONDING_CURVE_PRICE_IMPACT_TOO_HIGH=1`. Live remains paused; next gate is `LAUNCH0T_WARMUP_CALIBRATION_REPLAY`.

**Launch0T warmup calibration replay milestone 2026-06-02:** record the no-wire pass as `LAUNCH0T_WARMUP_CALIBRATION_REPLAY_PASS`. Same root: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z`. Safety stayed closed with `wire_send=0`, `live_landed=0`, and `acceptance_pass=true`. Warmup replay reports `best_entry_bucket=10-30s`, `normal_warmup_min_age_ms=10000`, `entry_quality_ready_rows=32`, `entry_edge_ready_rows=18`, and `exit_liquidity_guard_ready_rows=205`. This is a HOLD warmup rule, not a sub-second scalp validation. Early override remains allowed only when quote state, exit liquidity, entry edge, nonzero exit quote, and price-impact limits are all green. Static markout fields were not present in this coverage root, so do not infer profit hold-duration from Launch0T alone. The top non-ready quote blocker is `BONDING_CURVE_COMPLETE_OR_MIGRATED=171`; next gate is `LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN`.

**Launch0M / Launch0M3 migration route milestone 2026-06-02:** record the fresh root `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T135525Z` as `LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN_PASS`, still no-wire and not live-ready. Launch0M2 proved the key distinction `completed curve != migrated route`: `CURVE_COMPLETE_NOT_MIGRATED=394`, `PARK_MIGRATION_POOL_NOT_FOUND=1`, and `wire_send=0` / `live_landed=0`. Launch0M3 then watched actual realtime migration activity and passed with `migrate_instruction_seen_count=182`, `pumpswap_pool_joined_count=8`, and `pumpswap_quote_state_ready_rows=8`. The patched Launch0M consumed those watcher-proven pools and produced `MIGRATION_PUMPSWAP_ROUTE_READY=5`, `PARK_PUMPSWAP_BASE_RESERVE_BELOW_FLOOR=3`, `pumpswap_quote_state_ready_rows=5`, and `pumpswap_exit_liquidity_guard_ready_rows=5`. Operational rule: active bonding-curve launches are branch A and can use the bonding-curve route; completed-not-migrated curves are branch B and must be parked; migrated PumpSwap launches are branch C and require a proven pool from migration event/instruction/PDA/account-scan before PumpSwap quoting or live eligibility.

**Launch0U active bonding-curve R3 candidate gate 2026-06-02:** `22_launch0u_active_bonding_curve_r3_candidate_gate.sh` is implemented and no-wire. On the fresh 600s root it reports `status=PARK_ACTIVE_CURVE_WARMUP_NOT_READY`, `active_bonding_curve_rows=1137`, `exit_liquidity_guard_ready_rows=107`, `entry_edge_ready_rows=2`, `active_curve_r3_ready_rows=0`, `wire_send=0`, and `live_landed=0`. This correctly refuses to resume live because the only active-curve edge-ready rows were outside the Launch0T `10-30s` warmup/early-override rules. Next R3 resume gate is a no-wire probe that produces either an active bonding-curve candidate inside those rules or a migration-proven PumpSwap candidate with entry quality, exit liquidity, and fragility all green.

**Launch0V / Launch0U2 candidate conversion gate 2026-06-02:** `22_launch0v_migrated_pumpswap_r3_candidate_gate.sh` and `22_launch0u2_active_curve_warmup_blocker_audit.sh` now split the two resume branches before any live send. Launch0V is no-wire and currently parked: `migrated_pumpswap_rows_seen=5`, `quote_state_ready_rows=5`, `exit_liquidity_guard_ready_rows=5`, `true_loss_floor_lamports=12410000`, PM/account/cash rows green, but `entry_quality_ready_rows=0`, `entry_edge_ready_rows=0`, and `r3_ready_rows=0`; first blocker is `entry_quality_last_trade_not_buy`. Launch0U2 confirms the active curve branch is not PM/cost blocked: `active_bonding_curve_rows=1137`, `quote_not_ready_rows=1030`, `entry_edge_weak_rows=105`, `warmup_age_not_ready_rows=2`, `pm_not_ready_rows=0`, `cost_floor_not_ready_rows=0`, and `active_curve_r3_ready_rows=0`. Continue no-wire until either branch emits R3-ready rows.

**Launch0U3 / Launch0V2 blocker refinement 2026-06-02:** the no-wire blocker split is now precise. Launch0U3 says active curve quote-not-ready is mostly state hydration: `quote_not_ready_rows=1030`, `top_quote_blocker=BONDING_CURVE_STATE_MISSING`, `BONDING_CURVE_STATE_MISSING=882`, `BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR=141`, `BONDING_CURVE_PRICE_IMPACT_TOO_HIGH=6`, and `BONDING_CURVE_RESERVES_MISSING=1`; route unsupported and mint/curve missing are both zero in this sample. Launch0V2 says migrated PumpSwap is quote-ready but lacks live momentum context: `route_ready_rows=5`, `momentum_ready_rows=0`, `momentum_state_missing_rows=5`, `toxic_sell_rows=0`, `wire_send=0`, and `live_landed=0`. Next work is active-curve bonding-curve account state hydration and migrated-route flow carry-forward, not live trading.

**Launch0U4 / Launch0V3 hydration repair 2026-06-02:** the follow-up no-wire repair implemented `22_launch0u4_active_curve_state_hydration_repair.sh` and `22_launch0v3_migrated_pumpswap_flow_state_hydrator.sh`. Launch0U4 tried canonical Pump bonding-curve PDA fallback and preserved existing non-state-missing classifications, but did not reduce the active-curve state leak: `BONDING_CURVE_STATE_MISSING` stayed `882 -> 882`, `quote_ready_rows` stayed `107`, and downstream active R3 remained `active_curve_r3_ready_rows=0`. Treat this as upstream active-curve account identity/source-decode failure, not quote math. Launch0V3 consumed `flow_rows_seen=424081` but matched `0` of the `5` route-ready migrated PumpSwap rows, leaving `momentum_state_missing_rows=5`; treat this as missing flow-source join for migration-discovered PumpSwap pools. Continue no-wire; no Batch008/live R3 until one branch emits R3-ready rows.

**Launch0U5 / Launch0V4 identity-join repair 2026-06-02:** the next no-wire repair implemented `22_launch0u5_active_curve_account_identity_autopsy.sh` and `22_launch0v4_migrated_pumpswap_flow_join_repair.sh`. Launch0U5 added transaction-aware Pump instruction account-set inference, which collapsed the stale fixed-index diagnosis: `PARK_PDA_DERIVE_MISMATCH` dropped from `877` to `96`. It still did not unlock active curve: `identity_ready_rows=107`, `BONDING_CURVE_STATE_MISSING` moved only `882 -> 881`, `quote_ready_rows` stayed `107`, and `active_curve_r3_ready_rows=0`; dominant active blocker is now `PARK_ACCOUNT_NOT_FOUND=785`. Launch0V4 makes the migrated branch honest: `route_ready_rows=5`, `flow_join_decision_counts={"PARK_MOMENTUM_STATE_MISSING": 5}`, `first_flow_join_blocker_counts={"momentum_not_buy_led": 5}`, and migrated R3 remains `r3_ready_rows=0`. Continue no-wire; no Batch008/live R3 until one branch emits R3-ready rows.

**Launch0U6 / Launch0U7 / Launch0V5 identity-momentum follow-up 2026-06-02:** record this no-wire diagnostic as `LAUNCH0U6_U7_V5_IDENTITY_MOMENTUM_NO_WIRE_PARKED`. Safety stayed closed with `wire_send=0` and `live_landed=0`. Launch0U6 split the active-curve account-state/source problem: `account_not_found_rows=785`, `ACCOUNT_FOUND_AFTER_IX_ACCOUNT_JOIN=106`, `ACCOUNT_NOT_FOUND_WRONG_PDA=97`, and stale/non-live state remains dominant (`ACCOUNT_NOT_FOUND_STALE_STATE=934` across the classified active set). Launch0U7 then proved the `107` identity-ready active rows are blocked by entry signal, not PM/cost/fragility/liquidity: `active_curve_r3_ready_rows=0`, `ENTRY_EDGE_WEAK=105`, `WARMUP_NOT_READY=2`, and all execution/risk blockers were zero (`EXIT_LIQUIDITY_BLOCKED=0`, `COST_FLOOR_BLOCKED=0`, `PM_BLOCKED=0`, `FRAGILITY_BLOCKED=0`). Launch0V5 watched migrated PumpSwap for 900s on `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_900s_20260602T165046Z`; route/liquidity remained available (`route_ready_rows=5`), but buy-led momentum never appeared (`momentum_ready_rows=0`, `migrated_pumpswap_r3_ready_rows=0`, `flow_join_decision_counts={"PARK_MOMENTUM_STATE_MISSING": 5}`, status `PARK_MIGRATED_PUMPSWAP_MOMENTUM_NOT_BUY_LED`). Live remains paused. Next active-curve work is fresh-state filtering/enhanced-create join plus entry-edge calibration on real current rows; migrated PumpSwap remains watchable, not sendable, until momentum flips buy-led.

**Strict DirectArb structural-readiness update 2026-05-26:** the strict DirectArb lane has now crossed the architecture valley and should be treated accordingly. After fixing the pre-Tier0 signer-inventory export, preserving all signer token accounts, and teaching Tier0 to use a funded non-canonical WSOL account consistently, the live-track scope is now structurally green: `Run77C private egress = green`, `Run78 scout wiring = green`, inventory refresh before Tier0 is green, WSOL/account lineage is closed, and `Run78H` can now certify `DIRECTARB_STRUCTURAL_HUNT_READY`. A fresh three-window post-certificate batch then produced `57` AF_XDP DirectArb rows across `3` valid windows but `0` fresh selected candidates, with `26` EV-negative rows and `13` `unsupported_non_wsol_input` rows. Current interpretation: the engine is ready to convert a valid bounded DirectArb row, but the sampled windows did not contain a fresh EV-positive supported candidate. This is now a market/habitat problem, not a reopened architecture problem. Broad discovery may still surface ATA/account/policy noise, but those rows must be classified as out-of-scope unless the readiness filter admits them. The next DirectArb work should therefore use bounded Pool Book mode, not generic broad `Run78` sampling. New scripts for that lane are `g1-host-pack/scripts/81_directarb_pool_book_selector.sh` and `g1-host-pack/scripts/82_focused_directarb_poolbook_run78.sh`; `Run78B` remains closed until a fresh selected candidate appears.

**Lane-readiness / 19-day proving update 2026-05-26:** the next window through **2026-06-14** is now a lane-by-lane structural-readiness program, not random attempts to go live. Current map: DirectArb is `DIRECTARB_STRUCTURAL_HUNT_READY` and market/habitat gated; Backrun is `BACKRUN_STRUCTURAL_HUNT_READY` after AFBR1 structural repair, AFBR-H certification, and AFBR1H immediate freshness probing, but is now fresh-target/source-timing gated; post-shock/rebalancing/continuation have real exact-positive no-send signal but are not architecture-ready until a structural instrument-readiness certificate proves a usable account/inventory/policy/template path; Housekeeping is under-tested and must split real tiny cleanup trades from saved-loss / Drop-only / artifact rows before promotion. AFBR1F/AFBR1G remain historical evidence that the old saved-sample status measurement was late. AFBR1H supersedes that measurement path by probing from a hot bundle-ready row and now has a 10-sample no-send batch showing `SOURCE_LATE_DOMINANT`. Add `66psh_postshock_structural_readiness.sh` and `66h2_housekeeping_structural_readiness.sh` before treating post-shock or Housekeeping as architecture-ready. No `LS12`, no `AFBR2`, no live `sendBundle`, no acquisition, no ATA creation, and no Token-2022 explicit allow until the relevant structural certificate and proposal gate pass.

**AFBR1H batch correction 2026-05-26:** the AFBR1G-only read above is superseded for Backrun freshness measurement. `21_afbr1h_immediate_bundle_ready_freshness_batch.sh` drives native AF_XDP zero-copy no-send probes from tiny `/dev/shm` hot bundle-ready rows before heavy artifact construction. The completed batch has `status=SOURCE_LATE_DOMINANT`, `valid_afbr1h_sample_count=10`, `freshness_class_counts={"SOURCE_LATE":10}`, `fresh_at_bundle_ready_count=0`, `invalid_preflight_count=0`, `invalid_post_cleanup_count=0`, `probe_nonzero_count=0`, and `unsafe_live_activity_count=0`. Distribution: `target_seen_to_bundle_ready_us_p50=148`, p90 `16017`, p99 `30562`; `bundle_ready_to_status_start_us_p50=997`, p90 `1377`, p99 `1587`; `monitor_seen_to_status_start_us_p50=37`; `provider_sim_latency_us_p50=67213`; `sendBundle_called=false`, `wire_send=0`, `live_landed=0`. Treat Backrun as structurally ready but `.15` AF_XDP source/fresh-target gated. The next serious Backrun source work is earlier target availability, such as Helius preprocessed / Shred Delivery entitlement or another earlier target-source topology. This does not authorize `AFBR2`, `LS12`, live send, or a one-bundle live proof.

**Supervisor transition / Rebalancing certificate amendment 2026-05-27:** Rebalancing is now parked at execution-plumbing readiness, not proposal readiness. The fresh native `.15` Run65 plus normalized `66r*` chain proved `66RB` execution plumbing at `/home/ubuntu/Guerrila/g1-host-pack/results/66rb_rebalancing_structural_readiness_fresh_20260527_final/summary.txt`: `rebalancing_rows_seen=3997`, `exact_positive_rows=91`, `quote_min_out_ready_rows=324`, `entry_rule_ready_rows=3451`, `exit_rule_ready_rows=2916`, `pm_ready_rows=72`, `account_ready_rows=47`, `inline_ata_possible_rows=9`, `template_ready_rows=69`, `sender_no_send_ready_rows=12`, `bundle_no_submit_ready_rows=9`, `roundtrip_positive_rows=91`, `structural_ready_rows=21`, and `architecture_blocker_counts={}`. `66RG` then reconciled the certificate at `/home/ubuntu/Guerrila/g1-host-pack/results/66rg_rebalancing_certificate_owner_reconciliation_20260527/final_verdict.json`: `final_verdict=REBALANCING_PARKED_CERTIFICATE_AMENDED`, `66rb_meaning=execution_plumbing_structural_ready`, `execution_plumbing_ready_rows=21`, `owner_mode_match_rebalancing_rows=0`, `pm_owner_lock_ready_rows=0`, `restoration_pressure_measured_rows=0`, `false_restoration_gate_input_ready_rows=0`, and `proposal_eligible_rebalancing_rows=0`. Safety counters stayed closed: `send_transaction_called=false`, `sendBundle_called=false`, `wire_send=0`, `live_landed=0`. Correct wording: `21` rows passed structural execution plumbing, but `0` rows passed Rebalancing owner-mode eligibility. Next supervisor task is `66RG2` owner-selector instrumentation repair, while DirectArb stays sentinel-only and Backrun stays source-timing blocked.

**Run65 corrected full-Shadow epoch update 2026-05-24:** the current fresh 5-minute `.15` conversion map is `65_full_shadow_hybrid_private_epoch_20260524_5m_valuation`, built from a fresh 300s hybrid AF_XDP/LaserStream capture and a valuation-only rerun with private route simulation enabled. It supersedes any stale "private route blocked" reading: `observe_candidates=5096`, `continuous_candidates_seen=3953`, `candidate_deduped_count=3953`, `private_route_candidate_count=3829`, `private_route_executable_count=3829`, `jito_bundle_executable_count=3829`, `blocked_private_route_count=0`, and `blocked_route_unavailable_count=0`. Actual-inventory no-send scheduling still only fired `44` rows (`housekeeping=31`, `rebalancing=12`, `drop=1`, `directarb=0`, `backrun=0`) and captured `6.414265985` SOL-equivalent, while the model reports `438.053171931` SOL-equivalent missed EV. Treat that missed number as a modeled post-shock/rebalancing markout reservoir, not realized PnL. The active blocker stack is now readiness, not egress: `account_ata_missing=2956`, `not_held_input=917`, `quote_missing=33`, `no_free_shard=3`. The new offline planner `g1-host-pack/scripts/65ab_epoch_unlock_planner.sh` writes `Run65A` and `Run65B` artifacts at `/home/ubuntu/Guerrila/g1-host-pack/results/65ab_epoch_unlock_planner_20260524_5m/`; first pass found `run65a_precreate_allowed_group_count=0` under conservative policy/liquidity gates and `run65b_overlay_allowed_group_count=23` / `run65b_overlay_expected_would_fire_delta=229` for exit-qualified held-input overlay candidates. Current supervisor action: preserve this Run65 as the reference, do not live-send, run `Run65A/Run65B` unlock analysis and `Run66_rebalancing_rule_proof` before any live discussion.

**Run65C / Run66 conversion proof update 2026-05-24:** the follow-up analysis is now fresh on `.15` and remains no-send/no-acquire/no-ATA. `g1-host-pack/scripts/65c_policy_liquidity_unlock_audit.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/65c_policy_liquidity_unlock_audit_20260524_5m/` and keeps `Run65A` closed for action: `run65a_row_count=24`, `run65a_decision_counts={"REJECT_DUST_EXIT":10,"REJECT_POLICY":12,"RETRY_LATER":2}`, `run65a_precreate_action_allowed_now_count=0`. The top `AuRoVyXAcMueM1FP9owQYdWeQ8YCMXK8xjXoY9it7PdZ` ATA row remains `PARK_NO_PRECREATE` because it is Token-2022 explicit-allow blocked and its unit exit value is dust (`0.000019 USDC`). `Run65B` is cleaner but still not execute-approved: `run65b_row_count=130`, `SIMULATE_ONLY=21`, `candidate_for_one_mint_simulate_count=21`, `simulate_only_expected_would_fire_delta=212`, `simulate_only_strict_ev_sol_equivalent=0.001902811`; top mint `AeVzGq6z1RndB9Tk9dGT6RPADjZJpQwX6tudqSRkmAgp` needs about `56.716678299 USDC`, exits at about `56.664323 USDC`, and is only a one-mint simulation candidate. `g1-host-pack/scripts/66_rebalancing_rule_proof.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66_rebalancing_rule_proof_20260524_5m/` and converts the 438 SOL-equivalent reservoir into an explicit rule-proof autopsy: `rebalancing_rows_seen=1660`, `rebalancing_rule_candidates=1116`, `rebalancing_entry_rule_count=1541`, `rebalancing_exit_rule_count=1213`, `rebalancing_pm_reserve_sim_ok=1640`, `rebalancing_account_ready=31`, `rebalancing_inventory_ready=1640`, `rebalancing_quote_ready=12`, `rebalancing_numeric_quote_min_out=0`, `rebalancing_positive_after_cost=1368`, and `rebalancing_would_fire=0`. Current blocker counts are `account_ata_missing=1096`, `exit_rule_missing=425`, `entry_rule_missing=119`, `inventory_not_ready=12`, and `quote_missing=8`. Supervisor interpretation: the post-shock/rebalancing reservoir is real as a modeled surface, but not live/profit proof; the next engineering repair is account/ATA readiness plus numeric quote/min_out preservation for rebalancing, while 65B overlay mints may only move to bounded one-mint simulate after route-stability and signed-swap simulation pass.

**Run66Q / 66A / 66C / 66H follow-up (`2026-05-24`):** P0 quote/min_out preservation is no longer the primary rebalancing blocker. `66q_rebalancing_quote_minout_preservation.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66q_rebalancing_quote_minout_preservation_20260524_5m/` and rehydrated `397` rebalancing rows with unique-safe quote/min_out evidence: `rebalancing_quote_ready=397`, `rebalancing_numeric_quote_min_out=397`, `rebalancing_min_out_positive=397`, `quote_match_mode_counts={"candidate_id":774,"none":475,"slot_pool_mints_amount_ambiguous":14,"slot_pool_mints_amount_unique":397}`, and `wire_send=0` / `live_landed=0`. These are unique-safe fallback matches, not exact `quote_request_id` matches; also `numeric_unique_safe_net_ev_after_fee_tip_positive=0` under the matched raw fee/tip model, so this is visibility proof, not executable profit proof. The patched `66_rebalancing_rule_proof.sh` overlay rerun at `/home/ubuntu/Guerrila/g1-host-pack/results/66_rebalancing_rule_proof_20260524_5m_quote_overlay/` now shows `rebalancing_quote_overlay_applied=397`, `rebalancing_quote_ready=406`, `rebalancing_numeric_quote_min_out=397`, but `rebalancing_would_fire=0` because `account_ata_missing=1096` remains dominant. `66a_rebalancing_account_ata_unlock_planner.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66a_rebalancing_account_ata_unlock_planner_20260524_5m/` and found exactly one full-band-liquid planner candidate (`HyDKNdnhZNVYQMruBevbNsUWruA9STmAQrS4srXApump`, `PRECREATE_ALLOWED`, `expected_would_fire_delta=2`, full-band exit about `76.85171 USDC`), but no ATA was created and this remains planner-only. `66c_continuation_rule_proof.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66c_continuation_rule_proof_20260524_5m/`: continuation has real rule shape (`continuation_rows_seen=175`, `continuation_rule_candidates=175`, `positive_after_cost=175`) but `would_fire_continuation=0` because all rows are `inventory_not_ready` and `numeric_quote_min_out=0`. `66h_housekeeping_executable_audit.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66h_housekeeping_executable_audit_20260524_5m/`: housekeeping has `646` rows but `housekeeping_would_fire=0`, `account_ready=0`, and main blocker `account_ata_missing`. `65b_one_mint_simulation_candidates.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/65b_one_mint_simulation_candidates_20260524_5m/` with `21` `SIMULATE_ONE_MINT` candidates, still `execute_mode_allowed=false` and `live_send_allowed=false`. Current next blocker is therefore account/inventory readiness and exact quote identity, not provider route availability.

**Run66Q2 / 66A-Q2 / 66C-Q / 65B-SIM1 update (`2026-05-24`):** the latest no-send repair pass sharpened, but did not remove, the identity caveat. `66q2_rebalancing_exact_quote_route_ev.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66q2_rebalancing_exact_quote_route_ev_20260524_5m/` and shows `rebalancing_numeric_quote_min_out=397`, `unique_safe_quote_match_count=397`, `slot_pool_mints_amount_unique_match_count=397`, and `net_ev_after_fee_tip_positive_count=397` under the configured private-vs-TPU route-cost model, with `wire_send=0` / `live_landed=0`. However, exact identity is still not solved: `quote_request_id_preserved_count=2`, `quote_request_id_match_count=0`, `exact_quote_match_count=0`, and `matched_rollout_net_ev_after_fee_tip_positive_count=0`. Treat this as route-cost-positive unique-safe evidence, not exact `quote_request_id` executable proof. The Q2-linked ATA planner rerun at `/home/ubuntu/Guerrila/g1-host-pack/results/66a_rebalancing_account_ata_unlock_planner_20260524_5m_q2/` keeps exactly one planner-only candidate, `HyDKNdnhZNVYQMruBevbNsUWruA9STmAQrS4srXApump`, now classified `top_ata_review_decision=ONE_ATA_PLAN_REVIEW` with `precreate_candidate_quote_positive_rows=1`, `precreate_candidate_net_ev_after_fee_tip_positive_rows=2`, and `precreate_candidate_expected_would_fire_delta_after_quote_overlay=1`; no ATA was created. `66cq_continuation_quote_minout_preservation.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/66cq_continuation_quote_minout_preservation_20260524_5m/` and upgrades continuation from quote-red to unique-safe quote-positive: `continuation_rows_seen=175`, `continuation_numeric_quote_min_out=116`, `continuation_min_out_positive=116`, `continuation_net_ev_after_fee_tip_positive=116`, and `matched_rollout_net_ev_after_fee_tip_positive_count=31`, while exact identity remains red (`quote_request_id_preserved_count=0`, `exact_quote_match_count=0`). `65b_sim1_top_mint_simulation_plan.sh` wrote `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim1_top_mint_simulation_plan_20260524_5m/` for the top three held-input overlay mints; all three are `PLAN_ONLY_SIGNED_SWAP_SIM_NOT_RUN`, with `simulation_ready_count=0`, `execute_mode_allowed=false`, `acquisition_allowed=false`, `live_send_allowed=false`, `wire_send=0`, and `live_landed=0`. Current supervisor truth: rebalancing and continuation now have unique-safe numeric/min_out and configured route-cost-positive rows, but exact quote identity, account/ATA readiness, inventory readiness, and signed swap simulation remain the gates. No live send, no acquisition, no ATA creation.

**Run66X exact quote identity bridge implementation (`2026-05-24`):** the next repair is now scripted locally rather than left as a verbal caveat. New script `g1-host-pack/scripts/66x_exact_quote_identity_bridge_audit.sh` emits row-level bridge evidence for `rebalancing` and `continuation` candidates, including `candidate_quote_request_id`, `candidate_template_id`, `candidate_ingress_unix_ns`, candidate/raw pool-mint-amount identity, `quote_request_id_match`, `ingress_identity_match`, `candidate_id_exact_match`, `exact_quote_match`, `unique_safe_fallback_match`, `owner_mode_match`, and `first_exact_identity_blocker`. `66q2_rebalancing_exact_quote_route_ev.sh` and `66cq_continuation_quote_minout_preservation.sh` now also report `owner_mode_match_on_numeric_count`, `unique_safe_owner_mismatch_count`, and `first_exact_identity_blocker_counts` so unique-safe rows cannot be accidentally promoted to exact proof. Run this on `.15` against the fresh Run65/66 artifacts before any readiness promotion; local syntax checks pass, but the current local workspace does not contain the `.15` Run65 result files, so no new artifact counters are claimed here. Expected command shape: run `66x_exact_quote_identity_bridge_audit.sh`, then rerun `66q2_rebalancing_exact_quote_route_ev.sh`, `66cq_continuation_quote_minout_preservation.sh`, and the Q2-linked `66a_rebalancing_account_ata_unlock_planner.sh`. Acceptance remains strict: `quote_request_id_match_count > 0`, `exact_quote_match_count > 0`, and `matched_rollout_net_ev_after_fee_tip_positive_count > 0` for the relevant owner mode. Until that appears, `HyDK...` stays `PLAN_REVIEW_ONLY`; no live send, no ATA creation, and no acquisition.

**Run66X / Q2 bridge2 / 66R host evidence rerun (`2026-05-24`):** the `.15` no-send host rerun is now complete and materially upgrades the post-shock branch. `Run66X` at `/home/ubuntu/Guerrila/g1-host-pack/results/66x_exact_quote_identity_bridge_audit_20260524_5m/` proves exact identity via ingress identity, not quote-request ID: `exact_quote_match_count=499`, `ingress_identity_match_count=499`, `quote_request_id_match_count=0`, and `matched_rollout_net_ev_after_fee_tip_positive_count=99`, with `wire_send=0` / `live_landed=0`. Rebalancing Q2 bridge2 at `/home/ubuntu/Guerrila/g1-host-pack/results/66q2_rebalancing_exact_quote_route_ev_20260524_5m_bridge2/` now shows promotion-grade no-send quote identity: `rebalancing_numeric_quote_min_out=409`, `exact_quote_match_count=387`, `ingress_identity_match_count=387`, `matched_rollout_net_ev_after_fee_tip_positive_count=59`, and still `quote_request_id_match_count=0`. Continuation bridge2 at `/home/ubuntu/Guerrila/g1-host-pack/results/66cq_continuation_quote_minout_preservation_20260524_5m_bridge2/` is also active now: `continuation_numeric_quote_min_out=125`, `continuation_net_ev_after_fee_tip_positive=125`, `exact_quote_match_count=112`, and `matched_rollout_net_ev_after_fee_tip_positive_count=40`. The Q2-linked ATA planner keeps `HyDKNdnhZNVYQMruBevbNsUWruA9STmAQrS4srXApump` planner-only with `precreate_candidate_quote_positive_rows=1`, `precreate_candidate_net_ev_after_fee_tip_positive_rows=2`, and `precreate_candidate_expected_would_fire_delta_after_quote_overlay=1`; no ATA was created. New `g1-host-pack/scripts/66r_rebalancing_exact_positive_readiness_overlay.sh` then split the exact-positive rebalancing rows by synthetic readiness at `/home/ubuntu/Guerrila/g1-host-pack/results/66r_rebalancing_exact_positive_readiness_overlay_20260524_5m_bridge2/`: `exact_positive_q2_rows=44`, `baseline_would_fire=0`, `synthetic_ata_overlay_would_fire=39`, `combined_synthetic_readiness_would_fire=39`, blocker split `baseline={"account_ata_missing":39,"entry_rule_missing":1,"exit_rule_missing":4}`. Important correction: HyDK itself is **not** ATA-only convertible yet; `hydk_exact_quote_positive_rows=1`, `hydk_matched_rollout_positive_rows=1`, but `hydk_synthetic_ata_would_fire_delta=0` and `hydk_review_result=SYNTHETIC_ATA_OVERLAY_FAIL_NOT_ATA_ONLY` because the exact HyDK row is `entry_rule_missing`. Current supervisor truth: post-shock/rebalancing and continuation now have exact-identity, min_out-safe, route-cost-positive no-send evidence, but live remains closed; next conversion target is the 39 ATA-only synthetic rebalancing rows, not automatic HyDK creation.

**Run66R-ATA mint-level planner (`2026-05-24`):** the new no-action planner `g1-host-pack/scripts/66r_ata_only_convertible_planner.sh` was deployed and run on `.15` against `66r_rebalancing_exact_positive_readiness_overlay_20260524_5m_bridge2`. Artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66r_ata_only_convertible_planner_20260524_5m_bridge2/`. It confirms the 66R shape (`source_exact_positive_overlay_rows=44`, `ata_only_convertible_rows=39`) and groups the 39 synthetic ATA-only rows into `8` missing output mints, but produces **zero** precreate-review groups: `precreate_review_group_count=0`, `precreate_review_row_delta=0`, `decision_counts={"REJECT_POLICY":8}`, `wire_send=0`, `live_landed=0`. Top group is `4D8qUHm334fxqeTauPvF8gQ7fYgrD4Mpmb1Wy6ftUSWR`, `row_count=11`, `policy_verdict=metadata_only_token2022_requires_explicit_allow`, `exit_quote_ok=false`, `exit_dust=true`, and `total_net_ev_after_ata_cost_lamports=-2003407`. Several other groups have full-band exit quotes around `19-22 USDC`, but remain Token-2022 explicit-allow blocked and still negative after ATA rent under this 5m evidence. `AuRo...` remains rejected (`metadata_only_token2022_requires_explicit_allow`, dust full-band exit), and HyDK remains parked because it was not ATA-only convertible. Current supervisor truth: **do not create any ATA from 66R-ATA**. The next work is policy/repeatability/rent-aware research on these 8 mint groups, or a fresh longer epoch to prove recurrence and non-dust economics under an explicit Token-2022 allow policy before any one-ATA plan review.

**Run66T Token-2022 explicit-allow research (`2026-05-24`):** `g1-host-pack/scripts/66t_token2022_explicit_allow_research.sh` was deployed and run on `.15` against the 66R-ATA mint groups. Artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66t_token2022_explicit_allow_research_20260524_5m_bridge2/`. It separates policy possibility from actionability: `mint_group_count=8`, `explicit_allow_possible_count=7`, `explicit_allow_candidate_count=0`, `decision_counts={"REJECT_AUTHORITY_RISK":1,"REJECT_DUST_EXIT":3,"REJECT_RENT_NEGATIVE":4}`, `wire_send=0`, `live_landed=0`. The seven Token-2022 groups are metadata-only (`MetadataPointer` + `TokenMetadata`) with no `TransferHook`, `PermanentDelegate`, `NonTransferable`, or `DefaultAccountState`, and no mint/freeze authority, so they are **explicit-allow research candidates in principle**. They are still not action candidates: 3 fail dust exit, 4 are rent-negative in this 5m window, and all have only `fresh_epoch_seen_count=1`. Top economic group after safety parsing is `8FSVipViijpT4aE86e58PYRd73bQdZtyzng4FT2THSTT` (`DROPEE`): `explicit_allow_possible=true`, full-band exit `22.684588 USDC`, but `rent_adjusted_ev=-1816179` lamports and decision `REJECT_RENT_NEGATIVE`. The SPL-token `Es9v...` group is rejected for authority risk (`mint_authority_present`, `freeze_authority_present`). Current rule: no ATA creation; next proof must be longer/repeated no-send epochs showing rent-adjusted recurrence plus non-dust exit before any explicit-allow or one-ATA plan review.

**Epoch6 approval / Run73 / Run73B / Run74 conversion update 2026-05-21:** the `6jUs` branch is now closed correctly as a plumbing proof rather than the business path. Epoch6 proved `approval propagation = PASS` (`approved_held_input_mints` and `held_input_mints` both included `6jUs...`), but fresh recurrence stayed absent (`6jUs` fresh rows = `0`), so `6jUs` is no longer the next blocker. The important lever moved to repeated WSOL-entry Meteora DirectArb. `Run73` isolated seven repeated metadata-only `Token-2022` output mints and showed WSOL was already effectively sufficient (`current_wsol_balance_sol=0.999241658`, `required_wsol_band_bounded_sol=1.0`, `bounded_top_up_recommendation_sol=0.000758342`). After the narrow metadata-only output-ATA rule was approved, `Run73B` executed exactly the bounded prep that was intended: all seven output ATAs now exist, WSOL was topped from `0.999241658` to `1.000260001`, and signer inventory was refreshed. The fresh retry epoch then materially changed the surface: `deployable_by_us_opportunity_count=226`, `would_fire_total=219`, `would_fire_directarb=136`, `would_fire_continuation=20`, `would_fire_rebalancing=22`, `would_fire_housekeeping=41`. The strict forensic delta is the key result: `strict_current_state_would_fire` moved `7 -> 159`, `strict_current_state_ev_usd` moved `0.24503 -> 290.169953`, and `directarb_strict_would_fire` moved `1 -> 133`, with `route_leak_count=0` and `jito_bundle_executable_count=0`. On the exact seven-target WSOL-entry Meteora scope, the branch converted cleanly: `missing_output_ata_rows 14 -> 0`, `input_balance_not_ready_rows 14 -> 0`, `strict_rows 0 -> 126`, `strict_ev_usd_delta=+3870.193832`. `Run74` then closed the next question: the strict DirectArb lane is now real, deduped, and TPU-clean. Fresh strict DirectArb came back as `133` unique rows with `strict_directarb_deduped_net_ev_usd=287.47512` and `strict_directarb_deduped_retained_ev_usd=4101.998934`; all `133` rows are `afxdp`, all routes are `tpu_quic_public`, venue split is `meteora_damm_v2=127` and `raydium_clmm=6`, and every row is `quote_ready`, `inventory_ready`, `approved_held_input`, `output_account_ready`, `template_ready`, `tpu_or_public_usable`, and `would_enqueue_tpu`. The remaining strict-lane caveat is now also explicit: `heuristic_ev_only_count=133`, `quote_numeric_preserved_count=0`, and `min_out_numeric_preserved_count=0`. Current interpretation: the narrow Run73B ATA + WSOL conversion branch worked and DirectArb is now a real strict no-send lane. Do not reopen `6jUs`, do not rerun ATA creation, and do not broaden weird-token acquisition. The immediate next gate before any controlled live proof is preserving or revalidating raw quote/min-out bounds so the strict lane is not only executable-now but also numerically quote-backed. After that, the broader expansion blocker remains the `route_private_only` / non-public execution mass outside the converted WSOL-entry Meteora DirectArb lane.

**Run75 ingress-identity bridge update 2026-05-21:** the `quote_numeric_preserved_count=0` / `min_out_numeric_preserved_count=0` result is now diagnosed as a join-identity failure, not as proof that Tier0 never emitted numeric quote data. Remote artifact inspection on `.15` proved that raw `shock_research.jsonl` already contains populated `quote_expected_out_lamports`, `quote_target_out_lamports`, `min_out_lamports`, and related numeric fields on the later **final no-send rollout events**, while the strict-row forensic join was still keyed to the original ingress shell identity. Because the final no-send event changes `signature` and `unix_ns`, the old exact `candidate_id` join lands on zero-filled shell rows even when the populated quote tuple exists later in the tape. The fix now landed locally is an explicit ingress bridge on the rollout event/export path: `template_id`, `quote_request_id`, and `ingress_unix_ns` are now preserved through `rollout_shadow`, emitted by the final no-send path, exported into `shock_research.jsonl`, and consumed by the patched `Run39` / `Run75` shell scripts so they can prefer the final quote-bearing row for the same ingress candidate. Important constraint: historical artifacts do **not** contain the new bridge fields, so this fix is **not** retroactive. The next authoritative proof must be a fresh rerun after deploy; acceptance is `quote_numeric_preserved_count > 0` and `min_out_numeric_preserved_count > 0` on the regenerated strict DirectArb artifacts.

**Run75 historical rehydration update 2026-05-21:** the existing `.15` `epoch7b` artifact set has now been reprocessed with the patched `Run75` script, and the result is materially positive. `Run75` now reports `strict_directarb_rows=133`, `quote_numeric_preserved_count=88`, `min_out_numeric_preserved_count=88`, `expected_out_numeric_preserved_count=88`, `quote_target_out_numeric_preserved_count=88`, `policy_tip_raw_preserved_count=88`, and `base_fee_raw_preserved_count=133`, while preserving the clean execution shape (`all_selected_candidates_tpu_quic_public=true`, `all_selected_candidates_afxdp=true`, `all_selected_candidates_ready=true`). The safe historical fallback that unlocked those rows is narrower than the future ingress bridge: for old artifacts only, `Run75` now accepts `slot + pool + mint_in + mint_out` **only when that yields exactly one quote-bearing raw rollout row**. That produces `quote_numeric_unique_slot_pool_mint_count=88`. The remaining `35` rows stay intentionally non-numeric because the old artifact set still has `quote_numeric_ambiguous_slot_pool_mint_count=35`; they are not exact enough to claim accountable quote/min-out preservation. Historical caveats remain explicit too: `quote_request_id_preserved_count=0` and `template_id_preserved_count=0` on the old artifact set, so the ingress-identity bridge rerun is still required for full fresh-proof quality. Current interpretation: Run75 is now good enough to select a first strict DirectArb candidate class from the `88` unique numeric rows, while the future fresh rerun remains the path to eliminate ambiguity and restore exact ingress-linked quote identity on all rows.

**Run76 / Run77 update 2026-05-21:** the next strict-lane step is now concretely scoped. `Run76` selected exactly one numeric-safe DirectArb class from the `88` unambiguous rows: pool `GqyUGfUXjQWtks8C4xf9yBgbwT3WvdPat5iisFVc23NZ`, `WSOL -> BkK43JimdWEHnkBB7miFTUXre4h23n3qseQaqBp3AZAF`, `18` recurring rows, `40.141052` deduped EV, selected signature `4mwxGETtpSLEmFi52gcYZv9NEQbunMeDAY4udhVSAp9NPaz4qM8cPE4JoXvLWbAcFd9w3jDM4vf8usDw7CV2zkyL`, `selected_expected_out_raw=4672508530`, `selected_min_out_raw=4625783444`, `selected_net_ev_usd=2.522484`, `selected_route=tpu_quic_public`, `selected_source=afxdp`. The first `Run77` host attempt then failed on a pure infrastructure gap: `shadow-tier0` aborted because `/sys/fs/bpf/shadow_xsks_map` was missing. That gap is now fixed in the host-pack wrapper: `g1-host-pack/scripts/77_selected_numeric_safe_directarb_tpu_nosend.sh` now pre-arms bpffs + XDP + pinned XSKMAP on `eno1` before delegating to `52a -> Run40`, and the relevant host shell files on `.15` were normalized to LF so the wrapper actually runs. The patched rerun proves the old blocker is closed: XDP loads on `eno1`, `/sys/fs/bpf/shadow_xsks_map` is pinned, `shadow-tier0` and `shadow-egress-tpu` both start, and Run40 logs `PASS  scorecard replay injected` for the exact selected signature on pool `GqyUGf...`. The current blocker has therefore moved back to the deeper replay path: host artifacts under `/dev/results/77_selected_numeric_safe_directarb_tpu_nosend_fixxskmap_run40/` show the selected replay spec and the replay injection, but `tpu_summary.txt` is still empty and no final `tpu_quic_candidate_seen / would_enqueue / ring_enqueue / sender_dequeue / no_send_accept` proof has been produced yet. Current supervisor reading: Run77 is no longer blocked at AF_XDP/XSKMAP startup; it is now blocked at post-injection worker-to-TPU conversion / finalization.

**Epoch8 fresh ingress-identity rerun update 2026-05-21:** the first fresh rerun after deploying the ingress bridge is now complete on `.15`, and it materially sharpens the state of the blocker. Fresh `Run61` came back as `surface_rows=9268`, `deployable_by_us_opportunity_count=111`, `would_fire_total=106`, `would_enqueue_tpu_count=106`, `owner_mode_directarb_count=222`, `would_fire_directarb=74`, with `jito_bundle_executable_count=0`. Fresh `Run62` confirmed the same strict DirectArb lane shape at the conversion layer: `directarb_total_rows=222`, `directarb_unique_opportunity_count=211`, `directarb_quote_valid_count=85`, `directarb_route_usable_count=85`, `directarb_inventory_ready_count=211`, `directarb_would_fire_count=74`, `directarb_main_blocker=route_private_only`, and only `10` fresh rows remained `not_held_input`. Fresh `Run66` / `Run69` then closed the strict-current-state side: `strict_current_state_would_fire=83`, `strict_current_state_ev_usd=154.017528`, `directarb_strict_rows=74`, `directarb_deduped_ev_usd=152.916773`, `route_leak_count=0`, `jito_bundle_executable_count=0`. Fresh `Run75` is the key new identity result: `strict_directarb_rows=74`, `quote_numeric_preserved_count=44`, `min_out_numeric_preserved_count=44`, `quote_numeric_unique_slot_pool_mint_count=44`, `quote_numeric_ambiguous_slot_pool_mint_count=25`, `template_id_preserved_count=44`, but `quote_request_id_preserved_count=0`, and `all_selected_candidates_min_out_positive=false`. Current interpretation: the fresh rerun proves the blocker is no longer “all quote numerics disappear on fresh artifacts.” Numeric quote/min-out preservation is now real on `44` fresh strict rows, and `template_id` survives there too. But the exact ingress-identity acceptance gate is still not closed, because `quote_request_id` does not survive and `25` fresh rows remain ambiguous. Supervisor call: treat Epoch8 as a **partial fresh success** that meaningfully narrows the blocker, not as a final exact-identity win.

**Epoch8 fresh Run76 / Run77 correction 2026-05-21:** the freshest selector/replay path is no longer the old historical `BkK4...` class. A fresh `Run76` against the `44` numeric-safe Epoch8 rows now supersedes it and selects pool `E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4`, `WSOL -> 456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o`, `8` recurring rows, `14.481405` deduped EV, `selected_expected_out_raw=438884922`, `selected_target_out_raw=7297936851`, `selected_min_out_raw=434496072`, `selected_min_out_positive=true`, and `selected_quote_match_mode=unique_slot_pool_mint_amount_template`, with `excluded_ambiguous_count=25`, `excluded_min_out_nonpositive_count=0`, and `excluded_nonunique_composite_count=0`. The fresh `Run77` replay then did hit the correct selected candidate: host `Run40` logged `PASS  scorecard replay injected` for signature `2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4` on pool `E9dp...`. But this replay must be classified honestly: the host wrapper armed `eno1` in **`xdpgeneric`**, not native AF_XDP zero-copy, so this is **non-canonical infrastructure replay evidence**, not the AF_XDP zero-copy no-send proof we actually want. The blocker also remains unresolved after injection: `tpu_summary.txt` stayed empty, no final `tpu_quic_candidate_seen / would_enqueue / ring_enqueue / sender_dequeue / no_send_accept` proof appeared, and the live config still resolved `runtime route set: {drop, jito_bundle, harmonic_bundle}`, `public execution route: unreachable`, and `quic_workers=0`. Current reading: fresh Run76 is valid and should stand; fresh Run77 is still blocked at post-injection worker-to-TPU conversion and must not be counted as canonical AF_XDP zero-copy proof.

**.15 Agave handback recovery update 2026-05-21 late:** after the non-canonical Run77 attempt, `.15` has been cleanly returned to Agave ownership. XDP is detached from `eno1`, `/sys/fs/bpf/shadow_xsks_map` is absent, `eno1` is restored to `Combined: 16`, Shadow runtime processes are stopped, and `sh1-agave-no-geyser-2.service` is active. Local RPC is listening on `127.0.0.1:8899`, `getHealth` is `ok`, UDP listeners on `8000-8025` are present, and the sampled processed slot advanced through `421279977`. The recovery proof is that Agave caught up from slot lag `1851 -> 1673 -> 1540 -> 1239 -> 707 -> 246 -> ok` without increasing the sampled NIC error counters. Treat `.15` as clean Agave-owned and ready for the next bounded native AF_XDP lease after the usual OOB/preflight checks.

**Native Run77 retry update 2026-05-22:** the previous `xdpgeneric` mistake has now been corrected in the Run77 path. The host-pack wrapper forces native AF_XDP zero-copy posture, checks `afxdp_bind_mode=zero_copy`, proves local Agave green before attach, and lets Run40 trust that pre-attach proof instead of repeating the local/reference RPC freshness check after `eno1` is under Shadow's bounded lease. Host retry `epoch8_auto_strict_directarb_quote_identity_run77_native_zero_copy_preattach_retry2` crossed the old blocker: native XDP attached, XSKMAP was pinned/registered, AF_XDP queue 0 bound, and oracle/TPU egress started. It still did **not** produce Run77 acceptance. `shadow-tier0` failed before replay injection with `scorecard replay getTransaction failed` for signature `2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4`, because it attempted to fetch the selected transaction from the config Helius RPC during the native lease and that HTTPS request failed. After manual handback, `.15` returned cleanly to Agave-owned state: XDP detached, XSKMAP absent, `Combined: 16`, no Run77 runtime PIDs, UDP listeners present, local RPC `getHealth=ok`, sampled slot `421339855`. Current blocker is therefore no longer `xdpgeneric` or post-attach preflight; it is replay transaction hydration. Next meaningful fix: fetch/cache the selected transaction before native attach, or feed Run40/tier0 an already-hydrated replay artifact so no Helius `getTransaction` is needed while native XDP owns `eno1`.

**Run77B prehydrated replay build update 2026-05-22:** the replay hydration fix is now implemented and deployed on `.15`. New wrapper `g1-host-pack/scripts/77b_prehydrated_selected_numeric_safe_directarb_tpu_nosend.sh` performs the selected-signature `getTransaction` before touching `eno1`, verifies the fresh Run76 class (`E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4`, `WSOL -> 456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o`, signature `2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4`), writes the hydrated artifact at `/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_run77_prehydrated/selected_replay_transaction.json`, and only then proceeds to the native AF_XDP lease. `SHADOW_RUN77B_PREHYDRATE_ONLY=true` has now passed on `.15`: `selection_loaded=1`, `prehydrated_replay_tx_loaded=1`, `replay_hydration_before_xdp=true`, `replay_hydration_rpc_during_lease=false`, hydrated slot `421267673`, artifact size `14729` bytes, no XDP attached, no XSKMAP pinned, local RPC `getHealth=ok`, UDP listener count `30`, sampled slot `421347394`. `shadow-tier0` now accepts `SHADOW_RUN77_REPLAY_TX_PATH` / `SHADOW_RUN40_SCORECARD_REPLAY_TX_PATH` and, when `SHADOW_RUN77B_REQUIRE_PREHYDRATED_REPLAY=true`, refuses to fall back to RPC. It validates signature, slot, pool, mints, and `selected_route=tpu_quic_public` before decoding the local base64 transaction. `Run40` and `52A` summaries now carry `replay_hydration_mode=prehydrated_artifact`, `replay_hydration_before_xdp=true`, `replay_hydration_rpc_during_lease`, `replay_tx_artifact_exists`, and `prehydrated_replay_tx_loaded`. Run77B now also writes a temporary proof config with `quic_workers=2` by default (`SHADOW_RUN77B_TPU_QUIC_WORKERS`), requires `quic_workers>=2`, and fails as `blocked_quic_workers_zero` if the override is zero; this is explicitly scoped by `SHADOW_RUN77B_TPU_QUIC_PROOF=true` / prehydrated no-send envs and does not alter the global production config. The new wrapper also scans the native-lease Tier0 log and fails if `getTransaction`, `Helius`, or selected-signature RPC fetch appears after attach. This is a real host-side hydration proof, not yet full Run77B no-send acceptance.

**Run77B native no-send acceptance update 2026-05-25:** the missing `g1-host-pack/results` selector path was restored from the fresh epoch8 `Run76` selector, `Run77B` prehydrate-only was rerun successfully under the expected results root, and the bounded native AF_XDP zero-copy no-send lease is now green on `.15`. The only real mid-run blocker was a proof-config parse failure, not transport: the temporary `run40.config.yaml` inherited `system.proxies.ingress.grpc_source_grade=laserstream`, which the currently running parser on `.15` rejected. `g1-host-pack/scripts/77b_prehydrated_selected_numeric_safe_directarb_tpu_nosend.sh` now normalizes `grpc_source_grade` to `external_geyser` in the **temporary Run77B proof config only** while still forcing `quic_workers=2`; no permanent Guerrila config was changed. Final artifact chain: `/home/ubuntu/Guerrila/g1-host-pack/results/epoch8_auto_strict_directarb_quote_identity_run77_prehydrated/` and inner `/home/ubuntu/Guerrila/g1-host-pack/results/epoch8_auto_strict_directarb_quote_identity_run77_prehydrated_run40/`. Final counters: `selection_loaded=1`, `selected_signature=2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4`, `selected_pool=E9dp...`, `selected_route=tpu_quic_public`, `replay_hydration_mode=prehydrated_artifact`, `replay_hydration_before_xdp=true`, `replay_hydration_rpc_during_lease=false`, `prehydrated_replay_tx_loaded=1`, `route_resolved_tpu_quic_public=1`, `tpu_quic_candidate_seen=1`, `tpu_quic_would_enqueue=1`, `tpu_quic_ring_enqueue_ok=1`, `tpu_quic_sender_dequeue_count=1`, `tpu_quic_no_send_accept_count=1`, `tpu_wire_send_count=0`, `live_landed=0`, `inner_status=tpu_quic_no_send_ready`, and `acceptance_pass=true`. Cleanup also recovered fully after a transient DNS failure on the first Agave restart attempt: `cleanup_verified=true`, `eno1_xdp_lines=0`, `xsk_map_after_cleanup=absent`, `shadow_runtime_process_count_after_cleanup=0`, `udp_8000_8025_count=30`, and `agave_health_after_cleanup=ok`. Important doctrine note: this is **not** a return to public-TPU-first egress. Current egress doctrine remains **private lanes**; `Run77B` uses `tpu_quic_public` only as a narrowly scoped replay/funnel proof artifact. Current meaning: `Run77B strict DirectArb replay` has crossed from hydration-proof scaffolding into real no-send transport proof, while live/production egress policy stays private-lane-first.

**Run77-family AF_XDP doctrine 2026-05-22:** the next canonical Run77 / Run77B lease must use native XDP with AF_XDP zero-copy. `xdpgeneric` / copy mode is now explicitly disallowed for this proof family; it should be classified as a refused posture, not as a partial replay proof. The risk that native attach can briefly disrupt SSH on routed `eno1` is accepted and handled through OOB coverage plus the bounded Agave-first handback procedure. Do not rerun Run77-family proof in generic XDP mode.

**Epoch3 recurrence update 2026-05-20:** the fresh post-Run68B repeatability check is now complete and it kept the DirectArb lane alive, but thinner. The authoritative fresh Epoch3 chain had to be completed manually from the valid AF_XDP native zero-copy `Run39` tape because the outer wrapper still hung after the bounded window, but the recurrence numbers themselves are real: `strict_current_state_would_fire=20`, `strict_current_state_ev_usd=11.270719`, `directarb_strict_would_fire=8`, `directarb_deduped_ev_usd=10.574753`, `route_leak_count=0`, and `jito_bundle_executable_count=0`. Current interpretation: Run68B's ATA conversion gain was real and recurring, but the next dominant blocker stayed `not_held_input`, not route leakage or private-bundle leakage.

**Run70 DirectArb held-input recurrence update 2026-05-20:** the first disciplined held-input planner is now complete on the fresh Epoch2 + Epoch3 pair. `Run70` narrowed `97` total DirectArb missing-input mints down to `7` that recur across both fresh epochs, and only `3` remained acquisition-allowed under the current conservative policy/liquidity rules. The best first target is now explicit: `6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M`, a Tokenkeg mint with no extensions, no freeze or mint authority, `3 + 3` recurring rows across Epoch2/Epoch3, `6` unique opportunities, `9.906505` recurring DirectArb EV, a single dominant pool `HgDk49W8PhrLFjcp2jgJ3eNEFfgYRoXjPrrrpyfKVSbE`, and a single dominant direction `6Gzs... -> WSOL`. Current interpretation: after the Coinini dust lesson, Shadow now has a real recurring/liquid shortlist rather than a metadata-only wish list.

**Run70B one-mint simulation update 2026-05-20:** the first one-mint DirectArb acquisition simulation is now also complete on `.15` for `6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M`, in `simulate` mode only, with `max_spend_usdc=1.0`, `max_slippage_bps=100`, `live_send=false`, and `execute_result=null`. The good news is that the economics and mechanics stayed clean on the Helius-backed rerun: the simulator found a fresh quote for `1015.649384` tokens (`expected_output_raw=1015649384`) at only `0.42043 USDC`, with `min_output_native=1005.492891`, fresh exit quote `0.420496 USDC`, exit ratio `1.079944936`, ATA-create simulation success, and a signed swap transaction that was fully constructed. The red gates are now precise: the fresh route drifted away from the planned `GoonFi V2 -> Meteora DLMM -> Raydium CLMM` path and instead came back as `TesseraV -> Meteora DLMM -> Raydium CLMM`, and the signed swap simulation failed at Jupiter with `custom program error: 0x1771`. Current interpretation: `6Gzs...` remains the correct first held-input target, but it is still **not execute-approved**. The next execute gate is route-stable fresh simulation plus swap-simulation success, not just attractive entry/exit quotes.

**Run64D metadata-held-input update 2026-05-20:** the bounded metadata-only Token-2022 acquisition path is now host-proven as a real executor, but it also exposed the new economic-liquidity gate Shadow must enforce. Run64D successfully plan-ran, simulate-ran, and executed one approved mint on `.15`: `8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx` (`Coinini`), creating ATA `5HD87yy4ByMUxrVpC6BSbim96PXYB32YxMHer3ZTsDGp`, finalizing swap `wNywHHJp9mcBQAX9FSEZAv5H67q6Pw1wnsLEzKBMs6HRVZRMLTDsPhwqFM8qqTZRcYX5wPMABJm761dxoYktidG`, and acquiring `6506.555997` units while the signer USDC balance moved only from `230.088909` to `230.088908`. That means the bounded executor itself is real. But the immediate executable exit quote for the full acquired balance is only `0.000001 USDC`, so `Coinini` is now classified as **metadata-policy-safe but economically dead / dust**, not useful held-input inventory. Run64D has therefore been tightened with an exit-liquidity gate: future acquisitions must have both entry and exit quotes, and will now fail with `blocker=exit_quote_dust` unless exit value is at least `0.10 USDC`, at least `50%` of acquisition cost, and not just `1` raw USDC unit of dust. Do **not** acquire the second metadata mint until the first acquisition’s conversion delta is measured.

**.15 Agave-first recovery update 2026-05-20:** the host-side packet-ownership rule was re-proven the hard way. The post-acquisition rerun initially failed because `shadow-tier0`, `shadow-oracle`, and native XDP were still attached to `eno1`, leaving local Agave red even though the validator service remained active. After stopping the Shadow datapath processes, detaching XDP from `eno1`, and restarting `sh1-agave-no-geyser-2.service`, local Agave recovered back to green: `local_health=ok`, `processed_slot_lag=1`, `finalized_slot_lag=1`, `udp_listener_count=30`, and `eno1` returned `LOWER_UP` with no XDP attached. One repo/operator footgun also became explicit: on hosts without a populated `.env`/override, preflight can still fail closed on `reference_rpc_unconfigured` even when local Agave is healthy, so serious host-pack runs should currently export `SHADOW_BOOTSTRAP_RPC_URL=https://api.mainnet-beta.solana.com` and `SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL=https://api.mainnet-beta.solana.com` until the environment defaults are cleaned up.

**Epoch1 canonical monetization update 2026-05-20:** the project now has a fresh canonical bounded monetization window under the new naming convention, `Epoch1 = epoch1_canonical_monetization`. This was a real `.15` `eno1` AF_XDP native zero-copy run with Agave-first recovery before attach, a bounded `900s` observe window, no live send, all supported venues, all owner modes, TPU/QUIC as the executable route family, and private bundle routes blocked as executable. The fresh tape came back strong at the observe layer: `sample_slots=2550`, `observe_candidates=27498`, `continuous_candidates_seen=11711`, `continuous_candidates_seen_afxdp=694`, `would_fire_total=273`, `would_enqueue_tpu_count=273`, `would_public_rpc_send_count=0`, `would_wire_send_count=0`, `jito_bundle_executable_count=0`, `blocked_private_bundle_only=11017`. But the headline money number now needs the Run66 forensic framing: the `427.434763 USD-equiv` surface is the **fixup-allowed no-send surface**, not strict-current-state executable PnL. That relaxed surface still splits as `housekeeping=388.754217`, `directarb=32.659576`, `rebalancing=5.837205`, `continuation=0.183765`. Venue split on the same fresh tape is `meteora_damm_v2 observed=7357 / would_fire=242`, `raydium_clmm observed=4039 / would_fire=25`, `raydium_cpmm observed=240 / would_fire=6`, `raydium_v4 observed=75 / would_fire=0`. Current interpretation: Shadow now has fresh canonical Epoch1 evidence, but the apparent `$427` opportunity set is a **fixup-conditioned heuristic monetization surface** dominated by post-shock / housekeeping rather than already deployable strict execution.

**Epoch1 conversion-autopsy update 2026-05-20:** the fresh strict autopsy is now also complete on the same Epoch1 tape. `Run62` returned `directarb_total_rows=56`, `directarb_unique_opportunity_count=55`, `directarb_quote_valid_count=56`, `directarb_route_usable_count=56`, `directarb_inventory_ready_count=13`, `directarb_ata_ready_count=43`, `directarb_template_ready_count=56`, `directarb_unit_normalized_positive_ev_count=13`, but `directarb_would_fire_count=0` with `directarb_main_blocker=not_held_input`. Fresh route scarcity is still mostly private-route-only post-shock flow, not missing candidate detection: `route_unavailable_total_rows=11017`, all of them `route_private_only`, led by `rebalancing` on `meteora_damm_v2`. The other material fresh blocker is still output-account readiness on post-shock rows: `rebalancing_account_ata_missing_count=250`. Continuation remains abundant but only lightly executable on current rules: `continuation_total_rows=979`, `continuation_route_usable_count=108`, `continuation_deployable_count=1`. Run66 sharpened the interpretation further: strict-current-state execution on the full relaxed would-fire set is only `1` row / `$0.183765`, while `272` rows require ATA/account fixup. Current interpretation: the next real blockers remain `not_held_input` for DirectArb, recurring output-account / ATA surface across the relaxed would-fire set, and the private-route-only mass that still does not convert into TPU/public executable flow.

**Epoch1 held-input shortlist update 2026-05-20:** the fresh exit-qualified held-input pass did not produce a clean immediate expansion set. The same Epoch1 tape returned `baseline_directarb_inventory_ready_count=13`, `baseline_directarb_would_fire_count=0`, and the latest clean rerun of `Run64E` ended with `eligible_candidate_count=0`. The highest-frequency missing-input mint from the fresh directarb autopsy is `5vcZEaTs8ssbW5WXV5MD16dsVoJU7x5teveRX6WSooY3`, but it is currently blocked by policy; the next recurring mints were blocked by missing entry quotes. Current interpretation: do **not** broaden held-input inventory from Epoch1 yet. The evidence still supports the monetization doctrine in `shadow_monetization_strategy_UPDATED.md`: liquid DirectArb remains desirable, but the current fresh strict surface is tiny, the relaxed surface is overwhelmingly ATA-fixup dependent, and held-input acquisition must remain exit-qualified and bounded.

**Epoch1 artifact / handoff update 2026-05-20:** the bounded window itself succeeded and host cleanup completed: `eno1` is back detached from XDP and `sh1-agave-no-geyser-2.service` is active again. The authoritative fresh Epoch1 artifacts are now:
- `/home/ubuntu/shadow-host-audit/dpdk-runs/epoch1_canonical_monetizRun70B is a **correct no-execute result**.

The candidate is economically interesting, but the transaction path is not stable enough to execute.

## What passed

For:

```text
6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M
```

The fresh economics were still good:

```text
0.42043 USDC -> 1015.649384 tokens
min_output = 1005.492891
exit quote = 0.420496 USDC
exit_value_ratio = 1.079944936
```

So this is not a Coinini-style dust problem.

ATA creation simulation also passed.

## What failed

Two hard gates failed:

```text
entry_route_matches_planned = false
swap_simulation_passes = false
```

Planned route:

```text
GoonFi V2 -> Meteora DLMM -> Raydium CLMM
```

Fresh route:

```text
TesseraV -> Meteora DLMM -> Raydium CLMM
```

Swap simulation failed with:

```text
Jupiter custom program error: 0x1771
```

So the right answer is:

```text
Do not execute.
```

## Supervisor interpretation

This is not a bad result. It means the **liquidity/economics screen is working**, but the **route stability / transaction simulation gate protected us**.

The candidate remains potentially useful, but it cannot be acquired until one of these happens:

```text
1. same planned route returns and simulates cleanly
2. new route is explicitly accepted and simulates cleanly
3. we decide TesseraV route is allowed after separate route-policy review
```

Right now, route drift plus simulation failure means no execution.

## Next instruction

Run70B should be retried in simulate mode later, not executed.

Use this decision tree:

```text
If fresh route = planned route and swap simulation passes:
  approve one-mint execute under <=1 USDC cap.

If fresh route differs but simulation passes:
  pause; require route-policy review before execute.

If swap simulation fails:
  no execute.

If exit quote becomes dust:
  reject candidate.

If route keeps drifting/failing across repeated attempts:
  demote 6Gzs... and move to next A-tier mint.
```

## Current next step

Do **one of two things**:

```text
A. Retry Run70B simulate for 6Gzs... after route conditions change.

or

B. Run70B simulate on the next A-tier mint.
```

Do not keep forcing `6Gzs...` if it repeatedly route-drifts or sim-fails.

## Project-level meaning

This is actually the correct behavior of Shadow’s prep pipeline:

```text
recurring DirectArb candidate found
entry/exit economics pass
ATA sim passes
route drift detected
swap sim fails
execution blocked
```

That is the kind of fail-closed behavior we need before any real capital goes in.
ation_run55_run39/summary.txt`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/epoch1_canonical_monetization/run61_continuous_shadow_live_simulation.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/epoch1_canonical_monetization_run62/summary.txt`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/epoch1_canonical_monetization_run55_run39/candidate_autopsy.jsonl`

One tooling caveat is now explicit for the next agent: the fresh AF_XDP window and the fresh Run61/Run62 data are valid, but the `Run65` post-process wrapper / resume chain is still brittle and can fail after the fresh tape is already safely captured. Treat that as a wrapper-quality issue, not as invalidation of the Epoch1 canonical data.

**Run66 Epoch1 forensic-audit update 2026-05-20:** the first profitable-looking Epoch1 surface has now been audited directly rather than reused as headline alpha. `Run66` reconstructed the exact `273` would-fire rows from the fresh Epoch1 artifacts and matched the fresh owner split exactly: `housekeeping=203`, `directarb=13`, `rebalancing=56`, `continuation=1`. The key answer is that the `$427.434763` surface is **almost entirely fixup-dependent, not strict-current-state executable**: `strict_current_state_would_fire=1` for only `$0.183765`, while `ata_fixup_would_fire=272` for `$427.250998`; `template_fixup_would_fire=0`. There was no dedupe collapse (`unique_would_fire_events=273`, `duplicate_rows=0`, `duplicate_ev_removed=0`) and no executable-route leakage (`route_leak_count=0`; all rows still stayed on `tpu_quic_public` or the allowed public-scout route family). The Housekeeping lane is now explicitly classified as **fixup-only, not currently real-trade executable**: `housekeeping_real_trade_count=0`, `housekeeping_real_trade_ev=0`, `housekeeping_fixup_only_count=203`, `housekeeping_fixup_only_ev=388.754217`. DirectArb is now reconciled cleanly too: `directarb_seen=56`, `directarb_unique=55`, `directarb_route_usable=56`, `directarb_inventory_ready=13`, but `directarb_strict_would_fire=0` and `directarb_fixup_would_fire=13`, with `directarb_top_blocker=not_held_input` and the firing rows also blocked on missing output ATA. All `273` would-fire rows are currently labeled `heuristic_ev_only=true`, so the `$427` should be treated as **fixup-conditioned heuristic no-send surface**, not realized-PnL proof. Current interpretation: the next monetization lever is not “trade harder”; it is precreating the recurring ATA/account surface and only then rerunning a fresh Epoch to see how much strict-current-state EV remains.

**Run66 authoritative forensic artifacts 2026-05-20:** the new forensic outputs now live at:
- `/home/ubuntu/shadow-host-audit/dpdk-runs/66_epoch1_monetization_forensic_audit/summary.txt`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/66_epoch1_monetization_forensic_audit/epoch1_would_fire_rows.jsonl`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/66_epoch1_monetization_forensic_audit/epoch1_would_fire_forensic_summary.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/66_epoch1_monetization_forensic_audit/epoch1_housekeeping_forensic_report.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/66_epoch1_monetization_forensic_audit/epoch1_directarb_forensic_report.json`

**Run67 ATA-precreation planner update 2026-05-20:** the next planner lane is now landed and host-proven against the completed Epoch1 / Run66 artifacts. `g1-host-pack/scripts/67_recurring_ata_precreation_planner.sh` reads the `272` ATA-fixup rows, audits mint policy, runs a conservative unit-sized exit probe per output mint, ranks candidates by owner mode, and emits an operator task file for ATA work plus the epoch-rerun comparison commands. The critical result is that **current conservative policy yields no automatic ATA shortlist**: `selected_candidate_count=0`, `projected_strict_rows_if_selected_atas_precreated=1`, `projected_strict_ev_usd_if_selected_atas_precreated=0.183765`. The planner inspected `47` owner-mode ATA candidates in total; blocker split is now explicit: `policy_blocker=extensions_not_understood` on `25`, `freeze_authority_present` on `2`, and only `20` mints are policy-clear. Of the policy-clear set, most still fail the unit-sized exit probe as dust (`16`) or missing (`2`). The single candidate that passed the core recurrence/unlock gate but still failed liquidity is `CwhSfYsGUBVZyLP96Uyfau6hsWcdwe5GMEcBFKeZTRT4` (`housekeeping`, `2` rows, `$1.523103` unlock, but only `$0.000486` on the unit exit probe). The clearest policy-clean liquid mint is `RAY` (`4k3Dyj...`), but it appears only once for `$0.955050`, so it does not yet clear the recurrence / expected-unlock threshold. Current interpretation: the next blocker is no longer just “missing output ATA.” Under current conservative policy it is now **Token-2022 extension acceptance plus exit-liquidity reality**. No fresh broad epoch should be opened until that policy/surface decision is made.

**Run67 authoritative planner artifacts 2026-05-20:** the new planner outputs now live at:
- `/home/ubuntu/shadow-host-audit/dpdk-runs/67_recurring_ata_precreation_planner/summary.txt`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/67_recurring_ata_precreation_planner/run67_recurring_ata_precreation_planner.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/67_recurring_ata_precreation_planner/run67_owner_mode_ata_plan.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/67_recurring_ata_precreation_planner/run67_selected_ata_precreation_plan.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/67_recurring_ata_precreation_planner/run67_mint_policy_audit.json`
- `/home/ubuntu/shadow-host-audit/dpdk-runs/67_recurring_ata_precreation_planner/run67_operator_tasks.sh`

**`g12-qc` refresh update 2026-05-20:** the stale mirror set has been cleared and replaced with the current Epoch workflow only. `g12-qc` now contains the active monetization doctrine (`shadow_monetization_strategy_UPDATED.md`), current config / env / runbooks, and the current execution chain only: `20_g12_r0_observe_only.sh`, `39_broad_surface_discovery*.sh`, `54_all_venue_current_scorecard_execution_autopsy.sh`, `55_all_venue_bounded_live_readiness.sh`, `60_post_shock_capital_efficiency.sh`, `61_post_shock_conversion.sh`, `62_conversion_autopsy.sh`, `64e_exit_qualified_held_input_analysis.sh`, `65_full_potential_continuous_shard_simulation.sh`, `66_epoch1_monetization_forensic_audit.sh`, `67_recurring_ata_precreation_planner.sh`, `epoch_canonical_monetization.sh`, plus the shared helpers. The mirror should now be treated as a clean current-state handoff set, not an archive of superseded runs.

**Run61 -> Run62 -> Run63 post-acquisition result 2026-05-20:** this remains a side-branch metadata-acquisition result, **not** the current canonical Epoch1 monetization baseline. The fresh no-send rerun was negative for the first Coinini experiment. `Run62` finished `run61_status=run61_continuous_shadow_live_simulation_complete` and showed `directarb_inventory_ready_count=24`, `directarb_would_fire_count=0`, `directarb_main_blocker=not_held_input`, with Coinini itself still the top not-held-input mint. `Run63` then confirmed **no realized DirectArb lift** from the one-mint acquisition: `baseline_directarb_inventory_ready_count=24`, `modeled_directarb_inventory_ready_count=24`, `directarb_inventory_ready_delta=0`; `baseline_directarb_would_fire_count=0`, `modeled_directarb_would_fire_count=0`, `directarb_would_fire_delta=0`. The broad modeled unlock still exists only as a counterfactual (`modeled_total_deployable_count=785`, `deployable_by_us_delta=784`, `directarb_would_fire_if_policy_safe_held=682`), but the real measured blocker remains `not_held_input`, not policy metadata alone. Current interpretation: the bounded Coinini acquisition was a cheap executor proof and a useful dust-inventory lesson, but it did **not** convert DirectArb into an actually firing lane and should not be confused with the fresh canonical Epoch1 / Run66 picture.

**Run55 update 2026-05-18F:** bounded all-venue live-readiness is now host-proven on `.15`. `g1-host-pack/scripts/55_all_venue_bounded_live_readiness.sh` ran a bounded `600s` native AF_XDP zero-copy Run39 window, then classified all supported venues for `public_rpc_live_ready`, `tpu_quic_no_send_ready`, and `tpu_quic_tiny_live_ready` without pretending unimplemented routes existed. Preflight started green (`local_health=ok`, processed/finalized lag `0`), the full window completed (`sample_slots=1354`, `observe_candidates=147`, `worker_route_actionable=68`, `worker_quote_ok=33`, `r1_surface_observations=68`), and the venue mix was `raydium_clmm=34`, `raydium_cpmm=26`, `raydium_v4=2`, `meteora_damm_v2=6`, `pumpfun_amm=0`. Final classification was `status=blocked_no_implemented_live_route_ready_candidate` with `public_live_ready_count=0`, `tpu_no_send_ready_count=0`, and `tpu_tiny_live_ready_count=0`. The nearest current row was `raydium_clmm` pool `BZtg...` on `EPj -> Es9`, but it remained `external_geyser`, `jito_bundle`, and `blocked_not_positive_ev`; CPMM and Meteora still died first at held-input. Agave only went red on the **post-run** health check (`local_rpc_unhealthy`, about `1530` slots behind), so the bounded window itself remained usable. Current interpretation: the blocker is current-candidate quality / source-grade / implemented-route truth, not native AF_XDP darkness or immediate validator collapse.

**Run56 ATA + inventory overlay update 2026-05-18G:** the first concrete execution blocker on the current Meteora target is now cleared. The current all-venue target remained `meteora_damm_v2` pool `AMdgoDAzGAe5BZYwNozjgi8BYhzZJg5EqR3Ds1t3xvs9`, `WSOL -> 3e2M9ieHh95tbEgQ3Emi4eRkmdEL9kfaApj5dT7dDGRR`, `source=afxdp`, `route=jito_bundle`. The output mint was verified directly against live RPC as a real Token-2022 mint (`owner=TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`, symbol `Print`, name `Printr`, no freeze authority). A signer-owned Token-2022 ATA was then created out of band for worker signer `ANutm...`: `5JNPFiPix9Q9Nd14s9imGnWY8eNwe4cnnQCC89Z2bez7`, tx `5eku3Q43iLXeu3HiUKN2M8jgZoiEJW3i3xm15efxn2rKPKwv5nZ2kpYLMfKLMTH6ZJxtJhS65ZrRsZkeQAnt7817`. A new helper, `g1-host-pack/scripts/refresh_signer_token_inventory.sh`, now refreshes `/etc/shadow-engine/signer-token-inventory.json` from live RPC instead of manual edits, and both `54_all_venue_current_scorecard_execution_autopsy.sh` and `40_tpu_quic_tiny_live_scout.sh` now overlay refreshed inventory truth on top of scorecard rows. Host `.15` proof with the patched Run54 against the pinned current scorecard now returns `inventory_overlay_used=true`, selects the same `AMdgo...` row, and upgrades it from `first_blocker=output_account_ok` to `first_blocker=ready_path_open` with `held_input=true`, `positive_ev=true`, `input_balance_ok=true`, `output_account_ok=true`, `template_ready=true`, `tiny_cap_safe=true`, and `tpu_convertible=true`. The next blocker is no longer output ATA readiness; it is that `.15` is currently red on local Agave preflight (`local_rpc_unhealthy`, slot lag above threshold), so a fresh Run55 or Run40/52A no-send proof is presently gated by host/local-source health rather than missing inventory.

**Latency-QC correction 2026-05-09:** the supported-path probe report is now fixed. After exporting `process_intent_total`, `process_intent_unaccounted`, and `worker_wrapper_unaccounted`, the pinned `.15` `release-guerrila` supported no-send worker-path probe came back low-microsecond, not millisecond: DPDK-tagged `worker_to_nosend p99=2690ns`, `p999=5680ns`, `process_intent_total p99=2390ns`, `process_intent_unaccounted p99=240ns`, `worker_wrapper_unaccounted p99=140ns`; the AF_XDP-labeled control matched closely (`worker_to_nosend p99=2710ns`, `p999=5420ns`, `process_intent_total p99=2410ns`). That means the earlier `.15` readings around `1.77ms / 52ms` were an invalid superseded probe outcome, not current host truth. Raw DPDK RX/classify remains separately fast (`dpdk_rx_to_classify_p99_ns=740`, `p999_ns=740` on the guarded harness). This still does **not** prove full NIC RX -> classify -> extract -> feed -> worker end-to-end DPDK latency, because the supported-path probe begins from a synthetic DPDK/AF_XDP-tagged worker record; real wire-to-wire DPDK truth still requires actual DPDK `IngressRecord`s crossing the feed boundary. But the specific `.15` supported no-send worker-path latency blocker is now explained and materially cleared.

**Execution-route / dependency update 2026-05-14:** Public RPC blackhole remains the first proof harness because it gives controlled failure/NACK, cleaner PM reserve/release evidence, and lower accidental-send risk. Ordinary direct TPU/QUIC does **not** require Jito/Harmonic-style private authorization; it requires leader schedule/contact discovery, QUIC connection management, fanout, signed transaction bytes, and separate status/reconcile plumbing. The route plan is now explicit: **public RPC = proof/tiny-live stepping stone, TPU/QUIC = target production submit path, SWQoS / validator peering = later landing-rate optimization if ordinary TPU/QUIC is not enough.** The infrastructure target is also explicit: paid external RPC/gRPC should leave the hot path over time and become fallback/comparison/emergency only, while owned local Agave/AF_XDP/DPDK ingest, local RPC/state hydration, direct TPU/QUIC submit, and local reconcile become primary.

**Operating-mode correction 2026-05-14:** Foreground doctrine has changed. `Run38` is now regression-only for CLMM quote, public-RPC blackhole, PM reserve/release, and one-shot latch proof. `Run39` is the live foreground path: broad-surface discovery, no-send/live-scout classification, and `surface_scorecard.json` generation on `.15`. `Run40` exists as a host-deployed scaffold but remains honestly blocked with `blocked_unimplemented_tpu_quic_sender` because `shadow-tier0` still has `quic_workers` config surface but no real TPU/QUIC sender path.

**Run52A correction 2026-05-18:** the older Run40 blocker statement is now superseded. `shadow-tier0` and `shadow-egress-tpu` do have a working host-deployed TPU no-send scaffold on `.15`; the active exact-target blocker is no longer "missing TPU sender infrastructure." The real Run52A wiring fixes were: 1. `parse_public_scout_filter_venue()` needed to recognize `meteora_damm_v2`, 2. `52a_tpu_route_conversion_9i1gx.sh` needed `SHADOW_RUN40_ENABLE_TPU_QUIC=true` before the inner Run40 `exec`, and 3. the earlier `quic_workers: 1` idea was wrong and has been reversed. Guerrila static validation rejects nonzero `system.proxies.egress.quic_workers` with `NO_PUBLIC_QUIC`, and the host proof now starts cleanly with `quic_workers: 0`. After rebuilding on `.15`, rerendering config, and rerunning `52a_tpu_route_conversion_9i1gx.sh`, the inner Run40 probe stayed healthy (`inner_probe_status=tpu_quic_no_send_ready`, `worker_template_build_ok=52`, `worker_no_send_backend_ok=52`, `tpu_wire_send_count=0`, `live_landed=0`) but acceptance still ended `RUN40_NO_SEND_NO_CANDIDATE` with `tpu_quic_candidate_seen=0`, `tpu_quic_would_enqueue=0`, `tpu_quic_ring_enqueue_ok=0`, `tpu_quic_sender_dequeue_count=0`, and `tpu_quic_no_send_accept_count=0`. Current interpretation: the Run52A bug cluster is fixed, but the exact-target `9i1Gx...` / `Momh...` rerun remained target-cold in that window; the blocker has moved back to target recurrence / source-window truth rather than sender absence.

**Run52A watcher-mode update 2026-05-18B:** the next local patch is now landed to avoid blind repetition of the exact `9i1Gx...` window. `g1-host-pack/scripts/52a_tpu_route_conversion_9i1gx.sh` now accepts `SHADOW_RUN52_TARGET_MODE` (or `SHADOW_RUN52A_TARGET_MODE`) and can hand Run40 into watcher-armed selection instead of hardcoding exact pool+mints. The new minimum mode is `any_meteora_hunt_tpu_convertible`, wired through `g1-host-pack/scripts/40_tpu_quic_tiny_live_scout.sh`. That selector no longer requires rows to already be `tpu_ready=true`; instead it can choose a current Meteora HUNT-style candidate that is held-input, positive-EV, template-ready, tiny-cap-safe, output-account-ready, and route-convertible from the observed private family into forced `tpu_quic_public` no-send. This patch is synced to `.15` but not yet host-proven; the next TPU rerun should use watcher-armed mode rather than another blind exact-only window.

**Run52A watcher host-proof correction 2026-05-18C:** watcher mode is now host-proven in both useful directions. First, it successfully selected the expected archived Run47 Meteora row when allowed to search the whole audit tree, confirming that the new selector path itself works. Second, when forced to use the live scorecard `SHADOW_RUN52_SCORECARD_PATH=/home/ubuntu/shadow-host-audit/dpdk-runs/39_broad_surface_discovery_current/surface_scorecard.json`, the run ended `selection_status=no_meteora_hunt_tpu_convertible_candidate` and `RUN40_NO_SEND_NO_CANDIDATE` without moving any TPU counters. That means the current blocker is no longer stale-scorecard reuse and no longer selector failure; it is that the current Run39 scorecard does not contain a live Meteora row strong enough to enter the watcher-armed TPU conversion path. The next diagnosis is therefore current-scorecard scarcity / source-window truth, not TPU impossibility.

**Run53 autopsy update 2026-05-18D:** the Run40/Run52A watcher path now emits explicit current-scorecard autopsy fields and supports a broader selector mode, `any_hunt_tpu_convertible`. Host `.15` proof against the pinned live scorecard (`/home/ubuntu/shadow-host-audit/dpdk-runs/39_broad_surface_discovery_current/surface_scorecard.json`) returned `current_scorecard_loaded=true`, `stale_scorecard_used=false`, and `selection_attempted=true` in both inner and outer summaries. The broad any-HUNT pass ended `selection_status=no_any_hunt_tpu_convertible_candidate` with `gate_autopsy_scope=hunt_rows_all_venues` and all autopsy counters `0`, which means the current Run39 scorecard has **no HUNT rows at all**, not merely no TPU-convertible one. The companion Meteora-specific autopsy is more informative: `observed=7`, `held_input=4`, `positive_ev=1`, `input_balance_ok=1`, `output_account_ok=0`, `template_ready=0`, `tiny_cap_safe=0`, `tpu_convertible=0`, with first-blocker counts `held_input=3`, `positive_ev=3`, `output_account_ok=1`. Current interpretation: the blocker has sharpened again. Run53 shows that the live Run39 slice is currently empty at the HUNT level, and within Meteora specifically the only row surviving into funded-positive territory dies at output-account readiness before template or TPU conversion. The next work is not another blind TPU rerun; it is output-account / venue-support cleanup for current-scorecard rows, or a separately labeled dev-source enqueue proof if the goal is TPU-ring exercise rather than canonical AF_XDP truth.

**Run54 all-venue autopsy update 2026-05-18E:** the next step is now implemented as a dedicated all-venue current-scorecard autopsy wrapper, `g1-host-pack/scripts/54_all_venue_current_scorecard_execution_autopsy.sh`. It reads the pinned current Run39 scorecard, applies the same execution-readiness semantics used by Run40, reports per-venue stage counts, and promotes one global nearest-to-executable row with no hidden venue preference. Host `.15` proof against `/home/ubuntu/shadow-host-audit/dpdk-runs/39_broad_surface_discovery_current/surface_scorecard.json` returned `current_scorecard_loaded=true`, `stale_scorecard_used=false`, and selected:
- `venue=meteora_damm_v2`
- `pool=AMdgo...`
- `mint_in=WSOL`
- `mint_out=3e2M...`
- `source=afxdp`
- `route=jito_bundle`
- `first_blocker=output_account_ok`
- `selected_output_account_fixable=true`
- `selected_tpu_convertible=true`

Per-venue current-scorecard split:
- `raydium_clmm`
  - `observed=2`
  - nearest row is `8sLb...`
  - `positive_ev=true`, `template_ready=true`, `tiny_cap_safe=true`, `public_convertible=true`, `tpu_convertible=true`
  - but `held_input=false` / `input_balance_ok=false`, so the first blocker is still inventory
- `raydium_cpmm`
  - `observed=1`
  - nearest row is `1iq1...`
  - `positive_ev=true`, `template_ready=true`, `tiny_cap_safe=true`, `tpu_convertible=true`
  - but `held_input=false`, so inventory is still the first blocker
- `meteora_damm_v2`
  - `observed=7`
  - nearest row is `AMdgo...`
  - `held_input=true`, `positive_ev=true`, `input_balance_ok=true`, `template_ready=true`, `tiny_cap_safe=true`, `tpu_convertible=true`
  - but `output_account_ok=false` while the output account remains safe/fixable, so this is now the closest current row to TPU/public execution
- `raydium_v4=0`, `pumpfun_amm=0`

Corrected interpretation: broadening to all supported venues did **not** overturn the next target this time, but now that result is justified by current-scorecard ranking rather than historical Meteora preference. The live nearest-to-executable row is still a Meteora `WSOL -> 3e2M...` AF_XDP row because CLMM and CPMM both fail earlier on held-input/inventory, while the Meteora row has already cleared held-input, EV, template, and tiny-cap gates and is now blocked only by output-account readiness.

**Broad-surface support correction 2026-05-14:** The currently wired discovery surface is broader than CLMM but not universal. Discovery-ready today: Raydium V4, Raydium CPMM, Raydium CLMM, Pump.fun AMM / PumpSwap, and Meteora DAMM v2. Not yet wired or not yet first-class: Orca Whirlpool, Meteora DLMM, explicit thin-SOL bucketing, and explicit migration / launchpad bucketing. Broad discovery therefore does help answer the “narrow opportunity” problem, but it is not yet broad end-to-end proof.

**Execution-template correction 2026-05-14:** Broad discovery is not the same thing as broad venue-correct execution. CLMM has venue-specific runtime template and exact-quote handling. Non-CLMM venues still fall through to the single configured execution template loaded from `config.guerrila.yaml`, which is currently a Raydium V4 template. So Run39 can honestly prove `decoded -> route/quote/classification` on the widened surface, but it cannot yet prove venue-correct end-to-end execution for CPMM, Pump.fun, or Meteora.

**Host-source regression correction 2026-05-14 late:** The backend comparison is now cleaner. A real DPDK Run38 timeout and a real regular AF_XDP safe/copy Run38 timeout on `.15` both came back with the same business-path zeros (`sample_slots=0`, `observe_candidates=0`, `worker_candidates_entered_process_intent=0`, `worker_route_resolved_public_rpc_scout=0`, `public_rpc_transport_attempt=0`). The shared cause was **not** the ingest backend. At the time of those runs, `.15` had again lost its live local Agave source: there was no local RPC on `127.0.0.1:8899`, no UDP listeners on `8000-8025`, and no live persistent SH1 unit. The countermeasure is now implemented in `g1-host-pack`: the main AF_XDP/DPDK/scout entrypoints fail closed on a local-Agave preflight requiring a live SH1 unit/process, local RPC, acceptable slot lag versus the bootstrap/reference RPC, and bound `8000-8025` listeners. `.15` has now been put back onto the SH1 restore path: transient `sh1-agave-no-geyser.service` is running again, persistent `sh1-agave-no-geyser-2.service` is installed/enabled, the watchdog timer is active, storage guardrails are enforced, and snapshot bootstrap is in progress. The operating rule has changed too: in the AF_XDP posture, local Agave on `.15` should stay continuously live and current rather than being repaired only right before a run. Until that preflight passes, zero-counter `.15` runs are not valid ingress verdicts.

**Native AF_XDP correction 2026-05-14 late-late:** The next blocker was narrower than expected and is now closed. Native zero-copy Run38 on `.15` was no longer dying because of missing local Agave and no longer dying because feed rings were absent; it was dying because the AF_XDP survival recycle logic in `shadow-tier0` used **Fill-ring free-slot count** instead of **posted-buffer occupancy**. On a native XSK this inverted signal meant survival recycle did not fire when the kernel had consumed almost the entire initial prime, so the path looked like “alive for one Fill-ring, then dark” and capped out around `4096` frames. That software bug is now fixed locally and on-host. After syncing the patch, rebuilding `shadow-tier0`, and rerunning a fresh native zero-copy Run38 smoke on `.15`, the ingress path stayed alive deep into the window: `afxdp_frames_seen=1578080`, `afxdp_xsk_rx_batches=606309`, `afxdp_reassembly_complete_entries=1121209`, `afxdp_tx_record_emit_attempt=12367`, `afxdp_feed_enqueue_ok=12367`, `worker_signal_afxdp=1552`, `worker_records_seen=1552`, `worker_pool_state_hit=114`, and `worker_route_actionable=114`. That closes the old “feed darkness / worker handoff dead” boundary. The current failure boundary is now later-stage again: the regression window still timed out, but with real route activity and a first latched signature while CLMM remained fail-closed because the quote sidecar was unavailable / mismatched (`CLMM exact quote sidecar unavailable (WrongVersion)`) and the native tape still carried a large unsupported-program slice. In other words: native AF_XDP liveness on `.15` is now real again; the next help needed is on CLMM sidecar/runtime compatibility and downstream route-quality/coverage questions, not on XSK refill starvation.

**Current next steps 2026-05-14:** 1. Keep `.15` in the always-live AF_XDP posture: leave the validator/watchdog green, keep local RPC healthy, and treat any future preflight regression as a host fault rather than a run result. 2. Treat the old `4096 frames then dark` native Run38 behavior as superseded; do **not** spend more time re-debugging native AF_XDP refill unless fresh evidence contradicts the fixed-smoke result. 3. Escalate the current Run38 blocker as a downstream issue: CLMM quote sidecar compatibility / `WrongVersion`, plus the remaining unsupported-program share and route-quality split in the native tape. 4. Keep Run38 in regression only. 5. Re-arm Run39 to produce and archive the first shred-bearing `surface_scorecard.json` on `.15`. 6. Use that scorecard to rank venue/pool/mint classes by `route_ready`, `template_ready`, `quote_ready`, `quote_invalid`, `template_missing`, `state_missing`, `cap_failed`, and `HUNT/WATCH/DENY`. 7. If broad end-to-end proof is required before TPU/QUIC work resumes, implement venue-correct execution templates for Raydium CPMM, Pump.fun AMM, and Meteora DAMM v2. 8. Resume Run40 only after the TPU/QUIC sender implementation exists in `shadow-tier0`.

**Foreground correction 2026-05-15:** Run39 broad-surface discovery is now genuinely producing usable inventory-aware scorecards on `.15`. The current archived slice at `/home/ubuntu/shadow-host-audit/dpdk-runs/39_broad_surface_discovery_current` finished `status=scorecard_generated` after the wrapper was fixed to arm directly from a successful prelaunch UDP probe and to log every failed watch probe. That slice produced `candidate_rows=10`, `held_input_candidate_rows=8`, `would_admit_rows=5`, `top_routes=[["jito_bundle", 10]]`, `held_input_public_rpc_scout_rows=0`, and top venues `meteora_damm_v2=7`, `raydium_clmm=2`, `raydium_cpmm=1`. The conclusion changed: Run38 public-RPC CLMM proof is now too narrow for the current surface, because held-input candidates do exist but are not naturally recurring on `public_rpc_scout`.

**Funding-semantics correction 2026-05-15:** the worker signer still holds about `12.500010001 SOL` natively, but the current fixed CLMM template uses the pre-funded WSOL ATA from `/etc/shadow-engine/signer-token-inventory.json`, which currently reports `0.2 WSOL` (`200000000` raw). So the old Row-4 "insufficient balance" diagnosis means insufficient pre-wrapped WSOL for the observed `1.0 WSOL` candidate size, not insufficient total signer capital. `auto_wrap_supported=false` remains the invariant for the current proof path until native-SOL wrapping is explicitly designed and proven.

**Run41 correction 2026-05-15:** the next proof family is now targeted Raydium CLMM + `jito_bundle` no-send proof for pool `8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj`, `WSOL -> USDC`, at a conservative cap of `100000000` raw (`0.1 WSOL`). The current `0.2 WSOL` ATA already covers that proof cap, so no extra wrapping is needed. A first Run41 smoke harness is now built and deployed on `.15`, but the latest smoke ended `INNER_PRECHECK_FAILED` because local Agave preflight was red (`local_rpc_unhealthy`, processed/finalized lag about `16190` slots). The immediate blocker has therefore moved again: repair local Agave / local RPC health before interpreting any Run41 no-target result as market evidence.

**Historical optimization evidence prior to the 2026-05-14 operating-mode correction:** Private access is gatekept/sparse enough that private-lane-only routing is no longer the primary live path. R1-JITO-JSON no-send is green, and the **R1-PUBLIC-A** local foundation is now implemented: explicit public RPC scout config/validation, explicit `sendTransaction` egress wiring, first-landed stop/reconcile behavior, and machine-enforced class-D floors (`min_retained_ev_lamports > 0`, `min_net_after_fee_lamports > 0`) that fail closed before real send while leaving observe-only / no-send paths intact. The fresh `.51` PS-P0 rerun remains decisive negative calibration, and the current external-geyser/public slice remains **NEGATIVE_CALIBRATION_A_ONLY** (`3477` rollout observations, `3470` forward-markout observations, `0` positive `t0` retained EV, `0` positive forward markout, `0` positive after-fee markout, `0` D-class candidates). So do **not** attack `.51` as-run, do **not** fee-tune this slice, and do **not** promote pools `439504`, `705258`, or `705282`; keep them as negative examples of “busy but worthless” structure. Cross-pool / route-lag markouts are landed locally as additive `ps_p0_route_lag_markout` records keyed by venue + route family and excluding same-pool reuse, and the first decoder expansion slice is landed locally: Raydium CPMM exact-input decode in `shadow-ingest`, Raydium CPMM pool classification/decode in `shadow-state`, widened observe configs, and the offline expanded-surface report path `run\ps_p0_abcd_report.js`. The first clean host-generated expanded-surface rerun has now been restated from `.15` host Run10 (`psp0-run10-targetpools-extgeys-60m-20260508-root`) using the current analyzers on the sanitized tape: `observe_candidates=18084`, `worker_candidates_entered_process_intent=9042` (**50.0% decoded-actionable rate**), `36092` motif observations across `meteora_damm_v2`, `raydium_v4`, and `raydium_cpmm`, but `0` positive `t0` retained EV, `0` positive forward markout, `0` positive after-fee markout, `0` landable-under-cap, and `0` D-class candidates. The route-sync rerun on the same tape with the corrected `.15` worker-path denominator (`--execution-latency-us 5.68`, buckets `6/25/100/1000us`) stayed clean-negative as well: `route_sync_impulse_count=33020`, `positive_on_first_response_count=0`, `positive_anytime_count=0`, `survives_our_latency_positive_count=0`, and every survival bucket remained `0`. The busiest observed pools remain **DENY** under clean evidence, while the six targeted absent pools remain **UNKNOWN** because they never appeared in the run. Do **not** arm R1-PUBLIC-A from this surface. The gRPC/external-geyser unsupported-program top-list blocker is now cleared by a fresh short `.15` rerun (`psp0-run24-extgeys-unsupported-current`): `grpc_pipeline_drop_unsupported_program=52837`, with helper-program-heavy top entries followed by real unsupported venue families led by `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` (**Pump.fun AMM / PumpSwap**, `6902`), `EtrnLzgbS7nMMy5fbD42kXiUzGg8XQzJ972Xtk1cjWih` (**Phoenix Eternal**, `6045`), `BiSoNHVpsVZW2F7rx2eQ59yQwKxzU5NvBcmKshCSUypi` (**BisonFi**, `3242`), and `9H6tua7jkLhdm3w8BvgpTn5LZNU7g4ZynDmCiNN3q6Rp` (**HumidiFi**, `3190`). Pump.fun AMM / PumpSwap support is now landed locally across `shadow-ingest`, `shadow-state`, recorder labels, and active pool-program config surfaces. A short `.15` PumpSwap-enabled rerun (`psp0-run25-extgeys-pumpfun-current`) no longer showed `pAMMB...` in the exported unsupported top-15, which is the expected immediate proof that the venue moved out of the unsupported histogram. The longer post-Pump.fun rerun is now complete as `.15` Run26 (`psp0-run26-extgeys-pumpfun-60m-current`) with the widened `pool_programs` block explicitly including `CAMMCzo...` and `pAMMB...`: `sample_slots=9566`, `observe_candidates=486`, `worker_candidates_entered_process_intent=222` (**45.7% decoded-actionable rate**), and `grpc_pipeline_drop_unsupported_program=1528300` while `pAMMB...` stayed absent from the unsupported top-16. The clean tape widened materially (`485` rollout observations, `951` motif observations), but it still emitted only `raydium_clmm` (`943` observations) and `raydium_v4` (`8`) venue rows; `pumpfun_amm` still did **not** appear in the clean tape. Economics are no longer globally zero: overall there were `81` positive retained-EV observations, `184` positive forward markouts, `138` positive after-fee observations, and `28` D-class observations. But those D-class rows were **same-pool Raydium only** (`28` same-pool D, `0` cross-pool route-lag D), while the refreshed route-sync study remained non-executable at Shadow latency (`627` impulses, `positive_on_first_response_count=0`, `positive_anytime_count=70`, survival `0` at `6us`, `25us`, `100us`, and `1ms`). Run26 same-pool D isolation is now complete from raw tape and collapses to two durable pool+mints: `raydium_clmm | 5e64... | [10b7..., ce01...]` with **26 D** in Run26 and only a weak **Run25 precursor** (`2` B, `0` D), plus `raydium_v4 | b99d... | [3799..., ce01...]` with **2 D** in Run26 and no earlier hits in the archived clean windows. Strict replay is now also landed as `run\ps_p0_strict_replay_report.js` and the first report artifact is archived as `files\ps_p0_strict_replay_report_run26_same_pool.json`. Under the current tiny public-scout envelope (`0.05 SOL` trade cap, `0.1 SOL` notional cap, `require_afxdp_source=true`), the CLMM pair grades **TRAINING**: `18` exact D rows were positive at recorded `t0` after fee on the recorded Jito path, but `0` stayed positive at `t0` while also fitting the tiny cap envelope, only `8` exact D rows fit the cap at all, all final outcomes stayed `pending_observe_only`, all rows were `external_geyser + jito_bundle`, and `5` exact D rows still carried `would_drop_dominance_gate`. The V4 pair grades **WATCH**: both exact D rows were positive at recorded `t0` after fee on the recorded Jito path, but both sat just above the tiny trade cap (`56.9M` lamports), both were still `external_geyser + jito_bundle`, and PM release truth remained unresolved. Both pairs keep ShadowGuard/freshness proxy passes on the recorded path (`would_build/sign` true, no stale/egress reroute/drop), but public-scout route legality is **not** proven by this observe-only tape and inventory/PM release truth remain unresolved. The focused `.15` Run27 repeatability window is now also complete as `psp0-run27-samepool-repeatability-60m-current`: `sample_slots=6242`, `observe_candidates=362`, `worker_candidates_entered_process_intent=169`, and `grpc_pipeline_drop_unsupported_program=883809`. The host shock export arrived as a concatenated JSON-object stream rather than clean line-delimited JSONL, but local recovery split out `6423` valid objects and dropped a truncated trailing tail. The new focused analyzer `run\ps_p0_pair_repeatability_report.js` shows the CLMM lead **repeated** in the fresh window with `55` same-pool observations and class split `B=18, C=22, D=15`; it kept `15` positive `t0` after-fee observations, `28` positive-after-fee observations that also fit the current tiny cap envelope, best / p50 / p90 `best_net_after_fee_lamports = 6645730 / 3406949 / 4714555`, and label **WATCH**, but it still came entirely from `external_geyser + jito_bundle` and still showed `7` repeated `would_drop_dominance_gate` rows. The V4 lead did **not** repeat at all in Run27 (`0` matched observations) and is now **DENY** for the active set. The focused `.15` Run28 CLMM-only blocker proof is now also complete as `run28-clmm-blocker-proof-afxdp-current`: the AF_XDP window itself was alive (`sample_slots=4435`, `observe_candidates=1112`, `worker_candidates_entered_process_intent=547`, `worker_signal_afxdp=71`), so owned-source visibility exists on the host in the same capture. But the surviving CLMM pair still repeated **only** on `external_geyser`: `41` same-pool observations, `12` D, `11` positive `t0` after fee, `20` positive after fee + tiny cap, `5` repeated `would_drop_dominance_gate` rows, source distribution `external_geyser` only, route distribution `jito_bundle` only, and final outcomes `pending_observe_only` only. The pair therefore stays **WATCH**, not HUNT. Source-grade remains blocked because the target pair never appeared on AF_XDP even when AF_XDP traffic was present elsewhere in the same window. Public-route legality remains blocked because the explicit `public_rpc_scout` path exists in `R1-PUBLIC-A`, but the current observe-only proof still does not produce any public-route-compatible rows and the recorder route-family labels still expose only `jito_bundle`, `harmonic_bundle`, or `drop`. PM release truth remains blocked because observe-only rollout rows still hard-code `final_realized_outcome = pending_observe_only`; the recorder has reconcile/release emission paths, but this run mode did not exercise them for the target pair. The dominance blocker is now narrower: a dedicated Run27 drilldown (`files\run27_clmm_dominance_analysis.json`) showed all `7` repeated dominance rows had same-slot non-dominance counterparts and `0` of them were tiny-cap positive on best-net-after-fee, so dominance currently looks like duplicate suppression rather than hidden clean tiny-cap alpha. So PumpSwap implementation/deployment is proven, same-pool Raydium CLMM remains the only active economic lead, direct-arb / inter-pool route-sync remains blocked, and the next task should stay CLMM-only: keep `R1-PUBLIC-A` blocked, stop broadening, and prove whether the surviving CLMM pair can ever clear public-route legality and PM lifecycle truth under a mode that can emit non-pending terminal outcomes without reopening broad venue discovery.

**Run29-C correction instruction 2026-05-10:** Do **not** rerun Run29 as-is. The previous Run29 was useful only because it proved what did **not** happen: no `process_intent`, no rollout, and no PM truth. The sole active lead remains `raydium_clmm` pool `5e64c7901fc7a8ecd6c262d6c3b6cfa4418ae5c1dd1dacee8adbfd7b8d0b09da` with status **WATCH**; the V4 `b99d...` lead remains dropped. Preserve these failure facts from the blocked proof: `worker_signal_afxdp=56`, `worker_signal_external_geyser=292320`, `worker_pool_state_hit=11`, `worker_route_resolved_drop=11`, `worker_candidates_entered_process_intent=0`, `worker_rollout_would_send_events_published=0`, and `shock_research.jsonl=0 bytes`. Interpretation: ShadowGuard proof was accepted, AF_XDP was alive, but the proof died at route resolution before `process_intent`, rollout, or PM, so PM reserve/release/reconcile truth remains unproven. Before any corrected rerun, fix the config first: patch the **actual** `public_rpc.endpoint` field to `https://127.0.0.1:1` (the failed script only patched `jito.json_rpc.endpoint`), then verify via generated config and runtime log that the live public scout points at the blackhole endpoint rather than `https://api.mainnet-beta.solana.com`. Also restore Raydium CLMM into `state.source.pool_programs`; the corrected list must include at least Raydium V4 `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8`, Raydium CPMM `CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C`, Raydium CLMM `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK`, and PumpSwap `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` if keeping widened surface; verify the generated config explicitly contains `CAMMCzo...`. Then decide whether worker route telemetry must be patched before claiming public-route legality. Current worker/runtime route family still collapses to `{drop, jito_bundle, harmonic_bundle}`; there is no honest worker-visible `public_rpc_scout` route family yet. Preferred fix: add an explicit `public_rpc_scout` route decision or telemetry-only route family label gated behind `R1-PUBLIC-A`, and never allow it as a hidden fallback from private-lane mode. If that patch is not done, state plainly that the corrected Run29-C can prove only public-scout session availability, not full route legality. The corrected target is **Run29-C: CLMM public-scout blackhole proof** in `R1-PUBLIC-A` tiny-live config shape with a blackhole `public_rpc.endpoint` and no real chain send possible. Required success is: `worker_pool_state_hit > 0`, `worker_candidates_entered_process_intent > 0`, `route_family_label = public_rpc_scout` or equivalent explicit public-route proof, `would_build > 0`, `would_sign > 0`, would-send attempt reaching public scout egress, public RPC transport failure/NACK recorded, PM reservation created, PM release recorded, fee leak accounted, `phantom_inventory_risk=false`, and `live_landed=0`. Do **not** live-send, do **not** use the mainnet-beta public RPC endpoint for this proof, do **not** call `jito_bundle` route “public route legal,” do **not** use `external_geyser` as private-grade, and do **not** broaden to Phoenix / BisonFi / HumidiFi until this CLMM blocker is resolved. Label rules after corrected Run29-C remain: **HUNT** only if public-route legality plus PM reserve/release truth pass and economics repeat; **WATCH** if economics repeat but public-route / PM remains partially unresolved; **TRAINING** if route / PM proof still fails before `process_intent`; **DENY** if the CLMM economics disappear in the corrected proof.

**Run29-C corrected host proof result 2026-05-10:** The config mismatch is now genuinely closed, but the live blocker narrowed into route semantics. The archived generated config proves `public_rpc.endpoint="https://127.0.0.1:1"` on the actual public scout path, not on `jito.json_rpc.endpoint`, and it also proves `state.source.pool_programs` now includes both `CAMMCzo...` and `pAMMB...`. The runtime log independently confirms `public-rpc-scout: enabled and sendable (https://127.0.0.1:1)`, so the blackhole public-scout session is real. Host counters from the corrected proof are now: `worker_signal_afxdp=9`, `worker_signal_external_geyser=43852`, `worker_pool_state_hit=2`, `worker_route_resolved_drop=2`, `worker_route_reason_public_target=2`, `worker_candidates_entered_process_intent=0`, `worker_rollout_would_send_events_published=0`, `rollout_public_rpc_no_send_would_sends=0`, `rollout_events_observed=0`, `live_landed=0`. That means the old config-generator bug is fixed, AF_XDP is alive, and the surviving CLMM window now reaches route resolution honestly — but it still dies before `process_intent` because Guerrila routing continues to treat the public-signal opportunity as `PublicTarget`. So this corrected Run29-C remains **TRAINING / blocked**, but for a much narrower reason than before: the next required patch is explicit **public-route semantics** for `R1-PUBLIC-A`, not another wrapper/config tweak. In other words, telemetry-only `public_rpc_scout` labeling is not enough by itself; the worker still hard-drops public-signal CLMM attempts before PM truth can be exercised. The next change should therefore either (A) add explicit public-scout route semantics so public-signal opportunities can progress without being misrepresented as private-grade, or (B) state formally that the current build proves only public-scout session availability, not public-route legality / PM lifecycle truth.

For direct-arb / route-sync interpretation, the current usable denominator is the synthetic supported no-send worker-path p999 of roughly **5.68µs** on `.15` (`dpdk worker_to_nosend p999=5680ns`; AF_XDP control `p999=5420ns`). That is excellent worker-path speed, but it is not landed-tx or wire-to-wire truth. The next study therefore needs to ask whether inter-pool desync survives long enough after first observation for Shadow to decide, build, send, and land before the gap decays. The intended report shape is:

```text
route_family | pool_a | pool_b | gap_bps_t0 | net_lamports_t0 | t_first_response_us | t_half_gap_us | t_break_even_us | survives_6us | survives_25us | survives_100us | class
```

Use these working class rules:

```text
D / HUNT:
  positive net-after-fee at Shadow p999
  survives 25us or 50us
  repeated across clean runs
  liquidity/slippage acceptable

C / WATCH:
  positive before fees or only survives at 0-5us
  needs better fee/landing/source edge

B / TRAINING:
  useful pattern but not tradable yet

A / DENY:
  busy but worthless, fees kill it, or synchronization is too fast
```

Host `.51` remains only a constrained rehearsal/transfer box, not the final combined target. Rescue-mode inspection of replacement host `.221` (`259S001355`, Intel `i40e`) still says XDP/AF_XDP on `eno1` is unsafe there, so reserve `eno1` for management / normal Agave traffic only and only consider AF_XDP on `eno2` if carrier is present. DPDK on `.221` remains materially farther along for feasibility, but the latest live sample on `eno1` is still only tiny UDP gossip (`132`-byte packets; no packets `>1200` bytes in a 20s sample), so the full DPDK producer -> feed-ring -> money-worker counters remain `0`. The per-source latency comparison foundation is landed locally as `g12_latency_by_source.json`, and gRPC end-to-end comparison is measurable immediately, but DPDK source-to-worker / source-to-no-send remains correctly empty until DPDK emits real `IngressRecord`s. Latitude has prepared the next active X550 proving host `109.94.99.15` (`G-12`, Intel X550 / `ixgbe`): routed-`eno1` AF_XDP zero-copy is now better understood there. The management risk is concentrated in the **native attach/detach transition** on `ixgbe`, which can drop carrier and briefly cut normal SSH unless OOB is available; once native XDP is attached and the link settles, `.15` has now also been proven to keep `eno1` routable and normal SSH usable while `afxdp_bind_mode: zero_copy` remains live. Guarded DPDK bring-up is still green there, but both AF_XDP and DPDK are still packet-dark for Solana UDP in the saved plain `tcpdump` / guarded harness samples. Treat `.221` as the completed i40e feasibility/archive box and `.15` as the active X550 proving host.



**Current research pivot:** Clean backrun-only samples have shown sparse/flat economics so far, while older class-D evidence was not cleanly reproducible under isolated per-run provenance. The next proof path is **Impulse/Continuation P0** plus **Optimization P0** plus **Speed Monetization P0**: treat any relevant on-chain market event as an impulse, record sub-slot and event/slot markouts, score all owner modes continuously, optimize the surfaces where speed can become money, and prove which opportunities are positive only because Shadow is faster. Public send remains blocked until clean per-run positive net-after-fee evidence exists. External/local geyser data may be used for observation, fallback, public-scout research, and comparison, but remains non-private-grade and must not become the main live decision surface.


**Optimization P0 required outputs:**

- **Coverage report:** unsupported-program frequency, decoded-actionable rate, venue/mint-pair coverage, and missing decoder ranking by expected market relevance.
- **Pool scorecard:** per durable `pool_pubkey + mint pair` metrics for event frequency, route-gap frequency, positive markout rate, D-class rate, median/p90 net-after-fee, gap half-life, toxicity, liquidity depth, slippage risk, and inventory burden.
- **Pool state labels:** `HUNT`, `WATCH`, `TRAINING`, `DENY`, `UNKNOWN`; labels must be computed from clean per-run evidence, not shared-tape intuition.
- **Route-family heuristics:** CPMM→CPMM, CPMM→CLMM, DAMM→CPMM, DAMM→DLMM, SOL/token→token/SOL, launchpad→migration, thin→deep, deep→thin; each reports `gap_bps_t0`, `net_gap_lamports_t0`, `t_first_response`, `t_half_gap`, `t_break_even`, and survival at p99/p999 engine latency.
- **Source hierarchy report:** owned AF_XDP/DPDK/local-Agave first; external gRPC/RPC only as fallback, reconciliation, comparison, or bootstrap. Private/partnered source access remains a business-development path, not a dependency that blocks owned-ingest optimization.
- **Inventory tolerance report:** per-mint and per-route-family exposure bands, aging, auto-flatten rules, bad-mint deny rules, and PM release truth.
- **Small-ugly trade report:** small positive-after-fee opportunities by venue, route, mint risk, slippage, fee, landing, and realized/markout outcome.
- **Neglected-venue report:** smaller Meteora/Raydium/PumpSwap/launchpad/migration surfaces where large players appear under-optimized.
- **Owner-selection ledger:** DirectArb, Backrun, Continuation, Housekeeping, Rebalancing, Flatten, and Drop scores for the same event, with exactly one selected owner unless PM records an explicit split rule.


**Speed Monetization P0 required outputs:**

- **Latency ablation report:** every clean candidate replayed at `0µs`, `5µs`, `10µs`, `25µs`, `50µs`, `100µs`, `250µs`, `1ms`, and `10ms` artificial delay.
- **First-seen race report:** AF_XDP/DPDK/local-Agave first-seen time versus external-gRPC/RPC first-seen time, with `ours_only_positive`, `gRPC_still_positive`, `both_positive`, and `both_dead` classes.
- **Route-sync speed report:** `t_first_response`, `t_half_gap`, `t_break_even`, survival at Shadow p99/p999 full-path latency, and speed-exclusive windows by pool pair / route family.
- **Late-binding fee report:** whether fresher leader, egress, fee, tip, and route state reduce fee leakage or stale sends versus earlier/slower decisions.
- **Micro-ugly trade report:** tiny awkward opportunities positive after fees at Shadow latency, including mint risk, slippage risk, landing risk, and inventory burden.
- **Continuation-after-arb-fail report:** cases where deterministic direct arb was gone, but `50ms/100ms/250ms/500ms` continuation remained positive.
- **Speed-as-defense report:** fast Drop/Flatten decisions that avoided negative markout, absorption, crowding, stale state, or route-collapse losses.
- **Required fields:** `candidate_id`, `owner_mode`, `first_seen_source`, `owned_first_seen_ns`, `external_grpc_first_seen_ns`, `grpc_seen_delta_us`, `our_full_path_latency_us`, `artificial_delay_us`, `net_after_fee_at_0us`, `net_after_fee_at_5us`, `net_after_fee_at_10us`, `net_after_fee_at_25us`, `net_after_fee_at_50us`, `net_after_fee_at_100us`, `net_after_fee_at_250us`, `net_after_fee_at_1ms`, `net_after_fee_at_10ms`, `ours_only_positive`, `grpc_still_positive`, `speed_exclusive_window_us`, `selected_owner`, `survives_6us`, `survives_25us`, and `survives_100us`.

**Impulse P0 required outputs:** `impulse_id`, durable `pool_pubkey`, venue, mint pair, impulse type/tier (`S0_micro_impulse`, `S1_reserve_displacement`, `S2_same_direction_burst`, `S3_route_lag_impulse`), direction, size, price impact, reserve displacement, `50ms/100ms/250ms/500ms` markouts, `+1/+2/+3` same-pool event markouts, `+1/+2` slot markouts, same-direction follow-through, absorption, route-gap decay, reserve recovery half-life, Backrun/Continuation/Housekeeping/Rebalancing/Drop scores, selected owner, fallback owner, and net-after-fee.


**Optimization-stage rule:** Do not turn the project into strategy soup. Direct arbitrage is the simplest deterministic probe; Backrun remains always-on for obvious `t0` shock value; Continuation handles propagation and follow-through; Housekeeping/Rebalancing handle restoration and inventory-state opportunities; Drop remains the default. Speed is the force multiplier for all of these, but speed diagnostics must become economic counterfactuals, not timing worship. The engine is allowed to observe and classify broadly, but live execution stays narrow: clean per-run provenance, positive net-after-fee, source-grade truth, route legality, ShadowGuard/freshness/inventory/egress health, PM reconciliation, and caps must all pass.

---

## 1. Executive state

G12 is **in progress**, not complete.

What is now green:

- G12-R0 long observe-only / no-send safety soak is green.
- SH7-A/B/C are green: final no-send publication, micro-market observation, and ShadowGuard ops proof are closed.
- SH8-A/B/C are green for observe-only AF_XDP private-grade proof.
- AF_XDP vs Triton gRPC latency A/B is complete and strongly favors AF_XDP.
- Standalone and in-process DPDK feasibility on `.221` is green enough to continue as an optional post-G12 ingress candidate: release `shadow-tier0 --features dpdk` now builds on-host, a guarded `eno1` VFIO observe-only run boots the full Tier0 process, and the rollback path restores host reachability.
- Replacement host `109.94.99.15` (`G-12`, Intel X550 / `ixgbe`) now proves routed-`eno1` AF_XDP zero-copy stability: real `g4-ingress-seal` zero-copy bind and XSKMAP registration succeed there, the host stays reachable, and kernel logs show no IOMMU/watchdog fault pattern.
- Standalone DPDK host bring-up is now also green on `109.94.99.15`: the guarded `eno1` VFIO harness completes with `dpdk_rx_to_classify_p999_ns=740`, `dpdk_kernel_faults=0`, and clean `ixgbe` restore.
- The comparable latency foundation is now landed locally: `g12_latency_by_source.json` exports source-split p9/p50/p90/p95/p99/p999/max/count snapshots for gRPC/AF_XDP/DPDK stages without changing the fixed IPC record contracts.
- Speed Monetization P0 doctrine is now defined: latency ablation, first-seen races, route-sync survival, late-binding fee advantage, micro-ugly trades, continuation-after-arb-fail, and speed-as-defense must show where speed changes economic outcome.
- R1 `auto_private_grade` remains the intended live gate; wallet allowlists are fallback-only.
- R1-FIX-B no-send proof is green on host `.57` under the corrected packaged artifact.
- R1 AF_XDP hot path is proven under temporary Jito no-auth diagnostics through route/actionable, R1 auto-admission, `process_intent`, template build, and pipeline success.
- R1-JITO-JSON no-send proof is green on host `.57` through no-send backend, final no-send publication, private-grade would-send evidence, and micro-market observation.
- Jito/Harmonic session diagnostics and retry safety are improved: Jito unauthorized auth now backs off, and Harmonic regional channel failures are explicitly logged.
- PS-P0 forward-markout recording is now landed and validated. The fresh `.51` rerun is decisive negative calibration: structurally active, but `0` positive forward markouts and `0` positive after-fee records.

What is not green:

- Full G12 promotion is still a separate gate.
- Productive live landing is **not** proven.
- Jito rejects the staged auth pubkey, and Harmonic endpoints fail channel connect.
- R1-PUBLIC-A is implemented locally, including machine-enforced positive retained-EV and positive net-after-fee floors for real send, but the latest `.51` PS-P0 rerun is A-only negative calibration. Keep the public scout ready, but do not arm it from this slice; only class-D PS-P0 candidates may trigger send.
- R1-PUBLIC-C-style surface expansion remains open, but the first clean host-generated expanded-surface report is no longer missing. `.15` host Run10 rerun with the current analyzers is still A-only even after Raydium CPMM + route-lag support: `observe_candidates=18084`, decoded-actionable rate `9042 / 18084 = 50.0%`, `36092` motif observations, `0` positive retained EV, `0` positive forward markout, `0` positive after-fee markout, `0` D-class, and route-sync survival `0 / 33020` at `6us`, `25us`, `100us`, and `1ms`. The gRPC/external-geyser unsupported-program top-list export is now landed and host-proven from `.15` Run24; after filtering helper programs, the dominant missing venue family was **Pump.fun AMM / PumpSwap** (`pAMMB...`, `6902` drops), followed by **Phoenix Eternal** (`6045`), **BisonFi** (`3242`), and **HumidiFi** (`3190`). Pump.fun AMM / PumpSwap support is now implemented. A short `.15` Run25 proof no longer shows `pAMMB...` in the top unsupported export, but the short cleaned tape still only yielded `raydium_clmm` rows and stayed `A/B-only` with `0` positive after-fee and `0` route-sync survival. The longer `.15` Run26 post-Pump.fun rerun is now also complete: `pAMMB...` stayed absent from the unsupported top-16 and the clean tape produced the first non-zero expanded-surface economics on this branch (`951` motif observations, `81` positive retained EV, `184` positive forward markout, `138` positive after-fee, `28` D-class). But those D-class rows were all same-pool Raydium (`28` same-pool D, `0` cross-pool route-lag D), `pumpfun_amm` still did not appear in the clean tape, and route-sync survival remained `0 / 627` at `6us`, `25us`, `100us`, and `1ms`. The open front is therefore no longer “does PumpSwap decode?”; it is (a) isolate the new same-pool D-class pools by durable `pool_pubkey + mint pair`, and (b) widen Pump.fun source coverage or move to **Phoenix Eternal** as the next missing venue family because PumpSwap still did not appear in the tape.
- Host `.57` is currently powered off after the R1-JITO-JSON no-send proof; `.51` has now also been reclassified as a constrained transfer host and had Agave bootstrap intentionally stopped at about `49.9%` snapshot download. Future host-side work now requires reprovision onto the intended `256/384 GiB` class.
- Full Tier0 DPDK p99/p999 is **not** green yet. The `.221` guarded `eno1` DPDK observe-only run boots and the `.15` standalone harness completes, but neither current host window carries shred-sized Solana ingress; on `.15`, plain `tcpdump` saw `0` UDP `8000-8025` and the harness saw `0` matching Solana UDP, so `worker_signal_dpdk` and all `dpdk_*` feed/worker latency counters remain `0` for real traffic-shape reasons.
- The new per-source latency export has not yet been exercised on a shred-bearing DPDK window. On current `.221` and `.15` data, the DPDK source-to-* buckets are correctly empty because no DPDK records crossed the feed boundary.
- The corrected `.15` pinned `release-guerrila` supported-path no-send probe is now low-microsecond and no longer a blocker by itself: residual-export runs show DPDK-tagged `worker_to_nosend p99=2690ns` / `p999=5680ns` and AF_XDP-tagged control `worker_to_nosend p99=2710ns` / `p999=5420ns`, with both residual buckets tiny. Do **not** use the earlier superseded `1.77ms / 52ms` probe output. Also do **not** misread the corrected worker-path probe as NIC RX -> classify -> extract -> feed -> worker end-to-end DPDK truth; real DPDK wire-to-wire latency still needs actual DPDK feed-crossing records.

---

## 2. Milestone board

| Gate | Status | Notes |
|---|---:|---|
| G9 | IMPLEMENTED | `shadow-oracle`, dual LUT banks, competition-adjusted shadow path. |
| G9A | IMPLEMENTED / USER-SIGNED-OFF | CU density, deterministic Jito-only exhaust, tick-boundary shadow telemetry. |
| G9B | PARTIAL / OBSERVE-ONLY FOUNDATION IMPLEMENTED | A-E landed; live F/G/H still deferred. |
| G10 | IMPLEMENTED / OPS-PROVED | ShadowGuard software proof plus mainnet deploy/smoke proof are green. |
| G11 | IMPLEMENTED / SOFTWARE-SIGNED-OFF | Rosetta auto-heal complete. |
| G12 | IN PROGRESS | R0 observe-only safety, SH8 AF_XDP private-grade proof, latency proof, R1-FIX-B no-send proof, R1 hot-path diagnostic proof, and R1-JITO-JSON no-send proof are green; R1-PUBLIC-A local foundation and PS-P0 forward-markout recorder are implemented, and the fresh `.51` rerun is decisive A-only negative calibration (`3470` forward-markout observations, `0` positive before fees, `0` positive after fees). The next move is surface expansion / motif discovery, while AF_XDP/private-grade source truth on `.51` remains a separate host gate before any live proof. |
| G13 | NOT STARTED | Graduation after sustained live proof. |

### 2.1 SolanaHost side board

| Gate | Status | Notes |
|---|---:|---|
| SH0 | CLOSED | Source-grade truth plumbing remains fail-closed. |
| SH1 | CLOSED FOR CONSTRAINED HOST | Agave / Yellowstone build + lab pack replayed. |
| SH2 | PARTIAL ACCEPT / CONSTRAINED HOST | Earlier constrained host was useful for replay but not final production sizing. |
| SH3-lite | CLOSED | Storage / firewall rehearsal repeatable on fresh hourly hosts. |
| SH4-lite | CLOSED | Localhost Yellowstone smoke passed. |
| SH5-A/B | CLOSED | Local-validator-Geyser source proof and supported-fixture software proof passed without private/live contamination. |
| SH7-A/B/C | CLOSED | Final no-send, micro-market uplift accounting, and ShadowGuard ops proof are green. |
| SH8-A/B/C | CLOSED FOR OBSERVE-ONLY AF_XDP PROOF | AF_XDP source probe, transaction-derived records, and final private-grade soak are green. |

---

## 3. Latest verified G12 truth

### 3.1 G12-R0 long observe-only safety soak

Artifact:

```text
/home/ubuntu/shadow-host-audit/g12-r0-observe/20260429_084358-long-accumulation
```

Key counters:

```text
status=threshold_reached
sample_slots=18401
observe_candidates=10335
rollout_would_send=3743
rollout_live_send_suppressed=3743
micro_market_observations=3743
micro_market_uplift_ok=true
shadow_guard_mainnet_deployed=true
shadow_guard_template_program_id_matches=true
worker_health_drops=0
live_landed=0
all_gates_green=false
promote_allowed=false
```

This closed the long no-send safety soak. It did not close private-grade admission because it was Geyser-only.

### 3.2 SH8 AF_XDP private-grade proof

Decisive artifacts:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_235006-af_xdp-32q-sh8-probe
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_031546-af_xdp-32q-sh8b-txdiag
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak
```

SH8-C final soak counters:

```text
afxdp_tx_extract_ok=286568
afxdp_tx_record_emit_ok=15
afxdp_feed_enqueue_ok=15
worker_signal_afxdp=66
observe_candidates_private_grade=15
rollout_private_grade_admissible_opportunities=5
live_landed=0
worker_health_drops=0
```

Interpretation: AF_XDP observe-only private-grade ingress is proven on `.57`; full G12 promotion remains a separate threshold decision.

### 3.3 AF_XDP vs Triton latency A/B

Valid artifact:

```text
/home/ubuntu/shadow-host-audit/g12-latency-ab/20260430_095649-afxdp-rerun-vs-triton-valid
```

Result:

```text
AF_XDP end-to-end p999=76700 ns
Triton gRPC end-to-end p999=128661054 ns
AF_XDP producer-to-feed p999=105140 ns
Triton gRPC producer-to-feed p999=142475814 ns
live_landed=0
worker_health_drops=0
```

Decision: keep AF_XDP primary for G12 and for any current private-grade proof, but DPDK is now an active **optional** post-G12 / AF_XDP-instability path across two host families: `.221` proves `i40e` VFIO + DPDK RX viability where AF_XDP on routed `eno1` is unsafe, while `109.94.99.15` proves X550 / `ixgbe` routed-`eno1` AF_XDP zero-copy stability plus guarded DPDK host bring-up.

### 3.3a DPDK feasibility on `.221`

Standalone DPDK RX-loop proof on `.221` (`eno1`) is green again:

```text
/home/ubuntu/shadow-host-audit/21_dpdk_rx_latency/dpdk-rx-latency.json
```

Latest 30-second result:

```text
dpdk_packets_seen=107
dpdk_udp_8000_8025_seen=73
dpdk_rx_to_classify_p99_ns=920
dpdk_rx_to_classify_p999_ns=950
dpdk_kernel_faults=0
```

The in-process Tier0 DPDK path is also now real on the same host:

```text
/home/ubuntu/shadow-host-audit/22_dpdk_tier0_observe_only/
```

What is proven:

- release `shadow-tier0 --features dpdk` now builds on Linux `.221`;
- guarded VFIO takeover on `eno1` can boot the full Tier0 process with `private_packet_backend=dpdk`;
- the host survives the run and returns to SSH reachability after restore;
- `shadow_blockhash` fallback/reuse is now needed on a management NIC because outbound RPC/DNS disappears during VFIO ownership.

What is **not** proven yet:

```text
worker_signal_dpdk=0
dpdk_producer_to_feed_count=0
dpdk_feed_to_worker_count=0
dpdk_worker_to_nosend_count=0
dpdk_end_to_end_observe_count=0
```

Interpretation: DPDK RX itself is alive, but the current `eno1` sample is not a valid full-Tier0 proof window. A live `tcpdump` sample on `.221` showed only `132`-byte UDP packets to ports `8000/8001`, and a 20-second `greater 1200` sample saw `0` packets. So the current blocker is **traffic shape**, not DPDK bootstrap or money-worker wiring.

### 3.3b Replacement host `.15` (`109.94.99.15`, X550 / `ixgbe`)

The newly prepared X550 host is now the active proving box for routed-`eno1` packet-path work:

- hostname `G-12`, dual Intel X550 (`8086:1563`) on `ixgbe`, with `eno1` carrying the public route;
- routed-`eno1` AF_XDP zero-copy is stable there: `g4-ingress-seal` reached zero-copy XSK bind, and after the pinned XSKMAP `EINVAL` fix in `crates/shadow-tier0/src/xsk.rs`, XSKMAP registration too;
- the host no longer should be described as “always reachable before and after” every native AF_XDP probe on routed `eno1`. The corrected `2026-05-14` finding is narrower and more useful: native attach/detach on `ixgbe` causes a one-time PHC / queue reinit and carrier flap on the management NIC, so the first native arm should be done with OOB coverage. After that transition settles, `.15` can keep routed `eno1` up, keep `afxdp_bind_mode: zero_copy`, and still accept normal SSH while native `xdp` remains attached;
- the guarded standalone DPDK harness now also runs on the same routed `eno1` and restores cleanly.

Current operator-corrected state (`2026-05-11`):

- the latest zero-counter AF_XDP/blackhole census on `.15` was not a valid ingress verdict because the host was **not** running a live local Agave validator at the time
- at that moment, `.15` had no UDP `8000-8025` listeners, `worker_signal_afxdp=0`, `observe_candidates=0`, and the Run60 blackhole census produced no meaningful feed-arrival evidence
- `.15` has now been corrected back onto the SH1 prerequisite path:
  - `sh1-agave-no-geyser.service` is running `mainnet-beta`
  - ledger path: `/mnt/solana-ledger`
  - snapshots path: `/mnt/solana-ledger/snapshots`
  - accounts path: `/mnt/solana-accounts`
  - RPC bind: `127.0.0.1`
  - dynamic Solana ports: `8000-8025`
- late `2026-05-14` regression correction:
  - after rescue/reboot and later host work, `.15` fell off the SH1 prerequisite path again
  - by the time the real DPDK Run38 timeout and the real regular AF_XDP safe/copy Run38 timeout were sampled, there was no local RPC on `127.0.0.1:8899`, no UDP listeners on `8000-8025`, and no live persistent SH1 unit
  - those equal-zero runs therefore do **not** distinguish DPDK from AF_XDP; they are missing-local-source evidence
  - the host-pack countermeasure is now landed: AF_XDP/DPDK/scout entrypoints fail closed on a local-Agave preflight requiring SH1 service/process + local RPC + acceptable slot lag + bound `8000-8025` listeners
  - the AF_XDP operating rule has changed too: keep local Agave continuously live/current on `.15`, with persistent `sh1-agave-no-geyser-2.service`, the watchdog timer, and the storage guard as part of the baseline host posture
  - latest restore state is green again: persistent `sh1-agave-no-geyser-2.service` is active, UDP `8000-8025` listeners are present, local RPC is listening on `127.0.0.1:8899`, `getHealth` is `ok`, `eno1` is `LOWER_UP` with no XDP attached, `/sys/fs/bpf/shadow_xsks_map` is absent, and the validator caught up cleanly from lag `1851` to `ok`
  - the guarded native zero-copy Run38 waiter is already armed behind the local-Agave preflight gate and explicit OOB arming; it will only launch after local RPC is healthy and `/home/ubuntu/shadow-host-audit/dpdk-runs/run38_native_oob_ready.flag` exists
- with that validator active, `.15` can show transient `1225-1232` bootstrap/shred-sized windows on routed `eno1`, but that is **not** yet a standing guarantee that every later proof window is shred-bearing
- latest guarded `eno1` AF_XDP rerun with rebuilt XDP diagnostics proved the narrower current truth:
  - XDP/XSKMAP on `.15` is alive in `xdpgeneric` / copy mode
  - the XDP diag map saw packets on `eno1`, saw IPv4/UDP, and saw Solana-port hits
  - but `shred_variant=0`, `shred_version=0`, and `redirect=0` in that same runtime window
  - concurrent `tcpdump` only saw tiny `132`-byte UDP to local `8000`
- therefore the current blocker is upstream feed-arrival / validator traffic phase on `.15`, not the AF_XDP runtime itself
- immediate rule: do **not** run another `.15` AF_XDP/DPDK proof window unless both of these are true at runtime:
  - one of the SH1 validator units is active (`sh1-agave-no-geyser.service`, `sh1-agave-no-geyser-2.service`, or the transient `sh1-agave-no-geyser`)
  - live `tcpdump -ni eno1 'udp portrange 8000-8025'` in the same proof window shows real large packets, not just silence or tiny `132`-byte gossip/bootstrap UDP
- if those checks are absent, zero `worker_signal_*` / zero `observe_candidates` counters are missing-prerequisite evidence, not proof that the AF_XDP/DPDK hot path itself is broken

Mandatory zero-candidate debug rule going forward:

```text
L0 host/service alive
L1 Agave/live source alive
L2 UDP/shred-sized traffic visible
L3 XDP/DPDK counters moving
L4 feed ring push/pop moving
L5 worker records_seen moving
L6 route/pool/mint gates moving
L7 process_intent moving
L8 rollout/no-send/live-send evidence
```

- No more zero-candidate rabbit holes before `L0-L5` are green.
- Do not treat `0 WSOL/CLMM candidates` as strategy or market evidence until the same proof window has already cleared `L2-L5`.
- On `.15`, `L2` specifically means large shred-bearing UDP on `eno1`; tiny `132`-byte gossip/bootstrap packets do not count.

Current DPDK artifact:

```text
/home/ubuntu/shadow-host-audit/dpdk-runs/21_dpdk_rx_latency/dpdk-rx-latency.json
```

Latest 20-second result:

```text
dpdk_packets_seen=27
dpdk_udp_8000_8025_seen=0
dpdk_rx_to_classify_p99_ns=740
dpdk_rx_to_classify_p999_ns=740
dpdk_kernel_faults=0
```

Interpretation: the X550 host now proves both AF_XDP zero-copy host safety and guarded DPDK VFIO bring-up on routed `eno1`, but it is still not a packet-bearing Solana ingress window. A plain `tcpdump` sample on `.15` saw `0` UDP packets in `8000-8025`, and the DPDK harness saw only `27` total packets with `0` matching Solana UDP. So the next blocker on `.15` is again **traffic shape / feed arrival**, not packet-path stability.

### 3.4 R1-FIX-B no-send proof

Meaningful host outcome under the stronger pool-cap corrected no-send artifact:

```text
/home/ubuntu/shadow-host-audit/r1-damm-v2-nosend/20260501_040001-poolcap-1m-nosend-afxdp
```

```text
worker_signal_afxdp=5991
r1_afxdp_private_grade_route_actionable=2037
r1_afxdp_private_grade_process_intent_entered=2037
worker_no_send_backend_called=21748
worker_final_no_send_event_published=28964
rollout_afxdp_private_grade_would_send=2689
rollout_afxdp_private_grade_live_send_suppressed=2689
micro_market_afxdp_private_grade_observations=2689
live_landed=0
worker_health_drops=0
```

Interpretation:

- the pre-`process_intent` R1 choke point is fixed;
- the no-send proof reaches the intended downstream evidence surfaces;
- R1 admission, deployed-size capping, and final no-send publication are no longer the active blocker.

### 3.4a R1-JITO-JSON no-send host proof

R1-JITO-JSON was synced, built, and config-validated on `.57`, then run in observe-only/no-send with Harmonic disabled and public fallback still amputated:

```text
/home/ubuntu/shadow-host-audit/r1-jito-json/20260501_062327-r1-jito-json-nosend-afxdp
```

```text
afxdp_tx_extract_ok=62764
afxdp_tx_record_emit_ok=7227
worker_signal_afxdp=5441
r1_afxdp_private_grade_route_actionable=4702
r1_afxdp_private_grade_process_intent_entered=4702
worker_no_send_backend_called=56970
worker_final_no_send_event_published=74213
rollout_afxdp_private_grade_would_send=5999
micro_market_afxdp_private_grade_observations=5999
live_landed=0
worker_health_drops=0
```

Operational note: native XDP attach transiently dropped carrier on this host; the green run used `xdpgeneric`, and XDP/XSKMAP were detached/removed after the window.

### 3.5 Tiny-live private-lane diagnostics

The first tiny-live scout exposed producer-side unsupported-program/decode coverage. That was no longer the final blocker after subsequent DAMM/route/hydration work and a larger no-send proof. The key later diagnostic is temporary Jito no-auth, Jito-only, with the private-lane health recovery fix:

```text
/home/ubuntu/shadow-host-audit/r1-tiny-live/20260501_044401-r1-tiny-live-jito-only-noauth-healthfix-afxdp
```

```text
worker_route_actionable=1204
r1_afxdp_private_grade_route_actionable=1204
r1_afxdp_private_grade_process_intent_entered=1187
r1_auto_admission_attempt=1204
r1_auto_admission_admit=1187
worker_pipeline_attempts=1187
worker_pipeline_success=1187
worker_process_intent_ok=1187
worker_template_build_ok=1187
live_landed=0
worker_health_drops=0
```

Interpretation:

- AF_XDP ingress, route/actionability, R1 admission, `process_intent`, template build, and pipeline success are no longer the current blocker.
- This diagnostic used `jito.auth_mode: no_auth`; it is not an acceptable live landing configuration.
- Real private-lane live remains blocked by provider access.

Current private-lane auth/connectivity evidence:

```text
/home/ubuntu/shadow-host-audit/r1-tiny-live/20260501_051013-r1-tiny-live-dual-lane-authdiag-afxdp

worker_signal_afxdp=3039
afxdp_tx_extract_ok=47899
afxdp_tx_record_emit_ok=3232
worker_route_actionable=0
r1_afxdp_private_grade_route_actionable=0
worker_route_reason_no_viable_route=1277
worker_route_resolved_jito=0
worker_route_resolved_harmonic=0
live_landed=0
worker_health_drops=0
jito_auth_failed_log_count=2
harmonic_failed_log_count=69
```

Jito rejects staged auth pubkey `3i9N1VhsmKJBXB4STP4xdinmAXYCRecmNHBbLkSzwCW4` with `PermissionDenied`. Harmonic auth pubkey `9ZaQnhU56rxrw3mkGrVHaebTFXF1FgqAdvanTPUJMPwa` is staged, but configured `fra/ams/nrt.harmonic.xyz` endpoints fail at channel transport connect. The worker signer `ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX` was also tested as Jito auth and rejected.

---

## 4. R1 status and current blocker

The intended R1 product shape is still:

```yaml
r1:
  live_admission:
    mode: auto_private_grade
    source_required: afxdp
    require_private_grade: true
    require_shadow_guard: true
    require_micro_market_uplift: true
    require_pre_sign_egress_admission: true
    max_live_trade_input_sol: 1.0
    max_position_sol_per_mint: 1.0
    max_notional_sol_per_entity: 2.0
```

Important implementation facts:

- default R1 mode is `auto_private_grade`;
- manual signer allowlist remains explicit fallback only;
- `pass_all` is rejected for R1 live admission;
- tier0 bypasses signer identity prefilter only for R1 auto mode so the money worker can gate on candidate truth;
- rollout/G12 exports include R1 auto-admission counters.

Current private-lane blocker:

```text
AF_XDP records work
-> R1 hot path works
-> Jito JSON-RPC no-send path is green
-> Jito auth is unauthorized
-> Harmonic endpoints fail transport connect
-> JSON-RPC tiny-live send/landing remains unproven
```

So the active blocker is **live send/landing proof**, not host bootstrap, Agave bootstrap, ShadowGuard, AF_XDP ingress, R1 admission, or the JSON-RPC no-send path.

Because the user does not have practical provider contacts/access, but Jito JSON-RPC may not require the same gRPC token path, the next live path is now:

```text
R1-JITO-JSON
```

This must keep Harmonic disabled and public execution amputated:

```yaml
submit.routing.default_jito_searcher: jito_json_bundle
submit.routing.enable_harmonic_lane: false
jito.json_rpc.enabled: true
jito.json_rpc.endpoint: "https://tokyo.mainnet.block-engine.jito.wtf"
jito.json_rpc.mode: bundle_only
```

The next approval gate is JSON-RPC tiny-live. If Jito JSON-RPC tiny-live fails due auth/rate/landing, public-tiny becomes the fallback. Public-tiny must be a separate mode/config, may not be described as private-lane R1 green, and may not silently weaken the existing private-lane configuration.

---

## 5. Important negatives

Do **not** treat the current state as:

- permission to use a wallet allowlist as the normal R1 live gate;
- permission to use `pass_all` for R1;
- permission to relabel Geyser evidence as private-grade;
- permission to silently reopen public execution inside the private-lane Guerrila config;
- proof that productive live R1 tiny is green;
- proof that full G12 promotion is complete;
- proof that a new host alone fixes the current R1 blocker.

The correct production direction remains:

```text
AF_XDP packet
-> fixed-width decode/classify/quote
-> ShadowGuard/freshness/inventory/micro-market gates
-> R1 auto_private_grade admission
-> Jito JSON-RPC Block Engine send before public
-> explicit public-tiny fallback only if JSON-RPC tiny-live fails and it is deliberately configured/labeled
```

Not:

```text
observe traffic
-> dump wallets
-> manually choose wallets
-> trade against a static allowlist
```

---

## 6. Resume plan

When work resumes:

1. treat Jito gRPC/Harmonic private-lane R1 as blocked on provider access;
2. treat the earlier `.15` `1.77ms / 52ms` supported-path probe output as superseded and invalid;
3. treat the corrected `.15` supported no-send worker-path probe as low-microsecond, but keep it separate from real DPDK wire-to-wire truth because it starts from a synthetic worker record;
4. treat R1-JITO-JSON no-send as green, but do not keep private-lane-only routing as the next live path;
5. treat the fresh `.51` PS-P0 rerun as **NEGATIVE_CALIBRATION_A_ONLY** and preserve pools `439504`, `705258`, and `705282` as do-not-target negative examples for the current slice;
6. stop rerunning the same external-geyser surface expecting profit; the current answer is no;
7. resume R1-PUBLIC-C-style surface expansion and motif discovery: Raydium CPMM/CLMM, Orca, broader Meteora, PumpSwap, and cross-pool / route-lag markouts;
8. keep R1-PUBLIC-A local code/config work ready, but only arm the explicit public RPC scout under `0.05 SOL` input cap and `0.1 SOL` entity cap when PS-P0 yields class-D candidates;
9. keep AF_XDP source requirement, ShadowGuard/bounds logic where applicable, R1 auto-admission, inventory truth, fee-leak accounting, PM reconciliation, kill switches, micro-market uplift, and tiny caps intact;
10. Latitude has now prepared the next host; start there from `host_run_guide.md`, then `SH\SH_RUNBOOK.md`, then `SH\SolanaHost-checklist.md`;
11. before shutting down `.221`, preserve `~/shadow-host-audit/`, the current repo/config state, and any generated `g12_gate_input.json`, `g12_gate_report.json`, and `g12_latency_by_source.json` artifacts;
12. keep public execution out of private-lane configs; it must be explicit R1-PUBLIC mode, not a hidden fallback.

---

## 7. Files most relevant now

- `host_run_guide.md`
- `SH\SH_RUNBOOK.md`
- `SH\SolanaHost-checklist.md`
- `crates\shadow-tier0\src\afxdp_runtime.rs`
- `crates\shadow-tier0\src\afxdp_producer.rs`
- `crates\shadow-tier0\src\ingress_control.rs`
- `crates\shadow-tier0\src\main.rs`
- `crates\shadow-tier0\src\grpc_normalize.rs`
- `crates\shadow-tier0\src\grpc_producer.rs`
- `crates\shadow-tier0\src\tail_latency.rs`
- `crates\shadow-tier0\src\money_worker.rs`
- `crates\shadow-tier0\src\rollout_gate_export.rs`
- `crates\shadow-control\src\rollout_gate.rs`
- `crates\shadow-ingest\src\pipeline.rs`
- `crates\shadow-ingest\src\route.rs`
- `crates\shadow-egress-jito\src\session.rs`
- `crates\shadow-egress-jito\src\proxy.rs`
- `crates\shadow-harmonic\src\session.rs`
- `crates\shadow-harmonic\src\proxy.rs`
- `g1-host-pack\config.guerrila.yaml`
- `g12-qc\G12_REPORT.md`
- `g12-qc\G12-QC.md`

---

## 8. One-line resume point

**Resume from surface expansion, not another `.51` replay: AF_XDP/private-grade ingress, R1 no-send proof, R1 hot-path processing, JSON-RPC-selected no-send publication, and the explicit public-RPC scout foundation are green locally; the PS-P0 forward-markout recorder is also green, and the fresh `.51` rerun is decisive A-only negative calibration (`3477` rollout observations, `3470` forward-markout observations, `0` positive before fees, `0` positive after fees, `0` D-class candidates), so do not attack `.51` as-run or promote pools `439504`, `705258`, or `705282`; instead broaden surface coverage and add cross-pool / route-lag markouts, while keeping R1-PUBLIC-A ready but gated on class-D PS-P0 candidates.**

---

## 9. Addendum 2026-05-23: current condition update

This addendum supersedes the earlier "current blocker" wording **for the fresh `.15` no-send monetization branch**. Keep the older sections as historical truth for how the project got here, but resume from the state below.

### 9.1 Helius bundle-access change

The main Helius RPC endpoint is now upgraded and bundle-capable for this branch.

Configured endpoints:

```text
HELIUS_RPC_URL=https://mainnet.helius-rpc.com/?api-key=...
HELIUS_GATEKEEPER=https://beta.helius-rpc.com/?api-key=...
```

The important verification is that `getBundleStatuses` on the main Helius RPC now returns valid JSON-RPC success on `.15`, so the simulation branch may treat `jito_bundle` / private bundle egress as available for no-send grading. LaserStream gRPC is **not** the egress path; regular Helius HTTP RPC is.

### 9.2 Fresh 5-minute full-strength no-send rerun on `.15`

Fresh step:

```text
epoch8_curiosity_5m_fresh_fullstrength_post_business_20260523
```

Primary artifacts:

```text
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_curiosity_5m_fresh_fullstrength_post_business_20260523/summary.txt
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_curiosity_5m_fresh_fullstrength_post_business_20260523_run55_run39/summary.txt
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_curiosity_5m_fresh_fullstrength_post_business_20260523_run62/summary.txt
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_curiosity_5m_fresh_fullstrength_post_business_20260523_run64e/summary.txt
```

Fresh headline:

```text
continuous_candidates_seen=956
blocked_private_route_count=802
tpu_quic_public_candidate_count=154
public_rpc_scout_candidate_count=0
suppressed_route_unavailable_count=0
missed_ev_due_to_route=0.0
```

Interpretation:

- private-route access is no longer the simulated blocker on this fresh tape;
- public RPC still contributed nothing on this tape (`public_rpc_scout_candidate_count=0`);
- the monetization bottleneck moved from route unavailability to **held-input / inventory / source gating**.

DirectArb diagnosis on the same fresh tape:

```text
directarb_seen=82
directarb_route_usable=82
directarb_inventory_ready=0
directarb_would_fire=0
directarb_blocker_top=not_held_input
```

Continuous current-inventory result:

```text
would_fire_total=10
would_fire_directarb=0
would_fire_continuation=1
would_fire_housekeeping=6
would_fire_rebalancing=3
would_public_rpc_count=0
would_wire_send_count=0
```

So the current economic read is:

```text
route access moved from RED -> GREEN in simulation
held-input coverage is now the active directarb blocker
```

### 9.3 Fresh held-input overlay proof

Run64E identified two small not-held-input mints as economically viable bounded additions:

```text
8cCEn2qDfJm7stpqsaXBvw7CUV5fbi1HrF38YAcEmdUL
66ykXeJtR4WHWfZ1okYvcPEvnA8fRa9Qw1bSZBLLeeu1
```

Both were marked `eligible_for_bounded_review`, with only `1000000000` raw units needed for the bounded band and sub-`1 USDC` equivalent acquisition cost in the recorded review.

Simulation-only overlay rerun:

```text
epoch8_curiosity_5m_fresh_fullstrength_post_business_plus_overlay_20260523
```

Primary artifacts:

```text
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_curiosity_5m_fresh_fullstrength_post_business_plus_overlay_20260523/summary.txt
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_curiosity_5m_fresh_fullstrength_post_business_plus_overlay_20260523_run55_run39/summary.txt
```

Important delta:

```text
before overlay:
  directarb_inventory_ready=0
  directarb_would_fire=0
  would_fire_directarb=0

after overlay:
  owner_mode_directarb_deployable_by_us_count=2
  continuous_owner_selected_directarb=3
  would_fire_directarb=2
```

This is the current best news on the branch: **tiny held-input expansion unlocked fresh DirectArb in simulation** without changing the route-access assumption.

### 9.4 Reporting seam to remember

Two summary fields are currently misleading if read literally:

```text
blocked_private_bundle_only
jito_bundle_executable_count
```

On the fresh overlay run, inner tape summaries can still report:

```text
blocked_private_bundle_only=733
```

That does **not** mean route access was unavailable in the final simulation. It means `733` observed opportunities were naturally private-bundle-route opportunities on the tape. The more truthful outer simulation fields are:

```text
suppressed_route_unavailable_count=0
missed_ev_due_to_route=0.0
```

Do not regress on this interpretation. The cleaner future naming would be:

```text
observed_private_bundle_route_count
route_unavailable_blocked_count
```

### 9.5 Current condition, stated plainly

For the fresh `.15` no-send monetization branch:

```text
AF_XDP / fresh tape / multi-route grading = working
private bundle route access in simulation = working
public RPC route surface = still effectively zero on sampled tapes
current monetization blocker = bounded held-input coverage and related inventory/source readiness
current operational blocker before real money = controlled live sendBundle / landing proof is still not done
```

So there are now **two** separate truths:

1. the **economic simulation blocker** moved from route access to inventory/held-input coverage;
2. the **operational live-trading blocker** is still the absence of a controlled private-bundle live landing proof.

### 9.6 Deferred architecture condition: AF_XDP and LaserStream must have different roles

This is a memo condition for later implementation, not the immediate branch target.

Core rule:

```text
AF_XDP = primary first-seen / hot-path trigger
LaserStream = secondary enrichment / continuity / replay / backfill / model training
```

If LaserStream becomes the only ingest feeding live decisions, then AF_XDP is no longer being used for the core thesis. It becomes unused or only a benchmark.

Required dual-ingest shape:

```text
AF_XDP record
LaserStream record
local Agave/Geyser record
RPC/replay record
```

Each record must preserve source truth:

```text
source=afxdp
source=laserstream
source=local_agave
source=rpc_replay
```

Shadow should dedupe by:

```text
slot + signature + pool + mint_in + mint_out + owner_mode
```

And retain source-timing fields:

```text
first_seen_source
first_seen_ns
laserstream_seen_ns
afxdp_seen_ns
source_delta_us
```

Required architecture:

```text
shadow-ingest-afxdp
  -> normalized ingress bus
  -> source=afxdp
  -> can trigger hot live lanes

shadow-ingest-laserstream
  -> same normalized ingress bus
  -> source=laserstream
  -> enrich/replay/train/slow-path

source arbiter
  -> dedupe
  -> compare first_seen
  -> choose source grade

Tier0
  -> consumes only approved source grades
  -> never waits for LaserStream
  -> never calls gRPC
```

Initial source-grade policy:

```text
DirectArb live:
  source must be afxdp

Backrun live:
  target trigger should be afxdp
  LaserStream may enrich

Continuation/rebalancing research:
  LaserStream allowed

Continuation/rebalancing live:
  LaserStream allowed only after proof

Housekeeping:
  start no-send only
```

Do **not** do this:

```text
LaserStream and AF_XDP both feed Tier0 as equal live triggers
```

Do this instead:

```text
AF_XDP decides first
LaserStream explains, fills gaps, trains, validates, and keeps 24/7 continuity
```

This condition matters especially because `.15` AF_XDP same-`eno1` windows are short; LaserStream can keep continuous market memory, but AF_XDP must remain the race-winning hot trigger for latency-sensitive live lanes.

### 9.7 New one-line resume point

**Resume from fresh `.15` held-input expansion, not from route-access diagnosis: the Helius business-plan change moved private-bundle route access to green for no-send simulation, the fresh 5-minute tape proved route unavailability is no longer the main economic blocker (`suppressed_route_unavailable_count=0`, `missed_ev_due_to_route=0.0`), and the first cheap overlay on two exit-qualified mints converted fresh DirectArb from `0` to `2` simulated fires; keep AF_XDP as the only default live trigger for latency-sensitive lanes, keep LaserStream as secondary enrichment/continuity later, and do not blur those roles when dual-ingest work resumes.**

### 9.8 Helius Sender status (`2026-05-23`)

Helius Sender is now structurally integrated in the branch as:

```text
shadow-egress-helius-sender
route family = helius_sender_dual
```

What is now true:

```text
plain private single-tx flow -> helius_sender_dual
true bundle-only / ordered flow -> jito_bundle
host no-send smoke on `.15` -> green
wrong selected transport -> fast-NACK
```

Interpretation:

- Sender is now a usable **single-transaction private egress lane** for DirectArb / simple post-shock entry-exit style work.
- Sender is **not** the replacement for ordered bundle flow.
- Sender is currently parked. Do not reopen it unless the branch explicitly needs route-specific EV accounting, first live Sender proof, or bundle-vs-Sender comparison.

### 9.9 Dual-source update (`2026-05-24`)

The earlier LaserStream-vs-AF_XDP comparison was distorted by the shared no-send sink. That distortion is now removed.

First correction:

```text
RunLS5E = research sink fixed
sink_attempt_total=6180
sink_accept_total=6180
sink_suppress_total=0
sink_full_count=0
sink_drop_count=0
sink_backpressure_count=0
```

So the current source race is no longer being clipped by the old no-send sink.

Second correction:

```text
RunLS6B valid window
matched_signatures=4
laserstream_minus_afxdp median raw delta = +2434 us
laserstream_minus_afxdp median candidate delta = +2430 us
```

That valid-but-tiny sample favored AF_XDP on the overlap.

Third and more important correction:

```text
RunLS6C clean config-hardened window
/home/ubuntu/shadow-host-audit/dpdk-runs/runls6c_config_hardened_20260524_e/window1/20_g12_r0_observe_only
matched_signatures=15
matched_row_count=15
raw_receive_delta_us_median=-20790
normalized_delta_us_median=-20791
candidate_emit_delta_us_median=-21057
trade_ready_delta_us_median=null
```

Under the current `laserstream_minus_afxdp` basis, negative means LaserStream earlier. So the clean LS6C window favored LaserStream on the matched sample at raw receive, normalized, and candidate emit.

What is now safe to say:

```text
LaserStream is not merely connected; it is a credible matched-sample source.
AF_XDP remains the only private-grade / live-sensitive trigger source.
LaserStream has broader continuity / research / enrichment value than the earlier stale docs implied.
trade-ready still has no verdict
```

Current working conclusion:

```text
hybrid remains correct
  AF_XDP = hot private-grade trigger
  LaserStream = breadth, continuity, replay, research, and possible future strategy-specific source
```

Do **not** demote AF_XDP from the live trigger role yet, and do **not** promote LaserStream into the live trigger role yet. The next promotion threshold remains trade-ready proof, not just earlier matched-source arrival.

### 9.10 LaserStream-only near-trade-ready and Backrun no-send promotion (`2026-05-24`)

The next branch after LS6C/LS6D was not another plain source race. It was to ask whether LaserStream was already producing economically usable rows that AF_XDP simply did not overlap with during short windows.

`RunLS7` and the offline `RunLS8` audit answered that clearly:

```text
matched_near_trade_ready_count = 0
only_laserstream_near_trade_ready_count = 413
only_afxdp_near_trade_ready_count = 1
```

Those `413` LaserStream-only rows were not junk. The offline monetization audit proved:

```text
laserstream_only_near_trade_ready_count = 413
laserstream_only_ev_positive_count = 413
laserstream_only_pm_reserve_possible_count = 413
laserstream_only_sender_ready_count = 71
laserstream_only_bundle_ready_count = 342
laserstream_only_blocked_only_by_source_policy = 413
```

Owner-mode split:

```text
backrun = 187
drop = 226
directarb = 0
```

This changed the priority. The right next action was not LaserStream live promotion and not AF_XDP demotion. It was a narrow policy promotion:

```text
LaserStream Backrun -> no-send only
DirectArb stays AF_XDP-only for live-sensitive work
Drop stays audit-only until semantics are explicitly resolved
```

That proof is now green as `RunLS9` on `.15`:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only
```

What `RunLS9` proved:

```text
laserstream_backrun_seen = 763
laserstream_backrun_no_send_eligible = 124
laserstream_backrun_no_send_accept = 124
laserstream_backrun_bundle_ready = 83
laserstream_backrun_sender_ready = 41
laserstream_backrun_pm_reserve_ok = 83
laserstream_backrun_blocked_by_source_policy_before = 124
laserstream_backrun_blocked_by_source_policy_after = 0
wire_send = 0
live_landed = 0
cleanup_verified = true
```

Interpretation:

```text
LaserStream is now more than enrichment.
It is a proven no-send strategy source for Backrun.
This is still not a DirectArb promotion.
This is still not a live/private-grade promotion.
```

Current architecture after LS9:

```text
AF_XDP = live-sensitive / private-grade trigger
LaserStream = continuous breadth, research, enrichment, and now scoped Backrun no-send source
Helius Sender / Jito / Harmonic = economic egress lanes
```

The next strategic follow-up is no longer “is LaserStream connected?” That is answered. The next follow-up is either:

```text
1. Drop semantics audit
2. matched near-trade-ready first-fail decomposition
3. later, strategy-specific live promotion proof if trade-ready evidence appears
```

Those first two follow-ups are now partially resolved.

`RunLS10` says the LaserStream `drop` slice is **not** a hidden executable money lane. The saved audit at:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/20_g12_r0_observe_only/runls10_drop_semantics_summary.json
```

shows:

```text
drop_total = 226
drop_executable_count = 0
drop_no_trade_count = 226
drop_saved_loss_count = 226
drop_requires_egress_count = 0
drop_sender_ready = 37
drop_bundle_ready = 189
drop_pm_reserve_ok = 226
drop_realized_pnl_possible = 0
```

Interpretation:

```text
Drop is currently a non-trading saved-loss / suppression lane.
It may still be economically valuable, but it should not be counted as a live egress lane.
```

`RunLS9B` then audited the `41` LaserStream Backrun Sender-ready rows from LS9:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9b_laserstream_backrun_sender_pm_summary.json
```

Result:

```text
sender_subset_total = 41
sender_subset_no_send_accept_count = 41
sender_subset_pm_reserve_ok_count = 0
sender_subset_accept_without_pm_reserve_count = 41
result = failed_pm_reserve_gate
```

Interpretation:

```text
The LaserStream Backrun Sender subset is not ready for promotion.
It currently accepts no-send rows without proved PM reserve.
Do not discuss LaserStream Backrun live/Sender promotion until that PM/accounting gap is closed.
```

So the branch conclusion after LS10 + LS9B is:

```text
LaserStream Backrun no-send = proven
LaserStream Backrun bundle subset = cleaner
LaserStream Backrun Sender subset = blocked by PM/accounting proof gap
LaserStream Drop subset = saved-loss / suppression, not executable money lane
```

That branch was then tightened one level further with `RunLS9C` and `RunLS9D`:

```text
RunLS9C bundle subset
  laserstream_backrun_bundle_seen = 83
  laserstream_backrun_bundle_pm_ok = 83
  laserstream_backrun_bundle_envelope_built = 83
  laserstream_backrun_bundle_no_send_accept = 83
  laserstream_backrun_bundle_pm_release = 0
  blocker = pm_release_below_bundle_accept

RunLS9D Sender quarantine
  sender_subset_total = 41
  sender_pm_reserve_ok_count = 0
  sender_execution_no_send_accept_count = 0
  sender_research_only_accept_count = 41
  sender_blocked_pm_reserve_missing_count = 41
```

So the current operational rule is:

```text
LaserStream Backrun bundle subset = closest money lane, but not fully closed until PM release reconciles
LaserStream Backrun Sender subset = quarantine as research-only until PM reserve/accounting is proven
Drop = suppression / saved-loss analytics, not executable egress
```

That reconciliation gap is now closed for the no-send bundle proof.

Fresh PM/accounting correction:

```text
PM core fix:
  rollout-shadow observe-only rows with real reserved_notional
  now release through TerminalReason::ObserveOnlyNoSend

wrapper fix:
  RunLS9/20 wrapper now supports a short post-window drain
  used here as 2s to let final PM release counters flush
```

Fresh rerun on `.15` after that fix:

```text
RunLS9 rerun
  laserstream_backrun_seen = 701
  laserstream_backrun_no_send_eligible = 174
  laserstream_backrun_bundle_ready = 153
  laserstream_backrun_sender_ready = 21
  laserstream_backrun_pm_reserve_ok = 153
  laserstream_backrun_no_send_accept = 153
  laserstream_backrun_sender_execution_no_send_accept = 0
  laserstream_backrun_sender_research_accept_only = 21
  laserstream_backrun_blocked_by_source_policy_before = 174
  laserstream_backrun_blocked_by_source_policy_after = 0
  wire_send = 0
  live_landed = 0

RunLS9C rerun
  laserstream_backrun_bundle_seen = 153
  laserstream_backrun_bundle_pm_ok = 153
  laserstream_backrun_bundle_envelope_built = 153
  laserstream_backrun_bundle_no_send_accept = 153
  worker_pm_reservation_created = 153
  pm_reservation_released = 153
  blocker = none
```

So the branch state is now:

```text
LaserStream Backrun no-send = proven
LaserStream Backrun bundle subset = first fully reconciled no-send monetization lane
LaserStream Backrun Sender subset = still quarantined as research-only
LaserStream Drop subset = saved-loss / suppression only
```

`RunLS11` then moved the LaserStream Backrun bundle branch one level closer to a true private-bundle proof without submitting anything live.

Artifact root:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only
```

What the refreshed scoped readiness gate proved:

```text
run_status = completed
cleanup_verified = true
wire_send = 0
live_landed = 0
grpc_source_grade_configured = laserstream
runls11_bundle_probe_rows.jsonl rows = 28
selected_route_family = jito_bundle
bundle_envelope_built = true
selected_backrun_tx_template = jito_plain
selected_pm_reserve_ok = true
selected_expected_out_raw = 10329482098
selected_min_out_raw = 10226187277
selected_net_ev_after_tip = 2484055
selected_tip_lamports = 27012004
selected_target_relationship_recorded = true
selected_target_tx_bytes_available = true
selected_target_tx_bytes_source = captured_at_ingress
bundle_tx_count = 2
selected_pm_reserve_created = true
selected_pm_reserve_released = true
selected_pm_release_reason = observe_only_no_send
selected_pm_pending_after = 0
```

What the new `RunLS11D` target-capture proof added:

```text
selected_target_tx_decodes_locally = true
selected_target_tx_decoded_len_bytes = 743
selected_target_signature_matches = true
selected_target_pool_matches = true
selected_target_mint_in_matches = true
selected_target_mint_out_matches = true
selected_target_account_keys_count = 15
selected_target_program_ids_count = 10
```

So the ordered victim/backrun bundle artifact is now real in no-submit mode, and the blocker list has narrowed:

```text
1. target transaction capture / preservation:
   solved for the selected class by preserving target tx bytes at ingress/export time

2. host Jito auth readiness:
   still blocked; the host can reach Jito endpoints and has tip accounts, but it does not yet have
   usable Jito auth material at the expected path
```

Provider-readiness now resolves into two truths:

```text
RunLS11C rerun:
  provider_submit_probe = http_404
  provider_status_probe = http_405
  provider_auth_material_present = false

RunAUTH0:
  provider = jito_direct
  auth_source = jito_keypair
  tip_accounts_available = true
  tip_accounts_count = 1
  jito_auth_path_present = false
```

Those HTTP codes are acceptable here because they show the Jito endpoints are reachable in
no-submit mode. The branch is no longer blocked by generic network reachability or by missing
target bytes; it is blocked by missing private-bundle auth material.

One artifact-join caveat also surfaced and is now handled explicitly:

```text
selected_source_class_venue = meteora_damm_v2
selected_venue = raydium_cpmm
```

The same route/pool/mint class can present with venue-label drift between older LS9C audit rows
and fresh LS11 probe rows, so the current `RunLS11` matcher keys on:

```text
route family + pool + mint_in + mint_out
```

rather than venue text alone.

So the branch state after the refreshed `RunLS11` / `RunLS11D` / `RunAUTH0` chain is:

```text
LaserStream Backrun bundle = still the leading monetization branch
Shadow-side ordered no-submit bundle artifact = proven
target-linked ordered bundle = proven in no-submit mode
selected-scoped PM reserve/release = proven
next blocker = host Jito auth staging before any controlled live proof
```

`RunAUTH1` then resolved the first-provider path without requiring direct Jito keypair auth.

Artifacts:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only/runauth1_private_bundle_provider_matrix.json
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only/runauth1_private_bundle_provider_rows.jsonl
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only/runauth1_selected_provider.json
```

Current provider matrix:

```text
first_passing_provider = jito_via_helius
first_passing_provider_auth_source = helius_rpc_api_key
passing_provider_count = 1
auth_material_present = true
tip_accounts_available = true
tip_accounts_count = 11
status_api_reachable = true
simulate_bundle_available = true
send_bundle_disabled_in_test = true
send_bundle_called = false
wire_send = 0
live_landed = 0
```

The selected-provider `RunLS11C` rerun is also green:

```text
provider = jito_via_helius
provider_auth_source = helius_rpc_api_key
selected_target_tx_bytes_available = true
bundle_envelope_built = true
bundle_tx_count = 2
tip_present = true
min_out_positive = true
net_ev_after_tip_positive = true
selected_pm_reserve_created = true
selected_pm_reserve_released = true
provider_auth_material_present = true
provider_status_api_reachable = true
simulate_bundle_available = true
send_bundle_disabled_in_mode = true
wire_send = 0
live_landed = 0
```

Direct Jito remains auth-red (`jito_auth_path_present=false`), but it is no longer the first-proof blocker because Helius-mediated Jito bundles are authenticated and reachable. `RunLS12` is still not approved until target freshness is proven.

`RunLS11E` was added as the final hard pre-live gate:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only/runls11e_target_freshness_bundle_eligibility.json
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only/runls11e_target_freshness_bundle_eligibility_rows.jsonl
```

The current selected artifact fails closed:

```text
selection_result = blocked_target_already_landed
target_tx_bytes_available = true
target_tx_decoded_len_bytes = 743
target_signature_status_before_bundle.confirmationStatus = finalized
target_already_landed = true
target_recent_blockhash_valid = false
bundle_tx_count = 2
target_tx_first = true
our_backrun_tx_second = true
tip_present = true
simulate_bundle_available = true
simulateBundle_pass = false
sendBundle_called = false
wire_send = 0
live_landed = 0
```

This is the correct blocker. The Shadow-side bundle artifact is real, but it was built from a processed/finalized target row, so it is not a true live backrun bundle candidate. The next build is:

```text
LaserStream preprocessed target feed
```

or:

```text
AF_XDP target trigger + LaserStream enrichment
```

`RunLS13` now exists for the first branch:

```text
g1-host-pack/scripts/21_runls13_laserstream_preprocessed_target_freshness.sh
target/release-guerrila/runls13-preprocessed-probe
```

The first `.15` proof run was safe but blocked:

```text
selection_result = blocked_preprocessed_subscription_connect_failed
source = laserstream_preprocessed
preprocessed_subscription_connect_failed = true
preprocessed_target_seen = false
target_tx_bytes_available = false
bundle_tx_count = 0
provider = jito_via_helius
provider_auth_material_present = true
provider_status_api_reachable = true
simulate_bundle_available = true
sendBundle_called = false
wire_send = 0
live_landed = 0
```

Artifacts:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls13_laserstream_preprocessed_target_freshness/window1/runls13_preprocessed_target_freshness_summary.json
/home/ubuntu/Guerrila/g1-host-pack/results/21_runls13_laserstream_preprocessed_target_freshness/window1/runls13_preprocessed_target_freshness_rows.jsonl
```

Corrected root cause: the LaserStream URL/token shape is right. The dashboard gives:

```text
endpoint = https://laserstream-mainnet-ewr.helius-rpc.com
auth = x-token API key metadata
```

Do **not** append `?api-key=` to the LaserStream gRPC URL. The direct LS13 diagnostic now proves the endpoint is reachable, then the server rejects `SubscribePreprocessed`:

```text
direct_subscribe_probe.connect_ok = true
direct_subscribe_probe.error_stage = subscribe_preprocessed_initial_request
direct_subscribe_probe.error_code = PermissionDenied
direct_subscribe_probe.error_message = Unsupported plan type
```

This same error appears for selected-pool, no-filter, and JUP-style account filters, so the blocker is not filter syntax. Current reading: the Helius-mediated private-bundle route is still green, standard LaserStream is still usable, but preprocessed transactions / Shred Delivery are plan-entitlement blocked for this key. Do **not** infer anything about preprocessed target freshness yet. Next step is to enable/upgrade Helius preprocessed transaction access for this API key, then rerun LS13. If product access is not available, pivot to:

```text
AF_XDP target trigger + LaserStream enrichment
```

That fallback is now a first-class branch:

```text
RunLS14 = Hybrid Backrun join proof
AF_XDP = fresh target trigger / target tx byte capture
LaserStream processed = enrichment, recurrence, route/EV evidence
egress = no-submit bundle readiness only
```

Implementation note: `shadow-tier0` now records decoded AF_XDP target raw transaction bytes into `shadow_core::target_tx_cache`, so the bundle builder can use AF_XDP as the freshness source while preserving the LaserStream Backrun enrichment lane. The host wrapper is:

```text
g1-host-pack/scripts/21_runls14_hybrid_backrun_join_proof.sh
```

It writes:

```text
runls14_hybrid_backrun_join_rows.jsonl
runls14_hybrid_backrun_join_summary.json
```

and stays no-live by construction:

```text
sendBundle_called = false
wire_send = 0
live_landed = 0
```

Passing `RunLS14` only means the selected hybrid candidate is ready for `RunLS11E`. It does not approve LS12.

`RunLS15` then tested the inline hybrid freshness gate and produced the right negative result: the wrapper can detect/judge candidates inline, but if processed LaserStream enrichment is required before final bundle eligibility, the target is still already processed by provider simulation time. The representative blocker was:

```text
simulateBundle = "This transaction has already been processed"
sendBundle_called = false
wire_send = 0
live_landed = 0
```

Supervisor conclusion after LS15: processed LaserStream must not sit on the freshness-critical path for true ordered live Backrun. It remains valuable for enrichment, recurrence, accounting, markout, training, and post-shock research. The live ordered Backrun critical path must be:

```text
AF_XDP target trigger
-> immediate local projection
-> immediate bundle build
-> Jito via Helius
```

That led to the current active branch:

```text
RunAFBR1 = AF_XDP-only Backrun projection and bundle-readiness proof
```

Current `.15` AFBR1 artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/21_runafbr1_afxdp_backrun_projection_bundle_readiness/window1/20_g12_r0_observe_only/runafbr1_summary.json
```

Early AFBR1 remained no-live and failed closed before the target-entity/projection repair:

```text
historical_status = blocked_projected_quote_missing
sendBundle_called = false
wire_send = 0
live_landed = 0
worker_signal_afxdp = 120
r1_afxdp_private_grade_route_actionable = 10
r1_afxdp_private_grade_process_intent_entered = 9
shock_afxdp_rows = 19
shock_afxdp_backrun_rows = 19
shock_afxdp_backrun_bundle_rows = 9
shock_afxdp_backrun_bundle_positive_expected_out_rows = 0
shock_afxdp_backrun_bundle_positive_min_out_rows = 0
shock_afxdp_backrun_bundle_positive_ev_rows = 0
shock_afxdp_backrun_bundle_pm_ok_rows = 0
shock_afxdp_backrun_bundle_target_entity_rows = 0
bundle_probe_rows_seen = 0
```

Historical `RunAFBR1-repair` telemetry update (`2026-05-24`, 30s no-submit rerun on `.15`) added row-level projection autopsy output:

```text
artifact = /home/ubuntu/Guerrila/g1-host-pack/results/21_runafbr1_afxdp_backrun_projection_bundle_readiness/window1/20_g12_r0_observe_only/runafbr1_projection_autopsy_rows.jsonl
historical_status = blocked_projected_quote_missing
sendBundle_called = false
wire_send = 0
live_landed = 0
observe_candidates = 357
private_grade_ingress = 45
bundle_probe_rows_seen = 0
projection_autopsy_row_count = 3
historical_projection_autopsy_blocker_counts = {"blocked_target_entity_missing": 3}
shock_afxdp_rows = 6
shock_afxdp_backrun_rows = 6
shock_afxdp_backrun_bundle_rows = 3
shock_afxdp_backrun_bundle_target_entity_rows = 0
shock_afxdp_backrun_bundle_positive_expected_out_rows = 0
shock_afxdp_backrun_bundle_positive_min_out_rows = 0
shock_afxdp_backrun_bundle_positive_ev_rows = 0
shock_afxdp_backrun_bundle_pm_ok_rows = 0
```

The autopsy rows were all AF_XDP Backrun-shaped `jito_bundle` / `meteora_damm_v2` rows with `t0_exhaust_candidate=true`, but each failed before projection:

```text
target_entity_present = false
target_tx_bytes_present = false
target_pool_matches = false
pre_target_pool_state_present = false
projected_post_target_pool_state_present = false
expected_out_raw = 0
min_out_raw = 0
net_ev_after_tip = 0
pm_reserve_sim_ok = false
bundle_candidate_ready = false
historical_first_projection_blocker = blocked_target_entity_missing
```

This was the exact blocker at the time: not provider auth, not Jito/Helius egress, not PM release, and not bundle envelope shape. It has since been repaired through the AFBR1 target-entity/projection chain. AFBR-H now reports `BACKRUN_STRUCTURAL_HUNT_READY`, so do not use this historical projection blocker to reopen target relationship, projected quote, PM, provider, or bundle-shape work.

The current authoritative AFBR1H distribution is:

```text
script = 21_afbr1h_immediate_bundle_ready_freshness_batch.sh
status = SOURCE_LATE_DOMINANT
valid_afbr1h_sample_count = 10
freshness_class_counts = {"SOURCE_LATE": 10}
fresh_at_bundle_ready_count = 0
status_unknown_count = 0
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

Do not run LS12 or AFBR2 from this state. AFBR1H has now provided the immediate-probe distribution: `SOURCE_LATE_DOMINANT`.

The structural certificate for this branch is now explicit:

```text
RunAFBR-H = BACKRUN_STRUCTURAL_HUNT_READY certificate
script = g1-host-pack/scripts/21_afbrh_backrun_structural_hunt_readiness.sh
```

Backrun is structurally HUNT-ready while that certificate can prove, from no-submit artifacts:

```text
target_tx_bytes_available = true
target freshness measured at bundle-ready time
target_recent_blockhash_valid = true
target relationship populated
pre_target_pool_state populated
projected_post_target_pool_state populated
expected_out_raw > 0
min_out_raw > 0
net_ev_after_tip > 0
selected_pm_reserve_ok = true
selected_pm_release_on_no_send_ok = true
bundle_tx_count = 2
target_tx_first = true
our_backrun_tx_second = true
tip_present = true
provider_auth_material_present = true
provider_status_api_reachable = true
simulate_bundle_available = true
sendBundle_called = false
wire_send = 0
live_landed = 0
```

Current intended honest read is:

```text
status = BACKRUN_STRUCTURAL_HUNT_READY
main_blocker = immediate_target_freshness_source_timing
```

So the next Backrun work is earlier target availability / source timing, not AFBR1 projection repair, not habitat search, and not live bundle submission.

Next engineering branch:

```text
Backrun source-timing branch
  - pursue earlier target availability
  - Helius preprocessed / Shred Delivery entitlement is the cleanest next source route if available
  - otherwise audit earlier AF_XDP trigger/source topology
  - keep native AF_XDP zero-copy posture
  - preserve no-submit/no-send discipline
  - do not reopen projection/PM/provider/bundle unless AFBR-H regresses
```

Only after AFBR1 passes should the team discuss a strictly capped `RunAFBR2` / `RunLS12` one-bundle live plan:

```text
max_attempts = 1
max_inflight = 1
bundle route only
no Sender fallback
no DirectArb distraction
terminal bundle status required
PM reconcile mandatory
hard tip/loss cap
```

## 2026-05-24 Run66U repeatability state

Historical leading branch at that time, superseded by the 2026-05-27 certificate amendment:

```text
Post-shock / rebalancing conversion.
Backrun remains strategically real but parked behind AFBR1 projection repair.
No live send is approved.
```

Current corrected state: Rebalancing is execution-plumbing ready only, with `proposal_eligible_rebalancing_rows=0`; next work is `66RG2` owner-selector instrumentation repair.

Fresh host work on `.15` added the repeatability chain:

```text
g1-host-pack/scripts/66u_run_repeatability_chain.sh
g1-host-pack/scripts/66u_ata_repeatability_aggregate.sh
```

The chain runs fresh no-send epochs, then replays:

```text
Run65 -> Run66Q -> Run66X -> Run66Q2 -> Run66 rule proof
-> Run66R readiness overlay -> Run66R-ATA planner -> Run66T policy audit
-> Run66U aggregate
```

Important host artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66u_ata_repeatability_aggregate_20260524_3x5m_retry3_final/
```

Final aggregate:

```text
status=run66u_ata_repeatability_aggregate_complete
source_epoch_count=3
source_row_count=15
mint_group_count=5
decision_counts={"REJECT_DUST_EXIT":1,"REJECT_RENT_NEGATIVE":4}
one_ata_plan_review_count=0
retry_more_epochs_count=0
reject_dust_exit_count=1
reject_rent_negative_count=4
reject_policy_count=0
wire_send=0
live_landed=0
```

Top repeated mint:

```text
mint=8FSVipViijpT4aE86e58PYRd73bQdZtyzng4FT2THSTT
symbol=DROPEE
decision_after_repeatability=REJECT_RENT_NEGATIVE
epoch_seen_count=3
total_row_count_across_epochs=19
total_synthetic_ata_delta_across_epochs=19
total_rent_adjusted_ev=-5336272 lamports
full_band_exit_min_usdc=15.924327
full_band_exit_median_usdc=23.770404
```

Interpretation:

```text
The surface repeated.
The action did not.

ATA-only synthetic conversion is real across fresh samples,
but no mint currently clears rent-adjusted economics and exit/policy gates.
```

The third sample required a replacement epoch because the original epoch 3 hit a pre-run Agave/local RPC health block. The replacement window captured fresh evidence and then hung in post-window Agave restore; the final Run65 wrapper JSON was materialized from its completed fresh Run39/Run61 artifacts so the no-send 66 chain could run without using old market rows.

Hard stops remain:

```text
No live send.
No sendBundle.
No ATA creation.
No acquisition.
No Token-2022 explicit allow.
No HyDK creation.
Do not call modeled missed EV realized profit.
```

Next work:

```text
1. Keep Run66U as the current post-shock repeatability reference.
2. Do not create DROPEE or any other ATA from this result.
3. Improve rent-adjusted edge or find cleaner repeated mints.
4. Repair quote_request_id bridge as technical debt; ingress_identity exact matching is currently the proof path.
5. Treat Agave recovery slowness as host-health friction for future epoch automation.
```

## 2026-05-24 Continuation / held-input update

Continuation is now the cleaner readiness branch after the rebalancing ATA-action branch failed economics.

New local/host script:

```text
g1-host-pack/scripts/66c_continuation_readiness_planner.sh
```

It was run across the three fresh Run66U sample artifacts.

Aggregate continuation result:

```text
continuation_rows_seen=330
continuation_numeric_quote_min_out=243
continuation_net_ev_after_fee_tip_positive=242
exact_quote_match_count=226
matched_rollout_net_ev_after_fee_tip_positive_count=66
exact_positive_continuation_rows=63

baseline_would_fire=0
synthetic_ata_overlay_would_fire=0
synthetic_inventory_overlay_would_fire=58
combined_synthetic_readiness_would_fire=58

ata_only_convertible_exact_positive_rows=0
inventory_only_convertible_exact_positive_rows=58
wire_send=0
live_landed=0
```

Interpretation:

```text
Continuation has exact-identity, min_out-backed, route-cost-positive rows.
It is not an ATA/Token-2022/rent branch right now.
The clean blocker is held-input inventory readiness.
```

Run65B signed-swap simulation was also advanced from plan-only to actual signed simulation.

New script:

```text
g1-host-pack/scripts/65b_sim2_signed_swap_simulation.sh
```

Host artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim2_signed_swap_simulation_20260524_5m/
```

Result:

```text
row_count=3
signer_staged=true
python_deps_ok=true
swap_tx_built_count=3
swap_tx_signed_for_simulation_count=3
simulate_transaction_called_count=3
signed_swap_simulation_passed_count=3
simulation_ready_count=3
decision_counts={"SIMULATE_ONE_MINT_READY":3}
send_transaction_called=false
wire_send=0
live_landed=0
```

Top simulated mints:

```text
1. AeVzGq6z1RndB9Tk9dGT6RPADjZJpQwX6tudqSRkmAgp
   required_acquisition_usdc=56.716678299
   exit_value_usdc=56.623628
   expected_would_fire_delta=13
   simulate_transaction_ok=true

2. 2ACEWAyABfBB66w24XvbU3t9HNoRbMdszHQmEG8zEjfi
   required_acquisition_usdc=86.590679881
   exit_value_usdc=86.395227
   expected_would_fire_delta=8
   simulate_transaction_ok=true

3. vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump
   required_acquisition_usdc=7.55475554
   exit_value_usdc=7.528627
   expected_would_fire_delta=61
   simulate_transaction_ok=true
```

Current supervisor truth:

```text
Rebalancing ATA conversion:
  real surface, repeated, not action-safe after rent/liquidity.

Continuation:
  STRUCTURAL_HUNT_READY
  CURRENT_SELECTED_REPLAY_PARKED
  blocker=fresh_quote_identity_drift / negative fresh roundtrip
  replay_verdict=PARK_SELECTED_REPLAY_FRESH_QUOTE_DRIFTED
  next=same-window fresh replay + decay-window report

65B held-input overlay:
  top 3 now pass signed swap simulation, but remain simulation-ready only.

Still prohibited:
  no live send
  no sendBundle
  no ATA creation
  no acquisition
  no Token-2022 explicit allow
```

### 2026-05-24 Inventory planner follow-up

Two no-send lifecycle scripts were added and run on `.15`:

```text
g1-host-pack/scripts/66ci_continuation_held_input_inventory_planner.sh
g1-host-pack/scripts/65b_sim3_inventory_lifecycle_review.sh
```

Broad continuation inventory planner artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66ci_continuation_held_input_inventory_planner_20260524_3x5m/
```

Result:

```text
source_inventory_only_rows=58
mint_group_count=12
inventory_sim_review_count=0
decision_counts={"REJECT_DUST_EXIT":2,"REJECT_LOW_EV":3,"REJECT_POLICY":7}
acquisition_allowed=false
live_send_allowed=false
wire_send=0
live_landed=0
```

Interpretation:

```text
The broad continuation inventory-only surface is real,
but no grouped held-input mint clears policy + exit liquidity + EV checks yet.
```

65B lifecycle artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim3_inventory_lifecycle_review_20260524_5m/
```

Result:

```text
row_count=3
lifecycle_review_count=3
signed_swap_simulation_passed_count=3
auto_flatten_available_count=3
PM_reserve_sim_ok_count=0
acquisition_allowed=false
live_send_allowed=false
wire_send=0
live_landed=0
```

65B PM inventory reserve artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim4_pm_inventory_reserve_review_20260524_5m/
```

Result:

```text
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
acquisition_allowed=false
live_send_allowed=false
wire_send=0
live_landed=0
```

65B vRse one-mint synthetic replay artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim5_one_mint_conversion_replay_20260524_5m/
```

Result:

```text
status=run65b_sim5_one_mint_conversion_replay_blocked_no_vrse_continuation_rows
target_mint=vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump
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
first_replay_blocker=blocked_no_vrse_continuation_rows
decision=SYNTHETIC_REPLAY_BLOCKED
send_transaction_called=false
sendBundle_called=false
wire_send=0
live_landed=0
```

SIM5 owner-gated top-3 result:

```text
owner_mode_gate_added=true
target_owner_mode=continuation

vRse:
  decision=DROP_ONLY_RESEARCH
  would_fire_delta=61
  continuation_would_fire_delta=0
  drop_would_fire_delta=61
  pm_reserve_ok=true
  pm_release_ok=true

AeVz:
  decision=DROP_ONLY_RESEARCH
  would_fire_delta=13
  continuation_would_fire_delta=0
  drop_would_fire_delta=13
  pm_reserve_ok=true
  pm_release_ok=true

2ACE:
  decision=DROP_ONLY_RESEARCH
  would_fire_delta=8
  continuation_would_fire_delta=0
  drop_would_fire_delta=8
  pm_reserve_ok=true
  pm_release_ok=true

send_transaction_called=false
sendBundle_called=false
wire_send=0
live_landed=0
```

Run66V inline ATA lifecycle artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66v_inline_ata_lifecycle_model_20260524_5m_bridge2/
```

Result:

```text
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
wire_send=0
live_landed=0
ata_created=false
```

Run66CI2 owner-aligned continuation instrument search artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66ci2_owner_aligned_continuation_instrument_search_20260524_3x5m/
```

Result:

```text
exact_positive_continuation_rows=63
owner_aligned_inventory_only_rows=58
instrument_group_count=12
decision_counts={"REJECT_DUST_EXIT":2,"REJECT_LOW_EV":3,"REJECT_POLICY":7}
continuation_instrument_candidate_count=0
acquisition_allowed=false
wire_send=0
live_landed=0
```

Run66V policy-clean inline ATA scan artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66v_policy_clean_inline_ata_scan_20260524_5m_bridge2/
```

Result:

```text
source_row_count=39
policy_clean_row_count=0
policy_clean_inline_ata_candidate_count=0
wire_send=0
live_landed=0
ata_created=false
```

Run66CORE core-inventory-only post-shock scan artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66core_core_inventory_postshock_scan_20260524_3x5m/
```

Result:

```text
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
wire_send=0
live_landed=0
send_transaction_called=false
sendBundle_called=false
ata_created=false
```

Top row:

```text
owner_mode=rebalancing
venue=raydium_clmm
pool=3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF
mint_in=So11111111111111111111111111111111111111112
mint_out=Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB
matched_rollout_net_ev_after_fee_tip_raw=79889
min_out_raw=18785531
inventory_ready=true
account_ready=false
pm_reserve_sim_ok=true
first_blocker=account_ata_missing
```

Run66EA existing-account-only post-shock scan artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66ea_existing_account_postshock_scan_20260524_3x5m/
```

Result:

```text
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
send_transaction_called=false
sendBundle_called=false
ata_created=false
```

Run66V2 policy-clean inline one-shot artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66v2_policy_clean_inline_one_shot_20260524_5m_bridge2/
```

Result:

```text
source_row_count=39
policy_clean_row_count=0
policy_clean_inline_one_shot_count=0
wire_send=0
live_landed=0
send_transaction_called=false
sendBundle_called=false
ata_created=false
```

Run66CORE-USDT plan-only review artifact:

```text
/home/ubuntu/Guerrila/g1-host-pack/results/66core_usdt_plan_only_review_20260524_3x5m/
```

Result:

```text
usdt_core_row_count=1
usdt_ata_present=false
token_program_id=TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
ata_rent_lamports=2039280
core_ata_plan_review_count=0
park_low_ev_or_one_off_count=1
wire_send=0
live_landed=0
ata_created=false
```

Top row:

```text
pool=3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF
venue=raydium_clmm
exact_quote_match=true
min_out_raw=18785531
matched_rollout_net_ev_after_fee_tip_raw=79889
net_ev_after_route_and_ata_burned_raw=-1959391
synthetic_usdt_ata_would_fire_delta=1
repeat_observed_count=1
pm_reserve_sim_ok=true
decision=PARK_LOW_EV_OR_ONE_OFF
```

Interpretation:

```text
Inline ATA recovered-rent economics are interesting,
but there are zero policy-actionable candidates.

66W template no-send is not opened because the positive rows require
Token-2022 explicit allow, and explicit allow is still prohibited.

The new usable-now scans found one core-pair exact-positive row,
but it is still account/ATA blocked.
Existing-account-only post-shock would-fire is zero.
Policy-clean inline one-shot count is zero.
The narrow WSOL->USDT plan-only review also parks:
the row is real and synthetic ATA delta is 1,
but ATA rent dominates the current row EV and it was observed only once.
```

Top lifecycle candidate:

```text
mint=vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump
expected_unlocked_would_fire_delta=61
max_inventory_at_risk_usdc=7.55475554
roundtrip_loss_usdc=0.02612854
net_after_inventory_carry_cost_usdc=0.12078146
exposure_efficiency_delta_per_usdc=8.074384363
required_inventory_raw=286475131
pm_reserve_inventory_band_ok=true
pm_release_on_no_send_ok=true
pm_pending_after=0
```

Updated supervisor truth:

```text
Continuation broad inventory branch:
  real but fail-closed by policy / dust / low EV.

65B held-input branch:
  top three mints now pass signed swap simulation, lifecycle review,
  and PM synthetic inventory reserve/release in no-send mode.
  top three owner-gated SIM5 replays are all Drop-only research:
  vRse drop=61, AeVz drop=13, 2ACE drop=8, continuation=0 for all.

Rebalancing inline ATA branch:
  recovered-rent economics can be positive,
  but policy_actionable_now_count=0 and explicit_allow_required_count=38.

Owner-aligned Continuation instrument search:
  exact-positive continuation rows exist,
  but candidate_count=0 after policy / dust / EV gates.

Policy-clean inline ATA scan:
  candidate_count=0.

Core / existing-account usable-now scans:
  one core-pair exact-positive rebalancing row exists,
  but it is account_ata_missing.
  existing_account_would_fire=0 across all 107 exact-positive rows.
  policy_clean_inline_one_shot_count=0.
  66CORE-USDT plan-only review = PARK_LOW_EV_OR_ONE_OFF.

Still missing:
  a held-input mint with continuation_would_fire_delta > 0
  repeatability / class-level recurrence justification
  explicit acquisition caps and TTL/flatten policy
  a policy-actionable inline ATA candidate
  a no-new-mint / no-new-ATA / no-acquisition / no-explicit-allow candidate

Run77B wrapper repair state:

```text
The local/host wrapper drift is fixed.
SHADOW_RUN77B_TPU_QUIC_WORKERS now defaults to 2.
Run77B now requires quic_workers>=2.
Override 0 -> blocked_quic_workers_zero.
Override 1 -> blocked_quic_workers_too_low.
No native lease was armed as part of the repair.
```

Still prohibited:
  no acquisition
  no live send
  no sendBundle
  no ATA creation
  no Token-2022 explicit allow
```

Launch B002 freeze update (`2026-06-03`):

```text
milestone=B002_FROZEN_BASELINE
freeze_manifest=/home/ubuntu/Guerrila/g1-host-pack/results/22_launch_b002_freeze_manifest/manifest.json
replay_summary=/home/ubuntu/Guerrila/g1-host-pack/results/22_launch_b002_replay_frozen/summary.json
regression_matrix=/home/ubuntu/Guerrila/g1-host-pack/results/22_launch_b002_regression_matrix_20260603T070540Z/summary.json

attempts_landed=3
attempts_flattened=3
attempts_rent_recovered=3
exit_reason=EXIT_COST_PROTECTOR_TRAILING
total_pnl_after_rent_recovery=4527956
no_exit_tail_count=0
manual_intervention_required_count=0
open_exposure_count=0
pm_pending_after=0
```

Operational rule: Launch live money must use `22_launch_b002_live_clone_runner.sh` or `22_launch53_r3_batch002_frozen_runner.sh`. R4 broad/clone/HOLD-V2 work remains lab-only until it beats the frozen B002 lane and passes `22_launch_b002_regression_matrix.sh`.
