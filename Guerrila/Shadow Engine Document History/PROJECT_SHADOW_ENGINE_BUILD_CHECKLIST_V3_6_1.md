# PROJECT SHADOW ENGINE — Build Checklist

Version: **V3.6.1 (V3.6 + Addendum V3.6.1 / ARCHITECTURE_PATCHES_V3.7)**

Purpose: Engineering build-order checklist for the **program architecture** only (code + internal services). External infra provisioning is out of scope.

Principle: Build in layers, freeze interfaces early, and prove determinism via fixtures before performance tuning.

---

## A. Runtime topology (mandatory)

### A1. Create the Rust workspace + binaries

- [ ] Create a Cargo workspace under `rust/` with shared crates:
  - [ ] `types` (shared structs, schema versions)
  - [ ] `shared_mem` (mmap + seqlock + CRC)
  - [ ] `event_log` (append-only; Redis Streams backend + WAL fallback)
  - [ ] `feed` (Yellowstone/Jito adapters)
  - [ ] `alts` (ALT cache + v0 flattening)
  - [ ] `protocols` (Raydium/Pump adapters)
  - [ ] `quoter` (curve math + forward-sim + Token-2022 fee math)
  - [ ] `leader` (leader schedule + route selection)
  - [ ] `submitter` (Jito + staked fallback; single signer)

- [ ] Create Rust binaries:
  - [ ] `shadow-state` (standalone state daemon)
  - [ ] `shadow-entry-engine` (feed + mapping + gating + intent emission)
  - [ ] `shadow-position-manager` (lifeboat evaluator; emits ExitIntent)
  - [ ] `shadow-submit` (single signer authority; simulation + fees + send + receipts)

### A2. Create the Python package

- [ ] `python/shadow_oracle/` (Module 1 Oracle + Nearline mint risk)
- [ ] Shared-memory writers (publish watchlist/entities/mint_risk)

### A3. Shared memory segments (canonical hot reads)

- [ ] Define versioned schemas and seqlock headers for:
  - [ ] watchlist (wallet → entity)
  - [ ] entity registry (entity → gating + EV)
  - [ ] mint risk registry (mint → APPROVE/REJECT/UNKNOWN + freshness)
  - [ ] pool snapshots
  - [ ] mint invariant snapshots
  - [ ] heartbeats + kill switch

**Pass criteria:**
- [ ] Writer publishes N then N+1 versions; readers hot-reload without restart.
- [ ] CRC mismatch is detected and rejected.
- [ ] Readers never block; they either accept a snapshot or skip.

### A4. Intent transport (no Redis on hot-path)

- [ ] Implement EntryIntent and ExitIntent structs (idempotent intent_id)
- [ ] Implement shared-memory ring buffer (or UDS) for intent submission

**Pass criteria:**
- [ ] 1M intents pushed in a synthetic test without deadlocks.
- [ ] Submitter rejects duplicate intent_ids deterministically.

### A5. Canonical event log (position handoff)

- [ ] Implement append-only event log API:
  - [ ] Redis Streams backend (Phase 1)
  - [ ] WAL fallback backend (Phase 2 / outages)

**Pass criteria:**
- [ ] Position Manager can rebuild full state from an empty DB by replaying the log.
- [ ] Crash mid-run and restart preserves correctness (no lost PositionOpened).

---

## B. Pillar compliance checklist (maps to the 22-point matrix)

### Pillar 1 — Distributed topology & IPC

- [ ] **Single signer authority:** only `shadow-submit` holds keys and sends transactions
- [ ] **Exit preemption:** exits always processed before entries
- [ ] **Event-sourced position state:** PM consumes event log; does not depend on entry RAM
- [ ] **Standalone state daemon:** `shadow-state` provides pricing snapshots to both entry and PM

### Pillar 2 — State reality & latency

- [ ] Fork-aware rollbacks in `shadow-state`
- [ ] Absolute zero hot-path RPC for risk/toxicity/holders
- [ ] PM boot hydration from on-chain balances

### Pillar 3 — Adversarial MEV

- [ ] Target liveness guard (Phase 1)
- [ ] Dynamic slippage + bounded deterministic jitter
- [ ] Forward-sim quoting (T+1) for backruns
- [ ] Non-Jito fallback uses staked provider/relayer by default

### Pillar 4 — Asset mechanics

- [ ] Idempotent ATA creation (SPL vs Token-2022) + WSOL lifecycle
- [ ] Strict copyable profile gate (drop complex/aggregated tx shapes)
- [ ] Mint honeypot invariants (FreezeAuthority / TransferHook) from cached mint state
- [ ] Token-2022 TransferFeeConfig applied in quoter
- [ ] Pump curve completion abort (when Pump adapter exists)
- [ ] Proportional sizing (avoid “whale in puddle”)

### Pillar 5 — Capital preservation

- [ ] Native SOL reserve floor enforced
- [ ] Zombie exit max retry cap + DEAD_ASSET terminal state
- [ ] ATA close on full exit
- [ ] Transfer-bypass destination classification

---

## C. Module build order

### Module 1 — Oracle (Python)

#### 1A — Managed-data ingestion adapters + fixtures
- [ ] Fixture-driven ingestion into canonical tables for tests
- [ ] Production adapter that reads managed datasets (BigQuery/Dune/Bitquery)
- [ ] Explicitly forbid building a historical indexer in this repo

**Pass criteria:**
- [ ] Ingestion is idempotent (rerun produces identical tables)
- [ ] Dedupe rules are deterministic

#### 1B–1F — Entity scoring pipeline
- [ ] Copy-adjusted outcome labeling
- [ ] Entity resolution (funding/sweep/timing edges)
- [ ] Crowdedness modeling
- [ ] Conviction anomaly baselines
- [ ] Conservative EV outputs (`p_win_lcb`, `b_win_lcb`)

**Pass criteria:**
- [ ] Offline pipeline produces stable outputs on replay

#### 1G — Publishers
- [ ] Publish watchlist/entities/mint_risk to shared memory

**Pass criteria:**
- [ ] Rust readers can consume without parsing on hot path (background reload into maps)

---

### Module 2 — Feed ingestion (Entry Engine)

- [ ] Implement Yellowstone gRPC adapter (connect/reconnect/subscribe/resubscribe)
- [ ] Normalize into internal PreConsensus event model
- [ ] Signature dedupe

**Pass criteria:**
- [ ] Can replay recorded fixtures into identical normalized objects
- [ ] Resubscription updates without restart

---

### Module 3 — ALT cache + account mapping (Entry Engine)

- [ ] Implement ALT cache (cache-first)
- [ ] Implement v0 flattening
- [ ] Implement instruction selection + routing to adapters

**Pass criteria:**
- [ ] v0 flattening matches expected ordering
- [ ] ALT miss fails closed (no silent wrong account mapping)

---

### Module 4 — State daemon + quoting

#### 4B — `shadow-state` (fork-aware)
- [ ] Subscribe to account updates for pool/mint accounts
- [ ] Maintain slot status tracking
- [ ] Implement rollback buffer
- [ ] Publish snapshots to shared memory

**Pass criteria:**
- [ ] Synthetic fork-switch test rolls state back correctly

#### 4A — Raydium V4 adapter
- [ ] Intent extraction
- [ ] Strict copyable profile gate

**Pass criteria:**
- [ ] Decode fixtures and reconstruct intent deterministically

#### 4C — Quoter library
- [ ] Forward-sim target impact for backrun quote
- [ ] Token-2022 transfer fee adjustments
- [ ] Output price impact bps + expected_out + min_out

**Pass criteria:**
- [ ] Post-target expected_out is <= pre-target expected_out for same snapshot

---

### Module 5 — Risk + gating (Entry Engine)

- [ ] Online Oracle gate reads shared memory (cache-only)
- [ ] Mint invariants gate (FreezeAuthority/TransferHook)
- [ ] Mint risk registry gate (APPROVE/REJECT/UNKNOWN)

**Pass criteria:**
- [ ] No network calls during gating path (enforced by tests/mocks)

---

### Module 6/7 — Submitter (single signer)

- [ ] Blockhash tracking
- [ ] Leader schedule tracking
- [ ] Simulation + compute budgeting
- [ ] Dynamic slippage and proportional sizing enforcement (policy)
- [ ] Fee policy
- [ ] Jito sendBundle / sendTransaction
- [ ] Non-Jito fallback via staked provider
- [ ] Event log emission for position lifecycle

**Pass criteria:**
- [ ] Submitter rejects duplicate intent_ids
- [ ] Exit intents preempt entry intents
- [ ] Sends route deterministically based on leader

---

### Module 11 — Position manager (lifeboat)

- [ ] Boot hydration from chain
- [ ] Stop-loss / take-profit / timeout
- [ ] Transfer-bypass detection + destination classification
- [ ] Max exit retries → DEAD_ASSET
- [ ] ATA close on full exit
- [ ] Emits ExitIntents only (no signing)

**Pass criteria:**
- [ ] Exit retry cap prevents infinite fee bleed
- [ ] Destination classification fixtures pass

---

## D. Dry-run gates

- [ ] 48h dry-run with:
  - [ ] event log enabled
  - [ ] persistence enabled
  - [ ] benchmarking enabled
- [ ] Load test: feed replay while PM evaluates N open positions

**Pass criteria:**
- [ ] No queue unbounded growth
- [ ] No ingestion throughput collapse under PM load
- [ ] Deterministic replay outcomes

