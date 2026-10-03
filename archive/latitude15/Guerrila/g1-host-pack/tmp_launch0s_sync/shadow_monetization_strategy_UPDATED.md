# Shadow Monetization Strategy Checklist

**Purpose:** Turn Shadow from a detector into a continuous monetization engine. This checklist defines the required proofs, runs, data artifacts, and acceptance gates before capital scaling or real live execution.

**Primary directive:**

```text
No failure. Shadow must work.
Every blocker becomes the next conversion target.
```

**Current strategic diagnosis:**

```text
Shadow sees significant market structure.
Architecture is green enough to proceed.
The economic blocker is conversion:
observed opportunity -> executable TPU/public trade -> realized/reconciled PnL.
```

**Launch R2 live milestone (`2026-06-01`):**

```text
record_as = LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED
technical_status = LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN
strategy_status = NOT_A_STRATEGY_DURATION_TEST
pnl_status = SHORT_VALIDATION_COST_DOMINATED
primary_blocker_hit = PUMPSWAP_DIRECT_SELL_ROUTE_SHAPE
next_gate = R3_MONITORED_POSITION_PRIMARY_FLATTEN
```

The `.15` Launch hot R2 press window at `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_live_20260601T061754Z` is a valid live-path milestone, not a clean profitable launch and not a fair strategy-duration test. It proved the signer path, Helius Sender late-follow entry, landed acquisition, inventory refresh, rescue flatten, and PM/ledger return-to-flat. The acquired mint was `CLcqcknXPdYyFFA3w8JHvNkywZouKQrhv4B48avg5kdr`; acquisition landed with signature `hzPZKAwXTtnnhpHp68jtSieeuVWjvRZWXdXojndWZQjhqh1w3XEGjPSAfp9xnnkcVPfTh1C7bHBWa2UzC9bub4H`; the primary flatten failed with `flatten_sim_failed: Custom 3007`; rescue flatten landed with signature `yYqWb3vEcpsUE8ZeNvaCmXaRqVribV2SndWobrTWfGvctzyETq591erD1WzNab4Pu6QFmuJRjSYNxpBvPHcAa8v`; final inventory is `0`; daily ledger open exposure is `0`; ledger status is `LAUNCH_RESCUE_FLATTENED`.

Cost autopsy source of truth:

```text
gross_price_pnl_lamports = +135527
buy_sender_tip_lamports = -200000
sell_sender_tip_lamports = -200000
buy_base_fee_lamports = -5000
sell_base_fee_lamports = -5000
pumpfun_fee_lamports = -3068
slippage_loss_lamports = -16930
pump_account_setup_lamports = -2039280
pump_account_setup_recoverable = false
native_fixed_drag_lamports = -2449280
total_realized_pnl_lamports = -2313753
```

The gross price move was positive, but the deliberately short validation round trip was fixed-cost dominated. For this cost profile, the no-margin gross edge floor is about `2.45M` lamports before any safety margin. Do not classify R2 as a profitable launch, but also do not classify it as strategy failure. It did not test the intended strategy duration.

**Launch0Q quote-state hydration no-wire milestone (`2026-06-02`):**

```text
record_as = LAUNCH0Q_QUOTE_STATE_HYDRATION_NO_WIRE_PASS
technical_status = QUOTE_STATE_HYDRATION_PASS_NO_WIRE
live_status = PAUSED
wire_send = 0
live_landed = 0
cleared_blocker = ENTRY_QUALITY_MISSING_QUOTE_STATE
current_primary_blockers = QUOTE_ROUTE_UNSUPPORTED, entry_quality_warmup
next_gate = LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE
```

The `.15` no-wire coverage root `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z` now proves the Launch coverage path can hydrate executable quote/liquidity state before entry-quality evaluation. The 600s pass produced `unique_launch_states=566`, `trigger_b_rows=343`, `quote_state_ready_rows=166`, `exit_liquidity_guard_ready_rows=166`, `entry_quality_evaluated_rows=343`, `entry_quality_ready_rows=7`, `entry_edge_ready_rows=6`, and `fragility_absorption_ready_rows=165`, with `wire_send=0` and `live_landed=0`. Treat this as no-wire pipeline proof only, not live readiness. The next work is route unsupported taxonomy and warmup calibration; do not resume R3 live until a fresh no-wire run still has positive quote-state, entry-quality, entry-edge, and fragility rows under the calibrated gates.

**Launch0R route-support / warmup reconcile milestone (`2026-06-02`):**

```text
record_as = LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE_PASS
technical_status = ROUTE_SUPPORT_AND_WARMUP_DIAGNOSTIC_PASS_NO_WIRE
live_status = PAUSED
wire_send = 0
live_landed = 0
acceptance_pass = true
unsupported_route_split = PUMP_BONDING_CURVE_BUY=212, PUMP_BONDING_CURVE_SELL=121
warmup_route_split = PUMP_BONDING_CURVE=230, PUMPSWAP_AMM=19
current_primary_blocker = PUMP_BONDING_CURVE_QUOTE_SUPPORT
current_secondary_blocker = WARMUP_CALIBRATION
next_gate = LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE
```

Launch0R narrows the Launch0Q blockers to one dominant route family. The former generic `QUOTE_ROUTE_UNSUPPORTED=333` bucket is fully attributed to `PUMP_BONDING_CURVE`, split by direction as `PUMP_BONDING_CURVE_BUY=212` and `PUMP_BONDING_CURVE_SELL=121`. Warmup is also route-attributed: `warmup_blocked_rows=249`, with `PUMP_BONDING_CURVE=230` and `PUMPSWAP_AMM=19`. Do not tune warmup in isolation until Pump bonding-curve quote support exists. Regression rule: Launch0R is no-wire only; `wire_send=0`, `live_landed=0`, route-family breakdown, direction split, and warmup route-family breakdown are required before this milestone can be considered valid.

**Launch0S Pump bonding-curve quote support no-wire milestone (`2026-06-02`):**

```text
record_as = LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE_PASS
technical_status = PUMP_BONDING_CURVE_QUOTE_SUPPORT_PASS_NO_WIRE
live_status = PAUSED
wire_send = 0
live_landed = 0
acceptance_pass = true
pump_bonding_curve_buy_quote_rows = 53
pump_bonding_curve_sell_quote_rows = 38
pump_bonding_curve_ready_rows = 44
quote_route_unsupported_pump_bonding_curve_after = 0
next_gate = LAUNCH0T_WARMUP_CALIBRATION_REPLAY
```

Launch0S adds Pump bonding-curve buy/sell quote hydration and clears the generic Pump bonding-curve unsupported bucket. The same 600s root now reports `quote_state_ready_rows=205`, `entry_quality_ready_rows=19`, `entry_edge_ready_rows=11`, and `fragility_absorption_ready_rows=202`. Remaining Pump bonding-curve blockers are specific and actionable: `BONDING_CURVE_COMPLETE_OR_MIGRATED=171`, `BONDING_CURVE_STATE_MISSING=65`, `BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR=46`, `BONDING_CURVE_RESERVES_MISSING=6`, and `BONDING_CURVE_PRICE_IMPACT_TOO_HIGH=1`. This does not authorize live trading; it authorizes Launch0T warmup calibration replay over hydrated Pump bonding-curve rows.

**Launch0T warmup calibration replay no-wire milestone (`2026-06-02`):**

```text
record_as = LAUNCH0T_WARMUP_CALIBRATION_REPLAY_PASS
technical_status = R3_HOLD_WARMUP_READY_NO_WIRE
live_status = PAUSED
wire_send = 0
live_landed = 0
acceptance_pass = true
best_entry_bucket = 10-30s
normal_warmup_min_age_ms = 10000
entry_quality_ready_rows = 32
entry_edge_ready_rows = 18
exit_liquidity_guard_ready_rows = 205
top_non_ready_quote_state_blocker = BONDING_CURVE_COMPLETE_OR_MIGRATED
next_gate = LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN
```

Launch0T answers the strategy question as a HOLD warmup replay, not a sub-second scalp proof. The recommended rule is normal warmup at `10-30s` / `10000ms`, with early override allowed only when `quote_state_ready=true`, `exit_liquidity_guard_ready=true`, `entry_edge_ready=true`, `exit_quote_nonzero=true`, and price impact is within limit. Markout fields are currently missing from this static coverage root, so do not infer profit duration from Launch0T alone. Since `BONDING_CURVE_COMPLETE_OR_MIGRATED=171` is the top non-ready blocker, the next route repair is migration/PumpSwap join before any broad R3 live resumption.

R3 monitored-position validation gate:

```text
acquisition_landed = true
position_state = POSITION_OPEN -> MONITORING -> EXIT_TRIGGERED
entry_value_lamports captured
peak_exit_value_lamports captured
drop_from_entry_bps captured
drawdown_from_peak_bps captured
drop_1s_bps/drop_2s_bps/drop_5s_bps captured
exit_price_impact_bps captured
exit_slippage_bps captured
pool_liquidity_drop_bps captured
exit_trigger_reason in configured Launch-exit triggers
primary_flatten_sim_ok = true
primary_flatten_landed = true
rescue_flatten_used = false
final_inventory = 0
ledger_open_exposure = 0
wire_send_expected = true
```

Launch R3 exit model:

```text
Do not force immediate exit after acquisition.
After acquisition, enter POSITION_OPEN and monitor live pool/price behavior.
Exit on significant deterioration, not ordinary launch wiggle.
Hard stop from entry is configurable.
Trailing drawdown from peak is configurable and arms only after upside.
Fast-drop 1s/2s/5s velocity triggers are configurable.
Liquidity, price-impact, slippage, stale-route, and manual kill triggers are configurable.
Primary flatten is attempted first; rescue flatten is automatic only after primary sim/send failure.
```

Launch Continuation R2-HOLD doctrine:

```text
This is a new-launched-coin hunter, not a sub-5-second scalp bot.
Entry edge = early launch detection plus real buy pressure.
Hold edge = stay in while price/flow/liquidity thesis remains valid.
Exit edge = fast Sender flatten when the thesis breaks.
Default first live hold cap = 86400 seconds, not multi-day until restart/recovery proof.
Initial exit sentinel defaults: hard_stop=1000 bps, trailing_stop=1500 bps, panic_drop_10s=800 bps.
New scripts: 22_launch11_position_monitor_exit_sentinel.sh, 22_launch12_hold_markout_research.sh, 22_launch13_r2_hold_live_execute.sh.
```

Launch regression guards:

```text
Helius Sender tip instruction must be present before Sender buy or sell.
Transaction parser must confirm expected tip transfer before send.
PumpSwap direct sell route-shape builder must simulate before live send.
flatten_sim_failed: Custom 3007 must block primary flatten and route to rescue.
Rescue flatten must update PM/ledger to LAUNCH_RESCUE_FLATTENED.
Final inventory must be zero after rescue.
Ledger open exposure must be zero after rescue.
Acquisition success alone must never mark Launch as complete.
Rescue-flattened runs are operationally valid but not clean Launch success.
```

Launch economic gate:

```text
expected_gross_edge_lamports
>
buy_tip
+ sell_tip
+ base_fees
+ protocol_fees
+ expected_slippage
+ setup_cost
+ safety_margin
```

Reject tiny Launch trades where account/setup cost plus tips dominate possible upside, unless the setup cost is proven recoverable or already amortized.

**Current tactical update (`2026-05-21`):**

```text
Run70 shortlist state:
97 total DirectArb missing-input mints
7 recurring across Epoch2 + Epoch3
3 acquisition-allowed after policy/liquidity gates

6Gzs branch:
economics pass
ATA-create simulation passes
fresh route drifted
swap simulation failed
route_stability_status = drifted_and_sim_failed
=> keep A-tier, no-execute, bounded retries only

6jUs branch:
one-mint acquisition executed successfully
inventory refresh succeeded
mechanical acquisition complete
approval propagation proven offline/replay
status = approval_propagation_test_only
execute_status = not_execute_approved
Run70D live bundle proof:
  in_inventory = true
  balance_native = 1001.166755
  policy_pass = true
  current entry/exit quote pass is now false at this later check

Run70D runtime replay proof passed:
  runtime_replay_reason = bounded_replay_override_for_held_inventory
  approved_held_input_mints_after contains 6jUs
  held_input_mints_after contains 6jUs
  runtime_replay_target_in_approved_held_input_export = true
  runtime_replay_target_in_held_input_export = true
  worker_admission_would_not_reject_not_held_input = true
  target_rows_cleared_not_held_input_count = 1
  runtime_replay_target_mint_first_rejection_stage_counts =
    {"source_not_afxdp": 54, "strict_eligible": 1}

Fresh epoch propagation state:
epoch5 is not valid propagation proof because the approved-held-input override
was dropped at launch and Run39 wrote approved_held_input_mints=""

epoch6 launch was corrected and carried the literal approved-held-input list,
but the canonical run never reached fresh AF_XDP tape because Agave failed to
reach green preflight

Current verdict:
6jUs remains the bounded DirectArb-only approval target
the approval registry/export path is now proven offline
but fresh canonical Run39 propagation is still unproven
next epoch tag = approval_propagation_test_only
execute status = not_execute_approved
=> no more acquisition; next fresh epoch should only verify the already-proven
   bounded replay override on a green Agave window
```

**Current lane-readiness map (`2026-05-27`):**

```text
Rebalancing:
  architecture = execution plumbing green only
  status = REBALANCING_EXECUTION_PLUMBING_READY
  latest proof = 66RG certificate reconciliation
  execution_plumbing_ready_rows = 21
  proposal_eligible_rebalancing_rows = 0
  split = 12 sender no-send, 9 bundle no-submit
  role = parked until owner-selector instrumentation repair

DirectArb:
  architecture = green
  status = DIRECTARB_STRUCTURAL_HUNT_READY
  remaining blocker = policy-clean roundtrip economics / habitat quality
  role = sentinel only until a cleaner habitat appears

Backrun:
  architecture = green enough for structural hunt
  status = BACKRUN_STRUCTURAL_HUNT_READY
  remaining blocker = immediate target freshness / source timing
  latest proof = AFBR1H immediate bundle-ready freshness batch, 10/10 SOURCE_LATE

Post-shock / continuation:
  signal = real
  architecture = not green yet
  blocker = no certified usable instrument / account / policy path
  next proof = post-shock structural instrument-readiness certificate

Housekeeping:
  signal = under-tested
  architecture = not green yet
  blocker = real-trade vs saved-loss / Drop-only split unknown
  next proof = housekeeping structural readiness certificate
```

For the next period, ending **2026-06-14**, `.15` is a lane-certification host. The job is to prove which lanes are structurally ready and separate system blockers from market/speed/strategy blockers. It is not a mandate to keep sampling random pools until something looks live.

Backrun latest proof:

```text
21_afbr1h_immediate_bundle_ready_freshness_batch.sh
```

AFBR1G proved the saved AFBR1F samples were measured too late, not that `.15` was source-late. AFBR1H then corrected the proof path by emitting a tiny `/dev/shm` hot bundle-ready row and probing status before heavy artifact construction. The AFBR1H batch completed `10` valid native zero-copy no-send samples with `status=SOURCE_LATE_DOMINANT`, `freshness_class_counts={"SOURCE_LATE":10}`, `fresh_at_bundle_ready_count=0`, `invalid_preflight_count=0`, `invalid_post_cleanup_count=0`, `probe_nonzero_count=0`, and `unsafe_live_activity_count=0`. Distribution: `target_seen_to_bundle_ready_us_p50=148`, p90 `16017`, p99 `30562`; `bundle_ready_to_status_start_us_p50=997`, p90 `1377`, p99 `1587`; `monitor_seen_to_status_start_us_p50=37`; `provider_sim_latency_us_p50=67213`. Treat Backrun as structurally ready but `.15` AF_XDP source/fresh-target gated. The next proof is not AFBR2 or live; the next serious Backrun source work is earlier target availability, such as Helius preprocessed / Shred Delivery entitlement or another earlier target-source topology.

AFBR1H distribution classification:

```text
SOURCE_LATE_DOMINANT
FRESH_AT_BUNDLE_READY=0
STATUS_UNKNOWN=0
```

Rebalancing latest proof:

```text
66rg_rebalancing_certificate_owner_reconciliation_20260527
artifact=/home/ubuntu/Guerrila/g1-host-pack/results/66rg_rebalancing_certificate_owner_reconciliation_20260527/final_verdict.json
final_verdict=REBALANCING_PARKED_CERTIFICATE_AMENDED
66rb_status=REBALANCING_EXECUTION_PLUMBING_READY
66rb_meaning=execution_plumbing_structural_ready
rebalancing_rows_seen=3997
exact_positive_rows=91
quote_min_out_ready_rows=324
entry_rule_ready_rows=3451
exit_rule_ready_rows=2916
pm_ready_rows=72
account_ready_rows=47
inline_ata_possible_rows=9
template_ready_rows=69
sender_no_send_ready_rows=12
bundle_no_submit_ready_rows=9
roundtrip_positive_rows=91
structural_ready_rows=21
execution_plumbing_ready_rows=21
owner_mode_match_rebalancing_rows=0
pm_owner_lock_ready_rows=0
restoration_pressure_measured_rows=0
false_restoration_gate_input_ready_rows=0
proposal_eligible_rebalancing_rows=0
architecture_blocker_counts={}
wire_send=0
live_landed=0
send_transaction_called=false
sendBundle_called=false
```

This supersedes the old "Rebalancing structural hunt ready" wording. `66RB` proved execution plumbing, not Rebalancing owner-mode eligibility. Proposal work is closed until `66RG2` owner-selector instrumentation repair proves owner match, PM owner lock or split rule, restoration pressure, and false-restoration inputs.

Structural-readiness standard from here:

```text
source path works
quote/min_out works
PM exact reserve/release works
account/ATA/inventory path works
template/sign path works
private egress no-send works
reconcile/release works
```

Allowed remaining blockers after structural readiness:

```text
no fresh candidate
EV negative
source too late
market not suitable
unsupported out-of-scope pool
```

Not allowed as "market" blockers:

```text
stale inventory
unknown ATA state
template missing
PM mismatch
quote missing
ambiguous account lineage
egress unmapped
```

This document intentionally does **not** treat one old candidate or one old run as proof. Each economic claim must be backed by a fresh run, artifact, and acceptance gate.

Fresh canonical monetization windows are named `EpochN`.

```text
EpochN = fresh bounded AF_XDP zero-copy window
       + Agave-first eno1 recovery before and after
       + all venues
       + all owner modes
       + TPU/QUIC primary
       + public RPC scout fallback
       + no live send unless explicitly approved
       + self-contained fresh artifacts only
```

---

## 0. Non-negotiable operating doctrine

- [ ] Do not end the project because a run exposes a blocker.
- [ ] Do not call a blocker a failure. Convert the blocker into the next proof target.
- [ ] Do not raise capital because a model says the surface is large.
- [ ] Do not TinyLive until conversion and unit-normalized economics are proven.
- [ ] Do not acquire weird/illiquid held-input tokens without exit-liquidity proof.
- [ ] Do not count `jito_bundle` or `harmonic_bundle` as executable while we lack working access.
- [ ] Do not treat `selected_trade_size_lamports` as SOL unless `mint_in=WSOL/SOL`.
- [ ] Do not sum mixed-mint heuristic EV as SOL-denominated profit.
- [ ] Do not deploy multiple independent Shadow bots. Use one Shadow brain, one PM, many conflict-aware execution shards.
- [ ] Do not rely on old candidate rows as current proof.
- [ ] Do not attach AF_XDP to `eno1` before Agave is green.
- [ ] Do not leave XDP attached after a bounded `.15` window.

---

## 1. Current green proofs to preserve

Treat these as green unless a fresh same-window counter proves regression.

- [ ] Replay TPU no-send architecture remains green.
  - Required evidence if challenged:
    - `tpu_quic_candidate_seen > 0`
    - `tpu_quic_would_enqueue > 0`
    - `tpu_quic_ring_enqueue_ok > 0`
    - `tpu_quic_sender_dequeue_count > 0`
    - `tpu_quic_no_send_accept_count > 0`
    - `tpu_wire_send_count = 0`
    - `live_landed = 0`

- [ ] Replay-envelope fake-sender TinyLive remains green.
  - Required evidence:
    - `status=tpu_quic_tiny_live_terminal_reconciled`
    - `pm_release_or_landed_reconcile=1`
    - `tpu_quic_tiny_live_enabled=1`
    - `tpu_quic_sender_dequeue_count=1`
    - `tpu_quic_send_attempt_count=1`
    - `tpu_quic_send_ok_count=1`
    - `tpu_quic_pm_release_count=1`
    - `tpu_wire_send_count=0`
    - `live_landed=0`

- [ ] Route policy blocks private bundle routes as executable.
  - Required evidence:
    - `block_private_bundle_routes=true`
    - `jito_bundle_executable_count=0`
    - `harmonic_bundle_executable_count=0`
    - deployable rows use only `tpu_quic_public` or `public_rpc_scout`.

- [ ] Run64D bounded acquisition executor mechanics are validated.
  - Required evidence:
    - `plan` mode passes.
    - `simulate` mode passes.
    - one `execute` mode acquisition finalized and inventory refresh worked.
  - Caveat:
    - the Coinini/`8Nh2...` acquisition proved the executor, but it did **not** prove useful inventory.

---

## 2. `.15` / `eno1` bounded-window rule

Until `eno2` IPv4 / dedicated ingress is solved, `.15` is a bounded-window host.

### 2.1 Required pre-run recovery

- [ ] Stop stale Shadow processes:

```bash
sudo pkill -f 'shadow-tier0' || true
sudo pkill -f 'shadow-oracle' || true
```

- [ ] Detach XDP and clear stale XSK map:

```bash
sudo ip link set dev eno1 xdp off || true
sudo rm -f /sys/fs/bpf/shadow_xsks_map 2>/dev/null || true
ip -details link show dev eno1 | sed -n '1,12p'
```

- [ ] Verify `eno1` shows:

```text
LOWER_UP
no prog/xdp attached
```

- [ ] Restart Agave and wait until green:

```bash
sudo systemctl restart sh1-agave-no-geyser-2.service
```

- [ ] Require:

```text
active_systemd_unit=sh1-agave-no-geyser-2.service
local_health=ok
processed_slot_lag acceptable
finalized_slot_lag acceptable
udp_listener_count > 0 on 8000-8025
eno1 LOWER_UP
no XDP attached
```

- [ ] Ensure `shadow-state` is publishing leader schedule before Tier0 runs:

```bash
pgrep -af '^/home/ubuntu/Guerrila/target/release-guerrila/shadow-state /etc/shadow-engine/config.guerrila.yaml$' \
  || nohup /home/ubuntu/Guerrila/target/release-guerrila/shadow-state /etc/shadow-engine/config.guerrila.yaml >/tmp/shadow-state-run.log 2>&1 &
stat /dev/shm/shadow_leader_schedule
```

- [ ] OOB-arm the native window:

```bash
mkdir -p /home/ubuntu/shadow-host-audit/dpdk-runs
touch /home/ubuntu/shadow-host-audit/dpdk-runs/run39_native_oob_ready.flag
```

### 2.2 During bounded run

- [ ] AF_XDP zero-copy may attach only after green Agave and OOB arm.
- [ ] Run bounded windows, usually `900s`, not indefinite production on `eno1`.
- [ ] No TinyLive unless explicitly approved by a later live gate.
- [ ] No hidden live sends in research runs.

### 2.3 Required post-run handoff

- [ ] Stop Shadow and detach XDP:

```bash
sudo pkill -f 'shadow-tier0' || true
sudo pkill -f 'shadow-oracle' || true
sudo ip link set dev eno1 xdp off || true
sudo rm -f /sys/fs/bpf/shadow_xsks_map 2>/dev/null || true
```

- [ ] Return `eno1` to Agave:

```bash
sudo systemctl restart sh1-agave-no-geyser-2.service
```

- [ ] Verify:

```bash
ip -details link show dev eno1 | sed -n '1,12p'
curl -sS -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  http://127.0.0.1:8899
```

---

## 3. Route monetization policy

- [ ] Executable routes today:

```text
tpu_quic_public
public_rpc_scout
```

- [ ] Non-executable / theoretical private routes today:

```text
jito_bundle
harmonic_bundle
```

- [ ] Required config:

```yaml
submit:
  routing:
    block_private_bundle_routes: true
```

- [ ] Required route report fields:

```text
observed_route
usable_route_for_us
jito_bundle_executable_count
harmonic_bundle_executable_count
tpu_quic_public_candidate_count
public_rpc_scout_candidate_count
blocked_private_route_count
blocked_route_unavailable_count
```

- [ ] Acceptance gate:

```text
jito_bundle_executable_count = 0
harmonic_bundle_executable_count = 0
deployable_by_us rows must use tpu_quic_public or public_rpc_scout
```

---

## 4. Unit-normalized economics checklist

### 4.1 Required unit handling

- [ ] Never treat raw `*_lamports` fields as SOL unless the mint is SOL/WSOL.
- [ ] For each row, emit:

```text
input_mint
input_decimals
input_native_size
input_usd_equivalent
input_sol_equivalent
output_mint
output_decimals
quote_expected_out_native
min_out_native
retained_ev_input_native
policy_tip_input_native
base_fee_raw
net_after_fee_input_native
net_after_fee_usd_equivalent
net_after_fee_sol_equivalent
```

### 4.2 Known correction example

Run59 selected row was `mint_in=EPj...` / USDC:

```text
trade_size_raw=164121284 = 164.121284 USDC
retained_ev=0.410303 USDC-native heuristic
policy_tip=0.347536 USDC-native heuristic
base_fee=0.005000 raw heuristic units
net_after_fee=0.057767 USDC-native heuristic
```

- [ ] Use this example as a regression test against unit confusion.

### 4.3 EV quality labels

- [ ] Label each EV field as one of:

```text
exact_quote_ev
input_native_heuristic_ev
sol_equivalent_ev
usd_equivalent_ev
later_horizon_markout
saved_loss_ev
```

- [ ] Do not sum mixed labels into one “profit” number.

---

## 5. Lane A — DirectArb liquid-only monetization

**Objective:** Convert DirectArb from detected rows into immediate TPU/public would-fire decisions using only liquid, exit-qualified held inputs.

### 5.1 Required DirectArb autopsy

For every fresh AF_XDP DirectArb run:

- [ ] Report:

```text
directarb_seen
directarb_unique
directarb_quote_ok
directarb_route_usable
directarb_inventory_ready
directarb_account_ready
directarb_template_ready
directarb_unit_positive
directarb_would_fire
directarb_blocker_top
directarb_optimal_size_bucket
directarb_ev_by_size_bucket
```

- [ ] Split blockers:

```text
route_private_only
route_unavailable
not_held_input
inventory_insufficient
missing_output_ata
template_missing
quote_missing
unit_negative
cap_blocked
```

### 5.2 Liquid held-input shortlist

Do not acquire a held-input token unless all are true:

- [ ] Mint policy passes.
- [ ] Entry quote exists.
- [ ] Exit quote exists.
- [ ] Exit value is non-dust.
- [ ] Exit loss is bounded.
- [ ] Mint recurs across fresh windows.
- [ ] Mint is attached to current DirectArb or high-confidence post-shock rows.
- [ ] Route usable by us is TPU/public.
- [ ] Acquisition cost is bounded.
- [ ] Inventory TTL is defined.
- [ ] Auto-flatten or deny condition is defined.

Required fields:

```text
mint
symbol if available
token_program
decimals
extensions
policy_status
entry_quote_ok
entry_cost_usdc
acquired_amount
exit_quote_ok
immediate_exit_value_usdc
exit_loss_pct
recurrence_count
directarb_count
owner_mode_count
expected_conversion_delta
acquisition_allowed
blocker
```

Current recurring shortlist proof:

- [ ] Run70 must reduce the broad `not_held_input` universe to a recurring, liquid shortlist before any acquisition decision.
- [ ] Recurrence means at least `2` fresh epochs, not one old row and one stale replay.
- [ ] Prefer one dominant pool + one dominant direction over scattered one-offs.
- [ ] This checklist remains **DirectArb-only** until strict DirectArb recurrence is stable; do not mix in Housekeeping or broader post-shock inventory work here.

Required Run70B retry-tracking fields:

```text
target_mint
attempt_number
planned_route
fresh_route
route_stability_status
sim_result
exit_value_ratio
decision
```

Allowed `route_stability_status` values:

```text
stable_planned_route
drifted_but_sim_passed
drifted_and_sim_failed
no_route
```

### 5.3 DirectArb firing rule

- [ ] If DirectArb passes quote, route, inventory, account, PM, and positive unit-normalized EV, it gets highest priority.
- [ ] DirectArb preempts lower-priority owner modes in the same conflict domain.
- [ ] DirectArb size is the largest EV-positive executable size, not the old tiny proof cap.
- [ ] Post-shock owners stand down for the same conflict domain.

### 5.4 DirectArb success criteria

- [ ] `directarb_inventory_ready > 0`
- [ ] `directarb_would_fire > 0`
- [ ] DirectArb would-fire recurs across multiple fresh windows.
- [ ] Unit-normalized DirectArb EV remains positive after policy tip and base fees.
- [ ] DirectArb route is TPU/public, not Jito/Harmonic.
- [ ] No dust-exit held-input tokens are acquired.

---

## 6. Lane B — Post-shock / impulse monetization

**Objective:** Convert the abundant post-shock / post-impulse surface into executable entry/exit rules. This lane must not remain a research-only markout report.

**Strategic premise:**

```text
DirectArb and immediate Backrun are clean proof primitives.
Post-shock / impulse is likely the higher-volume business surface.
```

Post-shock opportunities are both deterministic and probabilistic:

```text
Deterministic:
  a transaction changed reserves, pool state, route price, inventory pressure, or participant positioning.

Probabilistic:
  the next few events may continue, absorb, restore, revert, or decay; Shadow must estimate which path is most likely and whether it is executable before the edge disappears.
```

A catastrophic architecture failure would be:

```text
Shadow detects thousands of impulses,
reports positive continuation / rebalancing / housekeeping markout,
but has no executable rule to enter, size, exit, or stop.
```

Therefore this lane has one primary deliverable:

```text
post-shock markout -> executable owner rule -> continuous would-fire / saved-loss simulation
```

### 6.1 Post-shock doctrine checklist

- [ ] Post-shock / impulse is treated as a first-class monetization lane, not a side report.
- [ ] The system does not judge Shadow’s business potential from DirectArb / Backrun alone.
- [ ] Every meaningful event is scored as an impulse candidate unless a fast reject applies.
- [ ] Every impulse resolves to exactly one owner mode unless PM records an explicit split rule.
- [ ] DirectArb preempts lower-priority post-shock owners on the same conflict domain.
- [ ] Backrun remains armed for immediate displacement capture.
- [ ] Continuation, Housekeeping, Rebalancing, Flatten, and Drop remain active and measured.
- [ ] Post-shock signals may widen observation and simulation, but may not bypass live guards.
- [ ] No post-shock mode may live-send without route, quote, inventory, PM, egress, and unit-normalized EV proof.
- [ ] Later-horizon markout is never called realized profit until an entry/exit rule proves capture.

### 6.2 Impulse taxonomy

Each relevant market event must be assigned one or more raw impulse labels before owner selection.

Required labels:

```text
reserve_displacement
same_direction_burst
opposite_direction_absorption
route_lag_impulse
thin_liquidity_shock
migration_or_launchpad_impulse
restoration_impulse
copy_wave_start
failed_continuation
liquidity_vacuum
inventory_overhang
pool_rebalance_pressure
unknown_impulse
```

Checklist:

- [ ] `impulse_id` is stable and dedupable.
- [ ] `impulse_type` is populated.
- [ ] `impulse_direction` is populated.
- [ ] `impulse_size_native` is populated.
- [ ] `impulse_size_usd_equivalent` is populated when mint is known.
- [ ] `pool`, `venue`, `mint_in`, `mint_out`, and source are recorded.
- [ ] `source=afxdp` rows are distinguished from external/local-geyser rows.
- [ ] The same underlying event is not counted as multiple independent monetizable opportunities without dedupe evidence.
- [ ] Impulse classification runs in bounded fixed-shape worker-visible form; no variable-time inference in money workers.

### 6.3 Owner-mode competition

Owner modes:

```text
DirectArb
Backrun
Continuation
Housekeeping
Rebalancing
Flatten
Drop
Deny / Unknown
```

Priority and semantics:

```text
DirectArb:
  deterministic route/pool gap; highest priority; largest EV-positive executable size.

Backrun:
  immediate post-displacement extraction; high priority; shorter horizon than continuation.

Continuation:
  same-direction follow-through after impulse; value is often next-event or short-time markout.

Housekeeping:
  small cleanup / stale-route / awkward low-size opportunity; high volume, low per-trade EV.

Rebalancing:
  restoration / reserve-pressure opportunity; not generic mean reversion; requires measured recovery behavior.

Flatten:
  reduce or close exposure; monetizes saved loss and inventory safety.

Drop:
  no trade; monetizes saved loss by avoiding toxic or decayed opportunities.
```

Checklist:

- [ ] Owner selector emits exactly one `selected_owner`.
- [ ] `fallback_owner` and `runner_up_owner` are recorded.
- [ ] DirectArb wins if exact deterministic EV passes all guards.
- [ ] Post-shock owners stand down for the same conflict domain when DirectArb fires.
- [ ] Continuation and Rebalancing cannot both reserve the same event unless PM records an explicit split rule.
- [ ] Flatten/Drop saved-loss cases are reported, not discarded.
- [ ] Owner-mode decision includes blocker reason if owner cannot execute.

### 6.4 Required post-shock artifacts

A post-shock proof is incomplete unless all of the following exist and are non-empty where applicable:

- [ ] `shock_research.jsonl`
- [ ] `owner_mode_scorecard.json`
- [ ] `continuation_p0_markout_report.json`
- [ ] `filter_false_negative_report.json`
- [ ] `target_filter_rejection_report.json`
- [ ] `candidate_autopsy.jsonl`
- [ ] `surface_scorecard.json`
- [ ] `g12_gate_input.json`
- [ ] `g12_gate_report.json`
- [ ] `shadow-tier0.log`

Required status fields:

```text
shock_research_present=true
shock_research_rows>0
owner_mode_counts_populated=true
speed_decay_valid=true
unit_normalized_ev=true or explicitly partial with reason
```

If `shock_research.jsonl` is missing:

```text
status=partial_missing_shock_research
```

Do not call the run complete.

### 6.5 Markout horizon ledger

For every impulse / candidate, record both immediate and later-horizon outcomes.

Same-pool-event horizons:

```text
next_same_pool_event_1
next_same_pool_event_2
next_same_pool_event_3
```

Time horizons:

```text
+50ms
+100ms
+250ms
+500ms
+1s
```

Required fields:

```text
immediate_net_ev
later_horizon_ev_by_horizon
best_later_horizon
best_later_net
max_adverse_excursion
time_to_decay
route_gap_decay_ms
same_direction_followthrough
opposite_direction_absorption
restoration_fraction
saved_loss_if_drop
saved_loss_if_flatten
```

Checklist:

- [ ] Immediate EV is separated from later-horizon EV.
- [ ] Later-horizon EV is separated from saved-loss EV.
- [ ] Best horizon is recorded per owner mode.
- [ ] Worst adverse excursion is recorded before best horizon.
- [ ] Horizon results are unit-normalized.
- [ ] Rows with no usable route are counted separately from rows with negative markout.
- [ ] Rows with stale/missing state are not counted as economic failures without a state blocker tag.

### 6.6 Continuation execution-rule checklist

Continuation is not monetized until it has an executable rule.

Required rule fields:

```text
entry_trigger
confirmation_condition
entry_delay_or_event_gate
size_rule
route_rule
hold_horizon
exit_trigger
stop_loss
max_adverse_excursion
inventory_requirement
PM_reservation_rule
egress_rule
```

Checklist:

- [ ] Entry trigger is defined.
- [ ] Confirmation condition is defined, or immediate entry is explicitly justified.
- [ ] Entry is tied to `same_direction_followthrough`, `route_gap_survival`, or another measured impulse feature.
- [ ] Hold horizon is one of: `next_same_pool_event_1`, `next_same_pool_event_2`, `next_same_pool_event_3`, `+50ms`, `+100ms`, `+250ms`, `+500ms`, or an explicitly measured custom horizon.
- [ ] Exit trigger is defined.
- [ ] Stop-loss is defined.
- [ ] Worst adverse excursion is below accepted threshold.
- [ ] Route usable by us: `tpu_quic_public` or `public_rpc_scout`.
- [ ] No reliance on `jito_bundle` / `harmonic_bundle`.
- [ ] Inventory is liquid and exit-qualified.
- [ ] Unit-normalized expected EV is positive.
- [ ] Continuous no-send simulation reports `would_fire_continuation > 0`.
- [ ] A failed continuation is reclassified into Drop / Flatten logic, not ignored.

### 6.7 Rebalancing execution-rule checklist

Rebalancing is a restoration-flow strategy, not generic mean reversion.

Required rule fields:

```text
displacement_threshold
restoration_probability
entry_side
target_recovery_fraction
exit_recovery_threshold
max_hold_horizon
inventory_band
route_rule
stop_loss
```

Checklist:

- [ ] Reserve displacement is measured.
- [ ] Restoration pressure is measured.
- [ ] Entry occurs only when restoration expected value exceeds fees/tips/slippage.
- [ ] Exit target is defined as a recovery fraction or event horizon.
- [ ] Rebalancing is not allowed to double-reserve with Continuation on the same conflict domain.
- [ ] Rebalancing inventory exposure is PM-visible.
- [ ] Rebalancing route is TPU/public usable.
- [ ] Rebalancing ATA gaps are ranked by EV and frequency.
- [ ] Rebalancing saved-loss / realized-recovery EV is reported separately.

### 6.8 Housekeeping execution-rule checklist

Housekeeping is for many small, awkward, high-frequency opportunities.

Checklist:

- [ ] Housekeeping rows are separated from deterministic DirectArb.
- [ ] Housekeeping is not used as a catch-all for unclear candidates.
- [ ] Minimum EV per attempt is defined.
- [ ] Minimum repeat frequency is defined.
- [ ] Token/account readiness is cheap and bounded.
- [ ] Route is TPU/public usable.
- [ ] Failure cost is lower than expected recurring edge.
- [ ] Housekeeping report includes total volume, median EV, p90 EV, failed-route count, and account-lock count.

### 6.9 Flatten / Drop saved-loss checklist

Flatten and Drop are monetization modes because avoiding loss is PnL.

Checklist:

- [ ] Drop decisions record expected avoided loss.
- [ ] Flatten decisions record expected avoided loss and inventory reduction.
- [ ] Saved-loss EV is not mixed with trade-entry EV.
- [ ] Drop/Flatten decisions are tied to measurable triggers:
  - [ ] adverse markout
  - [ ] route collapse
  - [ ] stale state
  - [ ] absorption failure
  - [ ] crowding / toxic flow
  - [ ] inventory overexposure
- [ ] Saved-loss count is reported by owner mode and venue.
- [ ] Speed-as-defense is measured: fast Drop/Flatten versus delayed Drop/Flatten.

### 6.10 Post-shock deployability gates

A post-shock row is deployable only if:

- [ ] `source=afxdp`
- [ ] `usable_route_for_us in {tpu_quic_public, public_rpc_scout}`
- [ ] `jito_bundle_executable=false`
- [ ] `harmonic_bundle_executable=false`
- [ ] input inventory is held and exit-qualified.
- [ ] output ATA/account readiness exists or is policy-approved for precreation.
- [ ] quote is valid or owner mode has a valid non-quote markout rule.
- [ ] unit-normalized EV is positive.
- [ ] PM reserve can be simulated.
- [ ] egress enqueue can be simulated.
- [ ] account-lock/conflict domain is available.
- [ ] shard assignment succeeds in Run65 simulation.

### 6.11 Required owner-mode report

For each owner mode, report:

```text
rows
unique_events
positive_immediate
positive_later_horizon
best_horizon
would_fire
saved_loss
top_pool_direction
main_blocker
immediate_ev
later_horizon_ev
saved_loss_ev
route_usable_count
inventory_ready_count
account_ready_count
template_ready_count
blocked_private_route_count
blocked_inventory_count
blocked_account_count
blocked_rule_missing_count
```

Checklist:

- [ ] Report is produced from fresh canonical AF_XDP data.
- [ ] Report has one row per owner mode.
- [ ] Report has top pools/directions per owner mode.
- [ ] Report shows exact blocker stack per owner mode.
- [ ] Report distinguishes market-positive from deployable-by-us.
- [ ] Report estimates 24h counts only after dedupe.

### 6.12 Post-shock success criteria

A post-shock lane is not considered monetizable until:

- [ ] At least one owner mode has a complete entry/exit rule.
- [ ] Rule is tested in continuous no-send simulation.
- [ ] `would_fire_<owner>` is materially nonzero.
- [ ] Route is TPU/public usable.
- [ ] Unit-normalized EV is positive.
- [ ] Later-horizon EV exceeds immediate/fee/slippage risk.
- [ ] Worst adverse excursion is bounded.
- [ ] Inventory is liquid and exit-qualified.
- [ ] Repeated fresh windows show recurrence.
- [ ] Shard simulation shows execution capacity is sufficient or quantifies missed EV.

### 6.13 Run66 — post-shock executable rule engine proof

Create a dedicated proof run after Run65 if post-shock rows remain abundant.

Objective:

```text
Convert continuation / rebalancing / housekeeping markout into executable entry/exit no-send rules.
```

Required outputs:

```text
run66_post_shock_rule_report.json
run66_continuation_rules.json
run66_rebalancing_rules.json
run66_housekeeping_rules.json
run66_flatten_drop_saved_loss.json
```

Acceptance gates:

- [ ] `shock_research_present=true`
- [ ] `owner_mode_counts_populated=true`
- [ ] At least one continuation or rebalancing rule generated.
- [ ] Rule includes entry, size, exit, stop, and route.
- [ ] Rule has positive unit-normalized simulated EV.
- [ ] Rule has bounded adverse excursion.
- [ ] Rule is simulated through PM reserve and egress enqueue no-send.
- [ ] No live send.
- [ ] No Jito/Harmonic dependency.
- [ ] Report identifies why non-executable post-shock rows failed.

---

## 7. Lane C — Inventory, ATA, and acquisition conversion

### 7.1 Inventory categories

- [ ] Core inventory:

```text
WSOL
USDC
maybe USDT/RAY only if policy-clean and useful
```

- [ ] Experimental inventory:

```text
one-mint only
tiny capped
TTL = one measurement window
exit-qualified
```

- [ ] Dead/dust inventory:

```text
policy_safe_but_exit_dust
no_scale
deny_for_inventory_expansion
test_artifact_only
```

### 7.2 Coinini / 8Nh2 lesson

- [ ] Mark `8Nh2DSKq... / Coinini` as:

```text
policy_safe_but_economically_dead_or_dust
no_scale
test_artifact_only
```

- [ ] Do not acquire the second metadata mint until it passes exit-liquidity and recurrence gates.

### 7.3 Token-2022 metadata-only policy

Allow only if all are true:

- [ ] Token program = Token-2022.
- [ ] Extensions subset is exactly safe metadata-only class:

```text
metadataPointer
tokenMetadata
```

- [ ] Parsed mint info stable across two RPC reads.
- [ ] `mint_authority = null`.
- [ ] `freeze_authority = null`.
- [ ] No transfer hook.
- [ ] No permanent delegate.
- [ ] No non-transferable extension.
- [ ] Default account state is not frozen.
- [ ] Not denylisted.
- [ ] Decimals sane.
- [ ] Entry quote and exit quote both exist before acquisition.
- [ ] Exit quote is non-dust.

### 7.4 ATA precreation checklist

Before creating any ATA:

- [ ] Mint policy passes.
- [ ] Mint is connected to repeated high-EV or high-frequency rows.
- [ ] Mint appears in current fresh-window blockers, not only stale/model rows.
- [ ] ATA creation is off hot path.
- [ ] Inventory refresh follows creation.
- [ ] Rerun measures real blocker delta.

Required proof after ATA work:

```text
missing_output_ata_delta
execution_blocker_none_delta
deployable_by_us_delta
owner_mode affected
top remaining blockers
```

### 7.5 Acquisition executor checklist

Any acquisition executor must support:

- [ ] `plan` mode.
- [ ] `simulate` mode.
- [ ] `execute` mode.
- [ ] Default mode is `plan`.
- [ ] Execute requires explicit flag:

```text
SHADOW_RUN64D_EXECUTE=true
```

- [ ] First execution is one mint only.
- [ ] Hard cap per mint.
- [ ] Hard total cap.
- [ ] Slippage cap.
- [ ] Fresh quote before execution.
- [ ] Fresh simulation before execution.
- [ ] Confirmation polling.
- [ ] Inventory refresh.
- [ ] Immediate exit-quote check.
- [ ] Route stability check against the planned acquisition lane.
- [ ] Signed swap simulation must pass before execute approval.
- [ ] Simulation artifact must record `live_send=false` and `execute_result=null`.

---

## 8. Lane D — Continuous execution and sharding

**Objective:** Simulate and then build Shadow as a continuous machine, not a post-run selector.

### 8.1 Production execution shape

- [ ] One shared observation brain.
- [ ] One owner selector.
- [ ] One global PM/capital authority.
- [ ] Many conflict-aware execution shards.
- [ ] No independent duplicate Shadow bots.

Production flow:

```text
AF_XDP event arrives
-> decode / route / quote / score
-> owner selected
-> PM reserves
-> scheduler assigns shard
-> egress enqueues TPU/public
-> PM reconciles
```

### 8.2 Conflict key

Every candidate needs a conflict key:

```text
pool
mint_in
mint_out
owner_mode
route_family
input token account
output token account
signer/shard
notional_bucket
```

- [ ] DirectArb preempts lower-priority owner modes in same conflict domain.
- [ ] Otherwise highest EV / priority wins.
- [ ] Suppressed candidates are counted as missed EV, not ignored.

### 8.3 Run65 shard simulation

Run65 must simulate:

```text
1 shard
2 shards
4 shards
8 shards
16 shards
```

For each shard count, report:

```text
would_fire_count
directarb_fire_count
post_shock_fire_count
captured_ev
missed_ev
conflict_rate
capital_utilization
best_owner_mode
worst_blocker
```

Required counters:

```text
continuous_candidates_seen
candidate_deduped_count
owner_selected_directarb
owner_selected_backrun
owner_selected_continuation
owner_selected_housekeeping
owner_selected_rebalancing
owner_selected_flatten
owner_selected_drop
scheduler_assignments_total
would_fire_total
would_fire_by_owner
would_fire_by_shard
would_enqueue_tpu_count
would_public_rpc_count
would_wire_send_count=0
suppressed_duplicate_count
suppressed_conflict_domain_count
suppressed_no_free_shard_count
suppressed_inventory_unavailable_count
suppressed_account_lock_count
suppressed_capital_unavailable_count
suppressed_route_unavailable_count
missed_ev_due_to_serial_execution
missed_ev_due_to_inventory
missed_ev_due_to_route
missed_ev_due_to_account_conflict
```

### 8.4 Shard success criteria

- [ ] Sharding materially increases captured EV versus one shard.
- [ ] Sharding identifies missed EV due to insufficient parallelism.
- [ ] PM can assign and reconcile simulated reservations globally.
- [ ] No duplicate execution of same candidate.
- [ ] No double-reserve of same inventory.

---

## 9. Fresh-run ladder and proof sequence

### 9.1 Run64E — exit-qualified held-input shortlist

Purpose:

```text
Find held-input mints worth acquiring without creating dead inventory.
```

Required outputs:

```text
not_held_input_mints_ranked
entry_quote_status
exit_quote_status
exit_value_usdc
exit_loss_pct
recurrence_count
directarb_count
owner_mode_count
expected_conversion_delta
acquisition_allowed
blocker
```

Acceptance:

- [ ] No acquisition candidates have dust exit quotes.
- [ ] No acquisition candidates are one-window-only stale rows.
- [ ] At least one candidate is liquid both ways and recurring.

### 9.2 Run64F — one-mint acquisition validation

Purpose:

```text
Acquire exactly one exit-qualified held-input band and measure real conversion delta.
```

Acceptance:

- [ ] Plan mode passes.
- [ ] Simulate mode passes, or records an explicit no-execute reason with route/sim history.
- [ ] Execute one mint only.
- [ ] Transaction finalized.
- [ ] Inventory refresh sees balance.
- [ ] Exit quote remains non-dust after acquisition.
- [ ] Run61/62/63 rerun measures conversion delta.

Current first recurring DirectArb simulation target:

- [ ] `6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M`
- [ ] Cap stays at `<= 1 USDC`
- [ ] Planned band stays at `1000` tokens
- [ ] Fresh quote must stay on the planned path `GoonFi V2 -> Meteora DLMM -> Raydium CLMM`
- [ ] ATA exists or ATA-create simulation passes
- [ ] Signed Jupiter swap simulation passes
- [ ] If fresh route drifts or swap simulation fails, execution remains blocked even if entry/exit quotes stay positive
- [ ] Current state is **no-execute** because `route_stability_status=drifted_and_sim_failed`
- [ ] Retry simulate at most `2` more times across different market moments
- [ ] Only reconsider execute if:
  - `route_stability_status=stable_planned_route`
  - `swap_simulation_passes=true`
  - `exit_value_non_dust=true`
  - spend/slippage gates remain within cap
- [ ] If retries remain route-drifted or sim-failed, demote `6Gzs...` and move to the next A-tier DirectArb mint

### 9.3 Run61 — continuous no-send simulation

Purpose:

```text
Measure what Shadow would continuously attempt during the bounded AF_XDP window.
```

Acceptance:

- [ ] All owner modes included.
- [ ] Jito/Harmonic not executable.
- [ ] TPU/public route split reported.
- [ ] Continuous would-fire counters present.
- [ ] Unit-normalized EV present.

### 9.4 Run62 — conversion autopsy

Purpose:

```text
Explain why observed opportunities did or did not become would-fire.
```

Acceptance:

- [ ] DirectArb autopsy complete.
- [ ] Route-unavailable autopsy complete.
- [ ] Not-held-input autopsy complete.
- [ ] ATA/template blockers ranked.
- [ ] Owner-mode blockers ranked.

### 9.5 Run63 — modeled-vs-real conversion pass

Purpose:

```text
Compare modeled interventions to real post-intervention reruns.
```

Acceptance:

- [ ] Modeled deltas reported.
- [ ] Real deltas reported.
- [ ] Model miss explained.
- [ ] Next concrete blocker identified.

### 9.6 Run65 — full-potential sharded simulation

Purpose:

```text
Measure how many opportunities Shadow would capture as a continuous multi-shard executor.
```

Acceptance:

- [ ] Shard matrix complete: 1/2/4/8/16.
- [ ] Captured EV and missed EV reported.
- [ ] Conflict suppression reported.
- [ ] Capital utilization reported.
- [ ] DirectArb priority enforced.

---

## 10. Capital strategy gates

### Rung 0 — current / minimal capital

- [ ] Architecture green.
- [ ] AF_XDP bounded-window workflow green.
- [ ] Continuous no-send simulation working.
- [ ] No capital scaling.

### Rung 1 — tiny bounded inventory experiments

- [ ] One-mint acquisition executor proven.
- [ ] Exit-qualified held-input candidate exists.
- [ ] Acquisition cost tiny and capped.
- [ ] Conversion delta measured.
- [ ] No live trading send.

### Rung 2 — small capital band

- [ ] `directarb_would_fire` becomes meaningfully nonzero.
- [ ] deployable_by_us increases materially.
- [ ] Unit-normalized EV positive across repeated windows.
- [ ] Routes executable by TPU/public.
- [ ] Inventory and ATA blockers controlled.

### Rung 3 — larger capital test

- [ ] Continuous would-fire count high enough to stress one shard.
- [ ] Run65 proves shards increase captured EV.
- [ ] Post-shock entry/exit rule simulated and positive.
- [ ] PM reservation/reconcile proof remains green.
- [ ] Real or near-real no-send/live evidence supports scaling.

### Rung 4 — 50–150 SOL discussion

Do not discuss serious capital until:

- [ ] DirectArb or post-shock lane repeats across multiple fresh windows.
- [ ] Unit-normalized realized/simulated EV is material.
- [ ] Route usable count is high.
- [ ] Shard simulation shows scalable capacity.
- [ ] Inventory is liquid and exit-qualified.
- [ ] Failure/markout tails are bounded.
- [ ] Tiny-to-small live proof lands and reconciles.

---

## 11. What not to do next

- [ ] Do not TinyLive.
- [ ] Do not buy another metadata-only token until exit-qualified shortlist passes.
- [ ] Do not acquire the second Run64D mint by default.
- [ ] Do not add more WSOL blindly.
- [ ] Do not broadly allow Token-2022.
- [ ] Do not count Jito/Harmonic as executable.
- [ ] Do not run multiple independent Shadows.
- [ ] Do not judge the project by one selected row.
- [ ] Do not treat post-shock markout as realized profit without entry/exit rule.
- [ ] Do not leave `eno1` with XDP attached.

---

## 12. Immediate next work order

1. [ ] Preserve the Run70 shortlist result: `97 -> 7 -> 3`.
2. [ ] Keep Coinini / `8Nh2...` classified as dust/dead inventory unless a fresh rerun proves conversion.
3. [ ] Keep `6Gzs...` classified as **A-tier but no-execute** while `route_stability_status=drifted_and_sim_failed`.
4. [ ] Re-run **Run70B simulate** for `6GzsDTj1JofW2BoBULNnNJamtsJd942p3GhvNf2NUa3M` at most `2` more times across different market moments.
5. [ ] Record `attempt_number`, `planned_route`, `fresh_route`, `route_stability_status`, `sim_result`, `exit_value_ratio`, and `decision` on every retry before deciding again.
6. [ ] Do **not** execute `6Gzs...` while route drift or Jupiter swap-sim failure remains.
7. [ ] Only if a retry returns `stable_planned_route` plus `swap_simulation_passes=true` and non-dust exit should one-mint execute be reconsidered.
8. [ ] If the bounded retries stay drifted/sim-failed, demote `6Gzs...` and move to the next A-tier recurring **DirectArb** mint.
9. [ ] Keep `6jUs...` as the only bounded held-input approval candidate already acquired for this branch; do **not** acquire more inventory yet.
10. [ ] Treat Run70D as an offline/short-form approval proof only, not as fresh canonical propagation proof.
11. [ ] Treat `epoch5` as an invalid propagation attempt because the approved-held-input override was dropped before Run39.
12. [ ] Treat the upgraded Run70D runtime replay as the cheap pre-epoch acceptance gate for approval propagation; it now proves `approved_held_input_mints_after` contains `6jUs...`, `held_input_mints_after` contains `6jUs...`, and the known target row clears `not_held_input`.
13. [ ] Tag the next fresh rerun as **approval_propagation_test_only** and **not_execute_approved** because the current live entry/exit quote gates for `6jUs...` are red.
14. [ ] Hard no for the next epoch: no acquisition, no TinyLive, no live send, no capital increase, no second mint, and no metadata-token expansion.
15. [ ] On the next fresh epoch, launch with the literal bounded approved-held-input list from the Run70D replay env file and require Agave green **before** the AF_XDP window.
16. [ ] Mandatory pass criteria on that rerun are at Run39, not later: `approved_held_input_mints` must include `6jUs...`, `held_input_mints` must include `6jUs...`, `r1_surface_approved_held_input > 0`, and target `6jUs...` rows must stop failing first at `not_held_input`.
17. [ ] Optional positive on that rerun: `directarb_inventory_ready`, `directarb_would_fire`, and `strict_current_state_would_fire` increase if fresh `6jUs...` opportunities recur.
18. [ ] If fresh `6jUs...` rows still fail first at `not_held_input`, classify the blocker as runtime config/export propagation, not economics.
19. [ ] If fresh `approved_held_input=true` lands but `would_fire` does not move, classify the next blocker as route/liquidity/recurrence/other admission gates rather than approval visibility.
20. [ ] Only after the Run39 approval-propagation proof should Run61/62/63 be rerun and compared on `directarb_inventory_ready_delta`, `directarb_would_fire_delta`, `deployable_by_us_delta`, and `strict_current_state_ev_usd_delta`.
21. [ ] Keep this branch scoped to **DirectArb** conversion only; do not broaden the checklist to Housekeeping or post-shock inventory work here.
22. [ ] If DirectArb `would_fire` remains low after fresh approved-held-input propagation is proven, focus on post-shock entry/exit rule design.
23. [ ] After deployable conversion improves, run Run65 shard simulation.
24. [ ] After the current DirectArb held-input branch is proven or exhausted, open **RunNIC2 / Epoch Lease Manager** as the next infrastructure component.
25. [ ] RunNIC2 must treat `eno1` as a leased resource with **Agave-owned by default**, **Shadow lease only inside an armed bounded epoch**, and **safe return to Agave** after drain.
26. [ ] RunNIC2 must use an explicit state machine: `AGAVE_OWNED -> WAIT_AGAVE_GREEN -> OOB_ARMED -> SHADOW_ATTACHING_XDP -> SHADOW_ACTIVE_EPOCH -> SHADOW_DRAINING -> SAFE_TO_DETACH_XDP -> AGAVE_RECOVERY -> AGAVE_OWNED`.
27. [ ] RunNIC2 must never detach XDP while any of these remain non-zero: `pm_inflight`, `egress_inflight`, `unflushed_result_ring`, `pending_nack_release`, `signatures_pending_status`.
28. [ ] RunNIC2 must stop candidate admission before detach, drain PM/egress/result rings, write artifacts, and only then release the `eno1` lease.
29. [ ] RunNIC2 should instrument Agave health, processed/finalized lag, UDP listener count, NIC/XDP counters, AF_XDP starvation/drop counters, candidate rate, would-fire rate, PM pending, and last artifact flush timestamp.
30. [ ] RunNIC2 should extend epoch duration by ladder only: `15m -> 20m -> 25m -> 30m`, and attempt `45m` only after repeated clean `30m` runs.
31. [ ] RunNIC2 acceptance for the first green host proof is: `30m` AF_XDP zero-copy epoch complete, Agave lag under threshold, PM pending `=0` before detach, egress pending `=0` before detach, artifacts complete, clean XDP detach, and Agave returns green without manual intervention.

---

## 13. Final strategic statement

Shadow’s current problem is not whether it can technically trade. It can.

Shadow’s current problem is monetization conversion:

```text
observed AF_XDP market structure
-> liquid and policy-safe inventory
-> usable TPU/public route
-> account/template readiness
-> owner-specific execution rule
-> continuous would-fire
-> PM reserve/reconcile
-> scalable shard execution
```

The strategy is:

```text
liquid DirectArb first
post-shock executable rules second
exit-qualified inventory only
TPU/public routes only
continuous no-send simulation
then sharded execution simulation
then small live proof
then capital scaling
```

**No failure. Shadow must work.**
