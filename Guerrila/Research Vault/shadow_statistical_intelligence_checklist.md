# Shadow Statistical Intelligence / Post-Shock Monetization Checklist

## Purpose

Build the statistical decision layer that turns Shadow from a detector into a continuous trading machine.

This checklist covers the models and proof artifacts needed to monetize:

- DirectArb
- Backrun
- Continuation
- Housekeeping
- Rebalancing
- Flatten
- Drop

The core idea is quantum-inspired only as a metaphor:

```text
candidate = superposition of possible owner modes
evidence updates probabilities
entropy measures uncertainty
execution collapses the candidate into exactly one action
alpha decays through decoherence
```

The implementation must be statistical and testable:

```text
Bayesian owner-mode inference
Hawkes / self-exciting flow models
Hidden Markov regimes
particle / scenario filters
optimal stopping
contextual bandits
queueing theory for shards
risk-controlled capital sizing
```

No mysticism. Every model must produce measurable predictions, post-run validation, and executable decision rules.

---

# 0. Non-Negotiable Guardrails

- [ ] No live send while this system is being built.
- [ ] No TinyLive unless explicitly approved after proof gates.
- [ ] No Jito/Harmonic counted as executable.
- [ ] Executable routes are only:
  - [ ] `tpu_quic_public`
  - [ ] `public_rpc_scout`
- [ ] All EV must be unit-normalized.
- [ ] Do not treat raw `lamports` as SOL unless `mint_in` or relevant value is actually WSOL/SOL-denominated.
- [ ] Distinguish:
  - [ ] immediate EV
  - [ ] later-horizon markout EV
  - [ ] saved-loss EV
  - [ ] heuristic EV
  - [ ] strict executable EV
  - [ ] fixup-assisted EV
- [ ] No weird-token inventory acquisition without:
  - [ ] policy pass
  - [ ] entry quote
  - [ ] exit quote
  - [ ] non-dust exit value
  - [ ] recurrence or immediate fresh relevance
  - [ ] explicit bounded cap
- [ ] All fresh canonical AF_XDP runs still obey the `.15` rule until `eno2`/observer path is solved:
  - [ ] Agave owns `eno1` first.
  - [ ] Agave must be green before AF_XDP attach.
  - [ ] Shadow gets bounded `eno1` lease only.
  - [ ] After window, detach XDP and return `eno1` to Agave.

---

# 1. Data Foundation

## 1.1 Canonical Event Schema

Every candidate/event row must have a stable schema.

- [ ] `event_id`
- [ ] `slot`
- [ ] `signature`
- [ ] `source`
  - [ ] `afxdp`
  - [ ] `external_geyser`
  - [ ] `local_validator_geyser`
- [ ] `venue`
- [ ] `pool`
- [ ] `mint_in`
- [ ] `mint_out`
- [ ] `mint_in_decimals`
- [ ] `mint_out_decimals`
- [ ] `input_native_size`
- [ ] `input_usd_equiv`
- [ ] `input_sol_equiv`
- [ ] `observed_route`
- [ ] `usable_route_for_us`
- [ ] `route_resolved_tpu_quic_public`
- [ ] `quote_valid`
- [ ] `quote_expected_out_native`
- [ ] `min_out_native`
- [ ] `retained_ev_native`
- [ ] `policy_tip_native`
- [ ] `base_fee_native_or_sol`
- [ ] `net_ev_native`
- [ ] `net_ev_usd`
- [ ] `net_ev_sol`
- [ ] `owner_mode_selected`
- [ ] `owner_mode_probabilities`
- [ ] `first_blocker`
- [ ] `strict_executable_now`
- [ ] `fixup_required`
- [ ] `fixup_type`
- [ ] `would_fire`
- [ ] `would_enqueue_tpu`
- [ ] `would_public_rpc_send`
- [ ] `would_wire_send=false`

## 1.2 Candidate Dedupe Key

- [ ] Define deterministic candidate dedupe key:

```text
slot + signature + pool + mint_in + mint_out + owner_mode + usable_route_for_us + notional_bucket
```

- [ ] Report duplicates removed.
- [ ] Report EV removed by dedupe.
- [ ] Never use raw row count as opportunity count without dedupe.

## 1.3 Markout Ledger

Every event must record markout horizons.

- [ ] immediate net EV
- [ ] +25ms markout
- [ ] +50ms markout
- [ ] +100ms markout
- [ ] +250ms markout
- [ ] +500ms markout
- [ ] +1s markout
- [ ] next_same_pool_event_1
- [ ] next_same_pool_event_2
- [ ] next_same_pool_event_3
- [ ] max adverse excursion
- [ ] time to max adverse excursion
- [ ] time to best favorable excursion
- [ ] decay / expiry reason

## 1.4 Required Artifacts

Each fresh epoch must produce:

- [ ] `candidate_autopsy.jsonl`
- [ ] `surface_scorecard.json`
- [ ] `owner_mode_scorecard.json`
- [ ] `shock_research.jsonl`
- [ ] `continuation_p0_markout_report.json`
- [ ] `filter_false_negative_report.json`
- [ ] `target_filter_rejection_report.json`
- [ ] `run61_continuous_shadow_live_simulation.json`
- [ ] `run62_conversion_autopsy.json`
- [ ] `run66_forensic_summary.json`
- [ ] `run69_strict_surface.jsonl`
- [ ] `g12_gate_input.json`
- [ ] `g12_gate_report.json`

---

# 2. Owner-Mode Probability Engine

## 2.1 Candidate as Superposition

Each event starts as a probability distribution over owner modes.

Required output:

```json
{
  "p_directarb": 0.0,
  "p_backrun": 0.0,
  "p_continuation": 0.0,
  "p_housekeeping": 0.0,
  "p_rebalancing": 0.0,
  "p_flatten": 0.0,
  "p_drop": 0.0,
  "owner_entropy": 0.0,
  "selected_owner": "...",
  "selection_confidence": 0.0,
  "second_best_owner": "...",
  "owner_margin": 0.0
}
```

Checklist:

- [ ] Compute owner-mode probability vector for every event.
- [ ] Compute entropy of owner distribution.
- [ ] Define entropy threshold above which Shadow must not trade.
- [ ] Define owner margin threshold above which Shadow may collapse to one owner.
- [ ] Record selected owner and second-best owner.
- [ ] Record why owner was selected.

## 2.2 DirectArb Priority Rule

DirectArb is not just another owner mode.

- [ ] DirectArb preempts other owners in same conflict domain if all strict gates pass.
- [ ] DirectArb must fire immediately in no-send simulation if:
  - [ ] quote exact / valid
  - [ ] route usable by us
  - [ ] inventory ready
  - [ ] output account ready
  - [ ] template ready
  - [ ] unit-normalized EV positive
  - [ ] PM simulated reserve available
- [ ] DirectArb sizing must be by marginal EV curve, not tiny proof cap.
- [ ] Post-shock owners must stand down for same conflict domain when DirectArb wins.

Proof:

- [ ] `directarb_seen`
- [ ] `directarb_unique`
- [ ] `directarb_quote_ok`
- [ ] `directarb_route_usable`
- [ ] `directarb_inventory_ready`
- [ ] `directarb_output_account_ready`
- [ ] `directarb_template_ready`
- [ ] `directarb_unit_positive`
- [ ] `directarb_would_fire`
- [ ] `directarb_ev_by_size_bucket`
- [ ] `directarb_first_blocker_split`

---

# 3. Impulse Taxonomy

## 3.1 Required Impulse Labels

Every post-shock candidate must be assigned one or more impulse labels before owner collapse.

- [ ] reserve displacement
- [ ] same-direction burst
- [ ] opposite-direction absorption
- [ ] route-lag impulse
- [ ] thin-liquidity shock
- [ ] restoration impulse
- [ ] failed continuation
- [ ] copy-wave start
- [ ] launchpad / migration impulse
- [ ] inventory-overhang impulse
- [ ] unknown / unclassified

## 3.2 Features Per Impulse

- [ ] reserve delta
- [ ] quote delta
- [ ] route delta
- [ ] trade direction
- [ ] shock size normalized by liquidity
- [ ] pool depth
- [ ] spread / price impact
- [ ] recent same-direction count
- [ ] recent opposite-direction count
- [ ] time since prior same-pool event
- [ ] time since prior same-mint event
- [ ] route repricing lag
- [ ] volatility regime
- [ ] inventory imbalance
- [ ] prior markout statistics for pool/direction

## 3.3 Impulse Artifact

Create:

```text
impulse_taxonomy_report.json
```

Required fields:

- [ ] impulse label counts
- [ ] owner-mode counts by impulse label
- [ ] EV by impulse label
- [ ] positive markout count by label
- [ ] deployable count by label
- [ ] top pools per label
- [ ] top mints per label
- [ ] main blockers per label

---

# 4. Hawkes / Self-Exciting Flow Model

## 4.1 Goal

Measure whether a shock increases the probability of future same-direction or opposite-direction events.

## 4.2 Per-Pool Intensity Model

For every pool + direction:

- [ ] estimate base event rate
- [ ] estimate same-direction excitation
- [ ] estimate opposite-direction excitation
- [ ] estimate impulse decay half-life
- [ ] estimate probability of next same-direction event within:
  - [ ] 25ms
  - [ ] 50ms
  - [ ] 100ms
  - [ ] 250ms
  - [ ] 500ms
  - [ ] next_same_pool_event_1
  - [ ] next_same_pool_event_2
  - [ ] next_same_pool_event_3

## 4.3 Hawkes Outputs

For each candidate:

- [ ] `hawkes_same_direction_intensity`
- [ ] `hawkes_opposite_direction_intensity`
- [ ] `impulse_decay_half_life_ms`
- [ ] `next_event_probability_50ms`
- [ ] `next_event_probability_250ms`
- [ ] `next_same_pool_event_probability_1`
- [ ] `next_same_pool_event_probability_2`
- [ ] `hawkes_confidence`

## 4.4 Acceptance Criteria

- [ ] Backtest shows same-direction intensity predicts positive continuation markout.
- [ ] Backtest shows opposite-direction intensity predicts rebalancing / flatten behavior.
- [ ] Model calibration report exists.
- [ ] High-intensity candidates outperform baseline.
- [ ] Low-intensity candidates are filtered or Drop/Flatten.

Artifacts:

- [ ] `hawkes_pool_direction_fit.json`
- [ ] `hawkes_calibration_report.json`
- [ ] `hawkes_owner_mode_lift_report.json`

---

# 5. Hidden Markov Regime Model

## 5.1 Pool Regimes

Infer hidden regime per pool / direction.

Required regimes:

- [ ] calm
- [ ] momentum
- [ ] reversal
- [ ] restoration
- [ ] thin-liquidity
- [ ] toxic
- [ ] copy-wave
- [ ] dead / no-liquidity
- [ ] unknown

## 5.2 Outputs

For each event:

- [ ] `p_regime_calm`
- [ ] `p_regime_momentum`
- [ ] `p_regime_reversal`
- [ ] `p_regime_restoration`
- [ ] `p_regime_toxic`
- [ ] `p_regime_copy_wave`
- [ ] `selected_regime`
- [ ] `regime_entropy`
- [ ] `regime_confidence`

## 5.3 Owner Mode Mapping

- [ ] DirectArb allowed independent of regime if deterministic gates pass.
- [ ] Continuation allowed primarily in momentum / copy-wave regimes.
- [ ] Rebalancing allowed primarily in restoration / overextension regimes.
- [ ] Flatten/Drop triggered by toxic / high-entropy regimes.
- [ ] Housekeeping requires separate proof of real economic transfer, not merely fixup accounting.

Artifacts:

- [ ] `pool_regime_model.json`
- [ ] `regime_owner_mode_lift_report.json`
- [ ] `regime_markout_report.json`

---

# 6. Particle / Scenario Filter

## 6.1 Goal

Represent post-shock future as multiple possible short-horizon paths.

For every candidate, simulate / estimate scenarios:

- [ ] continuation
- [ ] reversal
- [ ] flat/noise
- [ ] second same-direction shock
- [ ] opposite absorption
- [ ] liquidity disappears
- [ ] route reprices against us
- [ ] account/route conflict blocks execution

## 6.2 Outputs

For each candidate:

- [ ] scenario probabilities
- [ ] expected EV
- [ ] EV variance
- [ ] positive probability
- [ ] worst adverse excursion
- [ ] expected hold time
- [ ] recommended owner
- [ ] recommended action

## 6.3 Risk Gate

Candidate may only become a post-shock would-fire if:

- [ ] expected EV exceeds threshold
- [ ] probability of positive EV exceeds threshold
- [ ] max adverse excursion is within limit
- [ ] route usable by us
- [ ] inventory/account/template ready
- [ ] owner entropy below threshold

Artifacts:

- [ ] `particle_scenario_report.json`
- [ ] `post_shock_risk_gate_report.json`

---

# 7. Optimal Stopping / Exit Horizon Engine

## 7.1 Goal

Convert post-shock markout into executable entry/exit rules.

For each candidate, compare exit horizons:

- [ ] immediate exit
- [ ] +25ms
- [ ] +50ms
- [ ] +100ms
- [ ] +250ms
- [ ] +500ms
- [ ] +1s
- [ ] next_same_pool_event_1
- [ ] next_same_pool_event_2
- [ ] next_same_pool_event_3
- [ ] exit on opposite flow
- [ ] exit on route repricing
- [ ] exit on adverse stop

## 7.2 Required Outputs

- [ ] `best_exit_horizon`
- [ ] `best_horizon_ev`
- [ ] `best_horizon_adverse`
- [ ] `best_horizon_confidence`
- [ ] `stop_loss_threshold`
- [ ] `take_profit_threshold`
- [ ] `exit_trigger`
- [ ] `hold_time_expected_ms`

## 7.3 Owner-Specific Rules

### Continuation

- [ ] enter if same-direction probability high
- [ ] exit on next same-pool event or optimal horizon
- [ ] stop if adverse excursion exceeds threshold
- [ ] require route usable and input/output accounts ready

### Rebalancing

- [ ] enter when restoration probability high
- [ ] exit on reserve recovery / route restoration
- [ ] cap inventory exposure

### Housekeeping

- [ ] distinguish real economic trade vs fixup artifact
- [ ] only count as trading lane if actual economic transfer exists
- [ ] otherwise mark as account/readiness prep lane

### Flatten / Drop

- [ ] measure saved-loss EV
- [ ] do not mix saved-loss EV with realized trade EV
- [ ] record avoided adverse markout

Artifacts:

- [ ] `optimal_stopping_report.json`
- [ ] `continuation_entry_exit_rules.json`
- [ ] `rebalancing_entry_exit_rules.json`
- [ ] `housekeeping_real_trade_audit.json`
- [ ] `flatten_drop_saved_loss_report.json`

---

# 8. Bayesian Owner Arbiter

## 8.1 Posterior Score

For each owner:

```text
posterior(owner) = prior(owner | venue, pool, direction, regime)
                 × likelihood(markout evidence | owner)
                 × route feasibility
                 × inventory feasibility
                 × account feasibility
                 × risk feasibility
```

## 8.2 Required Inputs

- [ ] venue
- [ ] pool
- [ ] mint direction
- [ ] impulse class
- [ ] regime probability
- [ ] Hawkes intensity
- [ ] markout distribution
- [ ] route usability
- [ ] inventory readiness
- [ ] ATA/account readiness
- [ ] template readiness
- [ ] shard availability
- [ ] PM reserve feasibility

## 8.3 Required Outputs

- [ ] owner posterior vector
- [ ] selected owner
- [ ] owner entropy
- [ ] selected action
- [ ] confidence
- [ ] why not other owners
- [ ] owner override reason if DirectArb preempts

## 8.4 Acceptance Criteria

- [ ] Owner selection improves EV versus hardcoded mode.
- [ ] Owner entropy correlates with false positives / bad markouts.
- [ ] High-confidence owner selections outperform low-confidence ones.
- [ ] DirectArb preemption works correctly.
- [ ] Flatten/Drop reduce adverse exposure.

Artifacts:

- [ ] `owner_arbiter_backtest.json`
- [ ] `owner_entropy_calibration_report.json`
- [ ] `owner_selection_lift_report.json`

---

# 9. Contextual Bandit Learning Loop

## 9.1 Purpose

Learn which strategy works under which context.

Context features:

- [ ] venue
- [ ] pool
- [ ] mint pair
- [ ] impulse type
- [ ] regime
- [ ] liquidity
- [ ] shock size
- [ ] recent same-direction flow
- [ ] recent opposite-direction flow
- [ ] route usability
- [ ] inventory state
- [ ] shard availability

Actions:

- [ ] DirectArb
- [ ] Backrun
- [ ] Continuation
- [ ] Rebalancing
- [ ] Housekeeping
- [ ] Flatten
- [ ] Drop

Reward:

- [ ] immediate EV
- [ ] later-horizon EV
- [ ] realized or simulated exit EV
- [ ] saved-loss EV
- [ ] failed execution penalty
- [ ] inventory risk penalty

## 9.2 Required Model

- [ ] Thompson sampling or UCB baseline.
- [ ] Exploration only in no-send / simulation first.
- [ ] Small-size exploration only after live approval.
- [ ] Per-owner reward distributions.
- [ ] Per-pool reward distributions.
- [ ] Decay old evidence.

## 9.3 Acceptance Criteria

- [ ] Bandit learns to suppress poor owner modes.
- [ ] Bandit improves EV over static owner selection.
- [ ] Bandit does not increase tail loss in simulation.
- [ ] Bandit respects hard deterministic DirectArb gates.

Artifacts:

- [ ] `contextual_bandit_policy.json`
- [ ] `bandit_reward_report.json`
- [ ] `bandit_vs_static_owner_report.json`

---

# 10. Signal Decay / Decoherence Engine

## 10.1 Goal

Measure alpha decay after impulse.

Candidate starts with high coherence, then decays by time and contrary evidence.

## 10.2 Required Metrics

- [ ] `signal_coherence`
- [ ] `signal_entropy`
- [ ] `decay_half_life_ms`
- [ ] `elapsed_ms_since_impulse`
- [ ] `opposite_flow_decoherence`
- [ ] `route_reprice_decoherence`
- [ ] `quote_worsening_decoherence`
- [ ] `liquidity_disappearance_decoherence`
- [ ] `expired_reason`

## 10.3 Execution Rule

- [ ] If signal coherence below threshold, do not trade.
- [ ] If DirectArb deterministic, coherence rule may be bypassed only if exact EV still positive.
- [ ] For post-shock owners, coherence threshold is mandatory.

Artifacts:

- [ ] `signal_decay_report.json`
- [ ] `decoherence_threshold_backtest.json`

---

# 11. Capital Sizing Model

## 11.1 DirectArb Sizing

DirectArb size should be the largest EV-positive executable size.

Required buckets:

- [ ] 0.005 SOL-equivalent
- [ ] 0.01 SOL-equivalent
- [ ] 0.025 SOL-equivalent
- [ ] 0.05 SOL-equivalent
- [ ] 0.1 SOL-equivalent
- [ ] 0.25 SOL-equivalent
- [ ] 0.5 SOL-equivalent
- [ ] 1 SOL-equivalent
- [ ] 2 SOL-equivalent
- [ ] 5 SOL-equivalent
- [ ] 10 SOL-equivalent

Stop size increase when:

- [ ] marginal EV <= threshold
- [ ] min_out unsafe
- [ ] slippage exceeds limit
- [ ] route reliability falls
- [ ] PM exposure limit hit
- [ ] account conflict risk too high

## 11.2 Post-Shock Sizing

Use confidence / variance / adverse excursion.

- [ ] fractional Kelly baseline
- [ ] cap by owner mode
- [ ] cap by regime
- [ ] cap by pool liquidity
- [ ] cap by inventory band
- [ ] cap by shard capacity
- [ ] cap by max adverse excursion

## 11.3 Outputs

- [ ] recommended size
- [ ] size bucket
- [ ] expected EV
- [ ] EV variance
- [ ] loss bound
- [ ] route liquidity bound
- [ ] PM reserve requirement

Artifacts:

- [ ] `capital_sizing_report.json`
- [ ] `directarb_size_curve_report.json`
- [ ] `post_shock_size_curve_report.json`

---

# 12. Shard / Queueing Model

## 12.1 Purpose

Determine how many execution shards Shadow needs.

Do not deploy independent Shadows.

Correct architecture:

```text
one observation brain
one owner arbiter
one global PM/capital authority
many conflict-aware execution shards
```

## 12.2 Shard Simulation Matrix

Simulate:

- [ ] 1 shard
- [ ] 2 shards
- [ ] 4 shards
- [ ] 8 shards
- [ ] 16 shards

## 12.3 Metrics

- [ ] would_fire_total
- [ ] would_fire_by_owner
- [ ] would_fire_by_shard
- [ ] missed_due_to_no_free_shard
- [ ] missed_due_to_account_conflict
- [ ] missed_due_to_inventory_conflict
- [ ] missed_due_to_capital
- [ ] missed_due_to_route
- [ ] captured EV
- [ ] missed EV
- [ ] shard utilization
- [ ] capital utilization
- [ ] queue wait time
- [ ] alpha decay from waiting

## 12.4 Conflict Key

```text
pool + mint_in + mint_out + owner_mode + route_family + input_token_account + output_token_account + signer/shard + notional_bucket
```

## 12.5 Acceptance Criteria

- [ ] Shard model proves whether concurrency is a bottleneck.
- [ ] If strict would-fire is low, shard build is deferred.
- [ ] If missed EV due to no free shard is high, shard build becomes top priority.

Artifacts:

- [ ] `shard_simulation_report.json`
- [ ] `queueing_capacity_report.json`
- [ ] `missed_ev_due_to_serial_execution.json`

---

# 13. Run Plan

## Run74 — Impulse Feature Extraction

Goal:

```text
Build the event/impulse feature table from fresh AF_XDP artifacts.
```

Checklist:

- [ ] Extract all candidates from latest epoch.
- [ ] Add impulse taxonomy labels.
- [ ] Add route usability labels.
- [ ] Add unit-normalized EV fields.
- [ ] Add markout horizons.
- [ ] Add blocker classifications.
- [ ] Produce `impulse_feature_table.jsonl`.

Pass:

- [ ] every row has owner probabilities placeholder
- [ ] every row has markout fields
- [ ] every row has route usability
- [ ] every row has strict/fixup status

## Run75 — Hawkes / Regime Fitting

- [ ] Fit pool-direction event intensity.
- [ ] Fit impulse decay half-life.
- [ ] Fit regime probabilities.
- [ ] Validate against next-event markout.
- [ ] Produce Hawkes/regime reports.

## Run76 — Bayesian Owner Arbiter Backtest

- [ ] Compute owner-mode posteriors.
- [ ] Compare against current selected owner.
- [ ] Evaluate EV lift.
- [ ] Evaluate false-positive suppression.
- [ ] Produce owner-arbiter report.

## Run77 — Optimal Stopping / Entry-Exit Rule Backtest

- [ ] Evaluate continuation exit horizons.
- [ ] Evaluate rebalancing exit horizons.
- [ ] Evaluate flatten/drop saved-loss.
- [ ] Produce executable rule candidates.

## Run78 — Contextual Bandit Offline Simulation

- [ ] Build context/action/reward table.
- [ ] Simulate Thompson/UCB policy.
- [ ] Compare to static owner selection.
- [ ] Report lift and downside.

## Run79 — Continuous No-Send Integration

- [ ] Integrate owner arbiter into no-send simulation.
- [ ] No live send.
- [ ] All owner modes.
- [ ] TPU/public only.
- [ ] Report strict would-fire, post-shock would-fire, and blockers.

## Run80 — Shard Simulation

- [ ] Simulate 1/2/4/8/16 shards.
- [ ] Measure missed EV due to serial execution.
- [ ] Decide whether shard build is immediate priority.

---

# 14. Model Validation Requirements

Each model must report:

- [ ] sample count
- [ ] train/test split or epoch split
- [ ] calibration
- [ ] Brier score for probabilities
- [ ] log loss where applicable
- [ ] lift over baseline
- [ ] EV lift over baseline
- [ ] false positive rate
- [ ] false negative rate
- [ ] adverse excursion distribution
- [ ] route-usable subset performance
- [ ] strict executable subset performance

No model may be used for live decisions until:

- [ ] it improves no-send EV in at least two fresh epochs
- [ ] it does not increase adverse excursion beyond threshold
- [ ] it respects route/inventory/account gates
- [ ] it emits explainable owner/action decisions

---

# 15. Integration Into Shadow Runtime

## 15.1 No-Send First

- [ ] Statistical engine runs in observe/no-send mode only.
- [ ] It emits would-fire actions, not live sends.
- [ ] PM reserve is simulated.
- [ ] Egress is not live.

## 15.2 Runtime Fields

Add to candidate rows:

- [ ] impulse_label
- [ ] regime_probabilities
- [ ] owner_probabilities
- [ ] owner_entropy
- [ ] signal_coherence
- [ ] decay_half_life_ms
- [ ] selected_owner_reason
- [ ] best_exit_horizon
- [ ] optimal_size_bucket
- [ ] shard_assignment_simulated
- [ ] PM_reserve_simulated

## 15.3 Dashboard / UI Fields

Expose in Shadow Console:

- [ ] live impulse pulses
- [ ] owner-mode probability graph
- [ ] signal coherence meter
- [ ] regime state per pool
- [ ] would-fire rows
- [ ] blocker map
- [ ] EV by owner mode
- [ ] shard utilization
- [ ] missed EV by blocker

---

# 16. Success Criteria

## Phase 1 Success

- [ ] Post-shock rows have markout and impulse taxonomy.
- [ ] Owner-mode probabilities are emitted.
- [ ] DirectArb preemption is explicit.
- [ ] Housekeeping is separated into real trade vs fixup/prep.

## Phase 2 Success

- [ ] Hawkes/regime models show positive lift.
- [ ] Continuation/rebalancing optimal stopping rules are identified.
- [ ] Post-shock later-horizon EV becomes executable-rule EV.

## Phase 3 Success

- [ ] Continuous no-send simulation shows improved strict would-fire.
- [ ] Unit-normalized EV increases.
- [ ] Route leaks remain zero.
- [ ] Jito/Harmonic executable count remains zero.

## Phase 4 Success

- [ ] Shard simulation proves or disproves concurrency bottleneck.
- [ ] If proven, shard build plan is generated.

## Phase 5 Success

- [ ] One owner mode has repeated strict executable EV across fresh epochs.
- [ ] Small live proof can be considered under existing PM/TinyLive rules.

---

# 17. Failure / Redesign Triggers

Trigger redesign if:

- [ ] owner probabilities do not improve EV over current hardcoded owner selection
- [ ] post-shock markout is positive but not executable after entry/exit modeling
- [ ] Hawkes/regime models fail calibration
- [ ] strict executable EV remains near zero after account/inventory readiness
- [ ] DirectArb remains blocked after liquid held-input/account readiness
- [ ] Housekeeping remains fixup-only and never becomes real economic trade
- [ ] shard simulation shows no missed EV due to serial execution

No failure means:

```text
Every negative result becomes a blocker map and redesign input.
```

---

# 18. Immediate Next Tasks

- [ ] Build Run74 impulse feature table from the latest strong epoch artifacts.
- [ ] Add owner-mode probability vector schema.
- [ ] Add markout horizon table for every candidate.
- [ ] Build preliminary Hawkes intensity per pool/direction.
- [ ] Build first Bayesian owner arbiter baseline.
- [ ] Audit Housekeeping rows as real trade vs fixup artifact.
- [ ] Audit Continuation rows by later-horizon EV and adverse excursion.
- [ ] Keep DirectArb as deterministic preemption lane.
- [ ] Keep Jito/Harmonic blocked as executable routes.
- [ ] Keep `.15` Agave/`eno1` bounded lease rule until separate packet path is solved.

---

# 19. Supervisor Statement

Shadow must not remain a detector.

Shadow must become:

```text
continuous impulse machine
+ deterministic DirectArb executor
+ probabilistic post-shock owner arbiter
+ optimal stopping engine
+ global PM allocator
+ conflict-aware execution shards
```

The statistical intelligence layer is the path from observed market structure to executable trading behavior.
