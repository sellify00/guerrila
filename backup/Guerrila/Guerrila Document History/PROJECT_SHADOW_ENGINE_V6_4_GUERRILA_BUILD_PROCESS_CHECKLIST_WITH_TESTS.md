# Project Shadow Engine V6.4-Guerrila - Detailed Build Process Checklist (Milestone-Gated, Test-Driven)

**Document type:** Implementation process + milestone test plan  
**Applies to:** V6.4 data plane + V1.0 control plane running under the V6.4-Guerrila sidecar  
**Parent docs:** `PROJECT_SHADOW_ENGINE_V6_4_GRAND_UNIFIED_SYSTEM_SPEC.md`, `PROJECT_SHADOW_ENGINE_V6_4_BUILD_PROCESS_CHECKLIST_WITH_TESTS.md`, `V6.4-Guerrila.md`  
**Role:** Build the low-OpEx, private-lane-only deployment profile first, then scale only after live proof  
**Primary objective:** defend a 150 SOL treasury, keep public-lane execution amputated, and ship a lean Jito/Harmonic sniper that still preserves the core V6.4 correctness and latency firewalls.

---

## 0. Guerrila doctrine (what changes versus full V6.4)

This checklist is **not** the full-fat V6.4 rollout. It is the early deployment path for the Guerrila profile.

### 0.1 What stays mandatory from V6.4
- AF_XDP primary ingress and kernel-bypass discipline
- O(1) money-worker loop
- fixed-layout shared memory and wait-free double-buffer protocol
- `shadow_leader_schedule` for slot-correct venue routing
- dual tip LUTs (`shadow_tip_lut_base`, `shadow_tip_lut_jito`)
- `shadow_nack_ring` fast capital release
- `ShadowGuard::AssertReservesInRange { min_reserve_in, max_reserve_in, min_reserve_out, max_reserve_out }`
- Harmonic Death Clamp and 1-Strike Lethal Catch quarantine
- Rosetta Stone auto-heal

### 0.2 What is amputated in Guerrila mode
- No public TPU racing
- No SWQoS relay spend
- No public QUIC workers
- No public fallback from private orderflow
- No whale hunting above the Guerrila treasury envelope
- No engineering time spent optimizing lanes that Guerrila forbids using

### 0.3 Authoritative runtime overrides
These runtime semantics are non-negotiable in Guerrila mode:
- `submit.routing.default_public_geyser = drop`
- `submit.routing.default_jito_searcher = jito_bundle`
- `submit.routing.private_mempool_strategy = jito_bundle_if_compatible`
- `submit.routing.enable_harmonic_lane = true`
- `system.proxies.egress.quic_workers = 0`
- `submit.quic.kamikaze_enabled = false`
- `risk.reserve_floor_sol = 10.0`
- `risk.max_position_sol_per_mint = 15.0`
- `risk.max_notional_sol_per_entity = 30.0`
- `shadow-oracle` must filter out any opportunity requiring more than `15.0 SOL` backrun capital

### 0.4 What to build now vs later

**Build now:**
- one commodity bare-metal host
- AF_XDP ingress
- `shadow-state`
- `shadow-tier0` money workers
- Jito bundle lane
- Harmonic bundle lane
- `shadow-pm`
- `shadow-oracle`
- `shadow-control`

**Do not build now:**
- public QUIC blast lane
- SWQoS relay pool management
- relay fanout scheduler
- public-lane fee tuning
- TPU rescue logic
- any route branch that can turn a private-lane failure into a public send

### 0.5 Recommended rollout philosophy
The spec caps are 10 / 15 / 30 SOL, but the **operational** rollout should still start below them.

Recommended ramp:
1. Observe-only: no sends
2. Tiny live caps: `max_position_per_mint = 1 SOL`, `max_notional_per_entity = 2 SOL`
3. Small live caps: `3 SOL` / `6 SOL`
4. Medium live caps: `5 SOL` / `10 SOL`
5. Pre-spec caps: `10 SOL` / `20 SOL`
6. Full Guerrila caps: `15 SOL` / `30 SOL`

Advance only if the prior rung is profitable, stable, and free of phantom inventory or fee leaks.

---

## 1. Non-negotiable firewalls (must exist before performance work)

### 1.1 Firewall A - Rust RCU correctness
- payload behind `UnsafeCell<T>`
- volatile payload reads/writes
- `ActiveIdx128B` aligned to 128 bytes
- slot structs aligned to 64 bytes
- `atomic::fence(Ordering::Acquire)` after payload copy
- no plain unsynchronized struct copies on shared hot state

### 1.2 Firewall B - Absolute fee clamp
- clamp `tip_lamports` to the absolute hard ceiling immediately before CU-price conversion
- clamp must be applied to both:
  - public-lane code in the master tree
  - Harmonic lane priority-fee conversion in Guerrila mode

### 1.3 Firewall C - Death Clamp
- if Harmonic egress fails, drop the tx
- never re-route a Harmonic-built signed transaction to public blast

### 1.4 Firewall D - Harmonic 1-Strike quarantine
- if any Harmonic-routed tx lands on-chain with `Err`, `shadow-pm` flips `harmonic_quarantined = true`
- workers must bypass Harmonic in O(1) time after the flag is visible

### 1.5 Firewall E - Public-lane amputation
At least one of the following must be true:
- public lane is compiled out behind a feature gate for Guerrila builds, or
- public lane code remains in the tree but runtime config validation makes it impossible to start with a public execution path enabled

The stronger option is feature-gating it out for the Guerrila release artifact.

---

## 2. Repository strategy and build targets

### 2.1 Binaries
- `shadow-tier0`
- `shadow-state`
- `shadow-pm`
- `shadow-oracle`
- `shadow-control`

### 2.2 Recommended crate split
- `shadow-core`
- `shadow-ipc`
- `shadow-crypto`
- `shadow-ingest`
- `shadow-egress-jito`
- `shadow-egress-harmonic`
- `shadow-risk`
- `shadow-guard-program`

### 2.3 Recommended cargo features
- `guerrila_profile`
- `public_lane`
- `harmonic_lane`
- `control_plane`
- `loom`
- `deny_alloc`

**Guerrila release recommendation:**
- enable: `guerrila_profile`, `harmonic_lane`, `control_plane`
- disable: `public_lane`

### 2.4 CI lanes
- `dev`
- `release`
- `release-guerrila`
- `release-guerrila-perf`

`release-guerrila` must be the artifact used for all live Guerrila rollouts.

---

## 3. Milestone map

1. **G0 - Workspace, feature gates, config guardrails** --- DONE
2. **G1 - Commodity host determinism and kernel-bypass bringup**
3. **G2 - Shared memory, rings, RCU, leader schedule, NACK foundation** --- PARTIAL (blockhash RCU, leader schedule, risk flags done)
4. **G3 - `shadow-state` on shared Geyser + state publishing**
5. **G4 - AF_XDP ingress, broad-spectrum shred matrix, and decode**
6. **G5 - Money worker core under Guerrila routing semantics** --- PARTIAL (routing fork + route matrix tests done)
7. **G6 - Jito egress proxy**
8. **G7 - Harmonic egress proxy**
9. **G8 - `shadow-pm` inventory truth and quarantine logic**
10. **G9 - `shadow-oracle` dual LUTs + Guerrila target filter**
11. **G10 - On-chain `ShadowGuard` + exact template audits**
12. **G11 - `shadow-control` Rosetta Stone auto-heal**
13. **G12 - Observe-only, tiny-cap live rollout, and scale ladder**
14. **G13 - Graduation criteria to full V6.4**

Each milestone below includes deliverables, build steps, tests, and exit criteria.

---

## 4. Milestone G0 - Workspace, feature gates, config guardrails --- DONE

### Deliverables
- [x] monorepo builds all five binaries
- [x] `release-guerrila` artifact exists
- [x] Guerrila runtime validator exists and fails closed on forbidden config
- [x] optional compile-time amputation of public lane exists

### Build steps
1. [x] Lock toolchain and production profile (`panic = "abort"`).
2. [x] Add a `guerrila_profile` feature and config namespace.
3. [x] Add a boot-time `validate_guerrila_config()` routine.
4. [x] Add a separate build target or feature gate for `public_lane`.
5. [x] Ensure `shadow-tier0` refuses to start in Guerrila mode if any of the following are true:
   - `quic_workers != 0`
   - public routing target is anything other than `drop`
   - any routing branch can resolve to public blast
   - Harmonic is enabled without quarantine support
   - risk caps exceed Guerrila maximums
6. [x] Add a metric and startup log snapshot that prints the resolved routing profile and confirms `PUBLIC_EXECUTION_DISABLED=true`.

### Required tests
1. [x] **Config validation tests**
   - valid Guerrila config boots
   - invalid values hard-fail startup
2. [x] **Feature gate compile tests**
   - `release-guerrila` builds without `public_lane`
   - binary still builds with `public_lane` enabled in non-Guerrila profiles
3. [x] **Route-resolution regression tests**
   - `PUBLIC_GEYSER` never resolves to a send path in Guerrila mode
4. [ ] **Static lint**
   - grep or AST-based CI check that `release-guerrila` does not instantiate public QUIC worker constructors

### Exit criteria
- [x] Guerrila build artifact exists
- [x] public-lane execution cannot be enabled accidentally

---

## 5. Milestone G1 - Commodity host determinism and kernel-bypass bringup

### Deliverables
- one-host deployment recipe for Frankfurt or Tokyo commodity bare metal
- host audit script
- AF_XDP RX/TX sample app stable on the chosen host

### Build steps
1. Select a single AMD EPYC-class host with 128 GB+ RAM and NIC on the local NUMA node.
2. Apply BIOS profile:
   - lock CPU governor to `performance`
   - disable deep C-states
   - disable SMT on money cores
   - set AVX negative offsets to zero only if thermally safe under fixed all-core clock
3. Apply kernel args:
   - `isolcpus`
   - `nohz_full`
   - `rcu_nocbs`
4. Reserve hugepages at boot.
5. Set up NIC queueing and AF_XDP queue affinity.
6. Enable RX hardware timestamping.
7. Pin cores by domain:
   - worker + nonce cores
   - ingress cores
   - egress cores
   - utility/control cores
8. Keep all Tier-0 domains on the NIC-local NUMA node when possible.

### Required tests
1. **Host audit script**
   - CPU governor
   - hugepages mounted
   - NIC NUMA alignment
   - no IRQs on money cores
   - L3 isolation between Tier-0 and utility/control cores
2. **AF_XDP functional test**
   - inject synthetic UDP packets
   - verify zero corruption and expected packet counts
3. **Hardware timestamp test**
   - verify RX metadata timestamps are visible to userspace and monotonically increasing
4. **Latency microbench**
   - RX descriptor visible in userspace p50 and p99
5. **Thermal stability test**
   - run AVX-heavy workload and verify clock remains locked, not oscillating unpredictably

### Exit criteria
- AF_XDP stable on the commodity host
- host audit passes cleanly
- no reason remains to buy Equinix before software proof

---

## 6. Milestone G2 - Shared memory, rings, RCU, leader schedule, NACK foundation --- PARTIAL

### Deliverables
- [x] `shadow-ipc` provides all required segment layouts and bootstrap logic
- [x] versioned `/dev/shm` headers exist
- [x] wait-free reader/writer protocol exists for all double-buffered state

### Segments that must exist in Guerrila mode
- [ ] `/dev/shm/shadow_feed_ring_w{worker}_p{producer}`
- [ ] `/dev/shm/shadow_egress_ring_jito_*`
- [ ] `/dev/shm/shadow_egress_ring_harmonic_*`
- [ ] `/dev/shm/shadow_pool_index_map`
- [ ] `/dev/shm/shadow_pool_state`
- [ ] `/dev/shm/shadow_mint_index_map`
- [ ] `/dev/shm/shadow_mint_state`
- [ ] `/dev/shm/shadow_alt_cache`
- [ ] `/dev/shm/shadow_tip_lut_base`
- [ ] `/dev/shm/shadow_tip_lut_jito`
- [ ] `/dev/shm/shadow_inventory_fence`
- [ ] `/dev/shm/shadow_health`
- [x] `/dev/shm/shadow_blockhash`
- [x] `/dev/shm/shadow_leader_schedule`
- [x] `/dev/shm/shadow_risk_flags`
- [ ] `/dev/shm/shadow_nack_ring`
- [ ] `/dev/shm/shadow_parsing_manifest`
- [ ] `/dev/shm/shadow_control_flags`
- [ ] `/dev/shm/shadow_raw_archive`
- [ ] `/dev/hugepages/shadow_umem_*`

### Build steps
1. [x] Implement `SegmentHeader64B` and version checks.
2. [ ] Implement SPSC rings with drop-on-full semantics.
3. [ ] Implement 8-byte tag maps for pools and mints.
4. [x] Implement the 128B-isolated `active_idx` + 64B slots RCU pattern.
5. [x] Implement `shadow_leader_schedule` as a fixed-capacity `AtomicU8` array.
6. [ ] Implement `shadow_nack_ring` entries as fixed-size structs.
7. [x] Add segment bootstrap code that refuses mismatched versions.

### Required tests
1. [x] **Compile-time layout tests**
   - exact sizes and alignments for pool, blockhash, manifest, NACK, and other shared slots
2. [ ] **Loom concurrency tests**
   - RCU reader/writer correctness
   - `shadow_leader_schedule` atomic byte semantics
3. [ ] **Runtime stress tests**
   - one writer, many readers on RCU segments
   - ring saturation and drop-on-full behavior
4. [x] **Version mismatch tests**
   - reader refuses mismatched segment version
5. [ ] **NACK ring tests**
   - consumer reads exactly what producer wrote under burst

### Exit criteria
- all shared-memory primitives stable under 60-second stress
- no invalid snapshots and no torn reads accepted

---

## 7. Milestone G3 - `shadow-state` on shared Geyser + state publishing

### Deliverables
- `shadow-state` uses the premium shared Geyser/RPC source for state replication and warm-start data
- publishes pools, mints, ALTs, health, and leader schedule tags
- handles fork rollbacks via the same publish protocol

### Build steps
1. Implement pool account decoder and vault reserve extraction.
2. Implement mint metadata snapshot including Token-2022 transfer fee configuration.
3. Implement ALT cache snapshot.
4. Implement pool and mint dynamic append with index-map publish order.
5. Implement health snapshot publication.
6. Implement leader-schedule publisher:
   - fetch epoch leader schedule
   - map leaders to Vanilla/Jito/Harmonic
   - write `engine_tags[slot & mask] = enum`
7. Expose freshness metrics for state and leader schedule.

### Required tests
1. **Golden decode tests**
   - captured Raydium and mint fixtures decode correctly
2. **Token-2022 fee tests**
   - transfer fee config lands in the designated hot-state field correctly
3. **Dynamic append tests**
   - new pools and mints become visible without restart
4. **Fork rollback tests**
   - revert to previous pool state via inactive-slot publish
5. **Leader-schedule mapping tests**
   - Jito/Harmonic/Vanilla classification is deterministic
6. **Freshness alarm tests**
   - stale leader schedule or pool state triggers health flags

### Exit criteria
- `shadow-state` runs for 1 hour on read-only mainnet sources without crash
- new pools and mints can appear without restart

---

## 8. Milestone G4 - AF_XDP ingress, broad-spectrum shred matrix, and decode

### Deliverables
- AF_XDP ingress path receives both data shreds and coding shreds
- slot-scoped fragment contexts exist
- FEC recovery slow path exists
- gRPC fallback ingest remains available for warm-start and audit
- Flight Data Recorder exists

### Build steps
1. Implement AF_XDP RX loop using shared UMEM.
2. Parse Turbine `(slot, shred_index)` headers.
3. Maintain `SlotFragCtx[4]` with bounded reorder buffers.
4. Implement the Shred Matrix decision logic:
   - fast path on contiguous data
   - FEC recovery path when a data shred gap is detected and sufficient coding/data shreds are present
5. Implement the positive-gap flush rule and late-duplicate drop rule.
6. Extract ingress timestamp from RX metadata, not from syscalls.
7. Run signer allowlist across **all signers**, not just fee payer.
8. Perform ALT expansion and strict decode only after allowlist hit.
9. Route decoded `IngressRecord` by pool pubkey with the 80% overflow valve.
10. Implement Flight Data Recorder:
   - shared UMEM backing file
   - `delay_fifo`
   - archive ring with UMEM offsets
   - survival-mode flush back to Fill ring
11. Keep gRPC fallback running for redundancy and control-plane audit.

### Required tests
1. **Offline shred replay harness**
   - slot interleave correctness
   - contiguous crossover correctness
   - late duplicate handling
   - bounded positive-gap reset
2. **FEC recovery tests**
   - single missing data shred reconstructed from coding/data mix
   - insufficient shards leave slot unresolved but do not corrupt other slots
3. **Broad-spectrum ingest tests**
   - coding shreds are ingested and classified, not discarded
4. **Allowlist tests**
   - hidden trading wallet in signer 1 or signer 2 still matches
5. **Ingress timestamp tests**
   - timestamp in `IngressRecord` originates from RX metadata
6. **Archive tests**
   - archive drop-on-full does not block ingress
   - survival mode flushes FIFO when Fill ring approaches starvation
7. **Routing tests**
   - primary worker assignment is stable
   - overflow valve goes to adjacent worker only when the primary ring exceeds threshold

### Exit criteria
- ingress replay passes
- FEC recovery works on captured degraded flows
- no blocking under archive backpressure

---

## 9. Milestone G5 - Money worker core under Guerrila routing semantics --- PARTIAL

### Deliverables
- [ ] worker loop is O(1), no alloc, no lock, no syscall
- [x] public lane is amputated in the routing fork
- [ ] worker builds only Jito or Harmonic envelopes in Guerrila mode

### Build steps
1. [ ] Implement thread-local signature dedupe.
2. [ ] Implement fast staleness gates.
3. [ ] Implement pool and mint lookup through tag maps + arena verification.
4. [ ] Implement thread-local dirty overlay seeded from shared pool state.
5. [ ] Implement forward projection quote logic.
6. [ ] Apply Token-2022 transfer fee discount before AMM math.
7. [x] Implement `next_scheduled_slot` read from blockhash snapshot.
8. [x] Load `engine = shadow_leader_schedule[next_scheduled_slot & mask]`.
9. [x] Implement Guerrila routing fork:
   - `PUBLIC_GEYSER` -> drop
   - `PRIVATE_MEMPOOL` or `JITO_SEARCHER_API` -> Harmonic if leader is Harmonic, lane enabled, and not quarantined
   - otherwise Jito if compatible and timing margin is satisfied
   - otherwise drop
10. [ ] Implement a final worker-side hard reject if `trade_size_lamports > 15 SOL`.
11. [ ] Select the correct LUT:
   - Jito -> `shadow_tip_lut_jito`
   - Harmonic -> `shadow_tip_lut_base`
12. [ ] Apply shifted bucket rounding and fraction-u32 multiply.
13. [ ] Apply the absolute fee clamp.
14. [ ] Convert lamports to CU-price with u128 math.
15. [ ] Perform inventory reservation via `fetch_add` with rollback on breach.
16. [ ] Build templates:
   - Jito template with tip transfer and per-intent tip rotation
   - Harmonic template with CU-limit + CU-price and **no** Jito tip transfer
   - both include `ShadowGuard::AssertReservesInRange {4 bounds}` as the first non-budget instruction
17. [ ] Sign using the nonce ring.
18. [ ] If egress enqueue fails, immediately roll back inventory and abort the intent.
19. [ ] Update overlay only on successful enqueue.

### Required tests
1. [ ] **Deterministic math tests**
   - forward projection
   - min-out computation
   - Token-2022 fee discount
2. [x] **Guerrila route matrix tests**
   - `PUBLIC_GEYSER` -> drop
   - private + Harmonic leader + healthy -> Harmonic
   - private + Harmonic leader + quarantined -> Jito if valid else drop
   - private + non-compatible lane -> drop
3. [ ] **LUT selection tests**
   - Jito leader uses Jito LUT
   - Harmonic leader uses base LUT
4. [ ] **Whale filter tests**
   - trade requiring >15 SOL is dropped even if oracle made a mistake
5. [ ] **Template tests**
   - Harmonic template contains no tip transfer instruction
   - Jito template contains rotated tip account
   - both templates include four-bound `ShadowGuard`
6. [ ] **Fee clamp tests**
   - overflow-scale alpha still clamps correctly
7. [ ] **Inventory rollback tests**
   - breach rolls back immediately
   - egress enqueue failure rolls back immediately
8. [ ] **No-alloc tests**
   - custom allocator panics if worker allocates
9. [ ] **No-syscall tests**
   - worker thread issues no network or time syscalls
10. [ ] **Microbench**
   - worker dequeue -> egress enqueue p99

### Exit criteria
- worker loop stable and fully private-lane-only in Guerrila mode
- no path can emit a public envelope

---

## 10. Milestone G6 - Jito egress proxy

### Deliverables
- dedicated Jito egress proxy
- Searcher API sessions pre-warmed
- proxy emits `EgressResult` and `NackRecord64B` when local send fails before network transmission

### Build steps
1. Implement persistent Jito Searcher API client.
2. Create per-worker Jito egress rings.
3. Implement dequeue -> bundle frame -> send loop.
4. On dequeue failure after reservation but before network transmission, emit NACK immediately.
5. Keep this proxy byte-blast-only; do not parse BundleState telemetry here.

### Required tests
1. **Session loss tests**
   - no blocking on reconnect
2. **Bundle framing tests**
   - tx bytes preserved exactly post-sign
3. **NACK emission tests**
   - local send failure produces exactly one NACK
4. **No telemetry creep tests**
   - proxy does not instantiate or depend on BundleState websocket code

### Exit criteria
- Jito proxy drops fast when unavailable and never blocks worker progress

---

## 11. Milestone G7 - Harmonic egress proxy

### Deliverables
- dedicated Harmonic gRPC proxy with regional sessions
- regional fanout logic on proxy cores only
- Death Clamp enforced

### Build steps
1. Implement gRPC clients to all configured Harmonic regions.
2. Maintain authenticated sessions on proxy cores.
3. Convert worker envelopes into Harmonic `SendBundle` frames.
4. Record `region_mask`, `ok_mask`, and `err_code` per envelope.
5. Emit NACK on local pre-transmission failure.
6. Enforce Death Clamp:
   - Harmonic send failure -> drop
   - absolutely no TPU/public fallback

### Required tests
1. **Regional session tests**
   - all configured regions connect independently
2. **Template semantic tests**
   - Harmonic tx is priority-fee only
3. **Death Clamp tests**
   - auth error, timeout, deadline, not-leader, and RPC rejection all end in drop
4. **No-public-fallback tests**
   - no public send attempt occurs after Harmonic failure
5. **NACK tests**
   - local queue or framing failures generate immediate NACK

### Exit criteria
- Harmonic lane works as a pure private lane with no leakage into public execution

---

## 12. Milestone G8 - `shadow-pm` inventory truth and quarantine logic

### Deliverables
- pending table keyed by signature
- immediate capital release via `shadow_nack_ring`
- Jito BundleState and Harmonic BundleState terminal paths
- delta reconciliation
- Harmonic 1-Strike quarantine

### Build steps
1. Implement pending table storing:
   - signature
   - route
   - mint index
   - trade size
   - send timestamp
2. Continuously poll `/dev/shm/shadow_nack_ring`.
3. Subscribe to Jito BundleState on a utility core.
4. Subscribe to Harmonic BundleState on a utility core.
5. Poll `getSignatureStatuses` for all pending signatures.
6. Release fence reservations on:
   - NACK
   - bundle rejected/dropped/not landed
   - on-chain revert
   - expiry timeout
   - landed success
7. Implement delta-based reconciliation against settled chain position.
8. Implement Harmonic quarantine:
   - first on-chain `Err` for a Harmonic-routed tx flips `harmonic_quarantined=true`
9. Publish the quarantine flag to shared risk state visible to workers.

### Required tests
1. **Reservation leak tests**
   - large failure percentage still returns fence to baseline
2. **NACK fast-release tests**
   - local egress failure frees capital immediately
3. **Jito BundleState tests**
   - `Rejected`, `Dropped`, `NotLanded` release immediately
4. **Harmonic BundleState tests**
   - `SimulationFailed`, `Rejected`, `Dropped` release immediately
5. **Expiry tests**
   - stale pending signatures release after max age
6. **Quarantine tests**
   - first Harmonic on-chain revert flips the quarantine flag
7. **Worker visibility tests**
   - workers stop producing Harmonic-routed envelopes after quarantine becomes visible

### Exit criteria
- no phantom inventory under stress
- Harmonic lane can self-amputate autonomously

---

## 13. Milestone G9 - `shadow-oracle` dual LUTs + Guerrila target filter

### Deliverables
- dual LUT publisher
- hard target filter for >15 SOL required capital
- retail-only bid caps

### Build steps
1. Publish `shadow_tip_lut_base` and `shadow_tip_lut_jito` through a stable snapshot mechanism.
2. Implement target prefilter based on required backrun capital.
3. Implement separate cap policy for Guerrila that assumes retail mistakes, not whale wars.
4. Keep Jito tax compensation in `shadow_tip_lut_jito`.
5. Keep Harmonic and Vanilla on `shadow_tip_lut_base`.
6. Publish allowlist/cap snapshot for workers.

### Required tests
1. **Capital filter tests**
   - targets above 15 SOL required capital never become actionable
2. **Dual-LUT math tests**
   - Jito LUT >= base LUT for equivalent validator value when tax > 0
3. **Retail cap tests**
   - cap policy clamps high-alpha buckets to Guerrila-safe levels
4. **Publish/consume tests**
   - worker sees valid snapshots and fails closed on missing/stale LUTs
5. **Regime shift tests**
   - Jito tax multiplier change only affects the Jito LUT

### Exit criteria
- oracle can no longer accidentally steer the bot into whale contests

---

## 14. Milestone G10 - On-chain `ShadowGuard` + exact template audits

### Deliverables
- minimal guard program
- audited Jito and Harmonic templates
- exact Harmonic CU limit calibration

### Build steps
1. Implement `AssertReservesInRange { min_reserve_in, max_reserve_in, min_reserve_out, max_reserve_out }`.
2. Keep the guard pre-swap, no CPI, no routing.
3. Build local validator integration tests with the real template shapes.
4. Measure and lock the exact CU limit for:
   - Jito backrun template
   - Harmonic backrun template
5. Remove all arbitrary padding from the Harmonic CU limit.

### Required tests
1. **Program unit tests**
   - inclusive bounds
   - zero and max values
2. **Integration tests**
   - upstream target no-op -> guard aborts
   - pathological adverse reserve movement -> guard aborts
   - valid projected range -> swap continues
3. **Compute tests**
   - guard compute remains minimal
4. **Harmonic CU exactness test**
   - Harmonic template pays only the intended priority fee and not a padded overpayment

### Exit criteria
- both private lanes use the same state-proof discipline and Harmonic does not overpay for empty CU headroom

---

## 15. Milestone G11 - `shadow-control` Rosetta Stone auto-heal

### Deliverables
- parsing manifest double buffer
- Flight Data Recorder consumer
- Tripwire blindness detection
- SIMD calibrator
- executable hash venue quarantine

### Build steps
1. Implement `shadow_parsing_manifest` using the same RCU rules.
2. Implement AF_XDP ingress hook that reads the manifest only at slot boundaries or every N shreds.
3. Implement Tripwire comparing AF_XDP hits to gRPC hits.
4. Implement GRPC_ONLY downgrade flag in `shadow_control_flags`.
5. Implement overwrite guard on raw archive frames.
6. Implement Predictive Shift Matrix before deep SIMD sweep.
7. Implement deep scan fallback.
8. Validate on multiple Golden Records before publishing new offsets.
9. Implement program executable hash monitoring and venue disable flag.

### Required tests
1. **Blindness tests**
   - AF_XDP miss + gRPC hit triggers downgrade
2. **Manifest hot-swap tests**
   - wrong offsets healed without restart
3. **Overwrite guard tests**
   - stale/overwritten UMEM frames are rejected by the calibrator
4. **Predictive shift tests**
   - small offset drift is repaired without full scan
5. **Program upgrade tests**
   - venue is disabled immediately on executable hash change

### Exit criteria
- Tier-0 can degrade and heal without restarting the main engine

---

## 16. Milestone G12 - Observe-only, tiny-cap live rollout, and scale ladder

### Deliverables
- observe-only mode
- tiny-cap live mode
- hard checklist for each capital ramp

### Build steps
1. Add observe-only flag:
   - decode
   - quote
   - build
   - sign
   - send to sink or no-send backend
2. Run observe-only for enough captured and live traffic to validate:
   - routing mix
   - public drops
   - private-lane opportunities
   - leader-schedule correctness
   - fence accounting
3. Turn on live trading at the first rung:
   - `1 SOL` per mint
   - `2 SOL` per entity
4. Promote caps only after passing all ramp gates.

### Required tests and gates at every rung
1. **Public-lane amputation gate**
   - zero live public sends
2. **Inventory truth gate**
   - fence ~= settled position + in-flight reservations
3. **Fee leak gate**
   - no unexplained priority-fee burn
4. **Quarantine drill**
   - simulated Harmonic failure triggers quarantine correctly
5. **Latency gate**
   - worker p99 remains within target budget
6. **PnL gate**
   - realized net EV is positive after fees over the chosen sample window

### Exit criteria
- the engine survives tiny-size live conditions before it is trusted with spec-level caps

---

## 17. Milestone G13 - Graduation criteria to full V6.4

Do **not** spend money on the full public-war machine until all of the following are true in Guerrila mode:
- sustained positive realized net EV at full Guerrila caps
- no phantom inventory incidents
- no unresolved fee-leak incidents
- leader-schedule routing stable
- Harmonic quarantine tested and trustworthy
- Rosetta auto-heal tested in staging
- enough dropped public opportunities are visible to justify public-lane spend
- treasury growth and confidence justify SWQoS / Equinix expansion

Only after that should you consider enabling the larger V6.4 public-lane investment path.

---

## 18. Guerrila-specific acceptance criteria

The Guerrila build is done only when all of the following are true:
- `release-guerrila` starts only with public execution disabled
- no public target is ever sent
- all actionable flow stays inside Jito or Harmonic
- opportunities above 15 SOL required capital are filtered before they become expensive work
- Jito lane works
- Harmonic lane works
- Harmonic failure cannot leak into public send
- Harmonic on-chain revert quarantines the lane automatically
- `shadow_nack_ring` frees capital immediately on local send failure
- `ShadowGuard` protects both private lanes
- worker p99 stays within the V6.4 budget target on the chosen commodity host
- monthly infra stays near the Guerrila ceiling because public infra was never provisioned

---

## Appendix A - Guerrila boot validation checklist

A Guerrila deployment must fail startup if any of the following are true:
- `system.proxies.egress.quic_workers != 0`
- `submit.quic.kamikaze_enabled == true`
- `submit.routing.default_public_geyser != drop`
- `submit.routing.private_mempool_strategy` can resolve to public blast
- any active relay or SWQoS public endpoint is configured
- Harmonic lane is enabled while quarantine support is disabled
- `risk.reserve_floor_sol < 10.0`
- `risk.max_position_sol_per_mint > 15.0`
- `risk.max_notional_sol_per_entity > 30.0`

---

## Appendix B - Recommended minimum config patch for Guerrila

```yaml
system:
  mode: tier0_only
  proxies:
    ingress:
      primary: shred_capture
      shred_capture_workers: 1
      grpc_fallback: yellowstone_grpc
      grpc_workers: 1
    egress:
      quic_workers: 0
      jito_workers: 1
      harmonic_workers: 1

risk:
  reserve_floor_sol: 10.0
  max_position_sol_per_mint: 15.0
  max_notional_sol_per_entity: 30.0

submit:
  routing:
    default_public_geyser: drop
    default_jito_searcher: jito_bundle
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: true
  quic:
    kamikaze_enabled: false

oracle:
  guerrila:
    max_required_backrun_capital_sol: 15.0
    retail_only_mode: true
```

---

## Appendix C - What not to waste time on yet

Do not prioritize any of the following before Guerrila profitability is proven:
- public relay fanout
- staked QUIC lane tuning
- SWQoS identity management
- Equinix deployment
- public fee-optimization work
- TPU rescue logic
- multi-host redundancy for public sends

The correct Guerrila sequence is: private lanes first, proof second, scale third.
