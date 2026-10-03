# Shadow UI Wrapper Specification

**Document type:** implementation checklist + product/UX specification  
**Working name:** Shadow Console  
**Purpose:** build a modern, non-invasive UI/UX wrapper for operating Shadow, inspecting runs, controlling host posture, and understanding execution state without changing the hot path.

---

## 0. Prime directive

Shadow Console must be a **display and control wrapper**, not a new trading engine.

```text
UI observes, orchestrates, and audits.
UI does not become part of the money-worker hot path.
UI does not make market decisions.
UI does not change fixed IPC contracts.
UI does not inject latency into AF_XDP, worker, PM, or egress paths.
```

The backend may launch existing scripts, read logs, read summaries, inspect `/dev/shm` state, and manage host services through allowlisted commands. It must not add blocking calls, network calls, filesystem calls, UI calls, or dynamic waits inside the hot execution path.

---

## 1. Design doctrine

### 1.1 Non-invasive rule

- [ ] The UI must not modify `shadow-tier0`, money-worker hot path, AF_XDP producer loop, PM reservation logic, egress queues, or shared-memory ABI.
- [ ] The UI may only interact through existing outer surfaces:
  - [ ] host scripts in `g1-host-pack/scripts/`
  - [ ] systemd units
  - [ ] logs
  - [ ] result directories
  - [ ] summaries and JSON reports
  - [ ] `/dev/shm` read-only probes
  - [ ] explicit allowlisted control actions
- [ ] All write/control operations must go through a backend command allowlist.
- [ ] No arbitrary shell execution from the browser.
- [ ] No UI-originated command may run unless it is represented as a typed action with an explicit schema.

### 1.2 Latency isolation rule

The frontend may lag; the engine may not.

```text
UI latency acceptable: 100 ms to several seconds
Engine latency tolerance: unchanged
```

- [ ] UI polling must be bounded and rate-limited.
- [ ] UI must never poll money-worker internals in a way that competes with hot-path cache, CPU, or file descriptors.
- [ ] High-frequency visualizations should read from exported summaries, rolling snapshots, or a sidecar telemetry buffer, not from hot-path code.
- [ ] If the UI becomes slow or crashes, Shadow runs must continue unaffected.

### 1.3 Fail-closed rule

- [ ] UI commands must fail closed if host state is ambiguous.
- [ ] Real send controls must be hidden/disabled unless explicitly enabled by config and current proof state.
- [ ] Run buttons must surface required prerequisites before execution.
- [ ] Dangerous actions require a modal confirmation plus typed confirmation text.
- [ ] The default UI mode is observe/control/no-send only.

---

## 2. Product goals

Shadow Console should make the project feel like a controlled machine instead of a black-box pile of commands.

### 2.1 Primary operator goals

- [ ] See host and engine status at a glance.
- [ ] See Agave / `eno1` ownership state clearly.
- [ ] Launch bounded AF_XDP windows safely.
- [ ] Run diagnostics without SSH command recall.
- [ ] Run Run56 / Run60 / Run61 / Run62 / Run63 / Run64D / Run65 workflows from buttons.
- [ ] Inspect result artifacts and compare runs.
- [ ] Track blockers, deltas, and conversion funnels visually.
- [ ] See candidate flow as a live graph: ingest → worker → owner mode → PM → route → egress.
- [ ] Understand why opportunities did not fire.
- [ ] Export supervisor-ready summaries.

### 2.2 What the UI is not

- [ ] Not an HFT execution loop.
- [ ] Not a replacement for money workers.
- [ ] Not a new strategy layer.
- [ ] Not a hidden public fallback.
- [ ] Not a generalized remote shell.
- [ ] Not a manual candidate-picking production pipeline.

---

## 3. Recommended architecture

```text
Browser / desktop frontend
        ↓ WebSocket / HTTP
Shadow UI backend daemon
        ↓ allowlisted actions + read-only probes
Host scripts, systemd, logs, result artifacts, /dev/shm snapshots
        ↓
Existing Shadow engine remains unchanged
```

### 3.1 Frontend

Recommended stack:

- [ ] React or Svelte frontend.
- [ ] React Flow / D3 / Canvas for node graph and pulses.
- [ ] Tailwind or equivalent for fast modern UI.
- [ ] WebSocket stream for logs and run state.
- [x] Static local build served by backend or Tauri shell.

Acceptable packaging options:

```text
Option A: local web app on .15, accessed through SSH tunnel
Option B: desktop app via Tauri wrapping local backend API
Option C: local LAN-only web console with auth
```

Preferred first version:

```text
localhost-only web console + SSH tunnel
```

- [x] Implemented in `frontend/` as a localhost-only web console served by a small Node backend.

Reason: simplest, least invasive, easiest to secure.

### 3.2 Backend

Recommended stack:

```text
Rust backend if we want long-term type safety.
Python/FastAPI backend if we want fastest prototype.
```

For first implementation, use a small backend with:

- [x] typed action registry
- [x] command allowlist
- [ ] structured run state database
- [x] artifact indexer
- [ ] log tail streamer
- [x] `/dev/shm` read-only probe adapters
- [x] systemd adapter
- [ ] SSH/OOB awareness fields

### 3.3 Backend action model

Every action must be described like this:

```yaml
action_id: run56_canonical_afxdp_sequence
label: Run56 Canonical AF_XDP Sequence
risk_level: medium
requires_root: true
requires_oob_arm: true
requires_agave_green: true
live_send_possible: false
script: g1-host-pack/scripts/56_canonical_afxdp_current_candidate_sequence.sh
allowed_args:
  - window_seconds
  - results_dir
preflight_checks:
  - agave_green
  - eno1_no_xdp
  - shadow_state_running
  - route_policy_active
post_actions:
  - detach_xdp
  - restart_agave
```

No arbitrary user-supplied shell strings.

---

## 4. Core UI screens

## 4.1 Command Center

Purpose: single view for current readiness and safe run launch.

Required widgets:

- [x] Host identity: `.15`, hostname, uptime, timezone.
- [ ] Agave status:
  - [x] active unit
  - [x] local health
  - [ ] processed slot lag
  - [ ] finalized slot lag
  - [x] UDP listener count
  - [ ] dynamic port range status
- [x] `eno1` state:
  - [x] `LOWER_UP`
  - [x] XDP attached / detached
  - [x] XSK map present / absent
  - [x] current owner: Agave / Shadow / Unknown
- [x] Shadow process state:
  - [x] `shadow-state`
  - [x] `shadow-oracle`
  - [x] `shadow-tier0`
  - [x] `shadow-egress-tpu`
- [ ] Route policy state:
  - [x] `block_private_bundle_routes=true`
  - [ ] `jito_bundle_executable_count=0` latest run
  - [x] `tpu_quic_public` enabled
  - [ ] public RPC route state
- [ ] Current run status.
- [x] Latest blocker.
- [x] Safe action buttons.

Primary buttons:

- [ ] Recover Agave / release `eno1`.
- [x] Check local Agave preflight.
- [ ] Arm OOB flag.
- [ ] Run bounded AF_XDP window.
- [ ] Run Run56 canonical sequence.
- [ ] Run Run60/61/62/63 analysis.
- [ ] Run Run64D plan/simulate only.
- [ ] Refresh signer inventory.
- [ ] Detach XDP and return `eno1` to Agave.

Danger buttons must be separated:

- [x] Execute acquisition one mint.
- [x] TinyLive real send.
- [x] Increase live cap.

These should be disabled by default.

- [x] Prototype renders these danger controls separated and disabled by default.

---

## 4.2 Live Pipeline Graph

Purpose: modern node/pulse visualization of Shadow’s current pipeline.

Graph nodes:

```text
Agave / Local validator
AF_XDP
Decode
Pool/state lookup
Quote
Owner selector
PM reserve
Route decision
TPU/QUIC egress
Public RPC egress
PM reconcile
Inventory
```

Visual behavior:

- [x] Pulses moving from AF_XDP → worker → PM → egress.
- [x] Node color indicates state:
  - [x] green = healthy / moving
  - [x] yellow = moving but blocked
  - [x] red = failed / stale
  - [x] gray = inactive
- [ ] Edges show counts and rates.
- [ ] Click any node for details.
- [ ] Filters by owner mode.
- [ ] Filters by source: AF_XDP / external geyser / local validator.
- [ ] Filters by route: TPU / public / Jito theoretical / blocked.

Example node counters:

```text
AF_XDP frames seen
worker_route_actionable
worker_quote_ok
worker_process_intent_ok
owner_selected_directarb
would_fire_total
would_enqueue_tpu_count
pm_reserve_count
pm_release_count
```

---

## 4.3 Agave / eno1 Control Panel

Purpose: encode the `.15` operating rule into UI so it cannot be forgotten.

Required state machine:

```text
AGAVE_OWNS_ENO1
  -> SHADOW_ARMED
  -> AFXDP_WINDOW_RUNNING
  -> SHADOW_DETACHING
  -> AGAVE_RECOVERING
  -> AGAVE_OWNS_ENO1
```

Required checklist before AF_XDP attach:

- [ ] `shadow-tier0` stopped.
- [ ] `shadow-oracle` stopped.
- [ ] `eno1` has no XDP program.
- [ ] XSK map absent or cleared.
- [ ] Agave unit active.
- [ ] local health ok.
- [ ] processed/finalized lag acceptable.
- [ ] OOB arm flag present.

Buttons:

- [ ] Stop stale Shadow processes.
- [ ] Detach XDP from `eno1`.
- [ ] Restart Agave.
- [ ] Wait until Agave green.
- [ ] Arm OOB.
- [ ] Launch bounded AF_XDP run.
- [ ] Return `eno1` to Agave.

Warnings:

```text
Do not attach AF_XDP before Agave is green.
Do not leave XDP attached after a bounded run.
Do not trust candidate-zero windows until L0-L5 are green.
```

---

## 4.4 Run Launcher

Purpose: launch known run families with typed parameters.

Supported run cards:

- [ ] Run56 — canonical AF_XDP current-candidate sequence.
- [ ] Run60 — post-shock capital efficiency.
- [ ] Run61 — continuous shadow-live no-send simulation.
- [ ] Run62 — conversion autopsy.
- [ ] Run63 — DirectArb conversion pass.
- [ ] Run64B — metadata-only Token-2022 prep planner.
- [ ] Run64D — bounded metadata acquisition executor.
- [ ] Run65 — multi-shard continuous simulation.
- [ ] Run66 — post-shock executable rule proof.

Each run card must show:

- [ ] purpose
- [ ] risk level
- [ ] live send possible? yes/no
- [ ] root required? yes/no
- [ ] preflight requirements
- [ ] expected artifacts
- [ ] current defaults
- [ ] run history
- [ ] last success / failure
- [ ] launch button

Example Run61 card:

```text
Run61 continuous shadow-live no-send simulation
Risk: medium
Live send: no
Requires: Agave green, OOB arm, AF_XDP bounded window
Outputs: continuous would-fire metrics, owner mode split, route split, deployable_by_us
```

---

## 4.5 Diagnostics Center

Purpose: replace repeated SSH command fragments with audited buttons.

Diagnostics:

- [x] Local Agave preflight.
- [x] `eno1` XDP state.
- [x] Dynamic UDP listener check.
- [ ] AF_XDP live counters.
- [ ] Feed ring liveness.
- [ ] Worker record liveness.
- [x] Route policy check.
- [ ] Jito/Harmonic executable count check.
- [x] TPU ring readiness.
- [ ] PM result/reconcile readiness.
- [x] Signer inventory refresh/check.
- [ ] Token mint policy checker.
- [ ] Entry/exit liquidity checker.

Zero-candidate ladder panel:

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

- [x] Prototype exposes the zero-candidate ladder as an allowlisted diagnostic button.

UI must highlight the first red level.

---

## 4.6 Monetization Dashboard

Purpose: turn run outputs into business visibility.

Panels:

- [ ] Fresh AF_XDP opportunities.
- [ ] Deployable-by-us conversion ratio.
- [ ] Owner-mode counts.
- [ ] DirectArb autopsy.
- [ ] Post-shock / continuation markout.
- [ ] Route conversion.
- [ ] Inventory and ATA blockers.
- [ ] Capital efficiency buckets.
- [ ] Shard simulation output.
- [ ] EV by owner mode.
- [ ] Missed EV by blocker.

Required metrics:

```text
market_opportunity_count
shadow_ready_count
deployable_by_us_count
would_fire_count
would_fire_directarb
would_fire_continuation
route_unavailable_count
inventory_blocked_count
not_held_input_count
ata_missing_count
template_missing_count
quote_missing_count
```

For post-shock:

```text
immediate_ev
later_horizon_ev
best_horizon
max_adverse_excursion
saved_loss_ev
owner_mode_win_rate
```

---

## 4.7 Candidate Explorer

Purpose: inspect candidate rows without manually using `jq`.

Features:

- [ ] Search by pool, mint, signature, owner mode, source, route.
- [ ] Sort by EV, retained EV, later-horizon markout, blocker, route.
- [ ] Normalize units by mint decimals.
- [ ] Show raw and normalized values side-by-side.
- [ ] Show route usability.
- [ ] Show exact blocker stack.
- [ ] Show whether row is theoretical, shadow-ready, or deployable-by-us.
- [ ] Export selected row as supervisor paste block.

Required row fields:

```text
source
venue
pool
mint_in
mint_out
owner_mode
observed_route
usable_route_for_us
trade_size_raw
trade_size_native
trade_size_usd_equiv
net_after_fee_native
net_after_fee_sol_equiv
retained_ev_native
retained_ev_sol_equiv
quote_expected_out
min_out
execution_blocker
first_blocker
would_fire
```

---

## 4.8 Inventory and Token Risk Panel

Purpose: prevent another Coinini-style blind acquisition.

Panels:

- [ ] Current signer token inventory.
- [ ] Held-input mints.
- [ ] Approved but unheld mints.
- [ ] Experimental inventory.
- [ ] Dead/dust inventory.
- [ ] Output ATAs present / missing.
- [ ] Mint policy verdicts.
- [ ] Entry/exit liquidity status.

Every token should be classified:

```text
core_inventory
approved_held_input
experimental_held_input
policy_safe_but_exit_dust
denylisted
unknown
```

Acquisition cannot be allowed unless:

```text
entry_quote_ok=true
exit_quote_ok=true
exit_value_not_dust=true
policy_verdict=pass
max_spend_cap_pass=true
operator_approval=true
```

Coinini / `8Nh2...` should be displayed as:

```text
policy_safe_but_exit_dust
no_scale
test_artifact_only
```

- [x] Prototype displays Coinini / `8Nh2...` as `policy_safe_but_exit_dust`, `no_scale`, and `test_artifact_only`.

---

## 4.9 Shard Simulation Panel

Purpose: table the future parallelism work explicitly so it is not forgotten.

Run65 visualization should show:

```text
1 shard
2 shards
4 shards
8 shards
16 shards
```

For each shard count:

- [ ] would-fire count
- [ ] DirectArb fire count
- [ ] post-shock fire count
- [ ] captured EV
- [ ] missed EV
- [ ] conflict rate
- [ ] shard utilization
- [ ] capital utilization
- [ ] missed due to account conflict
- [ ] missed due to inventory conflict
- [ ] missed due to no free shard

Display model:

```text
one shared observation brain
one global PM/capital allocator
many conflict-aware execution shards
```

Hard warning:

```text
Do not run independent duplicate Shadows.
```

- [x] Prototype renders the 1/2/4/8/16 shard matrix placeholders and the shared-brain/global-PM warning.

---

## 5. Backend data sources

### 5.1 Filesystem artifacts

The UI backend should index:

```text
/home/ubuntu/shadow-host-audit/dpdk-runs/*/summary.txt
/home/ubuntu/shadow-host-audit/dpdk-runs/*/*.json
/home/ubuntu/shadow-host-audit/dpdk-runs/*/*.jsonl
/home/ubuntu/shadow-host-audit/dpdk-runs/*/shadow-tier0.log
/home/ubuntu/shadow-host-audit/dpdk-runs/*/shadow-egress-tpu.log
```

### 5.2 Runtime probes

Read-only probes:

- [x] `systemctl is-active` for known units.
- [x] `ip -details link show dev eno1`.
- [x] `ss -lunp` for UDP listeners.
- [x] local RPC health / slot.
- [x] file stat for `/dev/shm` segments.
- [ ] tail logs.
- [ ] parse gate JSON.

### 5.3 Command execution

All commands must be defined in a registry:

```text
command_id
script_path
args_schema
risk_level
requires_root
requires_confirmation
preflight_checks
postflight_checks
```

No raw user shell input.

---

## 6. Security and access control

### 6.1 Access model

- [x] Bind backend to `127.0.0.1` by default.
- [x] Access through SSH tunnel.
- [ ] Optional token auth for remote LAN.
- [x] No public internet exposure.
- [x] Audit log every action.

### 6.2 Authorization levels

```text
viewer:
  read dashboards and logs only

operator:
  run diagnostics and no-send windows

supervisor:
  acquisition simulate/execute one-mint tasks
  config patch tasks

admin:
  emergency actions and service control
```

### 6.3 Dangerous actions

Require supervisor/admin confirmation:

- [ ] execute acquisition
- [ ] run TinyLive
- [ ] change live caps
- [ ] execute any real send
- [ ] approve new token policy class
- [ ] acquire new held-input inventory

---

## 7. Run-state model

Each run should be tracked as:

```json
{
  "run_id": "65_full_potential_shard_simulation_2026...",
  "status": "running|complete|blocked|failed",
  "started_at": "...",
  "ended_at": "...",
  "host": ".15",
  "risk_level": "medium",
  "live_send": false,
  "oob_armed": true,
  "agave_preflight": "pass",
  "eno1_state_before": "agave_owned",
  "eno1_state_after": "agave_owned",
  "summary_path": "...",
  "primary_blocker": "route_unavailable",
  "next_action": "Run62 conversion autopsy"
}
```

UI must show run status and whether cleanup completed.

---

## 8. Implementation phases

## Phase 0 — Markdown + design freeze

- [x] Write UI wrapper spec.
- [ ] Review with operator.
- [x] Decide web app vs Tauri.
- [x] Choose backend language.

## Phase 1 — Read-only dashboard

- [x] Backend indexes run artifacts.
- [x] UI displays latest runs.
- [x] UI displays Agave / `eno1` status.
- [x] UI displays route policy state.
- [x] UI displays latest blockers.
- [ ] No command execution yet.

Exit proof:

```text
UI can load last 10 runs and summarize status/blockers without touching engine.
```

## Phase 2 — Diagnostics buttons

- [x] Local Agave preflight button.
- [x] `eno1` XDP status button.
- [x] signer inventory check button.
- [x] route policy check button.
- [x] zero-candidate ladder button.

Exit proof:

```text
UI diagnostics match manual CLI outputs.
```

## Phase 3 — Safe run launcher

- [ ] Run56 button.
- [ ] Run60/61/62/63 buttons.
- [ ] Run64D plan/simulate buttons.
- [ ] No execute mode yet.
- [ ] Typed parameters only.

Exit proof:

```text
UI-launched no-send run produces same artifacts as manual command.
```

## Phase 4 — Control actions

- [ ] Recover Agave / release `eno1`.
- [ ] OOB arm.
- [ ] Start bounded AF_XDP window.
- [ ] Detach XDP after run.
- [ ] Restart Agave.

Exit proof:

```text
UI can complete Agave -> AF_XDP -> Agave lifecycle without leaving XDP attached.
```

## Phase 5 — Monetization dashboards

- [ ] Candidate explorer.
- [ ] Owner-mode dashboard.
- [ ] Capital bucket dashboard.
- [ ] Route usability dashboard.
- [x] Token risk dashboard.
- [x] Shard simulation dashboard.

Exit proof:

```text
UI generates supervisor-ready monetization report from Run65/Run66 artifacts.
```

## Phase 6 — Controlled acquisition workflow

- [ ] Run64D plan mode.
- [ ] Run64D simulate mode.
- [ ] Run64D execute one-mint mode with confirmation.
- [ ] Exit-liquidity gate display.
- [ ] Inventory refresh and verification.

Exit proof:

```text
UI can simulate acquisition safely and blocks execution if exit liquidity is dust.
```

## Phase 7 — Live-send controls, only after proof

This phase is intentionally deferred.

Required before implementation:

- [ ] repeated deployable EV evidence
- [ ] TinyLive route proof
- [ ] PM reconcile proof
- [ ] cap policy approved
- [ ] loss budget approved
- [ ] operator kill switch implemented

---

## 9. UI visual design

### 9.1 Style

- [x] Dark modern interface.
- [x] Node graph with animated pulses.
- [x] Soft glows for active pipeline stages.
- [x] Red/yellow/green status tokens.
- [x] High-contrast warnings for dangerous actions.
- [x] Compact tables for run outputs.
- [ ] Expandable log drawers.
- [ ] Timeline view for run lifecycle.

### 9.2 Visual metaphors

```text
Pulses = candidate flow
Nodes = engine components
Edges = queue / route transitions
Glow intensity = event rate
Red breaks = blocker
Dotted edges = theoretical route only
Solid edges = usable route
```

Example route visualization:

```text
AF_XDP -> Decode -> Owner Selector -> PM -> TPU/QUIC
                         \-> Jito Bundle [dotted, unavailable]
```

### 9.3 Operator UX priorities

- [x] clear current state
- [x] clear next safe action
- [x] clear blocker
- [x] clear proof artifact
- [ ] clear cleanup status

The operator should never need to ask:

```text
Is Agave green?
Is XDP still attached?
Did the run finish?
What is the blocker?
Where is the summary?
Did this use Jito by mistake?
```

---

## 10. Acceptance checklist

Shadow Console v0 is acceptable when:

- [x] It runs without modifying engine hot path.
- [x] It can read and summarize run artifacts.
- [x] It can show Agave / `eno1` state.
- [ ] It can launch safe no-send runs through allowlisted commands.
- [ ] It can return `eno1` to Agave after a bounded run.
- [ ] It can display owner-mode and route split.
- [ ] It can show deployable-by-us conversion.
- [x] It can show token risk / exit liquidity.
- [x] It logs all commands.
- [x] It cannot execute arbitrary shell commands.
- [x] It cannot accidentally trigger TinyLive or real send.

---

## 11. Hard non-goals

- [x] No UI changes to money-worker hot path.
- [x] No candidate manual-picking as production pipeline.
- [x] No browser-originated arbitrary command execution.
- [x] No hidden public fallback.
- [ ] No Jito/Harmonic counted as executable while access is absent.
- [x] No live-send button until live-send proof gates are explicitly green.
- [x] No token acquisition without entry and exit liquidity gates.

---

## 12. Immediate next implementation task

Build Phase 1 + Phase 2 first:

```text
Read-only dashboard + diagnostics buttons.
```

Do not start with acquisition or live-send controls.

Minimum first demo:

```text
Open Shadow Console.
See .15 host status.
See Agave health and eno1 XDP status.
See latest Run61/62/63/64D summaries.
Click latest blocker.
Click zero-candidate ladder diagnostic.
No engine behavior changes.
```
