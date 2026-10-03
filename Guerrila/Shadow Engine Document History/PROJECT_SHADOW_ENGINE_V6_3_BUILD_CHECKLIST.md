# Project Shadow Engine V6.3 — Build & Verification Checklist

**Document type:** Step‑by‑step build process + milestone test plan (Data Plane + Control Plane)  
**Primary spec:** `PROJECT_SHADOW_ENGINE_V6_3_GRAND_UNIFIED_SYSTEM_SPEC.md`  
**Control plane memo:** “The Control Plane — Architecting the Rosetta Stone Auto‑Heal” (Quant Supervisor memorandum)

---

## 0) Non‑negotiable constraints (must be enforced from day 0)

### 0.1 Hot‑path contract (money workers)
- [ ] **No allocations** (heap) in the money‑worker loop.
- [ ] **No locks** (no mutexes, no condvars) in the money‑worker loop.
- [ ] **No syscalls** in the money‑worker loop (no networking, no `clock_gettime`, no logging writes).
- [ ] Worker does only: dequeue fixed record → O(1) math → template build → sign → enqueue.

**Tests / proof:**
- [ ] CI job: run `cargo build --release` with `-Z` flags / instrumentation (or a custom allocator shim) and assert **0 allocations** in the worker loop during a synthetic workload.
- [ ] CI job: run `strace -f` (or `perf trace`) on the worker benchmark and assert **0 syscalls** between dequeue and enqueue.
- [ ] CI lint: forbid `Ordering::SeqCst` in Tier‑0 hot modules.

### 0.2 Latency SLO gates
- [ ] Money‑worker latency p99 (dequeue→enqueue) **≤ 150 µs**.
- [ ] End‑to‑end latency p99 (NIC RX → NIC TX) **≤ 500 µs**.

**Tests / proof:**
- [ ] Dedicated perf harness, pinned cores, production kernel‑bypass backend.
- [ ] Report p50/p95/p99/p99.9 + dropped‑intent rates.

### 0.3 Shared memory invariants
- [ ] Feed rings are **SPSC**.
- [ ] If there are `P` ingress producers, each worker has **P distinct SPSC rings** (one per producer) and polls them.
- [ ] Wait‑free double buffers include **tear detection** via sequence.
- [ ] `ActiveIdx` is isolated on its own **128‑byte sector**.

**Tests / proof:**
- [ ] Concurrency stress tests for ring correctness and double‑buffer tear safety (see milestones below).

### 0.4 Key material safety (Tier‑0)
- [ ] `panic = abort` for Tier‑0.
- [ ] Secret nonce scalar `r` is never stored in `/dev/shm`.
- [ ] Nonce memory uses `mlock` + `MADV_DONTDUMP`, and process is non‑dumpable.
- [ ] Nonce scalar is zeroized with an optimizer‑resistant primitive.

**Tests / proof:**
- [ ] CI: validate build flags (`panic = "abort"`, LTO settings as required).
- [ ] Runtime self‑test: assert `RLIMIT_CORE=0` and `PR_SET_DUMPABLE=0`.
- [ ] Memory scan test (dev only): after N signatures, scan nonce ring pages for non‑zero remnants of `r`.

---

## 1) Repository + build system bootstrap (Milestone M0)

### 1.1 Create repository layout
- [ ] `crates/`
  - [ ] `shadow-tier0/` (data plane: money workers + ingress/egress proxies + crypto precompute)
  - [ ] `shadow-state/` (state daemon)
  - [ ] `shadow-pm/` (position manager)
  - [ ] `shadow-oracle/` (nearline oracle)
  - [ ] `shadow-guard/` (on‑chain program)
  - [ ] `shadow-control/` (control plane binary; independent build)
  - [ ] `shadow-common/` (shared structs for shared memory layouts; **no allocations in hot structs**)
  - [ ] `shadow-bench/` (perf harness and synthetic traffic generator)

### 1.2 Enforce compile profiles
- [ ] Tier‑0 build profile:
  - [ ] `panic = "abort"`
  - [ ] LTO enabled (if required)
  - [ ] `codegen-units = 1` (if required for determinism)
  - [ ] `opt-level = 3`
- [ ] Control plane build profile:
  - [ ] can use default panic settings; it is not latency‑critical

### 1.3 CI skeleton
- [ ] `cargo fmt` (all)
- [ ] `cargo clippy` (all)
- [ ] Unit tests (`cargo test`) for each crate
- [ ] Perf microbench suite (nightly job) pinned to dedicated runner hardware

**M0 tests:**
- [ ] Build all crates.
- [ ] Run unit test suite.
- [ ] Lints enforce: no `SeqCst` in `shadow-tier0` hot modules.

**Exit criteria:**
- [ ] CI green.
- [ ] Code owners set: **Data Plane vs Control Plane** (separate review teams).

---

## 2) Production host determinism + measurement harness (Milestone M1)

### 2.1 Host tuning checklist (prod‑like)
- [ ] BIOS: disable deep C‑states; lock freq; disable SMT/HT on worker cores.
- [ ] Kernel args: isolate Tier‑0 worker cores (`isolcpus`, `nohz_full`, `rcu_nocbs`).
- [ ] NUMA pinning: Tier‑0 worker cores on NIC‑local NUMA node.
- [ ] Hugepages reserved at boot (2MB or 1GB) for AF_XDP UMEM / rings.
- [ ] Disable `irqbalance`; ensure NIC IRQs never land on money cores.

### 2.2 Timing + tracing harness
- [ ] Use NIC hardware timestamps if available, otherwise AF_XDP RX timestamp + monotonic TSC conversion.
- [ ] Define standard spans:
  - [ ] `t_rx_first_byte` (ingress)
  - [ ] `t_feed_enqueue` (ingress→ring)
  - [ ] `t_worker_dequeue`
  - [ ] `t_worker_enqueue` (egress ring)
  - [ ] `t_nic_tx` (egress)

**M1 tests:**
- [ ] Validate core pinning: `taskset` / `sched_getaffinity` asserts.
- [ ] Validate IRQ affinity: NIC queues not on worker cores.
- [ ] Validate hugepages allocated and used by UMEM.

**Exit criteria:**
- [ ] Harness can print end‑to‑end latency distribution for a synthetic packet stream.

---

## 3) Shared memory schema + ABI lock (Milestone M2)

### 3.1 Define all shared memory segments
Implement fixed binary structs in `shadow-common`:
- [ ] `/dev/shm/shadow_feed_ring_w{w}_p{p}` (SPSC ring)
- [ ] `/dev/shm/shadow_egress_ring_*` (SPSC ring)
- [ ] `/dev/shm/shadow_pool_index_map` (8‑byte tag map)
- [ ] `/dev/shm/shadow_pool_state` (double buffer per pool; hot slots)
- [ ] `/dev/shm/shadow_mint_index_map` (8‑byte tag map)
- [ ] `/dev/shm/shadow_mint_state` (arena)
- [ ] `/dev/shm/shadow_alt_cache` (read‑mostly snapshot)
- [ ] `/dev/shm/shadow_tip_lut` (read‑only LUT)
- [ ] `/dev/shm/shadow_inventory_fence` (128‑byte stride)
- [ ] `/dev/shm/shadow_health`
- [ ] `/dev/shm/shadow_blockhash` (double buffer)
- [ ] `/dev/shm/shadow_parsing_manifest` (Control Plane module; ParsingManifest64B)
- [ ] `/dev/shm/shadow_raw_archive` (Control Plane module; UMEM pointer ring)

### 3.2 ABI lock rules
- [ ] All structs are `#[repr(C)]`.
- [ ] All hot structs have explicit alignment:
  - [ ] 64B alignment for fixed hot slots.
  - [ ] 128B alignment for highly contended atomics and `ActiveIdx`.
- [ ] No variable‑length arrays in records traversing the money loop.

**M2 tests:**
- [ ] `static_assertions`: size and alignment checks for every struct.
- [ ] Cross‑process ABI test: writer and reader compiled as separate binaries; verify correct interpretation.

**Exit criteria:**
- [ ] ABI snapshot locked (checksum of struct definitions) and versioned.

---

## 4) Ring buffers (SPSC) + multi‑producer topology (Milestone M3)

### 4.1 Implement SPSC ring primitive
- [ ] Power‑of‑two capacity.
- [ ] Producer cursor and consumer cursor isolated on separate cache lines.
- [ ] Drop‑on‑full behavior (never block).

### 4.2 Implement per‑worker array‑of‑rings topology
- [ ] If `P = shred_capture_workers + grpc_workers`, each worker has `P` distinct rings.
- [ ] Worker polls rings round‑robin (or weighted) and processes whichever has data.

**M3 tests:**
- [ ] Single producer/consumer correctness test (wraparound, full, empty).
- [ ] Drop‑on‑full correctness test: assert producer never blocks and consumer sees no corruption.
- [ ] Multi‑producer simulation test:
  - [ ] Create `P` producer threads writing to their own ring.
  - [ ] Verify no corruption at high rates.
- [ ] Perf: enqueue/dequeue throughput on pinned cores.

**Exit criteria:**
- [ ] No corruption under sustained max‑rate writes.

---

## 5) Wait‑free double buffer with tear detection (Milestone M4)

### 5.1 Implement generic RCU‑lite double buffer
- [ ] `ActiveIdx128B` aligned to 128 bytes.
- [ ] Two 64B slots.
- [ ] `sequence` inside each slot.
- [ ] Writer protocol: odd→write inactive→even→flip.
- [ ] Reader protocol: read active→seq1 even→copy→fence→seq2 match→accept else drop.
- [ ] **No spinning / no retries** in reader.

### 5.2 Apply to:
- [ ] Pool hot state slots.
- [ ] Blockhash slots.
- [ ] ParsingManifest64B (Control Plane).

**M4 tests:**
- [ ] Deterministic tear injection:
  - [ ] Writer flips twice quickly while reader is paused mid‑copy (simulate with barriers).
  - [ ] Assert reader drops (seq mismatch) and never returns torn struct.
- [ ] Stress: 1 writer thread, N reader threads, random sleep/interrupt patterns.
- [ ] Perf: measure reader latency (single load + memcpy).

**Exit criteria:**
- [ ] 0 observed torn reads in stress; intentional tears are detected and dropped.

---

## 6) shadow-state (Milestone M5)

### 6.1 Implement pool arena + index map
- [ ] Preallocate `PoolArena` and `PoolIndexMap`.
- [ ] Index map entry is atomic 8‑byte tag: `(pubkey_hash40 << 24) | pool_index_plus_one_u24`.
- [ ] Reader verifies full pubkey from hot slot.
- [ ] Dynamic append: new pools get a new index without restart.

### 6.2 Implement mint arena + index map
- [ ] Same tag scheme as pools.
- [ ] Mint metadata stored in arena.

### 6.3 Publish health
- [ ] `state_is_healthy`.
- [ ] `last_pool_update_unix_ns`, `last_mint_update_unix_ns`, `last_alt_update_unix_ns`.
- [ ] `snapshot_slot` monotonic.

### 6.4 Fork handling
- [ ] On fork switch, republish previous state via the double buffer.

**M5 tests:**
- [ ] Unit: index map probing bounds and collision behavior.
- [ ] Integration: simulate pool creation updates; verify workers can resolve new pools without restart.
- [ ] Fork simulation test:
  - [ ] apply updates for slot N, then fork to alternate chain; verify re‑publish and `snapshot_slot` monotonic.

**Exit criteria:**
- [ ] Worker can read pool and mint state wait‑free with pubkey verification.

---

## 7) Ingress proxy — gRPC fallback (Milestone M6a)

### 7.1 Implement gRPC ingestion
- [ ] Maintain provider sessions.
- [ ] Decode protobuf.
- [ ] Extract:
  - [ ] signature (64B)
  - [ ] slot
  - [ ] unix_ns (stamp once in proxy)
  - [ ] raw tx bytes (optional for audit)
- [ ] `signal_source` tagging.

### 7.2 Allowlist prefilter on signers
- [ ] Extract `num_required_signatures`.
- [ ] Check **all signers** in static account keys (not only fee payer).

### 7.3 ALT expansion + strict O(1) decode
- [ ] Expand ALTs using `/dev/shm/shadow_alt_cache`.
- [ ] Decode supported patterns only.
- [ ] Emit fixed `IngressRecord`.

### 7.4 Route to worker by pool pubkey
- [ ] `N_workers` is power‑of‑two.
- [ ] Use CRC32 + `& (N-1)` with boot salt.

**M6a tests:**
- [ ] Golden decode tests: feed known transactions, verify decoded `IngressRecord`.
- [ ] Allowlist signer tests:
  - [ ] fee payer != trading wallet; verify target still matches.
- [ ] Routing tests:
  - [ ] same pool always routes to same worker.

**Exit criteria:**
- [ ] gRPC path produces correct records and routes correctly.

---

## 8) Ingress proxy — AF_XDP shred capture (Milestone M6b)

### 8.1 AF_XDP bring‑up
- [ ] UMEM hugepage allocation.
- [ ] RX queue setup and busy‑poll loop.
- [ ] No kernel copies.

### 8.2 Slot‑scoped crossover scanner (network physics correctness)
Implement slot‑scoped fragment contexts:
- [ ] Maintain `SlotFragCtx[4]`:
  - [ ] `slot`
  - [ ] `tail_shred_index`
  - [ ] `crossover_tail[<=128]`
  - [ ] bounded reorder buffer (cap 16)

Scanner rules:
- [ ] Parse Turbine header for `(slot, shred_index, shred_type)`.
- [ ] Drop non‑data shreds (coding/FEC).
- [ ] Never stitch across slots.
- [ ] Only form `tail || prefix` when `shred_index == tail + 1`.
- [ ] Late/duplicate rule: if `shred_index <= tail`, drop.
- [ ] Deadlock escape: if `(shred_index - tail) > 16`, flush context.

### 8.3 Prefilter on signers using ParsingManifest
- [ ] Read `ParsingManifest64B` once per slot (or every N shreds) into stack local.
- [ ] Use manifest offsets for byte scanning.

### 8.4 Decode → IngressRecord → route
- [ ] After allowlist hit: do ALT expansion + strict decode.
- [ ] Route by pool pubkey.

**M6b tests:**
- [ ] PCAP/UMEM replay tests:
  - [ ] out‑of‑order shreds (4,6,5) must not create “Frankenstein” crossover.
  - [ ] missing shred causes flush on gap > 16.
  - [ ] interleaved slots N and N+1 do not corrupt scanning.
  - [ ] coding shreds are skipped.
  - [ ] late duplicate shred does not flush buffer.
- [ ] End‑to‑end decode parity test:
  - [ ] same tx observed via AF_XDP and gRPC yields identical `IngressRecord`.

**Exit criteria:**
- [ ] AF_XDP path correctly identifies and decodes Tier‑0 targets under reorder and slot interleaving.

---

## 9) Money worker core loop (Milestone M7)

### 9.1 Worker ring polling
- [ ] Poll all `P` ingress rings without starving.

### 9.2 Thread‑local signature dedupe
- [ ] Keep L1‑resident fixed window of `sig_tag64`.

### 9.3 Pool state read + overlay
- [ ] Wait‑free read of `PoolHotState64B` with tear detection.
- [ ] Thread‑local dirty overlay per pool (pool‑sharded serialization).
- [ ] Overlay TTL anchored to base shared state timestamp (not reset by optimistic emits).

### 9.4 Quote math
- [ ] Two‑phase forward projection (target impact, then our quote).
- [ ] Integer math uses `u128` intermediates.

### 9.5 Tip selection + priority fee conversion
- [ ] Tip LUT uses u32 fraction.
- [ ] Bucket selection uses shifted rounding.
- [ ] PUBLIC_GEYSER: convert lamports budget → micro‑lamports/CU via fixed‑point reciprocal.
- [ ] Multiplication uses `u128` to avoid whale overflow.

### 9.6 Risk reservation (inventory fence)
- [ ] `fetch_add(trade_size)` then rollback on breach.
- [ ] Ensure rollback on any later drop (egress ring full, blockhash stale, etc.).

### 9.7 Template build
- [ ] Always prepend `ShadowGuard::AssertReserveOutInRange {min,max}`.
- [ ] PUBLIC_GEYSER template includes `set_compute_unit_price` (micro‑lamports/CU).
- [ ] Jito bundle lane uses `tip_pubkey = JITO_ACTIVE_TIPS[worker_id & 7]`.

### 9.8 Signing
- [ ] Pop nonce `(r,R)` from in‑process nonce ring.
- [ ] Zeroize `r` immediately after signature.
- [ ] Drop if nonce ring empty (fail fast).

**M7 tests:**
- [ ] Unit: forward projection math against known vectors.
- [ ] Unit: micro‑lamport/CU conversion exactness:
  - [ ] property test: `(cu_price * cu_limit) / 1_000_000 ≈ tip_lamports` within ≤1 lamport rounding.
  - [ ] overflow test: whale tip does not wrap (u128 multiplication).
- [ ] Unit: overlay TTL tests:
  - [ ] optimistic emits don’t extend base timestamp.
  - [ ] dust spam can’t keep overlay alive forever.
- [ ] Integration: ring full triggers rollback of inventory reservation.
- [ ] Perf: p99 worker dequeue→enqueue ≤ 150 µs with synthetic records.

**Exit criteria:**
- [ ] Worker loop meets p99 target on production hardware.
- [ ] `strace/perf trace` proves no syscalls.
- [ ] Alloc instrumentation proves 0 allocations.

---

## 10) Egress proxy (Milestone M8)

### 10.1 QUIC/TPU blast lane
- [ ] Pre‑warm connections.
- [ ] “Kamikaze” configuration (no pacing / no throttling).
- [ ] Multi‑identity relay fanout (distinct SWQoS identities) when enabled.

### 10.2 Jito bundle lane
- [ ] Maintain Searcher API sessions.
- [ ] Submit bundles.
- [ ] Subscribe to BundleState stream.

### 10.3 Strict non‑mutation
- [ ] Egress never mutates transaction bytes post‑sign.

**M8 tests:**
- [ ] QUIC lane: synthetic TX send at max burst without blocking the worker.
- [ ] Fanout test: verify no single‑IP source‑port mutation redundancy.
- [ ] Jito lane: bundle state events are received and forwarded.
- [ ] Signature validity test: tx accepted by simulator when using selected tip pubkey.

**Exit criteria:**
- [ ] Egress can sustain burst rates without backpressure.

---

## 11) Position manager (shadow-pm) (Milestone M9)

### 11.1 Pending table
- [ ] Consume ExecutionRecords.
- [ ] Track `(signature, mint_index, trade_size, send_time)`.

### 11.2 Terminal state handling
- [ ] On **landed**: `fetch_sub(trade_size)` to clear reservation.
- [ ] On **reverted/dropped/expired**: `fetch_sub(trade_size)`.

### 11.3 Delta reconciliation
- [ ] Maintain `settled_position`.
- [ ] Query chain position.
- [ ] Apply drift as fetch_add/sub (never blind store).

### 11.4 Jito fast release
- [ ] Consume BundleState terminal outcomes and release immediately.

**M9 tests:**
- [ ] Unit: reservation lifecycle tests (landed, reverted, expired).
- [ ] Integration: simulate 50 bundles, 48 dropped; verify capital is released immediately via BundleState path.
- [ ] Concurrency: PM reconciliation concurrent with worker reservations does not erase in‑flight reservations.

**Exit criteria:**
- [ ] Fence invariant holds in steady state: `fence = chain_position + in_flight`.

---

## 12) Oracle (shadow-oracle) (Milestone M10)

### 12.1 Tip LUT publishing
- [ ] Publish u32 fraction LUT.
- [ ] Recompute on schedule.
- [ ] Atomic publish to shared memory.

### 12.2 Allowlist + caps
- [ ] Maintain signer allowlist.
- [ ] Maintain per‑entity caps, blacklists.

**M10 tests:**
- [ ] Unit: LUT fraction ranges and monotonic sanity checks.
- [ ] Integration: worker reads LUT snapshot and applies expected tip.

**Exit criteria:**
- [ ] LUT updates do not stall Tier‑0.

---

## 13) On‑chain ShadowGuard program (Milestone M11)

### 13.1 Implement ShadowGuard instruction
- [ ] `AssertReserveOutInRange { min_reserve_out, max_reserve_out }`.
- [ ] Reads reserve from SPL token vault.
- [ ] Reverts if outside range.

### 13.2 Range semantics design
- [ ] Bounds protect both:
  - [ ] upstream execution proof (target actually moved state), and
  - [ ] sandwich/negative slippage floor (don’t buy into extreme depletion).

**M11 tests:**
- [ ] Solana program‑test:
  - [ ] reserve within range → ok.
  - [ ] reserve above max → revert.
  - [ ] reserve below min → revert.
- [ ] Fuzz bounds:
  - [ ] random reserves and bounds with invariants.

**Exit criteria:**
- [ ] Guard is minimal and stable; CU cost measured.

---

## 14) End‑to‑end system test (Milestone M12)

### 14.1 Deterministic replay harness
- [ ] Capture real traffic shreds into replayable format.
- [ ] Feed both:
  - [ ] AF_XDP replay path
  - [ ] gRPC replay path

### 14.2 Parity checks
- [ ] For the same target tx:
  - [ ] AF_XDP decoded intent matches gRPC decoded intent.
  - [ ] worker produces identical quote decisions.

### 14.3 Swarm test
- [ ] Generate many targets buying same pool in same slot.
- [ ] Verify pool‑sharded routing funnels to one worker.
- [ ] Verify overlay updates sequentially.
- [ ] Measure revert rate reduction vs non‑overlay baseline.

### 14.4 Failure injection
- [ ] Ring full events.
- [ ] Blockhash staleness.
- [ ] Pool state tearing (forced).

**M12 tests:**
- [ ] E2E p99 ≤ 500 µs (prod hardware).
- [ ] Worker p99 ≤ 150 µs.
- [ ] No syscalls in worker.

**Exit criteria:**
- [ ] Meets SLO and correctness under replayed mainnet conditions.

---

# Control Plane Build Checklist (shadow-control)

**Objective:** asynchronous auto‑heal and quarantine without touching the Data Plane money loop.

---

## C0) Control Plane repo + isolation
- [ ] `shadow-control` is a standalone binary.
- [ ] Control Plane shares only:
  - [ ] shared memory schemas (`shadow-common`), and
  - [ ] read access to UMEM memory for the flight recorder.
- [ ] Control Plane MUST NOT modify Tier‑0 worker code.

**Tests:**
- [ ] Build and run `shadow-control` without `shadow-tier0` present (it should fail gracefully).

---

## C1) Module 1 — ParsingManifest64B (The Bridge)

### C1.1 Implement `ParsingManifest64B`
- [ ] 64‑byte struct with fixed offsets for:
  - [ ] tx header
  - [ ] account keys
  - [ ] instruction payload
  - [ ] any other scan anchors required

### C1.2 Publish via wait‑free double buffer
- [ ] `/dev/shm/shadow_parsing_manifest` uses the same tear‑safe protocol.

### C1.3 Data Plane hook
- [ ] AF_XDP ingress reads manifest once per slot or per N shreds into stack local.

**C1 tests:**
- [ ] ABI tests: writer updates manifest, reader sees consistent offsets.
- [ ] Tear detection tests identical to M4.

**Exit criteria:**
- [ ] AF_XDP scanner can run with manifest offsets (no hardcoded magic numbers in scanner).

---

## C2) Module 2 — Flight Data Recorder (Zero‑copy UMEM History)

### C2.1 Delayed release UMEM window
- [ ] AF_XDP proxy retains last `W` frames (e.g., 16,384) before returning to Fill Ring.

### C2.2 History pointer ring
- [ ] Proxy writes `(slot, shred_index, umem_ptr, umem_len, rx_timestamp)` into `/dev/shm/shadow_raw_archive`.

### C2.3 Control Plane UMEM access
- [ ] Control Plane maps UMEM pages read‑only and can read payload bytes by pointer.

**C2 tests:**
- [ ] Verify zero memcpy on hot path (perf counters / code audit).
- [ ] Verify frames are recycled only after falling out of window.
- [ ] Corruption test: ensure UMEM memory is not accessed after release (guard with generation counters in dev).

**Exit criteria:**
- [ ] Control Plane can read raw packets for last W frames without impacting Tier‑0 latency.

---

## C3) Module 3 — Tripwire watchdog (Divergence detection)

### C3.1 ExecutionRecord ingestion
- [ ] Watchdog subscribes to AF_XDP and gRPC ExecutionRecord exhaust streams.

### C3.2 Miss detection logic
- [ ] If gRPC sees target signature but AF_XDP does not within 10ms → Miss.
- [ ] If Miss_Count > 3 within 5s → assert BLINDNESS.

### C3.3 Emergency action: switch feeds
- [ ] Publish `tier0_ingress_mode = GRPC_ONLY` to a control flag segment.
- [ ] Ingress threads obey this flag by disabling AF_XDP rings.

**C3 tests:**
- [ ] Simulate misses; verify BLINDNESS triggers.
- [ ] Verify feed switch keeps trading alive (Tier‑1 latency acceptable).

**Exit criteria:**
- [ ] Automatic degradation to gRPC path occurs on AF_XDP blindness.

---

## C4) Module 4 — Rosetta Stone calibrator (SIMD offset discovery)

### C4.1 Golden Record capture
- [ ] Take decoded tx from gRPC: signature, pool pubkey, amount.

### C4.2 UMEM window extraction
- [ ] Pull candidate raw payloads from Flight Data Recorder around the timestamp.

### C4.3 SIMD scanning
- [ ] Scan payloads for:
  - [ ] 64B signature → index X
  - [ ] 32B pool pubkey → index Y
  - [ ] 8B amount_in → index Z

### C4.4 Derive offsets → manifest update candidate
- [ ] Compute relative offsets (header base to X/Y/Z).

### C4.5 Validate determinism
- [ ] Validate on next 2–3 Golden Records.

**C4 tests:**
- [ ] Unit: SIMD memmem correctness.
- [ ] Integration: with recorded packet captures, calibrator finds correct offsets.
- [ ] False positive defense: require multi‑record validation.

**Exit criteria:**
- [ ] Calibrator can produce a verified ParsingManifest64B from real traffic.

---

## C5) Module 5 — Hot‑swap injection (manifest publish)

### C5.1 Safe publish
- [ ] Write derived manifest into inactive slot; flip active idx.

### C5.2 Recovery
- [ ] AF_XDP scanner reads new manifest at next slot boundary and resumes parsing.
- [ ] Watchdog de‑asserts BLINDNESS when AF_XDP matches gRPC again.
- [ ] Publish `tier0_ingress_mode = HYBRID` (AF_XDP primary).

**C5 tests:**
- [ ] Controlled blindness + recovery scenario:
  - [ ] force manifest mismatch → blindness.
  - [ ] calibrator derives new manifest.
  - [ ] AF_XDP resumes.

**Exit criteria:**
- [ ] Auto‑heal restores Tier‑0 ingestion without restart.

---

## C6) Module 6 — Application‑layer quarantine (program hash killswitch)

### C6.1 Monitor executable code hashes
- [ ] `shadow-state` (or a dedicated daemon) watches executable account data hash for supported Program IDs.

### C6.2 Quarantine flag publish
- [ ] If hash changes: set `venue_enabled[program] = false` in `/dev/shm/shadow_risk_snapshot`.

### C6.3 Worker enforcement
- [ ] Workers check flag (single load) and drop intents for quarantined venues.

### C6.4 Alerting
- [ ] PagerDuty SEV‑1: “VENUE QUARANTINED: CONTRACT UPGRADE DETECTED.”

**C6 tests:**
- [ ] Simulate program hash change; verify:
  - [ ] flag flips,
  - [ ] workers stop trading that venue,
  - [ ] alert fires.

**Exit criteria:**
- [ ] System prevents trading on unknown math without stopping other venues.

---

## C7) Revert‑cost breaker wiring (operational survival)

### C7.1 Breaker metrics
- [ ] Track consecutive reverts per pool/target.

### C7.2 Tight default threshold
- [ ] Blacklist when >3 consecutive reverts.

### C7.3 PagerDuty integration
- [ ] Breaker pages on‑call immediately.

**C7 tests:**
- [ ] Generate consecutive revert sequence; verify blacklist and alert.

**Exit criteria:**
- [ ] Fee bleed is bounded under systemic mismatch.

---

# Final production gates (Release R1)

## R1.1 Correctness gates
- [ ] No torn reads observed (double buffer).
- [ ] Inventory fence invariant holds under swarm + failures.
- [ ] AF_XDP scanner passes reorder/interleave/coding/late‑packet suite.
- [ ] ShadowGuard range guard deployed + verified.
- [ ] Jito per‑worker tip sharding verified (no single tip write‑lock bottleneck).

## R1.2 Performance gates
- [ ] Worker p99 ≤ 150 µs.
- [ ] End‑to‑end p99 ≤ 500 µs.
- [ ] Under swarm, throughput sustained without systemic stalls.

## R1.3 Security / safety gates
- [ ] Nonce ring never stored in `/dev/shm`.
- [ ] Nonce scalar zeroization verified.
- [ ] Core dumps disabled.
- [ ] Control Plane cannot mutate Data Plane tx bytes.

## R1.4 Operational gates
- [ ] Watchdog → Blindness → Calibrate → Inject → Recover loop works.
- [ ] Program hash quarantine works and alerts.
- [ ] Revert‑cost breaker pages correctly.

---

## Appendix A — Suggested test tooling

- **Rust unit/property tests:** `proptest`, `quickcheck`.
- **Concurrency stress:** loom (where possible) + native stress harness on real hardware.
- **Perf:** custom pinned‑thread bench harness + `perf stat`.
- **AF_XDP replay:** pcap → synthetic UMEM frame injector.
- **Solana program tests:** `solana-program-test` for ShadowGuard.

