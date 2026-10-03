# Project Shadow Engine V6.4 - Build Process Checklist (Spec-Synced, Milestone-Gated, Test-Driven)

**Document type:** Implementation process + milestone test plan  
**Applies to:** Data Plane V6.4 (`shadow-tier0`, `shadow-state`, `shadow-pm`, `shadow-oracle`) + Control Plane V1.0 (`shadow-control`)  
**Status:** Synced to the latest uploaded V6.4 unified spec  
**Goal:** Produce a production-deployable system that meets the V6.4 latency, correctness, routing, and treasury-protection invariants, and ships with an out-of-band auto-heal that can restore Tier-0 ingestion without stopping the engine.

---

## 0) Sync Notes - What changed in this checklist

This revision updates the build plan to match the latest V6.4 spec deltas:

- Ingress now tracks the latest spec's **broad-spectrum shred ingestion + FEC slow path** instead of the earlier "drop coding shreds" model.
- The worker path is now explicitly **leader-schedule aware** and **dual-LUT aware**.
- The Harmonic lane is now treated as a first-class lane with:
  - dedicated gRPC egress,
  - priority-fee-only template semantics,
  - a hard **Death Clamp** (drop on Harmonic failure; no TPU salvage), and
  - a PM-enforced **1-strike quarantine**.
- Fast capital release now includes the spec's **`shadow_nack_ring`** path for local egress failures.
- `ShadowGuard` is updated to the current **dual-bound, four-field reserve range** contract.
- Control Plane now includes the spec's:
  - proactive AF_XDP self-amputation path,
  - Predictive Shift Matrix before deep SIMD sweep,
  - overwrite guard,
  - batch-polled ingress mode flag.
- Host bringup now includes the spec's:
  - AVX offset / fixed-clock determinism,
  - hardware ntuple routing to AF_XDP queues,
  - hardware RX timestamp enablement.
- Observability now requires explicit telemetry for the Harmonic lane so V6.4 does not ship with a route-level blind spot.

---

## 1) Ground Rules (Non-Negotiable)

### 1.1 Data Plane invariants
- Money-workers:
  - 0 allocations / 0 locks / 0 syscalls inside the worker loop.
  - Bounded O(1) per intent.
  - Fail-fast on any state uncertainty.
- Ingress and egress:
  - Stateful networking is off the money cores.
  - Kernel bypass is mandatory where it matters.
- Shared memory:
  - All shared structs are fixed-layout and size/align validated at compile time.
  - No dynamic schema parsing or hot reload on money cores.
- Route selection:
  - Routing is a function of `signal_source`, `next_scheduled_slot`, leader-schedule engine tag, and shared risk flags.
  - Workers must never guess `ingress_record.slot + 1`.

### 1.2 Firewalls that must exist before performance work
These are survival constraints, not optional hardening.

**Firewall 1 - Rust RCU / double-buffer correctness**
- Payload behind `UnsafeCell<T>`.
- Volatile payload reads/writes.
- `ActiveIdx128B` isolated at 128-byte alignment.
- Reader uses a real hardware `atomic::fence(Ordering::Acquire)` after payload copy.

**Firewall 2 - Absolute priority-fee clamp**
- Apply the hard ceiling to `tip_lamports` immediately before CU-price conversion.

**Firewall 3 - Harmonic Death Clamp**
- If Harmonic egress fails, drop the tx.
- Never salvage the same signed tx by blasting it to TPU or relays.

**Firewall 4 - Harmonic 1-strike guillotine**
- If any Harmonic-routed signature lands on-chain with terminal `Err`, PM must set `harmonic_quarantined = true`.
- Workers must treat this as an O(1) hard gate.

**Firewall 5 - Fast capital release via NACK ring**
- Any proxy-side failure after dequeue but before network transmission must produce a `NackRecord64B` into `/dev/shm/shadow_nack_ring`.
- PM must consume it and release the fence immediately.

---

## 2) Repository / Workspace Layout

### 2.1 Binaries
- `shadow-tier0` - money workers + ingress proxies + egress proxies + nonce precompute + background threads
- `shadow-state` - fork-aware state replication + pool/mint/ALT publish + leader-schedule publisher
- `shadow-pm` - inventory reconciliation + exits + terminal status tracking + Harmonic quarantine
- `shadow-oracle` - alpha model + dual tip LUT publisher
- `shadow-control` - watchdog + calibrator + manifest injector + venue quarantine

### 2.2 Crates
- `shadow-core` - shared types, layout-checked structs, tag/hash utilities, enums
- `shadow-ipc` - shm layouts, SPSC rings, RCU-lite double buffers, leader-schedule segment, NACK ring
- `shadow-crypto` - nonce ring, Ed25519 precompute/signing, zeroization glue
- `shadow-ingest` - shred scanner, replay harness, strict decode library
- `shadow-guard-program` - on-chain reserve-range assertion program

### 2.3 Build modes
- `dev` - correctness, sanitizers, extra logging; never used for perf claims
- `release` - production profile, LTO, `panic = "abort"`
- `release-perf` - production profile plus perf counters behind feature flags

---

## 3) Milestone Map

1. **M0 - Toolchain + CI skeleton**
2. **M1 - Host determinism + kernel-bypass bringup**
3. **M2 - Shared memory primitives**
4. **M3 - `shadow-state`**
5. **M4 - Ingress proxy**
6. **M5 - Worker core**
7. **M6 - Egress proxies**
8. **M7 - `shadow-pm`**
9. **M8 - `shadow-oracle`**
10. **M9 - On-chain `ShadowGuard`**
11. **M10 - End-to-end perf + observability validation**
12. **M11 - Control Plane V1.0 auto-heal**
13. **M12 - Production readiness gates + rollout**

Each milestone below includes:
- deliverables,
- implementation checklist,
- required tests,
- acceptance criteria.

---

## 4) Milestone M0 - Toolchain + CI Skeleton

### Deliverables
- Monorepo workspace builds all binaries.
- CI runs on Linux x86_64 and Linux aarch64 cross-build:
  - `cargo fmt --check`
  - `cargo clippy -D warnings`
  - `cargo test`
  - `cargo test --features loom`
  - `cargo test --features fuzz_smoke`

### Implementation checklist
- Lock Rust toolchain.
- Enforce `panic = "abort"` for production binaries.
- Add layout assertion macros.
- Add deny-alloc harnesses for hot-path crates.
- Add an assembly-audit CI job for the double-buffer readers.

### Required tests
- Compile-time layout assertion job for all RCU slots.
- Build `dev` and `release` profiles.
- Cross-build smoke for aarch64.

### Acceptance criteria
- CI is green on x86_64.
- Cross-build succeeds.

---

## 5) Milestone M1 - Host Determinism + Kernel-Bypass Bringup

### Deliverables
- Documented production host profile:
  - BIOS/firmware settings,
  - NUMA + CCX isolation,
  - hugepage reservation,
  - IRQ steering,
  - AF_XDP queue steering,
  - hardware timestamping.
- Minimal AF_XDP sample app can RX/TX with stable p99.

### Implementation checklist
- BIOS / firmware:
  - disable deep C-states,
  - disable SMT on money cores,
  - set governor to `performance`,
  - set AVX2 and AVX-512 negative offsets to `0`,
  - lock all-core frequency at or below the safe AVX base clock.
- Linux boot args:
  - `isolcpus`, `nohz_full`, `rcu_nocbs` for money cores.
- Hugepages:
  - reserve at boot,
  - mount hugetlbfs,
  - prove 1 GB pages exist for UMEM.
- NIC topology:
  - pin Tier-0 cores to the NIC-local NUMA node,
  - keep Control Plane on a separate CCX or separate NUMA domain where possible.
- AF_XDP / NIC steering:
  - configure hardware ntuple routing so Turbine UDP flows land on AF_XDP queues,
  - enable RX hardware timestamping,
  - disable IRQ handling on Tier-0 queues.

### Required tests
1. **Host audit script**
   - Asserts:
     - isolated cores are correct,
     - `irqbalance` is disabled,
     - governor is `performance`,
     - hugepages exist and are mounted,
     - Tier-0 cores are on the NIC-local NUMA node,
     - Tier-0 CCX/L3 does not overlap with Control Plane CCX/L3,
     - AVX offsets are zeroed,
     - ntuple routing is programmed,
     - RX hardware timestamping is enabled.
2. **AF_XDP functional test**
   - synthetic UDP send/receive correctness and loss rate.
3. **AF_XDP latency microbench**
   - RX descriptor visibility p50/p99 using hardware timestamps or calibrated TSC.

### Acceptance criteria
- Kernel-bypass RX/TX works at lab target PPS with zero packet loss.
- Host audit script passes.

---

## 6) Milestone M2 - Shared Memory Primitives

### Deliverables
- `shadow-ipc` crate provides:
  - SPSC ring implementation (drop-on-full),
  - RCU-lite double-buffer with sequence tear detection,
  - 8-byte tag maps for pools and mints,
  - leader-schedule segment support,
  - dual tip LUT segment support,
  - `shadow_nack_ring` support,
  - fixed segment header/versioning.

### Implementation checklist

#### 6.1 Segment versioning
For every `/dev/shm/shadow_*` file:
- first cache line is `SegmentHeader64B` with magic, version, struct size, build SHA prefix, checksum.
- reader/writer version mismatch must fail closed.

#### 6.2 SPSC rings
- power-of-two capacity,
- producer/consumer cursors isolated to their own cache lines,
- drop-on-full,
- stats counters placed off the hot cache line.

#### 6.3 Tag maps
- open addressed,
- bounded probe count,
- 8-byte tag only in the map,
- full pubkey verification only in arena.

#### 6.4 RCU-lite slots
- `ActiveIdx128B` aligned to 128 bytes,
- payload slot aligned to 64 bytes,
- `sequence: AtomicU32`,
- payload behind `UnsafeCell<T>` with volatile access,
- no plain struct copies in reader/writer paths.

#### 6.5 Shared memory segments that must exist by the end of M2
- `/dev/shm/shadow_feed_ring_*`
- `/dev/shm/shadow_egress_ring_*`
- `/dev/shm/shadow_pool_index_map`
- `/dev/shm/shadow_pool_state`
- `/dev/shm/shadow_mint_index_map`
- `/dev/shm/shadow_mint_state`
- `/dev/shm/shadow_alt_cache`
- `/dev/shm/shadow_health`
- `/dev/shm/shadow_blockhash`
- `/dev/shm/shadow_leader_schedule`
- `/dev/shm/shadow_tip_lut_base`
- `/dev/shm/shadow_tip_lut_jito`
- `/dev/shm/shadow_inventory_fence`
- `/dev/shm/shadow_parsing_manifest`
- `/dev/shm/shadow_control_flags`
- `/dev/shm/shadow_raw_archive`
- `/dev/shm/shadow_nack_ring`

### Required tests
1. **Compile-time layout tests**
   - `PoolPayload56B`, `PoolHotState64B`, `BlockhashPayload56B`, `BlockhashSlot64B`, `ParsingManifest64B`, `NackRecord64B`, `ActiveIdx128B`.
   - leader-schedule segment asserts:
     - `engine_tags.len() == 1_048_576`,
     - `size_of::<AtomicU8>() == 1`.
2. **Loom concurrency tests**
   - double-buffer reader/writer ordering,
   - reader returns `None` on odd sequence,
   - no torn snapshot acceptance,
   - leader-schedule slot store/load semantics,
   - NACK ring single-producer/single-consumer correctness.
3. **Runtime stress tests**
   - one writer + N readers on RCU slots for 60 seconds,
   - leader-schedule wrap-around behavior,
   - ring overflow behavior remains drop-on-full with no stalls.

### Acceptance criteria
- All layout and concurrency tests pass.
- Runtime stress shows no invalid snapshots or panics.

---

## 7) Milestone M3 - `shadow-state` (Pools, Mints, ALTs, Leader Schedule)

### Deliverables
- `shadow-state` maintains:
  - pool arena + pool index map,
  - mint arena + mint index map,
  - ALT cache,
  - health snapshot,
  - wait-free leader-schedule tags.
- Pool publish path writes the hot slot before publishing/updating the index tag.
- Transfer-fee metadata needed for Token-2022 drift protection is available to workers.

### Implementation checklist
- Implement subscriptions for pool accounts, vault balances, mint metadata, ALTs.
- Decode and publish hot pool fields needed by workers.
- Parse and publish Token-2022 `TransferFeeConfig` into the hot-state fee/flags field if applicable.
- Maintain `/dev/shm/shadow_leader_schedule`:
  - fixed-capacity array of `AtomicU8`,
  - `0 = Vanilla`, `1 = Jito`, `2 = Harmonic`,
  - `store(engine_enum, Release)` by masked slot index.
- Expose leader-schedule freshness in health metadata.
- On uncertainty, choose the documented fail-safe behavior and keep it consistent across workers and tests.

### Required tests
1. **Golden decode tests**
   - captured pool/vault blobs decode to expected reserves.
2. **Transfer-fee publish tests**
   - Token-2022 mint with transfer fee populates the published fee field correctly.
3. **Publish-order tests**
   - reader verifies full pubkey from arena and never sees a wrong pool.
4. **Fork simulation test**
   - state A -> revert -> state prior; readers see correct hot state and monotonic health slot.
5. **Leader-schedule mapping tests**
   - contiguous slots,
   - holes / unknown leaders,
   - overwrite on schedule refresh,
   - ring-wrap indexing.

### Acceptance criteria
- `shadow-state` runs read-only for 1 hour on live data without crash.
- Pool/mint freshness stays within thresholds during normal conditions.
- Leader-schedule freshness alarms behave correctly.

---

## 8) Milestone M4 - Ingress Proxy (AF_XDP Shreds + FEC + Decode + Routing)

### Deliverables
- AF_XDP ingest thread(s) implement the latest spec's shred path:
  - ingest both data shreds and coding shreds,
  - parse Turbine shred headers,
  - maintain 4 slot-scoped fragment contexts,
  - maintain bounded per-slot reorder buffers,
  - fast contiguous scan path,
  - FEC slow path for missing-data recovery,
  - deadlock escape on late/duplicate and large positive gaps.
- Flight Data Recorder:
  - shared 4 GiB UMEM backing,
  - `delay_fifo[1_048_576]`,
  - `shadow_raw_archive` publish by UMEM offset,
  - survival-mode flush when Fill ring nears starvation.
- gRPC fallback ingest thread(s):
  - protobuf parse,
  - allowlist prefilter,
  - strict decode,
  - identical fixed-size `IngressRecord` output.
- Both paths perform primary worker routing by pool pubkey with >80% overflow valve.

### Implementation checklist
- AF_XDP scanner must implement:
  - **broad-spectrum shred ingestion**: data + coding shreds enter userspace,
  - `SlotFragCtx[4]` with per-slot tail and reorder buffer,
  - no cross-slot stitching,
  - late/duplicate drop before gap logic,
  - positive-gap flush only when `(shred_index - tail_shred_index) > 16`,
  - FEC slow path using aligned recovery matrices and SIMD GF math,
  - resume fast path after reconstruction.
- Timestamping:
  - populate `IngressRecord.unix_ns` from RX hardware timestamp metadata,
  - no `clock_gettime()` or `_rdtsc()` in hot AF_XDP emit path.
- Allowlist:
  - check all signers, not only fee payer.
- Decode:
  - ALT expansion and strict pattern decode happen on ingress cores only.
- Routing:
  - primary worker by salted CRC32(pool_pubkey prefix),
  - adjacent fallback if primary ring >80% full.

### Required tests
1. **Offline shred replay harness**
   - contiguity test,
   - out-of-order per-slot test,
   - late duplicate test,
   - slot interleave test,
   - positive large-gap flush test,
   - all-signers allowlist test.
2. **FEC recovery tests**
   - single missing data shred with sufficient coding shreds,
   - multiple coding shreds present but insufficient parity to recover,
   - recovery matrix wrap and reuse.
3. **Flight Data Recorder tests**
   - FIFO recycling correctness,
   - archive drop-on-full behavior,
   - survival-mode flush test,
   - UMEM offset reconstruction correctness.
4. **Hardware timestamp propagation test**
   - recorded RX timestamp reaches `IngressRecord.unix_ns` exactly.
5. **Ingress performance tests**
   - packet RX -> record enqueue latency,
   - sustained PPS,
   - overload drop profile.

### Acceptance criteria
- Replay harness passes.
- FEC slow path recovers controlled loss bursts correctly.
- Ingress sustains target PPS with bounded CPU and no hot-path stalls.

---

## 9) Milestone M5 - Worker Core (O(1) Money Loop, Route Physics, Fee Physics)

### Deliverables
- Worker loop implements:
  - thread-local signature dedupe,
  - staleness gates,
  - pool/mint lookup,
  - shared-state read + thread-local pool overlay,
  - Token-2022 transfer-fee-aware forward projection,
  - slot-correct route selection,
  - dual-LUT selection,
  - absolute fee clamp,
  - CU-price conversion in `u128`,
  - atomic inventory reservation,
  - four-bound `ShadowGuard` parameter build,
  - nonce-ring signing,
  - egress enqueue,
  - overlay update only on successful enqueue.

### Implementation checklist

#### 9.1 Effective reserve overlay discipline
- Overlay carries `base_state_unix_ns` anchored to the shared pool snapshot that seeded it.
- Reset overlay if:
  - `ingress.unix_ns - overlay.base_state_unix_ns > 200 ms`, or
  - shared `pool.last_update_unix_ns > overlay.base_state_unix_ns`.
- Do not extend TTL based on optimistic emits.

#### 9.2 Transfer-fee drift protection
- During both target projection and our quote projection, apply transfer-fee discount before AMM math when the published mint/pool fee field indicates Token-2022 transfer fee.

#### 9.3 Slot-correct route selection
- Read `next_scheduled_slot` from the wait-free blockhash segment.
- Read leader engine tag from `/dev/shm/shadow_leader_schedule[next_scheduled_slot & 0xFFFFF]`.
- Combine:
  - `signal_source`,
  - leader engine tag,
  - `submit.private_mempool_strategy`,
  - `harmonic_quarantined`
  to select template and egress lane.
- Never use `ingress_record.slot + 1`.

#### 9.4 Dual-LUT selection
- `engine == 1` uses `shadow_tip_lut_jito`.
- `engine == 0 || engine == 2` uses `shadow_tip_lut_base`.
- Prefer branchless selector implementation.

#### 9.5 Absolute clamp and CU-price conversion
- Clamp `tip_lamports` to `ABSOLUTE_MAX_PRIORITY_FEE_LAMPORTS`.
- Convert to micro-lamports/CU in `u128`.

#### 9.6 Template discipline
- `PUBLIC_GEYSER`:
  - standalone blast only,
  - `set_compute_unit_limit`,
  - `set_compute_unit_price`,
  - `ShadowGuard::AssertReservesInRange`,
  - swap instruction.
- Jito bundle lane:
  - tip-transfer instruction allowed,
  - Jito tip selection occurs before signing.
- Harmonic lane:
  - priority-fee only,
  - no Jito tip-transfer instruction,
  - exact CU limit,
  - no TPU fallback on failure.

#### 9.7 Inventory reservation and rollback
- Reservation is optimistic `fetch_add` + immediate rollback on limit breach.
- Roll back on:
  - template failure,
  - nonce ring empty,
  - egress ring full,
  - any later pre-send drop.

### Required tests
1. **Deterministic math tests**
   - constant-product projection,
   - very small / very large reserves,
   - `u128` overflow boundaries,
   - Token-2022 transfer-fee adjusted projection.
2. **Overlay tests**
   - reset on TTL expiry,
   - reset on fresher shared state,
   - no TTL extension on optimistic emit.
3. **Route selection tests**
   - `PUBLIC_GEYSER` never bundles,
   - Jito source bundles only on compatible lane,
   - Harmonic routing only when engine tag is Harmonic and lane not quarantined,
   - private mempool strategy behavior,
   - no blind `slot + 1` regression.
4. **Dual-LUT tests**
   - correct LUT chosen for each engine tag.
5. **Fee tests**
   - whale overflow test,
   - absolute clamp test,
   - shifted-rounding bucket boundary test.
6. **Template tests**
   - Harmonic tx contains priority-fee instructions and no Jito tip transfer,
   - four-bound `ShadowGuard` parameters are serialized correctly.
7. **No-alloc / no-syscall tests**
   - deny-alloc harness,
   - `strace` smoke on worker thread IDs.
8. **Worker microbench**
   - dequeue -> egress enqueue p99 on production hardware.

### Acceptance criteria
- All worker tests pass.
- Worker p99 meets target.
- Under overload the worker drops intents but never blocks.

---

## 10) Milestone M6 - Egress Proxies (QUIC + Jito + Harmonic + NACK fast path)

### Deliverables
- QUIC egress proxy:
  - pre-warmed sessions,
  - kamikaze config,
  - optional relay fanout across distinct SWQoS identities,
  - no hot-path handshake waits.
- Jito egress proxy:
  - persistent Searcher API session,
  - bundle submission only,
  - no telemetry parsing on this core.
- Harmonic egress proxy:
  - persistent gRPC sessions to all configured Harmonic regions,
  - authenticated using the configured role,
  - bundle submission only,
  - Death Clamp semantics.
- All proxies emit `NackRecord64B` to `/dev/shm/shadow_nack_ring` when a dequeued envelope fails before network transmission.

### Implementation checklist

#### 10.1 QUIC blast path
- Disable pacing.
- Use an effectively unbounded or custom congestion policy suitable for Solana TPU micro-bursts.
- Fail over to alternate pre-warmed sessions instead of queueing.
- Size TX descriptor rings for relay fanout bursts.

#### 10.2 Jito tip-account rotation
- Workers must use:
  - `tip_index = (worker_id.wrapping_add(local_intent_counter_u32).wrapping_add(jito_boot_salt_u32)) & 7`
  - increment counter only on emitted bundle intent.
- Egress must not patch the tip account after signing.

#### 10.3 Harmonic lane details
- Maintain clients to all configured Harmonic endpoints.
- Use gRPC / HTTP2 framing with the configured auth role.
- Fanout happens on the proxy; worker enqueues once.
- On local Harmonic send failure: drop and emit NACK; do not salvage to TPU.

#### 10.4 NACK fast-release path
- When proxy dequeues but cannot transmit, push `NackRecord64B { signature, mint_out_index, trade_size_lamports }` to `/dev/shm/shadow_nack_ring`.
- Never silently drop a post-dequeue envelope.

### Required tests
1. **Mock QUIC endpoint test**
   - no blocking on reconnect,
   - drop when no session.
2. **Jito tip immutability test**
   - signed bytes match transmitted bytes,
   - boot salt rotation preserves expected tip pubkey.
3. **Relay fanout test**
   - one copy per SWQoS identity,
   - no repeated same-IP redundant sends.
4. **Harmonic auth + fanout test**
   - client authenticates with configured role,
   - all configured regions are attempted as designed.
5. **Harmonic Death Clamp test**
   - endpoint timeout or auth failure produces drop + NACK,
   - no TPU/relay fallback occurs.
6. **NACK emission test**
   - local ring-full / dead-session / no-auth cases produce exactly one NACK.

### Acceptance criteria
- Proxy failures fail fast and do not stall money cores.
- Every post-dequeue local send failure produces a recoverable PM signal.

---

## 11) Milestone M7 - `shadow-pm` (Fence Truth, Terminal Statuses, Harmonic Quarantine)

### Deliverables
- Pending table keyed by signature.
- PM consumes terminal state from:
  - RPC / Geyser signature status,
  - Jito `BundleState` stream,
  - Harmonic `BundleState` stream,
  - `/dev/shm/shadow_nack_ring` for local pre-send failures.
- Fence logic supports:
  - release on landed success,
  - release on revert / dropped / expired,
  - release on local NACK,
  - delta reconciliation without blind stores.
- Harmonic 1-strike quarantine is enforced from PM.

### Implementation checklist
- Track route/lane metadata in the pending table so PM knows which terminal stream applies.
- Poll `/dev/shm/shadow_nack_ring` continuously on a utility core.
- Maintain Jito `BundleState` WebSocket.
- Maintain Harmonic `BundleState` WebSocket.
- On any landed Harmonic tx with on-chain `Err`:
  - set `harmonic_quarantined = true`,
  - release any leaked reservation,
  - emit SEV-1 event.
- Delta reconciliation must apply additive drift, never blind `store()`.

### Required tests
1. **Reservation leak test**
   - many failures return fence close to baseline.
2. **Expiry test**
   - no status after max age releases reservation.
3. **NACK fast-release test**
   - local proxy failure immediately frees capital.
4. **Jito fast-release test**
   - off-chain bundle rejection releases immediately.
5. **Harmonic fast-release test**
   - off-chain Harmonic rejection releases immediately.
6. **Harmonic 1-strike quarantine test**
   - first on-chain `Err` flips `harmonic_quarantined = true` and workers stop routing to Harmonic.
7. **Delta reconciliation test**
   - preserves in-flight reservations while converging to settled truth.

### Acceptance criteria
- No phantom inventory under burst/revert tests.
- Harmonic quarantine becomes visible to workers within the configured polling window.

---

## 12) Milestone M8 - `shadow-oracle` (Dual Tip LUT Publisher)

### Deliverables
- Dual LUTs in shared memory:
  - `/dev/shm/shadow_tip_lut_base`
  - `/dev/shm/shadow_tip_lut_jito`
- Both LUTs use:
  - fraction_u32 entries,
  - shifted-rounding bucket math,
  - atomic snapshot publication.
- Oracle exposes route-tax calibration parameters and updates only the Jito LUT when tax parameters change.

### Required tests
1. **LUT layout tests**
2. **Bucket math tests**
3. **Tax scaling tests**
   - `lut_jito >= lut_base` when Jito tax > 0.
4. **Publish/consume tests**
   - worker sees valid snapshots for both LUTs.
5. **Leader-aware selection integration**
   - workers pick the correct LUT given synthetic engine tags.

### Acceptance criteria
- Workers always read valid base/jito LUT snapshots or fail closed on absence/staleness.

---

## 13) Milestone M9 - On-Chain `ShadowGuard` (Dual-Bound Reserve Range)

### Deliverables
- Program instruction:
  - `AssertReservesInRange { min_reserve_in, max_reserve_in, min_reserve_out, max_reserve_out }`
- Reads both vault token-account amounts.
- Aborts before swap if either reserve is outside range.
- No CPI, no routing, no extra state mutation.

### Required tests
1. **Program unit tests**
   - inclusive min/max behavior,
   - extreme values,
   - both reserve fields checked.
2. **Local validator integration tests**
   - target no-op / revert case fails guard,
   - pathological reserve depletion fails guard,
   - correct reserve transition passes guard and allows swap.
3. **Compute budget tests**
   - prove guard remains minimal and stable.

### Acceptance criteria
- Guard behavior matches the four-bound spec and stays minimal.

---

## 14) Milestone M10 - End-to-End Performance and Observability Validation

### Deliverables
- End-to-end timestamp chain:
  - NIC RX timestamp,
  - ingress enqueue timestamp,
  - worker dequeue/enqueue timestamps,
  - egress TX timestamp.
- Dashboards for:
  - worker p99,
  - end-to-end p99,
  - ingress p99,
  - route/lane mix,
  - revert rate,
  - fee spend,
  - dual-LUT efficiency.
- Telemetry schema updated so V6.4 lanes are explicit:
  - route/lane must distinguish Vanilla blast, Jito bundle, Harmonic bundle.

### Required tests
1. **Full-pipeline replay tests**
   - Vanilla path,
   - Jito path,
   - Harmonic path,
   - blinded / degraded gRPC-only path.
2. **Perf regression gate**
   - compare current p99s to last known-good baseline.
3. **Telemetry schema test**
   - execution/event records encode explicit lane and engine data.

### Acceptance criteria
- Worker p99 <= 150 us on production hardware.
- End-to-end p99 <= 500 us on production hardware.
- No observability blind spot for Harmonic lane behavior.

---

## 15) Milestone M11 - Control Plane V1.0 (`shadow-control`) Auto-Heal

> Control plane must be a standalone binary. It must not require code changes to money-worker logic.

### 15.1 Control Plane responsibilities
- Detect Tier-0 blindness.
- Keep collecting raw UMEM history without memcpy.
- Derive updated parsing offsets.
- Hot-swap parsing manifest via RCU-lite publish.
- Quarantine venues on executable hash change.

### 15.2 Module 1 - ParsingManifest64B bridge
**Deliverables**
- `/dev/shm/shadow_parsing_manifest` with double-buffered 64-byte manifest slots.
- AF_XDP ingress reads manifest only at slot boundaries or every configured shred batch interval.

**Tests**
- layout tests,
- wrong-manifest -> misses,
- corrected manifest -> hits without restart.

### 15.3 Module 2 - Flight Data Recorder
**Deliverables**
- 4 GiB shared UMEM backing file,
- `delay_fifo[1_048_576]`,
- `shadow_raw_archive` storing UMEM offsets,
- survival-mode flush when Fill ring availability falls below threshold.

**Tests**
- FIFO delay correctness,
- no-backpressure when control plane is paused,
- survival-mode flush behavior,
- UMEM overwrite safety under pressure.

### 15.4 Module 3 - Tripwire + proactive AF_XDP self-amputation
**Deliverables**
- `shadow-watchdog` detects gRPC hit / AF_XDP miss divergence.
- BLINDNESS trigger remains the 5-second watchdog backstop.
- AF_XDP thread also maintains `consecutive_structural_failures` and can autonomously switch `tier0_ingress_mode = GRPC_ONLY` before watchdog action.
- AF_XDP polls `tier0_ingress_mode` once per RX batch, not per shred.

**Tests**
- synthetic divergence test,
- proactive self-amputation test on repeated structural failures,
- restore path test after manifest repair.

### 15.5 Module 4 - Rosetta Stone Calibrator
**Deliverables**
- consumes Golden Records from gRPC,
- verifies `(slot, shred_index)` overwrite guard before scanning,
- tries the **Predictive Shift Matrix** first (`+/-1, 2, 4, 8, 16` byte shifts),
- falls back to deep AVX-512 or AVX2 sweep only if the predictive fast path fails,
- validates on multiple Golden Records before publication.

**Tests**
- protocol-shift simulation,
- overwrite-guard rejection test,
- Predictive Shift Matrix fast-path test,
- false-positive defense using multiple records.

### 15.6 Module 5 - Hot-swap injection + recovery
**Deliverables**
- write manifest to inactive slot,
- flip active index,
- AF_XDP resumes correct parsing on next manifest read window,
- watchdog clears BLINDNESS after parity with gRPC returns.

**Tests**
- concurrent hot-swap under load,
- lab recovery SLO test.

### 15.7 Module 6 - Application-layer venue quarantine
**Deliverables**
- monitor executable-account hash for supported venue programs,
- on change write `venue_enabled = false` into shared risk flags,
- emit SEV-1 event.

**Tests**
- local validator program-upgrade test,
- worker O(1) venue-disable gate test.

### Acceptance criteria
- Auto-heal corrects parsing offsets without restarting Tier-0.
- Degraded gRPC-only mode keeps the system trading while heal completes.
- Venue quarantine trips on executable hash change.

---

## 16) Milestone M12 - Production Readiness Gates + Rollout

### Deliverables
- Release artifacts:
  - `shadow-tier0`, `shadow-state`, `shadow-pm`, `shadow-oracle`, `shadow-control`
  - systemd units,
  - CPU pinning config,
  - hugepage config,
  - host audit script,
  - smoke test script.

### Required gates
- **Correctness**
  - all unit and integration tests pass,
  - replay harness passes on latest captured traffic.
- **Performance**
  - worker p99 <= 150 us,
  - end-to-end p99 <= 500 us.
- **Security**
  - core dumps disabled,
  - secret mappings use `mlock` and `MADV_DONTDUMP`,
  - no secret material in `/dev/shm`.
- **Operational**
  - `panic = "abort"` restart behavior verified,
  - paging hooks verified,
  - revert-cost breaker verified,
  - Harmonic quarantine and Death Clamp verified in staging,
  - leader-schedule freshness alarms verified,
  - NACK fast-release path verified.

### Rollout steps
1. Deploy in observe-only mode.
2. Enable send at minimal size caps.
3. Ramp size caps gradually.
4. Enable Jito bundling after confirming:
   - tip rotation works,
   - BundleState fast-release works,
   - telemetry distinguishes Jito lane cleanly.
5. Enable Harmonic after confirming:
   - leader-schedule routing is correct,
   - priority-fee-only template bytes are correct,
   - Death Clamp works,
   - 1-strike quarantine works in staging.
6. Enable Control Plane auto-heal after confirming:
   - BLINDNESS trigger,
   - self-amputation,
   - manifest hot-swap recovery.

---

## Appendix A - Mandatory Compile-Time Layout Assertions

Use dedicated layout-test modules in the relevant crates.

```rust
use static_assertions::const_assert;
use core::mem::{align_of, size_of};

const_assert!(size_of::<PoolPayload56B>() == 56);
const_assert!(size_of::<PoolHotState64B>() == 64);
const_assert!(align_of::<PoolHotState64B>() == 64);
const_assert!(align_of::<ActiveIdx128B>() == 128);

const_assert!(size_of::<BlockhashPayload56B>() == 56);
const_assert!(size_of::<BlockhashSlot64B>() == 64);

const_assert!(size_of::<ParsingManifest64B>() == 64);
const_assert!(size_of::<NackRecord64B>() == 64);
```

---

## Appendix B - No-Alloc Hot Path Test Pattern

- Provide a `deny_alloc` feature that replaces the global allocator with one that panics.
- Run a worker-loop harness under this feature in CI.
- Treat any allocation as a build-breaking regression.

---

## Appendix C - Offline Capture Formats

Supported replay inputs:
- PCAP,
- raw UDP payload dumps,
- UMEM archive records using offset-based frame references.

Define a stable replay-record header so regressions can be tested without recapturing traffic.

---

## Appendix D - Known Failure Modes and Required Alarms

1. Revert bleed in PUBLIC lane
2. Phantom inventory / fence pinned near limit
3. AF_XDP BLINDNESS asserted
4. Kernel-bypass RX/TX ring drops sustained non-zero
5. Venue program upgrade detected
6. Leader-schedule freshness stale
7. Harmonic simulator leak / lane quarantined
8. Fill ring starvation causing survival-mode flushes
9. NACK rate spike indicating local egress collapse
