# PROJECT SHADOW ENGINE V6 — Profit‑First Tier‑0 Sniper System Specification (Unified)

**Status:** Locked engineering implementation specification (single authoritative document)  
**Supersedes:** V5, V5.1, and all prior drafts/addenda  
**Target ecosystem:** Solana mainnet  
**Primary venues:** Raydium V4 (mandatory), Pump/PumpSwap (optional via separate adapter)  
**Execution model:** pre‑consensus ingestion → O(1) intent reconstruction → cache‑only quote → template build → sign → signal‑source‑aware multi‑path send  
**Primary objective:** maximize **conservative absolute Net EV per microsecond** under a hard end‑to‑end latency budget

---

## 0. Conventions

### 0.1 Normative keywords
- **MUST / MUST NOT**: production requirements.
- **SHOULD / SHOULD NOT**: strong recommendations; deviations require explicit measurement justification.
- **MAY**: optional.

### 0.2 Type conventions
- `Pubkey`: 32 bytes
- `Signature`: 64 bytes
- `Slot`: `u64`
- `Lamports`: `u64`
- `UnixNs`: `u64`
- `Bps`: `u16` in `[0, 10_000]`

### 0.3 Hot path definition
**Hot path** is the end‑to‑end critical loop from *feed packet arrival* to *signed transaction bytes ready for send*.  
V6 defines exactly one production lane: **Tier‑0 Sniper Lane**.

### 0.4 Latency budget
Tier‑0 Sniper Lane **MUST** satisfy:

- **p99 end‑to‑end ≤ 150 µs** from NIC packet DMA arrival to signed bytes DMA'd back to the NIC.
- **No blocking waits** in the worker loop.
- **No hot‑path network I/O** (RPC/HTTP) for decisions.
- **No kernel syscalls** on the critical path — neither ingress nor egress. Standard Linux POSIX sockets (`sendto`, `recvfrom`) traverse the VFS, UDP stack, and qdisc queues, consuming 30–60 µs per direction. In a 150 µs budget this is mathematically fatal. Kernel bypass is mandatory (Section 4.4).

If the system cannot meet this budget, it is not considered "production‑ready" regardless of architecture elegance.

---

## 1. Mission, scope, and explicit non‑goals

### 1.1 Mission
Project Shadow Engine is a Solana MEV execution system designed to win a first‑price, low‑latency auction (Jito and equivalent delivery paths). The system exists to extract maximum expected profit:

- **maximize**: `NetEV_LCB = Alpha_LCB − AuctionCost_P90`
- **subject to**: strict risk caps, strict latency constraints, and deterministic hot‑path behavior.

### 1.2 Scope (what V6 does)
V6:
1. Ingests a pre‑consensus transaction feed (public Geyser and/or private mempool sources).
2. Identifies *eligible target* swaps from a curated set of entities/wallets.
3. Reconstructs swap intent deterministically in **O(1)** time for supported venues.
4. Quotes a backrun (or protective) trade from cached pool state.
5. Builds and signs a **template transaction** with fixed compute budget.
6. Sends via **signal‑source‑aware multi‑path routing**: Jito bundles only when the target is held in a private pipeline (Jito Searcher API / private mempool); direct staked QUIC/TPU blast with priority fee when the target was observed on a public feed (Section 8.3).

### 1.3 Explicit non‑goals (removed from V6)
V6 explicitly removes the following because they add steady‑state latency, complexity, or compute tax without clear profit dominance in Tier‑0:

- Dynamic schema‑driven protocol decoders and hot reload
- Canary dual‑parsing pipelines
- Embedded SVM tracing / net‑intent extraction
- On‑chain MEV Router assertions on the entry hot path
- ML inference (ONNX / dual encoder) anywhere that gates first‑trade eligibility
- “Always landed” optimization as a primary objective

If a feature increases steady‑state Tier‑0 latency or compute without measured NetEV gain, it is out of scope.

---

## 2. System topology (single‑lane)

### 2.1 Processes (fault domains)
V6 uses four processes. Only one is latency‑critical.

1. **shadow-state** (state daemon; not in hot path)
   - Fork‑aware replication of required on‑chain accounts (pools, mints, vaults, ALTs if subscribed).
   - Writes pool state directly into seqlock‑protected shared‑memory segments (Section 5.4). Tier‑0 workers read state via O(1) wait‑free seqlock reads — no diff polling, no local map maintenance.
   - Publishes health + snapshot metadata.

2. **shadow-tier0** (monolithic hot path; latency‑critical)
   - Single binary containing N **run‑to‑completion worker threads**, each pinned to an isolated core.
   - Each worker executes the full pipeline on a single core: feed ingest → O(1) decode → gate → quote → template build → sign → send.
   - **No cross‑process or cross‑thread handoff** between decode and send. The transaction data stays in L1/L2 from packet arrival to NIC blast, eliminating the L3/main‑memory cache miss penalty of a process boundary.
   - Each worker thread holds a **read‑only copy of the hot wallet Ed25519 private key** in thread‑local memory. Ed25519 signing (~12–15 µs) occurs on the same core that built the transaction.
   - **M/D/1 queueing suicide eliminated:** in the prior two‑process split (N entry workers → 1 signer), a burst of N simultaneous intents serialized behind a single signing core, adding up to `(N−1) × 15 µs` of queueing delay. With run‑to‑completion, each worker signs independently; worker 6 does not wait for workers 1–5.
   - Each worker owns its own non‑blocking send socket(s) for relay fanout.

3. **shadow-pm** (position manager; not in hot path)
   - Inventory accounting.
   - Autonomous exits (including panic exits).
   - Writes risk caps / position limits to shared memory for hot path.

4. **shadow-oracle** (nearline analytics; not in hot path)
   - Maintains per‑entity performance stats, `Alpha_LCB`, `p_win` calibration inputs.
   - Maintains deterministic genesis fingerprint registry (burner attribution).
   - Writes "target allowlist + per‑entity caps" to shared memory.

**Key material distribution rule:**
- The hot wallet private key is loaded once at process start and cloned into each worker's thread‑local storage.
- The key MUST NOT be written to shared memory, IPC rings, or any persistent file beyond the encrypted keyfile loaded at boot.
- `shadow-pm` uses the same key for exit signing; it is a separate process for fault isolation (PM bugs must not crash Tier‑0) but shares the same keyfile.
- Nonce / recent‑blockhash coordination: each worker reads the rolling blockhash from a shared‑memory segment published by a background thread within `shadow-tier0`. No lock; atomic load of a cache‑line‑aligned struct.

**Cross‑worker synchronization (two atomic fences, no locks):**
- **Exec fence** (`/dev/shm/shadow_exec_fence`): because multiple workers may observe the same target signature, a lightweight **per‑mint/per‑slot execution fence** (compare‑and‑swap on `(mint, slot)`) prevents double‑sends. Cost: single atomic CAS (~5 ns).
- **Inventory fence** (`/dev/shm/shadow_inventory_fence`): per‑mint atomic `i64` tracking committed position in lamports. Workers execute `fetch_add` before signing; if the post‑add value exceeds the risk limit, they `fetch_sub` and drop. Prevents N‑worker burst from exceeding position limits (Section 6.7). Cost: ~5–10 ns.
- These are the only cross‑worker synchronization points on the hot path. Both are lock‑free atomic operations.

### 2.2 CPU, NUMA, and NIC requirements
Tier‑0 **MUST** run on dedicated isolated cores:

- `shadow-tier0`: N worker cores pinned (1 core per run‑to‑completion worker; each core handles ingest + decode + quote + sign + send) + 1 background core for blockhash refresh / health polling / async log drain
- `shadow-state`, `shadow-pm`, `shadow-oracle`: separate cores (not shared with Tier‑0 workers)

All Tier‑0 worker cores MUST be on the same NUMA node as the NIC and the shared‑memory segments they read. Tier‑0 MUST NOT share cores with `shadow-state` account processing, log encoding, or garbage collection tasks.

**NIC and memory requirements (kernel bypass):**
- NIC MUST support RSS (Receive Side Scaling) or hardware flow steering to distribute feed packets across per‑worker RX queues.
- NIC queues used by Tier‑0 workers MUST be IRQ‑free (poll mode); IRQ coalescing disabled on these queues.
- Packet buffers (UMEM for AF_XDP, mempools for DPDK) MUST be allocated from **2MB or 1GB hugepages** pinned to the Tier‑0 NUMA node.
- Hugepages MUST be reserved at boot via `hugepages=` kernel parameter or `hugetlbfs` mount. Runtime allocation of hugepages is forbidden (fragmentation risk).

---

## 3. Hard invariants (profit‑first)

### 3.1 Determinism + bounded work
Tier‑0 MUST be bounded and deterministic:
- O(1) decode for supported venues.
- O(1) quote from cached state.
- O(1) tx build from templates.

Anything not decodable in O(1) is dropped.

### 3.2 No hot‑path network I/O; no kernel networking
Tier‑0 MUST NOT make RPC/HTTP calls. This includes:
- ALT fetch
- mint/pool fetch
- fee telemetry fetch
- leader schedule fetch

All such data MUST be maintained by background threads/processes and presented via in‑memory caches or shared memory snapshots.

Tier‑0 MUST NOT use kernel POSIX sockets (`sendto`, `recvfrom`, `sendmsg`, `recvmsg`) for either ingress or egress on the hot path. All NIC I/O MUST use kernel‑bypass mechanisms (Section 4.4).

### 3.3 “Fail‑closed” only for correctness‑critical cache misses
V6 uses fail‑closed only where acting is provably dangerous:
- Required pool state missing/stale → drop
- Required mint metadata missing → drop
- Required ALT missing → drop
- Risk snapshot missing → drop

Optional enrichments MUST NOT block decisions.

### 3.4 Accept reverts; stop bleeding
Reverts are normal in a first‑price auction.
- Tier‑0 MAY revert.
- A revert‑cost breaker exists only to prevent *priority fee / tip bleed* under systemic mismatch.

### 3.5 No compute tax features in the money path
Tier‑0 MUST NOT introduce structural compute overhead without measured NetEV gain. Therefore Tier‑0 MUST NOT:
- route through on‑chain assertion routers,
- add validation CPIs,
- run embedded simulators on the critical loop,
- invoke kernel syscalls for networking (Section 3.2) or logging.

---

## 4. Shared memory and IPC (zero‑copy)

### 4.1 Shared memory segments (all binary; no JSON)
All IPC uses fixed binary layouts in `/dev/shm`. No Python writes binary structs.

Required segments:
- `/dev/shm/shadow_feed_ring_*` (SPSC rings; one per Tier‑0 worker)
- `/dev/shm/shadow_pool_state` (seqlock‑protected; pre‑allocated contiguous array of pool state structs; written by `shadow-state`, read O(1) by workers — Section 5.4)
- `/dev/shm/shadow_state_mints` (read‑only snapshot)
- `/dev/shm/shadow_state_alts` (read‑only snapshot)
- `/dev/shm/shadow_risk_snapshot` (read‑only snapshot)
- `/dev/shm/shadow_oracle_targets` (read‑only snapshot)
- `/dev/shm/shadow_tip_lut` (read‑only; pre‑computed optimal tip LUT published by oracle — Section 6.6)
- `/dev/shm/shadow_health` (read‑only snapshot)
- `/dev/shm/shadow_blockhash` (read‑only; cache‑line‑aligned rolling blockhash updated by `shadow-tier0` background thread)
- `/dev/shm/shadow_exec_fence` (atomic CAS fence for per‑mint/per‑slot duplicate prevention across workers)
- `/dev/shm/shadow_inventory_fence` (atomic `i64` array; per‑mint committed position in lamports; `fetch_add`/`fetch_sub` by workers, reconciled by `shadow-pm` — Section 6.7)
- `/dev/shm/shadow_tier0_stats` (read‑only snapshot; per‑worker send/sign/land counters)

Note: the prior per‑worker diff rings (`shadow_state_pool_diffs_*`) are eliminated. `shadow-state` writes pool state directly into a seqlock‑protected shared‑memory segment; Tier‑0 workers perform wait‑free O(1) reads instead of polling and applying diffs (Section 5.4).

### 4.2 Ring buffer requirements
All SPSC rings (feed rings only; pool state uses seqlocks) MUST be:
- SPSC
- preallocated
- power‑of‑two capacity
- single cache‑line producer/consumer cursors
- drop‑on‑full (never block Tier‑0)

### 4.2.1 Seqlock requirements (pool state and other writer‑side-updated segments)
Seqlock‑protected segments MUST satisfy:
- **Single writer** (`shadow-state`), **multiple readers** (Tier‑0 workers). No reader‑side writes.
- Writer increments an atomic `seq` counter to an **odd** value before writing, then increments to **even** after writing completes. Both increments use `Release` ordering.
- Reader loads `seq` with `Acquire` ordering, copies the data, re‑loads `seq`. If `seq` changed or was odd during the read, the reader retries (spin). This is wait‑free under normal conditions (writer completes in nanoseconds; reader retry probability is negligible).
- Each pool state entry MUST be **cache‑line‑aligned** (64 bytes or padded to 64‑byte boundary) to prevent false sharing between adjacent pool entries.
- The reader MUST NOT modify any byte in the seqlock segment. Reads are purely load‑only.
- No mutex, no futex, no syscall. Seqlock is entirely user‑space atomic operations.

### 4.3 Tier‑0 "no allocation / no lock / no syscall" contract
Inside the run‑to‑completion worker loop (ingest through send):
- **MUST NOT allocate** (no heap allocs)
- **MUST NOT lock** (no mutex; the only synchronization primitives are: atomic CAS on exec fence, atomic `fetch_add`/`fetch_sub` on inventory fence, and seqlock atomic loads for pool state reads — all are wait‑free / lock‑free)
- **MUST NOT syscall** — no `sendto`, `recvfrom`, `syslog`, `write`, `futex`, or any other kernel trap on the critical path. All NIC I/O MUST use kernel‑bypass (Section 4.4).
- **MUST NOT poll or drain diff queues** — pool state is read directly via seqlock (Section 5.4)
- Logging MUST be async, sampled, and off the hot path

### 4.4 Kernel bypass networking (mandatory for Tier‑0)

**Problem with POSIX sockets:**
A Linux `sendto()` syscall traverses the VFS layer, UDP stack, qdisc queues, and triggers context switches / IRQ handling. This costs 30–60 µs per direction (ingress + egress). In a 150 µs total budget, kernel networking consumes 40–80% of the budget before any application logic runs.

**Mandatory kernel bypass:**
Tier‑0 MUST use one of the following kernel‑bypass mechanisms for both ingress (feed packet reception) and egress (transaction send):

- **AF_XDP** (recommended for portability): user‑space XDP socket with UMEM ring buffers. Packets DMA directly from the NIC into pre‑allocated user‑space memory via the XDP fast path. Egress writes raw frames into the TX ring for NIC DMA pickup. Requires `CAP_NET_ADMIN` or `CAP_BPF`.
- **DPDK**: full kernel bypass via UIO/VFIO driver binding. NIC is detached from the kernel entirely. Highest performance but requires dedicated NIC and driver support.
- **Solarflare/Xilinx `ef_vi`**: vendor‑specific zero‑copy API for Solarflare NICs. Sub‑microsecond ingress/egress. Requires Solarflare hardware.

**Requirements (regardless of mechanism):**
- Packets MUST DMA directly from NIC hardware rings into pre‑allocated, hugepage‑backed user‑space memory.
- Egress MUST write raw Ethernet/UDP frames directly back to NIC hardware TX rings. No kernel UDP stack involvement.
- Each Tier‑0 worker thread MUST own its own RX/TX ring pair (or queue) to avoid cross‑thread contention. NIC RSS (Receive Side Scaling) or flow steering must distribute feed traffic across worker RX queues.
- Packet buffers MUST be pre‑allocated at boot (UMEM for AF_XDP, mempool for DPDK). Zero runtime allocation.
- Ingress must be poll‑mode (busy‑wait on RX ring descriptor), not interrupt‑driven. IRQ coalescing is forbidden on Tier‑0 NIC queues.

**Fallback (development / test only):**
- Standard POSIX sockets (`sendto`/`recvfrom`) MAY be used in development/test environments where kernel bypass hardware is not available.
- The config key `system.kernel_bypass` (default `true` in production) controls this. If `false`, the system MUST log `KERNEL_BYPASS_DISABLED` at boot as a production‑readiness warning.
- Latency benchmarks with `kernel_bypass=false` are not valid for production qualification.

---

## 5. State daemon (`shadow-state`)

### 5.1 Responsibility
`shadow-state` maintains fork‑aware, near‑real‑time views of:
- Raydium pool accounts required for quoting
- vault token account balances (or the data needed to derive reserves)
- mint metadata (decimals, supply, program)
- a subscribed set of Address Lookup Tables (ALTs)

### 5.2 Subscription scope (profit‑first)
State scope MUST be curated to what Tier‑0 needs:
- Required pools and their vaults for supported venues
- Mints that appear in those pools
- ALTs needed to decode those venue transactions for observed targets

Everything else is out of scope.

### 5.3 Fork handling
`shadow-state` MUST:
- track slot + parent
- rollback on fork switch: revert affected pool state entries in `/dev/shm/shadow_pool_state` using seqlock write protocol (Section 4.2.1); increment each affected entry's `seq` through odd→even cycle
- publish a monotonic `snapshot_slot` in `/dev/shm/shadow_health` so workers can detect discontinuities and staleness

### 5.4 Publishing model (zero‑copy seqlocks; no per‑worker diffs)

**Problem with per‑worker diff streams (eliminated):**
The prior model required `shadow-state` to enqueue pool diffs into per‑worker SPSC rings. Each Tier‑0 worker had to drain and apply pending diffs to a thread‑local map before quoting — an O(N) chore on the hot thread where N is the number of pending diffs. During peak volatility (when EV is thickest), `shadow-state` floods the rings; a worker receiving a target trade with 150 pending diffs must drain them all before it can quote, spiking latency from 150 µs to milliseconds. The branch‑heavy map update also evicts decode instructions from the CPU instruction cache, introducing non‑deterministic jitter.

**Seqlock model (mandatory):**
`shadow-state` maintains a single contiguous, pre‑allocated shared‑memory segment (`/dev/shm/shadow_pool_state`) containing a fixed‑size array of pool state structs. Each entry is cache‑line‑aligned and protected by its own seqlock (Section 4.2.1).

**Writer side (`shadow-state`):**
- On receiving an `AccountUpdate` from the Geyser feed, `shadow-state` locates the pool entry by index (hash map lookup, off hot path) and updates it in place using the seqlock write protocol:
  1. Atomic store `seq` to `seq + 1` (odd; signals "write in progress") with `Release` ordering.
  2. Write new pool state fields (reserves, fee rate, last_update_slot, last_update_unix_ns).
  3. Atomic store `seq` to `seq + 1` (even; signals "write complete") with `Release` ordering.
- Write duration is bounded (tens of nanoseconds for a fixed‑size struct copy). The probability of a reader hitting an in‑progress write is negligible.

**Reader side (Tier‑0 worker, O(1), wait‑free):**
- Worker looks up the pool's slot index (precomputed at config load; O(1) array index, not a runtime hash map lookup).
- Reads `seq` (Acquire), copies the fixed‑size pool struct, re‑reads `seq` (Acquire).
- If `seq` changed or was odd, retry (spin). Under normal conditions this completes on the first attempt.
- The worker **never writes to the segment** and **never maintains a local map**. Zero state maintenance on the hot path.
- Staleness check: `now − entry.last_update_unix_ns > max_pool_staleness_ns` → drop trade (fail closed).

**Why not pointer‑swap giant snapshot?**
A single global pointer swap forces every reader core to invalidate its entire L1/L2 cache view of the pool array when the pointer changes. Seqlocks update individual cache lines; only the modified pool entry's cache line is invalidated on reader cores, preserving the rest of the working set.

**Capacity:**
- The segment is pre‑allocated at boot for `max_pools` entries (configurable; default 4096). Each entry is padded to 64 bytes (one cache line). Total memory: `max_pools × 64 bytes` (default 256 KB — fits comfortably in L2).
- Pool‑to‑index mapping is static after config load. Adding a new pool requires a config change and process restart (acceptable for a curated Tier‑0 system).

### 5.5 Staleness and health
`shadow-state` publishes `/dev/shm/shadow_health`:
- `state_is_healthy: bool`
- `last_pool_update_unix_ns`
- `last_mint_update_unix_ns`
- `last_alt_update_unix_ns`
- `snapshot_slot: u64` (monotonically increasing; for fork discontinuity detection)

Tier‑0 workers MUST drop trades if:
- `state_is_healthy=false`, or
- `now − last_pool_update_unix_ns > max_pool_staleness_ns`, or
- required mint/alt snapshots are stale beyond thresholds.

Per‑pool staleness is also checked inline via each seqlock entry's `last_update_unix_ns` (Section 5.4, reader side).

---

## 6. Tier‑0 worker pipeline (`shadow-tier0` per‑worker loop)

### 6.1 High‑level loop (run‑to‑completion)
Each Tier‑0 worker thread executes the full pipeline on a single pinned core. No cross‑thread or cross‑process handoff occurs between steps.

For each feed transaction:
1. Tag `signal_source` based on which feed delivered the transaction (see below).
2. Fast reject if not from a tracked target entity.
3. Resolve full account list (including ALTs) from ALT cache.
4. Strictly decode supported swap patterns (O(1)).
5. Read pool state via O(1) seqlock read from `/dev/shm/shadow_pool_state` (Section 5.4); verify seqlock consistency and per‑pool staleness. No diff draining, no local map.
6. Compute quote + min_out + expected alpha.
7. Look up tip via O(1) index into pre‑computed tip LUT: `tip = tip_lut[alpha_lcb >> BUCKET_SHIFT]` (Section 6.6). No math, no loops.
8. Atomic `fetch_add` on `/dev/shm/shadow_inventory_fence[mint_index]`; if post‑add exceeds `max_position_lamports_per_mint`, `fetch_sub` and drop (Section 6.7). Cost: ~5–10 ns.
9. Atomic CAS on per‑mint/per‑slot exec fence (Section 2.1); if another worker already claimed this `(mint, slot)`, `fetch_sub` inventory and drop.
10. Build transaction from template, sign with thread‑local Ed25519 key (~12–15 µs).
11. Write raw frame to worker‑owned kernel‑bypass TX ring (Section 4.4); NIC DMA blasts to relay endpoints. Route selected per `signal_source` (Section 8.3). No syscall.

No step may block.

**Feed sources and `signal_source` tagging (mandatory):**
Solana does not have a global public mempool. The timing properties of the target transaction — and therefore the viable submission route — depend entirely on how it was observed. Every ingested transaction MUST be tagged with a `signal_source` at the point of ingestion. This tag is immutable and flows through the entire pipeline into `ExecutionIntent` and event logs.

- `PUBLIC_GEYSER`: transaction observed on a public Geyser/Yellowstone gRPC stream, shred gossip, or RPC pubsub. The target is already in flight to the leader's banking thread. Jito bundle wrapping adds 10–30 ms network hop + Block Engine simulation penalty; the natively propagating target will land before the bundle is validated. **Bundles are not viable for this signal class.**
- `PRIVATE_MEMPOOL`: transaction received via a private feed (bloXroute, MEV relay private channel) before public propagation. Timing exclusivity may exist but is not guaranteed.
- `JITO_SEARCHER_API`: transaction received via Jito's Searcher API as orderflow held inside Jito's block‑building pipeline. The target is not yet propagating publicly. **Jito bundles are the only viable path for this signal class.**

If a transaction arrives on multiple feeds, the first arrival determines the tag; subsequent arrivals are deduped by signature.

### 6.2 Supported decode set (strict O(1))
Tier‑0 MUST support only patterns with fixed parsing cost:
- Raydium V4 `swap_base_in` (primary)
- (Optional) Pump/PumpSwap direct swap, only if adapter provides O(1) decode

Unsupported patterns are dropped immediately:
- Jupiter multi‑hop, aggregators, unknown programs
- arbitrary CPI chains

### 6.3 ALT resolution
Tier‑0 requires ALTs to be present in `/dev/shm/shadow_state_alts` (or worker-local cache mirrored from it).
- On ALT miss: drop.
- Tier‑0 MUST NOT fetch ALTs.
- `shadow-state` MAY expand its ALT subscription set asynchronously based on observed traffic, but Tier‑0 never waits.

### 6.4 Quote + slippage
Tier‑0 uses deterministic integer math (no floats):
- Compute expected out for a backrun size derived from risk sizing.
- Set `min_out` using a conservative slippage buffer.
- `min_out` MUST be tight enough to prevent “copy into toxic dump” scenarios, but not so tight that normal pool variance causes universal reverts.

### 6.5 Expected profit model input
Tier‑0 reads from shared memory (all published by `shadow-oracle`):
- `/dev/shm/shadow_oracle_targets`: per‑entity `Alpha_LCB` estimate (conservative), per‑entity caps and blacklist/whitelist signals
- `/dev/shm/shadow_tip_lut`: pre‑computed optimal tip LUT (Section 6.6; O(1) array index replaces all tip math on hot path)

Tier‑0 NEVER calls the oracle directly.

### 6.6 Tip selection (O(1) LUT; no hot‑path math)

**Problems with discrete brute‑force loop (eliminated):**
The prior model iterated 8–16 candidate tip values on the hot path, evaluating `p_win * (alpha - cost)` for each. This is compute tax on the hottest loop. Worse, discretizing a continuous first‑price auction leaves free money: if bins are 3M and 8M lamports and the optimum is 3.1M, you either lose the trade or overpay by 4.9M. Hard‑capping at 35% of alpha guarantees Winner's Curse — you systematically lose every contested, profitable trade to searchers bidding 90%, and the only trades you win at 35% are mispriced or toxic flow nobody wanted.

**Pre‑computed tip look‑up table (mandatory):**
`shadow-oracle` (which sits idle 99% of the time) continuously computes the optimal tip for each `alpha_lcb` bucket off the hot path, solving the continuous first‑derivative of:

`NetEV(tip) = p_win(tip_per_cu) × (alpha_lcb − auction_cost(tip, cu_limit))`

It flattens the result into a 1D pre‑computed LUT in shared memory:

```
/dev/shm/shadow_tip_lut
  tip_lut[bucket] = optimal_tip_lamports   (bucket = alpha_lcb >> BUCKET_SHIFT)
  lut_size: u32                            (number of buckets)
  bucket_shift: u8                         (right-shift to convert alpha_lcb → bucket index)
  regime_id: u64                           (monotonic; incremented when oracle recomputes)
  tip_cap_fraction: f32                    (per-regime dynamic cap; see below)
```

**Hot‑path tip selection (single array index, ~1 ns):**
```
bucket = min(alpha_lcb >> BUCKET_SHIFT, lut_size - 1)
tip = tip_lut[bucket]
```

One memory access. No loops, no multiplications, no floating point.

**Dynamic tip cap (contest‑aware, no hard ceiling):**
- The oracle computes `tip_cap_fraction` per auction regime. For low‑contest entities/mints (few competing searchers), the cap may be 30–40%. For **highly contested, highly profitable trades**, the oracle MUST authorize scaling up to `tip_cap_max_fraction` (configurable; default 0.95 = 95% of EV).
- Profit is absolute: winning a trade at 5% margin on a 100 SOL alpha (= 5 SOL profit) beats losing the trade entirely at a 35% cap.
- The LUT entries already embed the regime‑appropriate cap — the Tier‑0 worker does not evaluate cap logic.

**Oracle LUT recomputation (off hot path):**
- Oracle recomputes the LUT whenever `p_win` curve parameters, fee regime, or entity alpha distributions shift materially.
- Recomputation frequency: configurable (default every 500 ms). The LUT is written atomically (seqlock or double‑buffer swap) so workers never read a partially‑written table.
- Oracle uses landed/reverted feedback from `ExecutionRecord` logs to calibrate the `p_win(tip_per_cu)` curve continuously.

**Invariant:** Tier‑0 MUST NOT perform any tip math beyond the single array index. All optimization logic lives in the oracle.

### 6.7 Risk gating (O(1), atomic inventory fence)

**Problem with read‑only snapshot gating (eliminated):**
The prior model had N workers read a read‑only `/dev/shm/shadow_risk_snapshot` to check position limits. If the max position for a mint is 500 SOL and 5 workers spot the same opportunity on the same network packet, all 5 read "Position: 0, Limit: 500", all 5 pass the gate, all 5 fire. Result: 2500 SOL position, 5× limit breach, panic exit, strategy bankruptcy.

**Atomic inventory fence (mandatory):**
`/dev/shm/shadow_inventory_fence` maps a cache‑line‑aligned array of atomic `i64` values, one per mint slot (indexed identically to the pool state array in Section 5.4). Each value represents **current committed position in lamports** for that mint.

**Risk check protocol (per worker, O(1)):**
1. Read risk limits from `/dev/shm/shadow_risk_snapshot` (read‑only; still published by `shadow-pm`):
   - `max_position_lamports_per_mint`
   - `max_notional_lamports_per_entity`
   - `global_kill_switch: bool`
   - `allowed_venues`
2. If `global_kill_switch == true` → drop immediately.
3. Compute `trade_size_lamports` for this intent.
4. Execute **atomic `fetch_add(trade_size_lamports)`** on `shadow_inventory_fence[mint_index]`.
5. If the returned value (post‑add) exceeds `max_position_lamports_per_mint`:
   - Execute **atomic `fetch_sub(trade_size_lamports)`** to roll back.
   - Drop the intent immediately. Log `RISK_LIMIT_EXCEEDED`.
6. If the post‑add value is within limits, proceed to sign and send.

**Rollback on send failure or revert:**
- If the worker drops the intent after the `fetch_add` for any reason (template build failure, deadline exceeded, etc.), it MUST `fetch_sub` to release the reservation.
- `shadow-pm` periodically reconciles `shadow_inventory_fence` against confirmed on‑chain position state. Drift (due to reverts, missed rollbacks, or confirmed fills) is corrected by atomically storing the reconciled value. Reconciliation frequency: configurable (default every 500 ms).

**Per‑entity notional cap:**
- Per‑entity notional limits remain in `/dev/shm/shadow_risk_snapshot` (read‑only) and are checked against the entity's cumulative notional from oracle targets. This check is still read‑only because per‑entity exposure changes slowly (oracle‑driven) and a slight overshoot is tolerable — unlike per‑mint inventory, which can spike N× in a single packet burst.

**Cache line alignment:**
- Each atomic `i64` entry in the fence array MUST be padded to a full cache line (64 bytes) to prevent false sharing between adjacent mint slots.

Risk gating MUST be constant‑time. The atomic `fetch_add` + conditional `fetch_sub` costs ~5–10 ns total.

### 6.8 Internal execution record (per‑worker, no IPC)
After signing and sending, each worker writes an `ExecutionRecord` to a thread‑local async log buffer (drained by the background thread). This record is used for observability, PM fill tracking, and oracle feedback — it is NOT an IPC message.

`ExecutionRecord` includes:
- `signal_source`: enum { `PUBLIC_GEYSER`, `PRIVATE_MEMPOOL`, `JITO_SEARCHER_API` }
- `route_taken`: enum { `STAKED_BLAST`, `JITO_BUNDLE` }
- raw target tx signature (for fill correlation)
- venue id + template id
- pool id and required accounts
- our trade parameters (amount_in, min_out)
- compute budget template selection
- chosen tip / priority fee
- `send_unix_ns` (timestamp of NIC blast)
- worker id

---

## 7. Quoting and templates

### 7.1 Templates are mandatory
Tier‑0 MUST build transactions from a small set of audited templates:
- fixed account metas ordering
- fixed instruction ordering
- fixed compute budget instructions (limit and price)
- fixed tip mechanism (bundle tip transfer)

Templates MUST be versioned. Hot path chooses a template id, not a dynamic AST build.

### 7.2 Compute unit limits
Compute unit limit is part of bid density in builders. Therefore:
- templates MUST use the smallest safe CU limit for the instruction set
- calibration occurs off hot path; new template limits are rolled out explicitly

Tier‑0 MUST NOT run per‑intent simulation to choose CU.

### 7.3 Blockhash and lifetime
A `shadow-tier0` background thread maintains a rolling recent blockhash in `/dev/shm/shadow_blockhash` (cache‑line‑aligned, updated via atomic store). Workers read it via atomic load — no lock, no syscall.
Tier‑0 workers include a deadline; a worker must not send after the deadline passes.

---

## 8. Signing and sending (inline in `shadow-tier0` worker loop)

### 8.1 Signing authority
Every `shadow-tier0` worker thread holds a read‑only copy of the hot wallet Ed25519 private key in thread‑local storage (Section 2.1). There is no separate signing process. The key is loaded once at boot from an encrypted keyfile and cloned per worker.

### 8.2 Inline build → sign → send (no separate loops, no kernel)
Within each worker's run‑to‑completion loop (Section 6.1, steps 10–11):
- **Build**: populate template transaction with trade parameters, blockhash (atomic load from `/dev/shm/shadow_blockhash`), and fee/tip fields. Zero allocations.
- **Sign**: Ed25519 sign the serialized message bytes (~12–15 µs). Data remains in L1/L2 — no cache flush to L3/main memory.
- **Send**: write raw UDP/QUIC frame containing signed bytes directly to the worker's kernel‑bypass TX ring (AF_XDP UMEM TX ring, DPDK TX queue, or ef_vi TX descriptor). NIC DMA picks up the frame and blasts it to relay endpoints. **No `sendto()` syscall, no kernel UDP stack, no qdisc traversal.** Route selection per Section 8.3.

### 8.3 Signal‑source‑aware multi‑path routing (mandatory)

Solana has no global public mempool. A target transaction observed on a public Geyser stream is already propagating natively to the leader's banking thread. Wrapping it into a Jito bundle adds ≥ 10–30 ms (Block Engine network hop + simulation); the raw target lands before the bundle is validated, and the bundle is rejected (duplicate signature). Routing MUST therefore be determined by `signal_source` (tagged at ingestion; Section 6.1), not by a blanket "bundle‑first" policy.

**Case 1 — `PUBLIC_GEYSER` (target already in flight):**
- Jito bundle wrapping is **forbidden**. The race against a natively propagating transaction is unwinnable with bundle overhead.
- Route: **direct staked QUIC/TPU blast** to the current and next leader's TPU ports via SWQoS‑aware staked connections.
- Fee strategy: allocate spend entirely as **priority fee** (`SetComputeUnitPrice`). No Jito tip instruction (avoids Dragon 1 tip donation to non‑Jito leaders).
- The transaction is a standalone swap attempting to land sequentially after the target by virtue of high priority fee and fast delivery.
- Bounded retries with fee bumping; strict rate limits.

**Case 2 — `JITO_SEARCHER_API` (target held in Jito's pipeline):**
- The target is orderflow inside Jito's block‑building pipeline; it is not propagating publicly.
- Route: **Jito bundle** (target tx + our backrun tx), submitted to the Jito Block Engine.
- Fee strategy: allocate spend as **Jito bundle tip**, respecting configured minimum.
- This is the only scenario where bundle backrunning is physically viable for copy‑trading.
- No TPU fallback: if the Jito leader slot passes, the opportunity is lost.

**Case 3 — `PRIVATE_MEMPOOL` (timing uncertain):**
- The target arrived via a private feed; timing exclusivity is not guaranteed.
- Route: **configurable** via `submit.private_mempool_strategy`:
  - `staked_blast` (default): treat as Case 1 (staked QUIC/TPU with priority fee).
  - `jito_bundle_if_compatible`: attempt Jito bundle only if the current/next leader is Jito‑compatible AND `submit.min_jito_timing_margin_ms` (default 15) is satisfied. Otherwise fall back to staked blast.
- Fee strategy follows the selected route.

**Relay fanout constraints (per signal source):**
- `PUBLIC_GEYSER`: fan out via staked relays / TPU only (Group B). Jito bundle endpoints forbidden.
- `JITO_SEARCHER_API`: fan out via Jito Block Engine endpoints only (Group A). Standalone TPU blast forbidden (target is not public).
- `PRIVATE_MEMPOOL`: follows the selected route's group rules.

**Dual‑variant racing** (broadcasting both Jito and non‑Jito variants for the same intent) remains **forbidden** by default (double‑buy risk / signature collision).

### 8.4 Duplicate prevention (cross‑worker exec fence)
Workers MUST prevent duplicate sends for the same target:
- per‑mint/per‑slot atomic CAS on `/dev/shm/shadow_exec_fence` (Section 2.1, Section 6.1 step 8)
- if CAS fails (another worker already claimed), drop immediately
- additional per‑worker dedup by signature hash (thread‑local, no synchronization)

### 8.5 Reserve floor (no "poverty engineering")
`shadow-tier0` MUST enforce a **native SOL reserve floor** sufficient for:
- base fees
- sustained tip/priority fee activity under normal operations

Default requirement:
- `reserve_floor_sol = 50 SOL`
- alert at `25 SOL`
- if below floor: disable new entries (exits still allowed) until replenished.

Workers MUST NOT mutate exit transaction shapes to "self‑fund" tips.

### 8.6 Revert-cost breaker (bleed prevention)
`shadow-tier0` background thread tracks (aggregated across all workers):
- landed rate
- revert rate
- lamports spent per landed trade
- tip/CU efficiency vs win probability

Breaker activates only when:
- revert rate is high **and**
- auction spend is high **and**
- reverts correlate with deterministic mismatch (e.g., consistently failing min_out due to stale state)

When activated:
- signal oracle to lower `tip_cap_fraction` in the next LUT recomputation (reduces aggressiveness)
- tighten freshness requirements
- or temporarily disable the offending target/entity

Breaker MUST NOT target “normal” slippage reverts in healthy markets.

---

## 9. Position manager (`shadow-pm`)

### 9.1 Responsibility
`shadow-pm` owns inventory and exits:
- tracks fills from chain confirmations
- maintains per‑mint position
- triggers exit transactions when risk triggers occur

### 9.2 Exit templates
Exits MUST use templates (same principle as entries):
- fixed instruction sequence
- fixed compute budget
- fixed tip policy for panic exits

### 9.3 Panic mode
When panic triggers (e.g., swarm/rug signals):
- exits take priority
- new entries are disabled globally
- submitter dedicates bandwidth to exits

---

## 10. Oracle and deterministic attribution (`shadow-oracle`)

### 10.1 Profit model outputs
Oracle produces and publishes to shared memory:
- per‑entity `alpha_lcb` (conservative) → `/dev/shm/shadow_oracle_targets`
- per‑entity caps and blacklist → `/dev/shm/shadow_oracle_targets`
- **pre‑computed tip LUT** → `/dev/shm/shadow_tip_lut` (Section 6.6)

**Tip LUT computation (off hot path, continuous):**
- Oracle solves the continuous first‑derivative of `NetEV(tip) = p_win(tip/cu) × (alpha − cost(tip, cu))` for each alpha bucket.
- `p_win(tip_per_cu)` curve is calibrated from landed/reverted `ExecutionRecord` feedback, partitioned by auction contest regime (number of competing searchers, recent win rates).
- The LUT is recomputed every `oracle.tip_lut_recompute_interval_ms` (default 500 ms) or immediately on regime shift.
- **Dynamic cap per regime:** for low‑contest flow, cap may be 30–40% of alpha. For highly contested, high‑alpha flow, cap scales up to `tip_cap_max_fraction` (default 0.95). The LUT entries embed the cap — Tier‑0 does not evaluate cap logic.
- LUT is published atomically (seqlock or double‑buffer pointer swap on the LUT header).

Oracle MUST NOT block Tier‑0. Tier‑0 uses last known snapshot.

### 10.2 Deterministic genesis fingerprinting (kept, but no ML)
Oracle maintains a deterministic fingerprint registry:
- exact instruction sequence hash
- compute budget signature
- funding mantissa bitmask (if applicable)
- optional ALT set hash

A fast Rust implementation MAY compute candidate fingerprints on ingest and check an in-memory hash map in < 1 µs.

No ONNX/ANN inference exists in V6.

### 10.3 Burner attribution semantics
Attribution outputs may enable earlier eligibility, but MUST be conservative:
- first-trade eligibility gets a **size discount**
- promotion requires subsequent validation
- mis-attribution must not cause large risk exposure

---

## 11. Observability and benchmarks (money‑grade)

### 11.1 Required metrics
- end‑to‑end latency p50/p95/p99 (ingest→signed)
- quote latency p99
- sign latency p99
- cache miss rates (pool/mint/ALT)
- seqlock retry rates (pool state reads)
- landed rate, revert rate
- realized net EV per slot and per entity
- auction cost per landed trade
- tip LUT effectiveness: actual tip vs counterfactual optimal (post‑hoc), win rate by alpha bucket, regime shift frequency

### 11.2 Required event log (minimal)
Persist:
- `ExecutionIntentCreated`
- `ExecutionSent`
- `ExecutionLanded` / `ExecutionFailed` (reason)
- `PositionOpened` / `PositionClosed`
- `PanicModeOn/Off`

Event payloads MUST include:
- template id
- cu_limit
- tip
- min_out
- `signal_source` (which feed delivered the target; determines route taken)
- `route_taken`: enum { `STAKED_BLAST`, `JITO_BUNDLE` } (actual submission path)
- state snapshot seq/slot used for quote (to diagnose staleness)

### 11.3 CI/rollout gates (must pass)
- Microbench: Tier‑0 p99 ingest→signed ≤ 150 µs on production hardware
- Hot loop allocs: 0
- Hot loop locks: 0
- Canary: not applicable (removed)
- End‑to‑end sim tests on recorded feed capture

---

## 12. Configuration (single YAML)

### 12.1 Required keys (illustrative)
```yaml
system:
  mode: tier0_only
  latency_budget_us_p99: 150
  kernel_bypass: true                        # mandatory for production; false only for dev/test (Section 4.4)
  kernel_bypass_backend: af_xdp              # af_xdp | dpdk | ef_vi
  hugepage_size_mb: 2                        # 2 or 1024 (1GB); must match kernel boot config
  nic_rx_queues_per_worker: 1                # RSS queue count must match entry.workers

venues:
  raydium_v4:
    enabled: true
  pump:
    enabled: false

risk:
  kill_switch: false
  reserve_floor_sol: 50.0
  alert_floor_sol: 25.0
  max_position_sol_per_mint: 500.0
  max_notional_sol_per_entity: 1000.0
  inventory_fence:
    capacity: 4096                           # must match state.pool_state.max_pools
    reconcile_interval_ms: 500               # PM reconciles fence vs on-chain position state

state:
  max_pool_staleness_ms: 20
  max_mint_staleness_ms: 200
  max_alt_staleness_ms: 2000
  pool_state:
    max_pools: 4096                          # pre-allocated seqlock pool array capacity
    cache_line_bytes: 64                     # pad each entry to cache line boundary
    seqlock_retry_warn_threshold: 3          # log warning if seqlock retry exceeds this (indicates writer contention)

entry:
  workers: 8                                 # N run-to-completion worker threads (1 pinned core each)
  drop_on_cache_miss: true
  supported_decode:
    - raydium_v4_swap_base_in
  tip:
    lut_bucket_shift: 20                     # alpha_lcb >> 20 = ~1M lamport granularity per bucket
    lut_buckets: 1024                        # number of buckets in tip LUT
    tip_cap_max_fraction: 0.95               # max dynamic cap (highly contested flow); oracle may set lower per regime
    tip_cap_default_fraction: 0.35           # oracle default for low-contest regimes
    min_alpha_lcb_lamports: 10000000         # below this alpha, do not trade

submit:
  routing:
    default_public_geyser: staked_blast      # Case 1: direct staked QUIC/TPU (bundles forbidden)
    default_jito_searcher: jito_bundle       # Case 2: Jito bundle only
    private_mempool_strategy: staked_blast   # Case 3: staked_blast | jito_bundle_if_compatible
    min_jito_timing_margin_ms: 15            # Case 3 only: min margin before attempting Jito bundle
    allow_dual_variant_racing: false         # forbidden by default (double-buy risk)
  fallback_sendtransaction: false
  template_set: "raydium_v4_backrun_v1"
  exec_fence_capacity: 4096                  # power-of-two; per-mint/per-slot CAS fence size
  revert_bleed_breaker:
    enabled: true
    window_slots: 25
    max_spend_lamports_per_slot: 200000000
    max_revert_rate: 0.6

oracle:
  snapshot_interval_ms: 50
  tip_lut_recompute_interval_ms: 500         # how often oracle recomputes the tip LUT
  attribution:
    enabled: true
    fast_match_enabled: true
    size_discount_on_unvalidated: 0.2
```

---

## 13. Implementation checklist (ordered)

1. Implement kernel‑bypass NIC I/O layer (AF_XDP/DPDK/ef_vi): per‑worker RX/TX ring pairs, hugepage‑backed UMEM/mempool, RSS flow steering, poll‑mode ingress. Validate sub‑5 µs round‑trip NIC latency before proceeding.
2. Implement SPSC feed rings + fixed binary IPC (feed rings, snapshot segments, blockhash segment, exec fence, inventory fence).
3. Implement seqlock‑protected `/dev/shm/shadow_pool_state` segment: pre‑allocated cache‑line‑aligned pool array, per‑entry seqlock write protocol in `shadow-state`, O(1) wait‑free reader in Tier‑0 workers (Section 5.4).
4. Implement `shadow-state` fork-aware account replication for Raydium V4 pools/vaults/mints with seqlock writes + fork rollback.
5. Implement strict Raydium V4 O(1) decode adapter.
6. Implement quote + min_out integer math.
7. Implement `shadow-tier0` monolithic binary: N run‑to‑completion worker threads, each performing kernel‑bypass ingest → decode → gate → seqlock pool read → quote → tip LUT lookup → inventory fence `fetch_add` → exec fence CAS → template build → Ed25519 sign → kernel‑bypass send. Zero allocations, zero locks, zero syscalls in the hot loop.
8. Implement per‑mint/per‑slot atomic CAS exec fence + atomic `fetch_add`/`fetch_sub` inventory fence with PM reconciliation for cross‑worker duplicate and risk enforcement.
9. Implement signal‑source‑aware multi‑path sender within each worker: staked QUIC/TPU blast (public signals) + Jito bundle path (Jito Searcher API signals) + per‑source fanout constraints.
10. Implement oracle snapshot with `alpha_lcb` + pre‑computed tip LUT (O(1) lookup, continuous recomputation, dynamic contest‑aware cap).
11. Implement deterministic fingerprint registry + fast matcher.
12. Implement PM exits + panic mode (`shadow-pm` separate process) + inventory fence reconciliation.
13. Implement latency microbench suite and make it a release gate (must include: kernel‑bypass NIC round‑trip, seqlock read under writer contention, inventory fence atomic latency, full pipeline p99).

---

## 14. What "done" means (money-grade)

V6 is "done" only when:
- Tier‑0 p99 ingest→signed→NIC‑DMA ≤ 150 µs on production hardware with kernel bypass enabled,
- kernel‑bypass NIC round‑trip (RX DMA → TX DMA) ≤ 5 µs validated independently,
- zero syscalls measured in hot loop (verified via `strace` / `perf trace`),
- compute templates are calibrated to minimize CU limit,
- tip LUT produces measurably higher realized NetEV vs fixed tips or discrete brute‑force,
- cache miss rates are low enough that drops do not starve EV,
- inventory fence prevents N‑worker burst from exceeding per‑mint position limits under synthetic load,
- and the system's realized net EV per slot is positive after auction costs in live conditions.

