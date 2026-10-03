# Project Shadow Engine V6.3 — Build Process Checklist (Milestone-Gated, Test-Driven)

**Document type:** Implementation process + milestone test plan  
**Applies to:** Data Plane V6.3 (`shadow-tier0`, `shadow-state`, `shadow-pm`, `shadow-oracle`) + Control Plane V1.0 (`shadow-control`)  
**Goal:** Produce a *production-deployable* system that meets the V6.3 latency + correctness invariants, and ships with a Control Plane auto-heal that can restore Tier‑0 ingestion without stopping the engine.

---

## 0) Ground Rules (Non-Negotiable)

### 0.1 Data Plane invariants (must remain true at all times)
- Money-workers:
  - **0 allocations / 0 locks / 0 syscalls** inside the worker loop.
  - **Bounded O(1)** per intent.
  - **Fail-fast** (drop intent) on any state uncertainty.
- Ingress and egress:
  - Stateful networking is **off** the money cores.
  - Kernel bypass is mandatory where it matters (AF_XDP/DPDK/ef_vi).
- Shared memory:
  - All shared structs are fixed-layout and size/align validated at compile-time.
  - No “hot reload” or dynamic schema parsing on Tier‑0 cores.

### 0.2 “Firewall” patches that must be in V6.3 before any performance work
These are survival constraints, not optional “later hardening.”

**Firewall 1 — Rust RCU (no UB; no false sharing; tear detection)**
- Use `UnsafeCell` for the payload and access via volatile reads/writes.
- Active index must be **128B aligned**, not 64B, to defeat adjacent-line L2 prefetch side effects.
- Reader must include a **hardware fence** after payload copy (not just `compiler_fence`) to prevent load-load reordering on weak memory-order CPUs.

**Firewall 2 — Absolute Priority Fee Clamp**
- Apply a hard ceiling to per-trade priority fee (or per-trade “tip lamports” for PUBLIC_GEYSER → CU-price conversion) immediately before building `SetComputeUnitPrice`.

> Treat these as “definition of V6.3,” not a later patch.

---

## 1) Repository / Workspace Layout (Build Once, Keep Stable)

### 1.1 Binaries (fault domains)
- `shadow-tier0` — Tier‑0 lane (money workers + ingress/egress proxies + nonce precompute + background)
- `shadow-state` — state daemon (fork-aware pool/mint/ALT replication → shared memory publishing)
- `shadow-pm` — position manager (inventory reconciliation + exits + status tracking)
- `shadow-oracle` — alpha model + tip LUT publisher
- `shadow-control` — **Control Plane** (watchdog + calibrator + manifest injector + quarantine)

### 1.2 Crates (recommended)
- `shadow-core` — shared types, layout-checked structs, ringbuffer primitives, hash/tag utilities
- `shadow-ipc` — `/dev/shm` segment layouts + SPSC ring + double-buffer implementation
- `shadow-crypto` — nonce ring + Ed25519 signing pipeline + zeroization glue
- `shadow-ingest` — decode library (shared by ingress proxy and offline replay tests)
- `shadow-guard-program` — on-chain guard (Anchor or raw Solana program)

### 1.3 Build modes
- `dev` — correctness, sanitizers, extra logging (never used for perf claims)
- `release` — production profile: LTO, `panic=abort`, no debug asserts on hot path
- `release-perf` — same as release plus extra perf counters compiled in (behind feature flags)

---

## 2) Milestone Map (What Must Exist Before the Next Layer)

1. **M0 — Toolchain + CI skeleton**
2. **M1 — Host determinism + kernel-bypass bringup**
3. **M2 — Shared memory primitives (SPSC rings + RCU double-buffer + index maps)**
4. **M3 — `shadow-state` (pool/mint/ALT replication + publish)**
5. **M4 — Ingress proxy (AF_XDP shred capture + scanner + strict decode + routing)**
6. **M5 — Worker core (O(1) pipeline + tip LUT + fee clamp + ShadowGuard params)**
7. **M6 — Egress proxies (QUIC “kamikaze” + Jito bundles + BundleState stream)**
8. **M7 — `shadow-pm` (revert-aware fence release + delta reconciliation)**
9. **M8 — `shadow-oracle` (tip LUT fraction_u32 + bucket rounding + publishing)**
10. **M9 — On-chain `ShadowGuard` (range guard)**
11. **M10 — End-to-end perf validation (worker p99 + e2e p99)**
12. **M11 — Control Plane V1.0 (`shadow-control`) auto-heal**
13. **M12 — Production readiness gates + rollout playbook**

Each milestone below includes:
- **Deliverables**
- **Implementation checklist**
- **Required tests**
- **Acceptance criteria (“done means”)**

---

## 3) Milestone M0 — Toolchain + CI Skeleton

### Deliverables
- Monorepo workspace builds all binaries.
- CI runs (Linux x86_64 and Linux aarch64 cross build):
  - `cargo fmt --check`
  - `cargo clippy -D warnings`
  - `cargo test` (unit tests)
  - `cargo test --features loom` (concurrency model tests)
  - `cargo test --features fuzz_smoke` (quick fuzz entrypoints)

### Implementation checklist
- Lock Rust toolchain (e.g., `rust-toolchain.toml`).
- Enforce `panic = "abort"` for production binaries.
- Add “layout assertion” macros (see M2).
- Add “no-alloc in hot path” lints (see M5).

### Required tests
- CI compile-only test that asserts:
  - all IPC structs are `#[repr(C)]`
  - sizes/alignments match spec (compile-time)
- “Link-time sanity” test: build `release` and `dev` profiles.

### Acceptance criteria
- CI green on x86_64.
- Cross-build succeeds for aarch64.

---

## 4) Milestone M1 — Host Determinism + Kernel-Bypass Bringup

### Deliverables
- Documented host tuning profile:
  - CPU isolation, IRQ steering, hugepages, NUMA pinning
  - “Tier‑0 cores” and “Utility cores” partition
- AF_XDP (or chosen bypass) can RX/TX packets in a minimal sample app with stable p99.

### Implementation checklist
- BIOS / firmware tuning:
  - disable deep C-states, disable SMT on money cores, lock CPU governor.
- Linux boot args:
  - `isolcpus`, `nohz_full`, `rcu_nocbs` for money cores.
- Hugepages reserved at boot and mounted.
- NIC queue setup:
  - dedicated RX queues per ingress core, IRQs off money cores.
- UMEM allocation from hugepages pinned to NIC-local NUMA.

### Required tests
1. **Host audit script** (must run on every deployment):
   - Confirms:
     - isolated cores are correct
     - `irqbalance` disabled
     - hugepages available and mounted
     - CPU governor is `performance`
2. **AF_XDP functional test**
   - Sends synthetic UDP packets; validates receive correctness and loss rate.
3. **AF_XDP latency microbench**
   - Measures RX→userspace visibility with TSC timestamps (p50/p99).
   - Must be repeatable and stable.

### Acceptance criteria
- Kernel-bypass RX works with zero packet loss at target PPS for lab load.
- Host audit script passes.

---

## 5) Milestone M2 — Shared Memory Primitives (Rings + RCU + Index Maps)

### Deliverables
- `shadow-ipc` crate provides:
  - SPSC ring implementation (drop-on-full)
  - RCU-lite double-buffer with tear detection (reader never spins)
  - 8-byte tag index maps for pools/mints
  - Shared memory segment bootstrap + versioning header

### Implementation checklist

#### 5.1 Shared memory segment versioning
For every `/dev/shm/shadow_*` file:
- First cache line: `SegmentHeader64B`
  - magic, version, struct_size, build_git_sha prefix, checksum
- A mismatch between writer and reader versions must fail closed (process refuses to start).

#### 5.2 SPSC rings
- Power-of-two capacity
- Producer/consumer cursors isolated (64B or 128B as required)
- Drop-on-full; never block money cores
- Provide ring stats counters (drops, high-watermark) in a separate cache line

#### 5.3 Index maps (8-byte tags)
- Open addressed, bounded probe (e.g., 8 probes)
- `tag = (hash40 << 24) | (index_plus_one_u24)`
- Full pubkey verification performed in arena (never in map)

#### 5.4 RCU-lite double-buffer “Firewall 1”
Implement the reader/writer protocol using:
- `ActiveIdx128B` aligned to 128 bytes
- Slot struct aligned to 64 bytes
- `sequence` as `AtomicU32`
- Payload in `UnsafeCell<T>` and read/write via volatile raw pointers

**Hard requirements**
- Compile-time layout checks:
  - `size_of::<PoolSnapshot>() == 60`
  - `size_of::<PoolHotState64B>() == 64`
  - `align_of::<PoolHotState64B>() == 64`
  - `align_of::<ActiveIdx128B>() == 128`

### Required tests

#### 5.5 Layout tests (compile-time)
Use `static_assertions`:
- `const_assert!(size_of::<PoolHotState64B>() == 64);`
- `const_assert!(align_of::<ActiveIdx128B>() == 128);`
- Repeat for blockhash slot and any manifest slots.

#### 5.6 Concurrency tests (model-checked)
Use `loom` tests for:
- Writer publishes sequence odd → payload write → sequence even → active flip
- Reader never returns a snapshot where `seq1 != seq2`
- Reader returns None when sequence odd
- Active index isolation prevents false sharing (cannot be modeled directly in loom, but verify struct layout)

> Note: TSan will still *report* a race for seqlock-style patterns. That’s expected. The “correctness” is enforced by tear detection + fail-fast semantics.

#### 5.7 Stress tests (runtime)
- Spawn:
  - one writer thread publishing at 5–20k Hz
  - N reader threads continuously reading
- Assert:
  - no panics, no invalid values (e.g., pubkey mismatch)
  - read(None) rate is bounded and reasonable (depends on writer rate)

### Acceptance criteria
- All layout + loom tests pass.
- Stress test runs 60s with no invalid snapshots.

---

## 6) Milestone M3 — `shadow-state` (State Replication + Publishing)

### Deliverables
- `shadow-state` maintains:
  - pool arena + pool index map
  - mint arena + mint index map
  - ALT cache
  - health snapshot
- Fork-aware rollback: publishes reverted pool state via the same RCU flip protocol.

### Implementation checklist
- Implement subscription:
  - Geyser account updates (or RPC polling as bootstrapping only)
  - decode Raydium pool accounts + vault balances
  - populate hot fields needed by workers
- Publish order per update:
  1. write inactive slot, sequence-protected
  2. flip active idx
  3. then publish/update index map tag

### Required tests
1. **Golden decode tests**
   - Given captured account data blobs, assert decoded reserves match expected.
2. **Publish order test**
   - Reader verifies full pubkey from arena; ensure no path returns wrong pubkey.
3. **Fork simulation test**
   - Feed update stream with simulated fork:
     - update pool to state A
     - switch fork, revert to previous state
   - Ensure readers see monotonic health slot changes and correct reserves.

### Acceptance criteria
- `shadow-state` can run for 1 hour on devnet/mainnet read-only without crashes.
- Shared memory state freshness metrics behave (no staleness beyond configured thresholds during normal conditions).

---

## 7) Milestone M4 — Ingress Proxy (AF_XDP Shreds + Scanner + Decode)

### Deliverables
- AF_XDP ingest thread(s):
  - parse Turbine shred headers
  - filter to **data shreds** only
  - slot-scoped fragment window (4 contexts)
  - bounded reorder buffer (cap 16)
  - deadlock escape:
    - drop old shreds (`shred_index <= tail`) before gap logic
    - flush only on *positive* large gaps
- gRPC fallback ingest thread(s):
  - parse protobuf
  - apply signer allowlist prefilter
  - strict decode patterns
- Routing: pool-pubkey sharding to exactly one worker ring.

### Implementation checklist (data shred physics)
- Enforce:
  - **FEC blindness fix:** ignore coding shreds (`is_data_shred` must be true)
  - **late packet underflow fix:** drop old/dup before `gap > 16` logic
- Ensure crossover stitching occurs **only** on contiguous indices within the same slot context.

### Required tests

#### 7.1 Offline shred replay harness (mandatory)
Build `shadow-ingest-replay`:
- Input: PCAP or raw shred dump
- Output: decoded `IngressRecord`s and parse stats

Tests:
1. **Contiguity test**
   - Feed out-of-order shreds; ensure crossover not stitched incorrectly.
2. **Late duplicate test**
   - tail=10, incoming shred=5:
     - must drop (no flush)
3. **Coding shred test**
   - insert coding shreds at realistic ratios (30–50%):
     - must ignore them; scanner must not corrupt buffer
4. **Gap flush test**
   - create missing shred causing gap > 16:
     - must flush only that slot context, not global
5. **Signature extraction + allowlist test**
   - confirm allowlist checks *all signers*, not just fee payer.

#### 7.2 Performance tests (ingress-only)
- Measure:
  - packet RX → record enqueue latency
  - record throughput at burst PPS
  - drop rate under overload (drop-on-full behavior)

### Acceptance criteria
- Ingress replay passes all correctness tests.
- Ingress can sustain target PPS with bounded CPU.

---

## 8) Milestone M5 — Money Worker Core (O(1) Pipeline, Safety Guards, Fee Physics)

### Deliverables
- Worker loop implements V6.3 pipeline:
  - dedupe
  - staleness gate
  - pool/mint lookup
  - effective reserves (shared snapshot + thread-local overlay)
  - forward projection quote
  - tip selection (fraction_u32 LUT)
  - **Absolute fee clamp** (Firewall 2)
  - PUBLIC_GEYSER CU-price conversion in u128
  - risk reservation via atomic fence
  - build template with ShadowGuard **range** parameters
  - sign via nonce ring
  - enqueue to egress ring
  - update overlay

### Implementation checklist

#### 8.1 Whale overflow defense (priority fee conversion)
- Always compute:
  - `cu_price_micro_lamports = ((tip_lamports as u128 * RECIPROCAL_Q32 as u128) >> 32) as u64;`

#### 8.2 Firewall 2 — absolute tip clamp
Before CU-price conversion, clamp `tip_lamports`:

- `ABSOLUTE_MAX_PRIORITY_FEE_LAMPORTS = 100_000_000` (0.1 SOL)  
- Clamp must be silent on the hot path (only async logging).

#### 8.3 ShadowGuard range construction
For copy-trade buy backruns:
- expected reserve_out **decreases** if target executed.
- Range guard:
  - `max_reserve_out` proves execution (must be low enough)
  - `min_reserve_out` protects against sandwich-driven “too depleted” reserve

For sell backruns / arb direction:
- choose range bounds that match the expected reserve motion.
- If unsure, fail closed (drop).

### Required tests

#### 8.4 Deterministic math unit tests
- Constant product projection tests:
  - verify post-target reserves and expected out match reference integer math.
- Boundary conditions:
  - zero reserves, tiny reserves, large reserves (u128 intermediate)
- ShadowGuard bound tests:
  - **target revert** case must fail guard
  - **sandwich extreme depletion** must fail guard via `min_reserve_out`

#### 8.5 Tip + CU conversion tests
1. Whale test (regression for overflow)
   - `tip_lamports = 500_000_000`
   - ensure u128 compute does not overflow and produces correct CU price
2. Clamp test
   - if computed tip > 0.1 SOL, ensure it clamps to 0.1 SOL
3. Rounding tests (bucket rounding)
   - alpha exactly at bucket boundary; confirm shifted rounding behavior

#### 8.6 Worker loop invariants tests
- “No alloc” test:
  - run worker loop in a harness with a custom global allocator that panics on alloc
- “No syscalls” test:
  - run under `strace -f` in CI smoke and assert 0 syscalls from worker thread IDs

#### 8.7 Microbench tests
- Benchmark components:
  - pool lookup p99
  - quote p99
  - template build p99
  - signing p99 (nonce ring pop + scalar ops only)
- Benchmark full worker loop:
  - dequeued record → envelope enqueue
  - p99 ≤ 150 µs on production hardware

### Acceptance criteria
- All unit tests pass.
- Worker p99 meets target on production hardware.
- Under overload, worker drops intents but never blocks.

---

## 9) Milestone M6 — Egress Proxies (QUIC + Jito Bundles + Fast Failure Signals)

### Deliverables
- QUIC egress proxy:
  - pre-warmed sessions
  - “kamikaze” config (no pacing, no throttling)
  - optional relay fanout across distinct identities
- Jito egress proxy:
  - persistent Searcher API session
  - bundle submission
  - BundleState subscription
  - emits immediate terminal events (Rejected/Dropped/NotLanded)

### Implementation checklist

#### 9.1 Jito tip account sharding (V6.3 requirement)
- Workers must choose:
  - `tip_pubkey = JITO_ACTIVE_TIPS[worker_id & 7]`
- Egress must NOT mutate tip pubkey post-sign.

#### 9.2 BundleState fast-release
- Egress proxy must publish:
  - `(signature, terminal_state)` into PM’s reconciliation ring immediately on terminal failures.

### Required tests
1. **Mock QUIC endpoint test**
   - Ensure proxy does not block on reconnect; drops envelope if no session.
2. **Mock Jito endpoint test**
   - Ensure bundle state events are consumed and forwarded within <10ms.
3. **Tip pubkey immutability test**
   - Ensure tx bytes sent match signed bytes; no “patching.”

### Acceptance criteria
- Under induced QUIC/Jito failures, system fails fast (drops) and does not stall money cores.
- Bundle terminal events reach PM within 10ms in local test.

---

## 10) Milestone M7 — `shadow-pm` (Inventory, Revert Tracking, Delta Reconciliation)

### Deliverables
- Pending table keyed by signature.
- Terminal status ingestion from:
  - RPC/Geyser confirmations
  - **Jito BundleState** terminal stream (fast path)
- Inventory fence logic:
  - workers reserve via `fetch_add` + rollback
  - PM releases on terminal failure/success/expiry via `fetch_sub`
  - PM delta reconciles drift without blind stores

### Required tests
1. **Reservation leak test**
   - Simulate 100 intents, 90 fail; ensure fence returns to near baseline.
2. **Expiry test**
   - No chain status after max age → must release.
3. **Jito fast-release test**
   - Bundle rejected off-chain → PM releases immediately.

### Acceptance criteria
- No “phantom inventory” under synthetic burst tests.

---

## 11) Milestone M8 — `shadow-oracle` (Tip LUT Publisher)

### Deliverables
- Tip LUT in shared memory:
  - fraction_u32 entries
  - shifted rounding bucket indexing
- Update loop publishes atomically.

### Required tests
1. LUT layout tests (size, alignment)
2. Bucket math tests (rounding correctness)
3. End-to-end “oracle publish → worker consume” test in integration harness

### Acceptance criteria
- Worker always reads a valid LUT snapshot; on LUT absence, worker fails closed.

---

## 12) Milestone M9 — On-Chain `ShadowGuard` (Dual-Bound Range Guard)

### Deliverables
- Program instruction: `AssertReserveOutInRange { min_reserve_out, max_reserve_out }`
- Reads vault token account amount; aborts before swap if outside range.

### Required tests
1. Program unit tests (local)
   - min/max inclusive
   - extreme values (0, u64::MAX)
2. Local validator integration tests
   - Construct swap tx with guard first; verify:
     - guard failure aborts before swap
     - guard success allows swap
3. Compute budget test
   - ensure guard costs expected compute and remains minimal

### Acceptance criteria
- Guard behavior matches spec and never performs CPI or routing.

---

## 13) Milestone M10 — End-to-End Performance Validation

### Deliverables
- End-to-end latency instrumentation:
  - NIC RX timestamp (ingress)
  - record enqueue timestamp
  - worker dequeue/enqueue timestamps
  - egress TX timestamp
- Dashboards:
  - worker p99 ≤ 150 µs
  - e2e p99 ≤ 500 µs

### Required tests
- Full pipeline replay test:
  - replay shreds (offline) → worker → egress mock
- Live “shadow trading” mode:
  - decode + sign but do not send (or send to sink)
- Perf regression gate:
  - compare current p99 to last known-good baseline

### Acceptance criteria
- Meets p99 budgets on production hardware with representative load.

---

## 14) Milestone M11 — Control Plane V1.0 (`shadow-control`) Auto-Heal

> Control plane must be a standalone binary. It must not require code changes to money-worker logic.  
> It is allowed to interact with shared memory, ingress proxy toggles, and new control-plane segments.

### 14.1 Control Plane responsibilities
- Detect Tier‑0 blindness (fast path fails, slow path succeeds).
- Capture raw UMEM history without memcpy.
- Derive new parsing offsets using SIMD search.
- Hot-swap parsing manifest via the same RCU double-buffer protocol.
- Quarantine venues on program upgrade (executable code hash change).

### 14.2 Control Plane modules and build steps

#### Module 1 — ParsingManifest64B (Bridge)
**Deliverables**
- `/dev/shm/shadow_parsing_manifest`:
  - `active_idx` + 2 slots with sequence tear detection
  - `ParsingManifest64B` payload:
    - base offsets: tx header, account keys start, instruction data start
    - any other fixed offsets needed by scanner

**Data plane hook (ingress proxy only)**
- At slot boundary (or every N shreds), ingress proxy reads manifest into stack locals and uses those offsets for scanning.
- Hot path impact: ~once per slot; must be amortized.

**Tests**
- Manifest layout tests (size/alignment)
- Ingress “manifest switch” integration test:
  - start with wrong offsets → expect misses
  - inject corrected offsets → expect hits without restart

#### Module 2 — Flight Data Recorder (UMEM history)
**Deliverables**
- Ingress proxy maintains delayed-release window of UMEM frames:
  - ring: `/dev/shm/shadow_raw_archive`
  - entries: `(slot, shred_index, umem_offset_bytes, len, rx_unix_ns)`

- **IPC pointer rule (mandatory):** never write virtual pointers across process boundaries.
  - Back UMEM by a shared hugepage file and `mmap(MAP_SHARED)` it into both `shadow-tier0` and `shadow-control`.
  - Data Plane publishes `umem_offset_bytes = (umem_ptr - umem_base_addr)` into the ring.
  - Control Plane reconstructs: `local_ptr = control_umem_base_addr + umem_offset_bytes`.
- **Backpressure rule (mandatory):** `shadow_raw_archive` MUST be drop-on-full (or overwrite-oldest).
  - AF_XDP hot path attempts one enqueue; if full, drop the history record and continue immediately.
- Frames recycled only when they fall out of rolling window.

**Tests**
- “No memcpy” assertion:
  - perf counters show no extra memcpy bandwidth
- History ring correctness:
  - offsets round-trip: control plane maps shared UMEM and can dereference `base + offset` without SIGSEGV
  - UMEM frames remain readable until they fall out of the rolling window
- Drop-on-full behavior:
  - kill/pause `shadow-control` and confirm AF_XDP ingestion continues (no stall); archive records are dropped/overwritten

#### Module 3 — Tripwire Watchdog
**Deliverables**
- `shadow-watchdog` monitors:
  - AF_XDP execution records
  - gRPC execution records
- Detects: gRPC sees a valid target signature but AF_XDP does not within 10ms.
- Trigger: Miss_Count > 3 within 5 seconds → BLINDNESS asserted.
- Action:
  - instruct ingress proxy to stop producing AF_XDP records (or mark invalid)
  - system degrades to gRPC-only rings (Tier‑1 latency) while healing
- **Control flag polling rule:** if Tripwire asserts BLINDNESS and flips `tier0_ingress_mode=GRPC_ONLY`, the AF_XDP proxy MUST read this flag **once per RX batch** (32/64 frames), not per shred.


**Tests**
- Synthetic divergence test:
  - drop AF_XDP outputs; ensure watchdog asserts blindness
  - restore; ensure de-asserts

#### Module 4 — Rosetta Stone Calibrator (SIMD decryption)
**Deliverables**
- `shadow-calibrator`:
  - consumes Golden Records from gRPC
  - scans UMEM payload window for:
    - 64B signature
    - 32B pool pubkey
    - 8B amount_in
  - derives new offsets and validates across multiple Golden Records

**Tests**
- Protocol-shift simulation:
  - mutate offsets in captured shreds; ensure calibrator re-discovers offsets
- False-positive defense:
  - require validation on >= 3 distinct records before publishing

#### Module 5 — Hot-Swap Injection
**Deliverables**
- Writes new ParsingManifest64B to inactive slot, flips active_idx.
- Ingress proxy reads new manifest next slot and resumes Tier‑0 hits.

**Tests**
- Hot-swap correctness test under load (concurrent reads)
- Recovery SLO:
  - BLINDNESS → healed and Tier‑0 restored within ~3 seconds in lab conditions

#### Module 6 — Application-Layer Quarantine (Cryptographic killswitch)
**Deliverables**
- `shadow-control` monitors executable account data hash for supported DEX program IDs.
- On change:
  - writes `venue_enabled=false` into a shared risk snapshot used by workers (read-only on hot path)
  - pages on-call (out of scope for code; but must emit event)

**Tests**
- Local validator test:
  - swap program ID to a new deploy; hash changes; ensure venue disables immediately
- Worker gate test:
  - with venue disabled, worker drops venue flow O(1)

### 14.3 Acceptance criteria
- Auto-heal can correct parsing offsets without restarting Tier‑0 process.
- Degradation to gRPC-only keeps system trading (Tier‑1) during heal.
- Venue quarantine triggers on program hash change.

---

## 15) Milestone M12 — Production Readiness Gates + Rollout

### Deliverables
- Release artifacts:
  - `shadow-tier0`, `shadow-state`, `shadow-pm`, `shadow-oracle`, `shadow-control`
  - systemd units, CPU pinning config, hugepage config
  - “Host audit” script
  - “Smoke test” script

### Required tests / gates
- **Correctness gates**
  - All unit/integration tests pass.
  - Offline replay harness passes against latest captured traffic.
- **Performance gates**
  - Worker p99 ≤ 150 µs on production hardware.
  - End-to-end p99 ≤ 500 µs on production hardware.
- **Security gates**
  - Core dumps disabled (verified)
  - secret mappings `mlock` + `MADV_DONTDUMP` verified
  - no secrets in `/dev/shm`
- **Operational gates**
  - Restart behavior safe with `panic=abort`
  - Metrics and alerts firing as expected
  - Revert-cost breaker wired to paging

### Rollout steps (recommended)
1. Deploy in “observe-only” mode (decode + quote + sign, but do not send).
2. Enable send at minimal size caps.
3. Ramp size caps gradually.
4. Enable Jito bundling after confirming:
   - tip account sharding works
   - BundleState fast-release works (no phantom inventory)
5. Enable Control Plane auto-heal after confirming:
   - BLINDNESS trigger and recovery in staging

---

## Appendix A — Mandatory Compile-Time Layout Assertions (Template)

Use a dedicated `layout_tests.rs` module in each crate:

```rust
use static_assertions::const_assert;
use core::mem::{size_of, align_of};

const_assert!(size_of::<PoolSnapshot>() == 60);
const_assert!(size_of::<PoolHotState64B>() == 64);
const_assert!(align_of::<PoolHotState64B>() == 64);
const_assert!(align_of::<ActiveIdx128B>() == 128);
```

---

## Appendix B — “No Alloc” Hot Path Test Pattern

- Provide a feature `deny_alloc` that replaces the global allocator with one that panics.
- In CI, run a worker loop harness under this feature and assert no panics.

---

## Appendix C — Offline Capture Formats

- Shred replay inputs supported:
  - PCAP (preferred)
  - raw UDP payload dumps
  - UMEM pointer dumps (control plane archive)

Define a stable “replay record” header format so future regressions can be tested without re-capturing.

---

## Appendix D — Known Failure Modes and Required Alarms

1. **Revert bleed**
   - Alarm: consecutive reverts > 3 for a pool (PUBLIC lane)
2. **Phantom inventory**
   - Alarm: fence utilization pinned at limit with low landed rate
3. **AF_XDP blindness**
   - Alarm: watchdog BLINDNESS asserted
4. **Kernel bypass drops**
   - Alarm: RX/TX ring drops non-zero sustained
5. **Venue program upgrade**
   - Alarm: executable hash change → venue quarantined
