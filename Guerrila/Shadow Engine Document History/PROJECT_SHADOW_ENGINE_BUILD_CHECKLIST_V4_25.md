# PROJECT SHADOW ENGINE — Implementation Checklist (V4.25)

Version: **V4.25.0**  
Date: **2026-03-01**  
Source of truth: `PROJECT_SHADOW_ENGINE_V4_ENGINEER_SPEC.md`

## Scope (V4.25 = first quarter of V4)

This checklist covers **the first ~25% of the V4 build**, focused on **runtime topology + inter-process contracts + the signing/dispatch fault-domain**.

**In-scope (must be fully implemented + tested in V4.25):**
- Rust workspace + binaries (process topology)
- Canonical types + versioning
- `/dev/shm` shared-memory segments (seqlock+CRC + triple-buffer for high-churn)
- Cross-language writer rule enforcement (Python→Rust schema authority)
- Intent transport (idempotent `EntryIntent` / `ExitIntent` via shared-memory queue)
- Canonical append-only event log (WAL backend required; Redis Streams optional behind feature flag)
- Process heartbeats + kill switch + “Entry must gate on PM heartbeat”
- `shadow-submit` (single signer authority) skeleton:
  - intent dedupe
  - exit-preemption
  - blockhash cache thread
  - leader-aware route selection *logic* (network I/O can be stubbed in dry-run)

**Explicitly out-of-scope (do NOT implement yet in V4.25):**
- Live Yellowstone gRPC/ShredStream ingestion
- ALT resolution + v0 key flattening
- Raydium/Pump protocol adapters (IDL/Borsh parsing)
- Local pool-state quoter math + forward-simulation math
- Token-2022 invariant extraction from real mint accounts
- Jito bundle construction + real network submission
- MEV Router on-chain program (`shadow_mev_router`)

Those begin in **V4.50**.

---

## V4.25 Exit Criteria (Quarter-1 “Done” Gate)

V4.25 is considered **PASS** only when all are true:

- [ ] `cargo test --workspace` passes on a clean machine
- [ ] Unit tests cover: shared-mem atomicity/CRC, triple-buffer, intent queue, event log replay, submitter preemption/dedupe, heartbeat gating
- [ ] An end-to-end offline integration test can:
  1) start the four Rust processes in “fixture mode”,  
  2) emit a synthetic `EntryIntent`,  
  3) have `shadow-submit` accept it and emit `EntryIntentAccepted`,  
  4) append events into WAL,  
  5) have `shadow-position-manager` reconstruct state from WAL replay.
- [ ] Entry Engine refuses to emit entries if PM heartbeat is stale/unhealthy.
- [ ] All hot-path reads are from shared memory only (no sockets).
- [ ] `shadow-submit` is the **only** component that can access signer key material.

---

## 0) Project conventions and “do-not-break” rules

### 0.1 Non-negotiables to enforce in code (compile-time if possible)

- [ ] **Single signer authority:** only `shadow-submit` loads keypair/seed and can sign.
  - **Test (PASS):** Attempting to build entry/PM crates with any key-loading module fails compilation via feature-gated crate separation.
- [ ] **No socket IPC for hot-path:** entry → submit uses shared-memory intent queue, not Redis.
  - **Test (PASS):** grep/static check in CI blocks `redis`/`reqwest`/`tonic` usage inside `shadow-entry-engine` hot loop module.
- [ ] **Shared-memory is canonical for hot reads:** all gating/quotes read from `/dev/shm` segments.
  - **Test (PASS):** integration test runs with network disabled and still processes intents offline.
- [ ] **Exits preempt entries** in submitter.
  - **Test (PASS):** submitter consumes an exit intent ahead of an older entry intent (unit test).

### 0.2 Repository layout (minimum for V4.25)

- [ ] `rust/` cargo workspace
- [ ] `python/` package skeleton (only for shared-mem writer tests; full Oracle is later)
- [ ] `specs/` contains V4 and checklists

---

## 1) Initialize the Rust workspace + binaries

### 1.1 Create Cargo workspace

- [ ] Create `rust/Cargo.toml` workspace root.
- [ ] Add shared crates (V4.25 requires these to be real, not placeholders):
  - [ ] `crates/types`
  - [ ] `crates/shared_mem`
  - [ ] `crates/intent_queue`
  - [ ] `crates/event_log`
  - [ ] `crates/submitter_core`
  - [ ] `crates/test_fixtures` (synthetic tx + synthetic intents + temp shm paths)
  - [ ] `crates/integration_harness` (spawns processes or runs them in-thread for tests)

- [ ] Add process crates / binaries:
  - [ ] `bins/shadow-state`
  - [ ] `bins/shadow-entry-engine`
  - [ ] `bins/shadow-position-manager`
  - [ ] `bins/shadow-submit`

**Implementation notes:**
- Keep “core logic” in `crates/*` and binaries as thin CLIs.
- Make “fixture mode” a first-class feature flag:
  - `--fixture-mode` (runtime) OR `--features fixture` (build-time).

**Tests (PASS):**
- [ ] `cargo build --workspace` succeeds.
- [ ] `cargo test -p types` succeeds.
- [ ] `cargo test -p shared_mem` succeeds.
- [ ] `cargo test -p intent_queue` succeeds.
- [ ] `cargo test -p event_log` succeeds.
- [ ] `cargo test -p integration_harness` succeeds.

---

## 2) Canonical type system + schema versioning (`crates/types`)

### 2.1 Define core primitives

- [ ] Implement canonical primitives:
  - [ ] `PubkeyBytes([u8; 32])`
  - [ ] `SignatureBytes([u8; 64])`
  - [ ] `Hash32([u8; 32])`
  - [ ] `Slot(u64)`
  - [ ] `UnixMs(u64)`
- [ ] Implement enums as `#[repr(u8)]` and never reorder variants after release:
  - [ ] `Protocol` (RAYDIUM_V4, PUMP, PUMP_V2, UNKNOWN)
  - [ ] `Side` (BUY, SELL)
  - [ ] `RiskStatus` (APPROVE, REJECT, UNKNOWN)
  - [ ] `EventKind` (EntryIntentAccepted, ExitIntentAccepted, ExecutionSent, ExecutionLanded, ExecutionFailed, PositionOpened, PositionIncreased, PositionReduced, PositionClosed, Heartbeat, KillSwitchChanged, ...)

**Tests (PASS):**
- [ ] `#[test] sizes_and_alignments()` asserts `size_of`/`align_of` for primitives.
- [ ] `#[test] enum_discriminants_stable()` asserts explicit discriminant values.

### 2.2 Shared memory framing types

- [ ] Define `SharedMemHeader` exactly as in V4:
  - `seq: u64`
  - `version: u32`
  - `payload_len: u32`
  - `crc32: u32`
  - `updated_at_slot: u64`

**Tests (PASS):**
- [ ] `#[test] shm_header_layout()` asserts `size_of::<SharedMemHeader>()` and field offsets (use `memoffset`).

### 2.3 Intent types (idempotent commands)

Implement the exact intent boundaries from V4:

- [ ] `IntentContext` (minimum fields):
  - `program_id: PubkeyBytes`
  - `side: Side`
  - `mint_in: PubkeyBytes`
  - `mint_out: PubkeyBytes`
  - `amount_in: u64`
  - `min_out: u64`
  - `target_ix_index: u8`
  - `target_signature: SignatureBytes`
  - `target_slot: Slot`
  - `pool_keys: bytes` (opaque, versioned)
- [ ] `Quote` (minimum fields):
  - `expected_out: u64`
  - `expected_out_after_token2022_fees: u64`
  - `min_out: u64`
  - `price_impact_bps: u32`
  - `slippage_bps: u32`
  - `used_forward_sim: bool`
  - `used_token2022_fee_math: bool`
- [ ] `EntryIntent` (minimum fields):
  - `intent_id: Hash32`  *(hash(signature, slot, mint_out, amount_in, strategy_version))*
  - `entity_id: u64`
  - `target_signature: SignatureBytes`
  - `target_slot: Slot`
  - `protocol: Protocol`
  - `intent: IntentContext`
  - `sizing: { our_amount_in: u64, max_amount_in: u64, sizing_mode: u8 }`
  - `constraints: { native_sol_reserve_floor_lamports: u64, max_tip_lamports: u64 }`
  - `quote: Quote`
- [ ] `ExitIntent` (minimum fields):
  - `intent_id: Hash32` *(hash(position_id, reason, slot))*
  - `position_id: Hash32`
  - `reason: u8` *(STOP_LOSS|TAKE_PROFIT|TIMEOUT|TRANSFER_BYPASS|MANUAL)*
  - `exit_mode: u8` *(MARKET_DUMP|LIMIT_GUARDED)*
  - `max_retries: u8`

**Tests (PASS):**
- [ ] `#[test] intent_id_determinism()`:
  - same inputs → same `intent_id`
  - changing any input or `strategy_version` changes `intent_id`
- [ ] `#[test] intent_bounded_size()`:
  - `EntryIntent` and `ExitIntent` serialize under a max size cap (configure cap; enforce via test)

### 2.4 Event log envelope type

- [ ] Define `Event` envelope (minimum):
  - `event_id: [u8; 16]` *(monotonic/offset id; backend-defined)*
  - `ts_unix_ms: UnixMs`
  - `kind: EventKind`
  - `intent_id: Option<Hash32>`
  - `position_id: Option<Hash32>`
  - `payload: Vec<u8>` *(versioned bytes)*

**Tests (PASS):**
- [ ] `#[test] event_roundtrip()` ensures events serialize/deserialize and preserve `kind` and ids.

---

## 3) Shared memory library + segments (`crates/shared_mem`)

### 3.1 mmap + seqlock + CRC (baseline)

- [ ] Implement `SharedMemSegment<T>` for seqlock-based segments:
  - [ ] `open_or_create(path, capacity_bytes)`
  - [ ] `write_snapshot(version, updated_at_slot, payload_bytes)`
  - [ ] `try_read_snapshot() -> Option<(header, payload_bytes)>`
- [ ] Implement `crc32(payload_bytes)` (use a single canonical library across the workspace).

**Tests (PASS):**
- [ ] `SHM-SEQ-01: seq_even_stable_read`
  - Writer publishes snapshot; reader reads stable even seq and CRC matches.
- [ ] `SHM-SEQ-02: detect_corruption`
  - Flip one byte in payload; reader rejects with CRC mismatch.
- [ ] `SHM-SEQ-03: no_blocking_reads`
  - During repeated writes, reader never blocks; it either returns `None` or returns the latest valid snapshot.

### 3.2 Triple-buffer / RCU for high-frequency segments (mandatory for pools)

- [ ] Implement triple-buffer publication pattern:
  - Maintain 3 buffers: `A/B/C`
  - Writer writes into inactive buffer, then atomically swaps “active index” pointer
  - Reader reads active pointer once and reads that buffer without spinning
- [ ] Apply triple-buffer to:
  - [ ] `/dev/shm/shadow_state_pools`
  - [ ] any “high-churn” segment updated > ~100 Hz (configurable threshold)

**Tests (PASS):**
- [ ] `SHM-RCU-01: never_spin_under_churn`
  - Writer updates 100k times; reader loops 1M reads and never observes an invalid half-written frame.
- [ ] `SHM-RCU-02: monotonic_slots`
  - `updated_at_slot` observed by reader is non-decreasing.

### 3.3 Segment schemas (payload formats)

Implement payload formats as **versioned**, with Rust as schema authority.

Minimum segments to support in V4.25:

- [ ] `shadow_oracle_watchlist` (seqlock)
  - payload v1: array of `{wallet_pubkey[32], entity_id(u64)}`
- [ ] `shadow_oracle_entities` (seqlock)
  - payload v1: array of `{entity_id(u64), enabled(bool), p_win(f32), b_win(f32), crowdedness_score(f32), conviction_mu(f32), conviction_sigma(f32), updated_at_slot(u64)}`
- [ ] `shadow_mint_risk` (seqlock)
  - payload v1: array of `{mint[32], risk_status(u8), reason_code(u16), freshness_slot(u64)}`
- [ ] `shadow_state_pools` (triple-buffer)
  - payload v1 (placeholder for V4.25): array of `{pool_id[32], vault_a[32], vault_b[32], reserve_a(u64), reserve_b(u64), fee_bps(u16), slot(u64)}`
- [ ] `shadow_state_mints` (triple-buffer)
  - payload v1 (placeholder for V4.25): array of `{mint[32], token_program(u8), freeze_auth_present(bool), transfer_hook_present(bool), interest_bearing_present(bool), permanent_delegate_present(bool), transfer_fee_bps(u16), slot(u64)}`
- [ ] `shadow_heartbeats` (seqlock)
  - payload v1: `{kill_switch(u8), processes: [ {proc_id(u8), healthy(bool), last_slot(u64), last_ts_ms(u64)} ... ] }`

**Tests (PASS):**
- [ ] `SHM-SCHEMA-01: write_read_watchlist_v1`
- [ ] `SHM-SCHEMA-02: write_read_entities_v1`
- [ ] `SHM-SCHEMA-03: write_read_mint_risk_v1`
- [ ] `SHM-SCHEMA-04: write_read_pools_placeholder_v1`
- [ ] `SHM-SCHEMA-05: write_read_mints_placeholder_v1`
- [ ] `SHM-SCHEMA-06: heartbeats_killswitch_roundtrip`

### 3.4 Schema evolution requirement (N → N+1 without restarts)

- [ ] Support version bumping for payloads:
  - reader must ignore unknown versions **or** support a v1+v2 decoder path
  - writer must be able to publish `v2` while reader continues running
- [ ] Provide a “capability handshake”:
  - at minimum: expose `SUPPORTED_VERSIONS` per segment in logs

**Tests (PASS):**
- [ ] `SHM-EVOL-01: publish_v2_while_reader_running`
  - Start reader; writer publishes v1 then v2; reader continues functioning and reports which versions it accepted.

---

## 4) Cross-language writer enforcement (Python → shared memory)

V4 rule: Python must not `struct.pack` bytes manually.

### 4.1 Choose and implement one sanctioned mechanism

Pick one:

- **Option A (recommended for V4.25):** `PyO3` Rust writer module (`python/shadow_ipc`)
- **Option B:** strict schema (`FlatBuffers` or `rkyv`) + generated code on both sides

V4.25 minimum deliverable is **Option A** unless you already have an established schema toolchain.

- [ ] Build `python/shadow_ipc` with functions:
  - `write_watchlist(path: str, rows: list[tuple[bytes32, int]])`
  - `write_entities(path: str, rows: list[...])`
  - `write_mint_risk(path: str, rows: list[...])`
- [ ] Functions must call the Rust shared-mem writer and must insert required atomic fences.

**Tests (PASS):**
- [ ] `IPC-01: python_writes_rust_reads_watchlist`
  - Python test writes 3 rows; Rust test reads and matches.
- [ ] `IPC-02: alignment_is_rust_controlled`
  - Add a regression test where Python attempts a manual-pack write → ensure reader rejects by version/CRC policy and CI forbids the call site.

---

## 5) Intent transport: shared-memory ring buffer (`crates/intent_queue`)

### 5.1 Implement the queue

- [ ] Implement a fixed-size, lock-free ring buffer in shared memory:
  - MPSC (2 producers: entry + PM; 1 consumer: submitter) is sufficient for now.
  - Each slot contains `{seq, payload_len, payload_bytes}`
  - Use cache-line padding to reduce false sharing
- [ ] Define the queue drop policy (must be non-blocking):
  - If full: producer returns `Err(QueueFull)` and caller can drop/metrics.
- [ ] Provide `push(bytes)` and `try_pop()` APIs.

**Tests (PASS):**
- [ ] `INTQ-01: 1M_intents_no_deadlock`
  - Push 1M intents from 2 producers; submitter drains; ensure completion.
- [ ] `INTQ-02: full_queue_is_non_blocking`
  - Force queue full; push returns error quickly (no sleep/spin).
- [ ] `INTQ-03: payload_integrity`
  - Payload bytes popped equal bytes pushed.

### 5.2 Intent idempotency + bounding

- [ ] Enforce max payload length (config-driven; tested).
- [ ] Enforce deterministic `intent_id` derivation in `types` crate (shared).
- [ ] Add a small “intent validation” function used by submitter before accepting:
  - rejects payload too large
  - rejects unknown schema versions

**Tests (PASS):**
- [ ] `INTENT-01: reject_oversize`
- [ ] `INTENT-02: accept_valid_entry_exit`

---

## 6) Canonical event log (`crates/event_log`)

### 6.1 Implement the event log interface

- [ ] Define `trait EventLog`:
  - `append(event: Event) -> Result<EventId>`
  - `subscribe(from: Option<EventId>) -> Stream<Event>` *(or iterator for tests)*
  - `ack(event_id)` *(no-op for WAL; required for Redis Streams)*

### 6.2 WAL backend (mandatory in V4.25)

- [ ] Implement WAL file format (recommended):
  - length prefix (u32)
  - payload bytes
  - per-record CRC32
  - fsync policy configurable (always vs batched)
- [ ] Implement recovery:
  - on startup: scan file, stop at first invalid/partial record, truncate tail

**Tests (PASS):**
- [ ] `ELOG-WAL-01: append_and_replay_exact`
- [ ] `ELOG-WAL-02: crash_recovery_truncates_partial`
  - Simulate partial write; recovery truncates and replays prior records only.

### 6.3 Redis Streams backend (optional feature flag)

- [ ] Implement behind feature `eventlog_redis`.
- [ ] Provide consumer-group semantics.
- [ ] Provide a test that runs only when `REDIS_URL` is present.

**Tests (PASS when enabled):**
- [ ] `ELOG-REDIS-01: consumer_group_replay`

### 6.4 Position reconstruction from event log (PM dependency)

- [ ] Provide a helper `PositionBook::rebuild(log)` that replays:
  - `PositionOpened`, `PositionIncreased`, `PositionReduced`, `PositionClosed`
- [ ] Ensure replay is deterministic and idempotent.

**Tests (PASS):**
- [ ] `PM-REPLAY-01: rebuild_from_empty_db`
  - Start with no DB; replay log; resulting positions match expected.

---

## 7) Heartbeats + kill switch (`shadow_heartbeats` segment)

### 7.1 Implement heartbeat writers/readers

- [ ] Define `ProcessId` enum:
  - ORACLE(1), STATE(2), ENTRY(3), PM(4), SUBMIT(5)
- [ ] Each process publishes heartbeat every `heartbeat_ms`:
  - `healthy`
  - `last_slot`
  - `last_ts_ms`
- [ ] Submitter and Entry Engine must read the segment at least every `heartbeat_ms`.

### 7.2 Entry Engine safety gate (OBSERVE-ONLY)

- [ ] Implement: Entry Engine refuses to emit **EntryIntent** unless:
  - PM heartbeat exists,
  - PM heartbeat age < `pm_ttl_ms`,
  - PM reports `healthy=true`.

**Tests (PASS):**
- [ ] `HB-01: entry_disabled_when_pm_stale`
- [ ] `HB-02: entry_enabled_when_pm_healthy`

### 7.3 Kill switch behaviors

- [ ] Kill switch modes (minimum):
  - `0 = NORMAL`
  - `1 = EXIT_ONLY` (reject new entries, allow exits)
  - `2 = HALT_ALL` (reject both, exit loop safely)
- [ ] `shadow-submit` must enforce kill switch regardless of producer behavior.

**Tests (PASS):**
- [ ] `KILL-01: exit_only_rejects_entry_accepts_exit`
- [ ] `KILL-02: halt_all_rejects_both`

---

## 8) `shadow-submit` skeleton (`crates/submitter_core` + `bins/shadow-submit`)

### 8.1 Key custody and compile-time enforcement

- [ ] Keep key material loading inside `shadow-submit` binary crate only.
- [ ] Other crates accept only `SignerPubkey` (public key bytes), never `Keypair`.

**Tests (PASS):**
- [ ] `SUB-KEY-01: no_key_dependency_in_entry_or_pm`
  - build entry/pm crates without key feature; verify compile.

### 8.2 Intent consumption + exit preemption + dedupe

- [ ] Submitter continuously drains intent queue.
- [ ] Always process exits before entries:
  - If using one queue: tag intents and reorder in memory
  - If using two queues: drain exits first every tick
- [ ] Maintain a bounded LRU of seen `intent_id` values.
- [ ] On accept, emit `EntryIntentAccepted` / `ExitIntentAccepted` events.

**Tests (PASS):**
- [ ] `SUB-01: exit_preempts_entry`
- [ ] `SUB-02: reject_duplicate_intent_id`

### 8.3 Blockhash cache thread (asynchronous; no hot-path I/O)

- [ ] Spawn a background thread that updates `latest_blockhash` at ~400ms cadence.
- [ ] Store in lock-free atomic pointer (`ArcSwap` or equivalent).
- [ ] Hot path reads blockhash from RAM only.

**Tests (PASS):**
- [ ] `SUB-BH-01: hot_read_never_blocks`
  - blockhash getter returns within a tiny bound in a loop while background updates.
- [ ] `SUB-BH-02: freshness_monotonic`
  - observed blockhash updates monotonically by slot/age metadata.

### 8.4 Parallel signing (Rayon)

- [ ] Implement signing for N permutations using `rayon::par_iter`.
- [ ] Ensure deterministic output ordering (sort by tip or index after parallel compute).

**Tests (PASS):**
- [ ] `SUB-SIGN-01: parallel_equals_sequential`
- [ ] `SUB-SIGN-02: deterministic_ordering`

### 8.5 Leader-aware route selection (logic only in V4.25)

- [ ] Implement `LeaderAwareRouter` pure logic:
  - input: leader schedule snapshot + current slot
  - output: `Route::Jito` or `Route::TpuFallback(StakedProvider)`
- [ ] Default non-jito fallback route must be “staked provider”.

**Tests (PASS):**
- [ ] `SUB-ROUTE-01: chooses_staked_provider_for_non_jito`

### 8.6 Dry-run “send” backend

- [ ] Implement a `SubmitBackend` trait:
  - `send_bundle(bytes) -> Receipt`
  - `send_tx(bytes) -> Receipt`
- [ ] In V4.25, implement `DryRunBackend` that:
  - never touches network
  - returns synthetic receipts
  - emits `ExecutionSent` and `ExecutionLanded` events (with dry-run flag)

**Tests (PASS):**
- [ ] `SUB-DRY-01: emits_events_for_sends`

---

## 9) Minimal `shadow-entry-engine` skeleton (`bins/shadow-entry-engine`)

### 9.1 Fixture feed consumer (no gRPC yet)

- [ ] Implement a “fixture feed” that yields a list of `PreConsensusTx` objects.
- [ ] Implement:
  - jitodontfront drop rule (`is_jitodontfront==true → drop`)
  - a placeholder “adapter” that produces a fake IntentContext

### 9.2 Online oracle gate (shared memory lookups)

- [ ] Read watchlist and entity registry from shared memory.
- [ ] Only emit EntryIntent if:
  - target_signer exists in watchlist
  - entity enabled
  - optional: entity score meets thresholds (config)

### 9.3 Emit EntryIntent

- [ ] Build deterministic `EntryIntent` with bounded payload size.
- [ ] Push into shared-memory intent queue.

**Tests (PASS):**
- [ ] `ENTRY-01: jitodontfront_dropped`
- [ ] `ENTRY-02: watchlist_gate_blocks_unknown`
- [ ] `ENTRY-03: entry_emitted_for_known_wallet_when_pm_healthy`

---

## 10) Minimal `shadow-position-manager` skeleton (`bins/shadow-position-manager`)

### 10.1 Event log replay + position book

- [ ] Subscribe/replay events from WAL from offset 0.
- [ ] Maintain an in-memory position map keyed by `position_id`.

### 10.2 Emit ExitIntent (fixture-only in V4.25)

- [ ] In fixture mode, emit an ExitIntent for any opened position after a short delay.
- [ ] Push ExitIntent into shared-memory intent queue.

**Tests (PASS):**
- [ ] `PM-01: rebuilds_state_from_log`
- [ ] `PM-02: emits_exit_intent_in_fixture_mode`

---

## 11) Minimal `shadow-state` skeleton (`bins/shadow-state`)

### 11.1 Fixture publisher (no Geyser yet)

- [ ] In fixture mode, publish a placeholder pool snapshot frame and mint invariant frame to shared memory using triple-buffer.
- [ ] Publish heartbeat.
- [ ] Expose a tiny CLI to write a single snapshot for tests.

**Tests (PASS):**
- [ ] `STATE-01: publishes_triple_buffer_frames`
- [ ] `STATE-02: readers_see_non_decreasing_slots`

---

## 12) End-to-end offline integration test (the V4.25 “Proof of Life”)

Create `integration_harness::test_e2e_offline()` that:

1. Creates temp files to emulate `/dev/shm/*` segments.
2. Starts:
   - `shadow-state` fixture publisher
   - `shadow-submit` in dry-run mode
   - `shadow-position-manager` in fixture mode
   - `shadow-entry-engine` in fixture mode (with a fixture tx whose signer is in watchlist)
3. Waits until:
   - submitter emits `EntryIntentAccepted`
   - submitter emits `ExecutionSent`
   - PM sees `PositionOpened` and emits `ExitIntent`
   - submitter emits `ExitIntentAccepted`

**Pass criteria:**
- [ ] All expected events appear in WAL in a valid order (no gaps).
- [ ] No deadlocks; test completes under a fixed timeout (e.g., 5 seconds).
- [ ] After simulated crash/restart of PM, replay reconstructs the last known position state.

---

## V4.25 Deliverables (artifacts)

- [ ] `PROJECT_SHADOW_ENGINE_BUILD_CHECKLIST_V4_25.md` completed with checked boxes
- [ ] A CI pipeline step running `cargo test --workspace`
- [ ] Golden fixtures:
  - [ ] shared-memory golden payload bytes (v1)
  - [ ] event log WAL golden replay file
- [ ] A single command to run the offline harness (example):
  - `cargo test -p integration_harness -- --nocapture`

---

## Notes for V4.50 (next quarter)

Do not start these until V4.25 is green:
- Live Yellowstone adapter and slot-delta benchmarks
- ALT cache + v0 flattening fixtures
- Raydium V4 adapter extraction + reconstruction
- Local quoter with forward-sim and Token-2022 fee math
- Real Jito / leader-aware sending
