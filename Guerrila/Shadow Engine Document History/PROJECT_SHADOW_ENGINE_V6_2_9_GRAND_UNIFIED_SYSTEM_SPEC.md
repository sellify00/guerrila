# PROJECT SHADOW ENGINE V6.2.9 — Profit‑First Tier‑0 Sniper System Specification (Unified)

**Status:** Locked engineering implementation specification (single authoritative document)  
**Supersedes:** V5, V5.1, V6.0, V6.1, V6.2, V6.2.1, V6.2.2, V6.2.3, V6.2.4, V6.2.5, V6.2.6, V6.2.7, V6.2.8, and all prior drafts/addenda  
**Target ecosystem:** Solana mainnet  
**Primary venues:** Raydium V4 (mandatory), Pump/PumpSwap (optional via separate adapter)  
**Execution model:** pre‑consensus ingestion → O(1) intent reconstruction → cache‑only forward‑projected quote → template build → sign → signal‑source‑aware multi‑path send
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
**Hot path** in V6.2.9 is the deterministic, bounded **money-worker loop** from:

1) `IngressRecord` dequeued from one of the worker’s ingress rings (`/dev/shm/shadow_feed_ring_w{worker}_p{producer}`), to
2) `SignedTxEnvelope` enqueued into the per-worker egress ring (`/dev/shm/shadow_egress_ring_*`).

Stateful networking (TLS/QUIC egress, Jito Searcher API sessions) is **explicitly excluded** from the money-worker loop and is handled by dedicated proxy threads pinned to separate cores (Section 4.4). The money-worker thread never performs segment reassembly, HPACK, TLS, or QUIC framing.

**However**, the money-worker p99 budget alone is a **necessary but not sufficient** metric. The market does not care how fast the O(1) math loop is if the ingress path took 500 µs to decode a gRPC feed. The **end-to-end competitive latency** — from first byte on the wire to signed transaction on the egress NIC — is what determines auction outcomes. Section 0.4 defines both budgets.

### 0.4 Latency budget
Tier‑0 Sniper Lane **MUST** satisfy:

**Money-worker budget (deterministic math loop):**
- **p99 money-worker latency ≤ 150 µs** from feed-ring dequeue to egress-ring enqueue (as defined in Section 0.3).
- **No blocking waits** in the worker loop.
- **No hot‑path network I/O** (RPC/HTTP) for decisions.
- **No kernel syscalls** inside the worker loop.

**End-to-end competitive budget (wire-to-wire):**
- **p99 end-to-end latency target ≤ 500 µs** from first byte of the relevant UDP shred arriving at the NIC RX ring to signed transaction bytes hitting the egress NIC TX ring.
- This budget encompasses: shred reception → transaction extraction → allowlist prefilter → decode → feed-ring enqueue → money-worker loop → egress-ring enqueue → egress proxy framing → NIC TX.
- If only the money-worker p99 is measured while the ingress path adds hundreds of microseconds of unaccounted TCP/gRPC/protobuf overhead, the latency metric is laundered and the system will lose auctions regardless of worker-loop speed.

**Why TCP/gRPC ingestion is not competitive for Tier-0 targets:**
Yellowstone gRPC is HTTP/2 over TCP. TCP segment reassembly, HPACK header decompression, and protobuf parsing add 200–800 µs of variable-latency overhead before the transaction bytes are even visible to application code. This overhead is invisible to the money-worker p99 metric (which starts after dequeue) but fully visible to competitor latency. For Tier-0 targets, TCP-based feeds are a fallback, not a primary ingestion path.

**Primary Tier-0 ingestion: raw UDP shred capture (mandatory):**
The ingress path for Tier-0 targets MUST ingest raw Turbine shreds directly from the network via AF_XDP (or equivalent kernel-bypass UDP capture) and extract transaction data before full block reassembly (Section 4.4.1).

**Networking proxy constraint (separate cores):**
- Ingress and egress proxy threads MUST keep sessions **pre-warmed** and MUST NOT introduce backpressure into the money-worker loop; all worker↔proxy handoff is drop-on-full ring buffers.

If the system cannot meet the p99 budgets above on production hardware, it is not considered "production‑ready" regardless of architecture elegance.


### 0.5 Compiler, memory model, and false‑sharing constraints (Rust/LLVM)
This specification assumes implementation in **Rust (LLVM)** and a production deployment where microseconds matter. The following are **production requirements** (violations silently destroy latency, correctness, or key material):

#### 0.5.1 Optimizer‑resistant zeroization (secret material)
- Any secret scalar nonce `r` (Ed25519) **MUST** be destroyed using a primitive the optimizer cannot remove.
- **MUST use** one of:
  - `libc::explicit_bzero()` (FFI call), or
  - the Rust `zeroize` crate (`Zeroize`, `ZeroizeOnDrop`), or
  - manual `write_volatile` / `volatile_set_memory` equivalents.
- **MUST NOT** rely on normal assignments like `r = 0`, `fill(0)`, or `memset` without explicit guarantees — LLVM may delete them as dead stores in `--release`.

#### 0.5.2 Atomic ordering policy (no `SeqCst` in Tier‑0)
- `Ordering::SeqCst` is **forbidden** in Tier‑0 hot code paths (adds unnecessary serialization).
- Default policy:
  - reader loads: `Acquire`
  - writer stores: `Release`
  - CAS / `compare_exchange`: `AcqRel` on success, `Acquire` on failure
- For **pure counters** where ordering is irrelevant (e.g., inventory fence arithmetic), `Relaxed` is allowed (still atomic), but only if it does not weaken correctness.

#### 0.5.3 Cache‑line isolation and prefetcher‑safe padding
- Any double‑buffer slot or read‑mostly struct **MUST** be at least **64‑byte aligned** (one cache line).
- **Highly contended cross‑worker atomics** (inventory fence entries, producer/consumer cursors) **MUST** use **128‑byte stride** to defeat adjacent‑cache‑line prefetch and eliminate MESI false sharing during bursts.
- Rust implementation **MUST** wrap each entry in an explicitly aligned struct:
  - double‑buffer slots / read‑mostly: `#[repr(C, align(64))]`
  - contended Tier‑0 fences/cursors: `#[repr(C, align(128))]`
- **MUST NOT** use packed arrays like `[AtomicI64; N]` or `[AtomicU64; N]` for contended fences; they will share cache lines and self‑DoS under contention.

#### 0.5.4 Core dumps and secret mappings
- Production Tier‑0 binaries **MUST** run non‑dumpable (`RLIMIT_CORE=0` and `prctl(PR_SET_DUMPABLE, 0)`).
- Any secret memory mapping (nonce rings, private keys) **MUST** be marked `MADV_DONTDUMP` and locked with `mlock()`.


#### 0.5.5 Panic strategy (`panic = "abort"` for Tier‑0)
- Production `shadow-tier0` **MUST** be compiled with `panic = "abort"` (no unwinding).
  - Rationale: unwinding introduces unpredictable tail latency and can skip critical cleanup/rollback paths.
- The process supervisor (systemd / watchdog) MUST treat any Tier‑0 panic as a crash and restart immediately.
- Because `panic = "abort"` skips `Drop`, security MUST rely on:
  - explicit, immediate zeroization after use (Section 0.5.1), and
  - non-dumpable + `MADV_DONTDUMP` secret mappings (Section 0.5.4),
  not RAII drop handlers.


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
4. Quotes a backrun trade using **two‑phase O(1) forward projection**: simulates the target's state impact on the cached pool, then quotes our trade against the projected post‑target state (Section 6.4).
5. Builds and signs a **template transaction** with fixed compute budget.
6. Sends via **signal‑source‑aware multi‑path routing**: Jito bundles only when the target is held in a private pipeline (Jito Searcher API / private mempool); direct staked QUIC/TPU blast with priority fee when the target was observed on a public feed (Section 8.3).

### 1.3 Explicit non‑goals (removed from V6)
V6 explicitly removes the following because they add steady‑state latency, complexity, or compute tax without clear profit dominance in Tier‑0:

- Dynamic schema‑driven protocol decoders and hot reload
- Canary dual‑parsing pipelines
- Embedded SVM tracing / net‑intent extraction
- On‑chain MEV Router assertions on the entry hot path **except** the mandatory single‑instruction `ShadowGuard` monotonic reserve bound used for **all** backrun executions (Section 7.1 / 8.3)
- ML inference (ONNX / dual encoder) anywhere that gates first‑trade eligibility
- “Always landed” optimization as a primary objective

If a feature increases steady‑state Tier‑0 latency or compute without measured NetEV gain, it is out of scope.

---

## 2. System topology (single‑lane)

### 2.1 Processes (fault domains)
V6 uses four processes. Only one is latency‑critical.

1. **shadow-state** (state daemon; not in hot path)
   - Fork‑aware replication of required on‑chain accounts (pools, mints, vaults, ALTs if subscribed).
   - Writes pool state directly into double‑buffered shared‑memory segments (Section 5.4). Tier‑0 workers read state via O(1) wait‑free double‑buffer reads — no diff polling, no local map maintenance.
   - Publishes health + snapshot metadata.


2. **shadow-tier0** (Tier‑0 lane; latency‑critical **money workers**)
   - Single binary containing:
     - **N run‑to‑completion money-worker threads**, each pinned to an isolated core.
     - **Ingress thread(s)** (shred capture via AF_XDP [primary] + gRPC fallback), pinned to dedicated cores.
     - **Egress proxy thread(s)** (QUIC/TLS + Jito sessions), pinned to dedicated cores.
     - **Crypto precompute thread(s)** for Ed25519 nonce material, pinned to dedicated cores (Section 8.2.1).
   - Money workers execute the deterministic money loop on a single core:
     `IngressRecord` (**pre-flattened, pre-decoded**) → gate → quote → template build → sign → enqueue `SignedTxEnvelope`.
   - **Money workers are stateless-networking-free:** they never perform TCP reassembly, HTTP/2 HPACK decode, TLS, QUIC framing, or AES-GCM. Those occur only on proxy cores (Section 4.4).
   - **No variable-length parsing on the money core:** ALT expansion and swap decoding occur on ingress proxy cores (Section 4.4.1). After dequeue, the worker performs only fixed-cost operations until enqueue.
   - Each money-worker thread holds a **read‑only copy of the hot wallet Ed25519 private key** in thread‑local memory. Signature generation uses precomputed nonce material (Section 8.2.1).
   - Worker↔proxy communication is **lockless SPSC** (drop-on-full) rings:
     - ingress producers → money workers: `/dev/shm/shadow_feed_ring_w{worker}_p{producer}` (SPSC; one ring per worker per ingress producer)
     - money workers → egress proxies: `/dev/shm/shadow_egress_ring_*`

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
- Nonce / recent‑blockhash coordination: each worker reads the rolling blockhash from a double‑buffered shared‑memory segment (`/dev/shm/shadow_blockhash`) published by a background thread within `shadow-tier0`. Double‑buffer read protocol (Section 4.2.1 / 7.3); no lock, no syscall.

**Cross‑worker synchronization (single atomic fence, no locks):**
- **Inventory fence** (`/dev/shm/shadow_inventory_fence`): shared per‑mint risk gating (Section 6.7). This is the only mandatory cross‑worker atomic in Tier‑0.

**Duplicate prevention:** There is **no global Exec Fence** in V6.2.9. Duplicate suppression is achieved by **pool‑sharded ingress routing** (each pool routed to exactly one worker) plus **thread‑local signature dedupe** inside that worker (Section 6.1 / 8.4).




### 2.2 CPU, NUMA, and NIC requirements
Tier‑0 **MUST** run on dedicated isolated cores:

- `shadow-tier0` (single binary, pinned cores):
  - N money-worker cores (1 per run‑to‑completion worker; decode→quote→sign→enqueue only)
  - 1+ ingress cores (shred capture via AF_XDP [primary]; gRPC fallback on separate threads)
  - 1+ egress proxy cores (pre-warmed QUIC/TLS sessions to TPU/staked relays; Jito Searcher API sessions)
  - 1 crypto precompute core per worker-group (or per worker) filling nonce rings (Section 8.2.1)
  - 1 background core for blockhash refresh / health polling / async log drain
- `shadow-state`, `shadow-pm`, `shadow-oracle`: separate cores (not shared with Tier‑0 money-worker cores) separate cores (not shared with Tier‑0 workers)

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
- O(1) quote via two‑phase forward projection (simulate target impact → quote backrun against projected state).
- O(1) tx build from templates.

Anything not decodable in O(1) is dropped.

### 3.2 No hot‑path network I/O; networking is off the money thread
Tier‑0 MUST NOT make RPC/HTTP calls. This includes:
- ALT fetch
- mint/pool fetch
- fee telemetry fetch
- leader schedule fetch

All such data MUST be maintained by background threads/processes and presented via in‑memory caches or shared memory snapshots.

Tier‑0 **money workers** MUST NOT perform stateful networking (TCP/gRPC, TLS, QUIC framing) and MUST NOT invoke kernel networking syscalls. Workers only enqueue/dequeue fixed binary records; ingress/egress proxies handle networking on separate cores (Section 4.4).


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
- route through on‑chain assertion routers **except** the mandatory single‑instruction `ShadowGuard` guard used for **all** Tier‑0 backrun executions (Section 7.1 / 8.3),
- add validation CPIs,
- run embedded simulators on the critical loop,
- invoke kernel syscalls for networking (Section 3.2) or logging.

---

## 4. Shared memory and IPC (zero‑copy)

### 4.1 Shared memory segments (all binary; no JSON)
All IPC uses fixed binary layouts in `/dev/shm`. No Python writes binary structs.

Required segments:
- `/dev/shm/shadow_feed_ring_w{worker}_p{producer}` (**SPSC rings; one per money worker per ingress producer**):
  - Producer = a single ingress thread (`shadow-ingest`): either an AF_XDP shred capture worker or a gRPC fallback worker.
  - Consumer = exactly one money worker (`shadow-tier0`).
  - Total rings = `N_workers × P_ingress_producers` where `P_ingress_producers = shred_capture_workers + grpc_workers`.
  - **Routing (mandatory, DOS‑resistant):** each parsed `IngressRecord` MUST be routed to **exactly one worker** by **pool pubkey**, not by signature:
    - `N_workers` MUST be a **power‑of‑two** (4/8/16/…).
    - At process boot, generate `route_salt_u64` (CSPRNG; not in YAML; not shared memory).
    - `h32 = crc32_u64(route_salt_u64, u64::from_le_bytes(pool_pubkey[0..8]))` (x86: `_mm_crc32_u64`)
    - `worker_id = (h32 as usize) & (N_workers - 1)`
    - The ingress producer MUST enqueue the record only into `/dev/shm/shadow_feed_ring_w{worker_id}_p{producer_id}`.
  - **No broadcast**. No global Exec Fence. (Duplicate suppression is thread-local inside the pool-sharded worker; Section 8.4.)
- `/dev/shm/shadow_egress_ring_*` (SPSC rings; one per money worker per egress lane; producer = worker, consumer = egress proxy)
- `/dev/shm/shadow_pool_index_map` (lock‑free fixed‑capacity **8‑byte tag** map: `pubkey_hash40(pool_pubkey) -> pool_index_plus_one_u24`; full pubkey verified in arena; written by `shadow-state`, read by workers; Section 5.4)
- `/dev/shm/shadow_pool_state` (**wait‑free double‑buffered (RCU‑lite) pool arena with tear detection**; two hot slots per pool + atomic active index; written by `shadow-state`, read O(1) by workers; Section 5.4)
- `/dev/shm/shadow_mint_index_map` (lock‑free fixed‑capacity **8‑byte tag** map: `mint_hash40(mint_pubkey) -> mint_index_plus_one_u24`; full pubkey verified in mint arena; written by `shadow-state`, read by workers)
- `/dev/shm/shadow_mint_state` (mint arena/snapshot; read‑mostly; written by `shadow-state`; used for hard rejects and risk indexing; Section 5.6)
- `/dev/shm/shadow_alt_cache` (ALT snapshot; read‑mostly; written by `shadow-state`; used by ingress proxy for ALT expansion; Section 6.3)
- `/dev/shm/shadow_tip_lut` (read‑only tip LUT; written by `shadow-oracle`, read by workers; Section 6.6)
- `/dev/shm/shadow_inventory_fence` (per‑mint atomic risk fence; written by workers and PM; Section 6.7; **128‑byte stride**)
- `/dev/shm/shadow_health` (state health + staleness timestamps; written by `shadow-state`, read by workers; Section 5.5)
- `/dev/shm/shadow_blockhash` (**double‑buffered with tear detection**; written by a background thread inside `shadow-tier0`, read by workers; Section 7.3)

Secrets:
- Secret nonce material (`r`) MUST NOT be stored in `/dev/shm`. Nonce rings MUST be anonymous/private locked memory (Section 8.2.1).

### 4.2 Ring buffer requirements
All SPSC rings (feed and egress rings) MUST be:
- SPSC
- preallocated
- power‑of‑two capacity
- single cache‑line producer/consumer cursors
- drop‑on‑full (never block Tier‑0)

### 4.2.1 Wait‑free double‑buffered state (RCU‑lite) requirements (pool state + blockhash)
Tier‑0 MUST NOT spin on shared state during volatility. Any design that can block, retry, or time out inside the money‑worker loop is an EV filter.

All writer‑updated shared structs used by Tier‑0 (pool state and rolling blockhash) MUST use **wait‑free double buffering** **with tear detection**:

- **Single writer**, **multiple readers**. Readers never write.
- Each published item consists of:
  - `active_idx: ActiveIdx128B`, where `ActiveIdx128B` is `#[repr(C, align(128))]` and contains `active_idx: AtomicU8` (values `{0,1}`) plus padding.
    - **Cache‑line isolation is mandatory:** `active_idx` MUST reside on its own 128‑byte cache sector (do not pack it adjacent to `slot[0]`). Otherwise, writer stores to `slot[0]` will invalidate the same line and stall readers on `active_idx` via MESI.
  - `slot[2]: StateSlot64B` (each exactly 64 bytes, `#[repr(C, align(64))]`).

Example layout (Rust; normative intent):

```rust
#[repr(C, align(128))]
struct ActiveIdx128B {
    active_idx: core::sync::atomic::AtomicU8,
    _pad: [u8; 127],
}
```

Note: In the pseudocode below, `active_idx` refers to the inner atomic field (`ActiveIdx128B.active_idx`).

- Each `StateSlot64B` MUST include an internal **sequence** field:
  - `sequence: AtomicU32` (or an aligned `u32` with atomic semantics),
  - sequence MUST be **even** when the slot is stable, **odd** while a writer is mutating the slot.
  - This protects against the “double‑flip tear” race where a reader is interrupted mid‑copy and the writer cycles back to overwrite the same slot.

**Writer protocol (RCU‑lite + sequence guard):**
1. `active = active_idx.load(Relaxed)`
2. `inactive = active ^ 1`
3. Mark write‑in‑progress on the inactive slot:
   - `seq = slot[inactive].sequence.load(Relaxed)`
   - `slot[inactive].sequence.store(seq + 1 /* odd */, Release)`
4. Write the entire payload of `slot[inactive]` using only plain memory writes (no alloc, no syscalls, no logging inside the write window).
   - The writer MUST NOT write `active` slot.
5. Mark write‑complete:
   - `slot[inactive].sequence.store(seq + 2 /* even */, Release)`
6. Publish:
   - `active_idx.store(inactive, Release)`

Writer preemption while writing the inactive slot is safe: readers continue using the current active slot.

**Reader protocol (wait‑free; no spinning):**
1. `active = active_idx.load(Acquire)`
2. `seq1 = slot[active].sequence.load(Acquire)`
   - If `seq1` is odd: **DROP INTENT** (writer is mid‑write on what we thought was active; this should be rare, but is correctness‑critical).
3. Copy the entire `slot[active]` (64 bytes) into a stack local.
4. `compiler_fence(Acquire)` (or equivalent) to prevent reordering across the copy.
5. `seq2 = slot[active].sequence.load(Relaxed)`
   - If `seq1 != seq2` OR `seq2` is odd: **DROP INTENT** (torn read detected).
6. Proceed using the stack-local copy.

**Policy:** No retries and no spin loops. If tearing is detected, fail‑closed by dropping that intent. A 1‑in‑a‑million dropped trade is acceptable; a single torn pool state leading to bad quotes is not.

**Atomic ordering:** Readers use `Acquire` loads; writers use `Release` stores. Tier‑0 MUST NOT use `SeqCst` on the hot path.

### 4.3 Tier‑0 "no allocation / no lock / no syscall" contract
Inside the run‑to‑completion worker loop (ingest through send):
- **MUST NOT allocate** (no heap allocs)
- **MUST NOT lock** (no mutex; the only synchronization primitives on the money path are: atomic operations on the inventory fence, atomic loads of `active_idx` for double‑buffered state reads, and 8‑byte atomic tag loads for index maps — all are wait‑free / lock‑free)
- **MUST NOT syscall** — no `sendto`, `recvfrom`, `syslog`, `write`, `futex`, or any other kernel trap on the critical path. Workers enqueue to `/dev/shm/shadow_egress_ring_*`; proxy cores perform networking (Section 4.4).
- **MUST NOT poll or drain diff queues** — pool state is read directly via wait‑free double buffer (Section 5.4)
- Logging MUST be async, sampled, and off the hot path

### 4.4 Stateful networking is off the money thread (proxy cores)

**Why:** Yellowstone is gRPC (HTTP/2 over TCP). TCP segment reassembly, HPACK, and protobuf parsing add 200–800 µs of uncontrolled latency before the transaction bytes are even visible. Staked TPU delivery is QUIC (UDP + TLS 1.3 + connection state + AES‑GCM). Implementing either inside the money-worker run-to-completion loop is incompatible with a 150 µs budget — but even on proxy cores, TCP/gRPC ingestion is too slow for Tier-0 targets (Section 0.4).

**Rule:** money workers MUST handle only fixed-size binary records and raw transaction bytes. All stateful networking and cryptography framing happens on dedicated proxy cores. For Tier-0 targets, the primary ingestion path MUST be raw UDP shred capture, not TCP/gRPC.

#### 4.4.1 Ingress: shred capture (primary) + gRPC fallback

The ingress path exists to keep **variable-length parsing** and **ALT expansion** off the money-worker core, and to deliver transaction data to the feed rings with minimal wire-to-dequeue latency.

**Primary path: raw Turbine shred capture via AF_XDP (mandatory for Tier‑0):**

For Tier‑0 targets, TCP/gRPC feeds are a latency‑laundering trap. The ingress MUST support a **raw UDP shred capture** path:

- **Shred reception:** Receive Turbine shreds directly from the gossip/Turbine tree via AF_XDP (or equivalent kernel‑bypass UDP capture) on dedicated ingress cores. Shreds arrive as raw UDP datagrams — no TCP reassembly, no HTTP/2 framing, no protobuf.
- **Zero‑copy lookahead scanner (with MTU crossover + out‑of‑order safety):** Before full shred reassembly into a block entry, the ingress core MUST perform a byte‑level scan to identify candidate Tier‑0 targets **without** doing full block reconstruction.

  **Boundary rule (mandatory):** the scanner MUST NOT assume that a single shred contains a contiguous transaction header. Transactions frequently span multiple shreds; the MTU boundary can bisect the message header or account key list.

  **UDP ordering rule (mandatory):** Turbine shreds are UDP. Packet delivery is not ordered. The scanner MUST NOT blindly append “previous tail + current prefix” unless the shreds are provably contiguous.

Therefore the ingress core MUST maintain a deterministic, pre‑allocated **sliding fragment window** that is scoped by **slot** (Turbine shreds from Slot N and Slot N+1 can interleave on the wire):

- Maintain a tiny, L1‑resident array of **4 active slot contexts** (`SlotFragCtx[4]`). Each context holds:
  - `slot: u64`
  - `tail_shred_index: u32` (last contiguous shred index observed for this slot)
  - `crossover_tail: [u8; 128] + len` (tail bytes from the last contiguous shred)
  - `reorder_buf`: fixed, bounded buffer for out‑of‑order shreds for **this slot only** (capacity 16).

- On each incoming shred:
  1. Parse `(slot, shred_index, is_data_shred)` from the Turbine header.
     - If `is_data_shred == false` (coding/FEC parity shred), drop immediately. Never append parity bytes into the crossover window.
  2. Select the matching `SlotFragCtx` by `slot`. If absent, evict the oldest context and initialize it for this `slot` (clear reorder + crossover).
  3. Insert the shred payload into `reorder_buf` keyed by `shred_index`.
  4. Only evaluate the Bloom filter / signer pubkey scan on a **contiguous** header view:
     - If `shred_index == tail_shred_index + 1`, build `crossover_view = crossover_tail || current_payload_prefix` and scan that contiguous view.
     - After a contiguous shred is processed, set `tail_shred_index = shred_index`, refresh `crossover_tail` (last 128 bytes), and drain any buffered shreds that become contiguous.
     - Otherwise, do **not** stitch bytes across disjoint indices; wait for the missing shred(s) or flush (next rule).
  5. **UDP deadlock escape (no timers):**
     - If `shred_index <= tail_shred_index`: drop as late/duplicate and continue (prevents unsigned underflow flush nukes).
     - Else if `(shred_index - tail_shred_index) > 16`: flush this slot context’s reorder buffer and reset `tail_shred_index = shred_index` (prevents stalling on a permanently dropped shred).

- Never stitch across slots. Slot‑scoped contexts prevent “Frankenstein” byte arrays under packet reordering + slot interleaving.

**Fallback path: Yellowstone gRPC (non‑Tier‑0 and warm‑start):**

gRPC/TCP feeds remain available for:
- Non‑Tier‑0 monitoring, analytics, and oracle inputs.
- Warm‑start / bootstrap before the shred capture pipeline is synchronized.
- As a **redundant secondary feed** — if a transaction is seen on gRPC but was missed on shred capture (due to erasure gaps), the gRPC path can still produce an `IngressRecord`.

Sharded routing (Section 8.4) ensures all sources route the same **pool** to the same worker; thread‑local signature dedupe prevents duplicates.

gRPC fallback ingress proxy responsibilities (pinned cores):
- Maintain provider connections (Yellowstone gRPC and any other stateful feeds).
- Parse protobuf messages and extract:
  - `signature`, `slot`, `unix_ns`, and the raw message bytes (if needed for auditing).
    - `unix_ns` SHOULD be stamped once in the proxy (TSC→ns or vDSO `clock_gettime`). Money workers MUST NOT call time syscalls.
  - `signal_source` tagging.

**Allowlist prefilter (both paths, before any variable‑length work):**
- Parse the Solana transaction message header to extract:
  - `num_required_signatures` (1 byte; offset 0 of the message header).
  - all signer pubkeys: `accounts[0..num_required_signatures]` from the static account keys (**no ALT expansion** required).
- Check **every signer** (`accounts[0]` through `accounts[num_required_signatures - 1]`) against the Tier‑0 target allowlist (or an L1‑resident Bloom filter + verify table).
- **Why all signers, not just `accounts[0]`:** on Solana, the fee payer (`accounts[0]`) is often NOT the trading wallet. When targets trade via Telegram bots (Trojan, Banana Gun, Maestro) or heavy web terminals (Photon, BullX), the fee payer is the bot's central fee‑collection relayer; the actual target wallet is passed as Signer 1 or Signer 2. Checking only `accounts[0]` silently drops the majority of high‑alpha order flow.
- If **no signer** matches the allowlist: **drop immediately** (do not expand ALTs, do not decode).
- If **any signer** matches: populate `entity_id` with the matched target's entity id (if multiple signers match, use the first match by signer index) and continue.

**Post‑filter processing (both paths):**
- **Expand Address Lookup Tables (ALTs)** and produce a deterministic **flattened account list**.
- **Strictly decode supported swap patterns** (Raydium V4 `swap_base_in`, etc.) and emit a fixed‑size record so the money worker does zero pointer chasing and zero array expansion.

Ingress proxy output:
- Writes fixed binary `IngressRecord` structs into `/dev/shm/shadow_feed_ring_w{worker}_p{producer}` (SPSC, drop‑on‑full).
- Each ingress thread has a stable `producer_id`:
  - `producer_id ∈ [0 .. shred_capture_workers-1]` for AF_XDP shred workers,
  - `producer_id ∈ [shred_capture_workers .. shred_capture_workers+grpc_workers-1]` for gRPC workers.
- After strict decode, the ingress thread MUST choose exactly one worker by **pool pubkey sharding** (DOS‑resistant; no DIV):
  - `h32 = crc32_u64(route_salt_u64, u64::from_le_bytes(pool_pubkey[0..8]))`
  - `worker_id = (h32 as usize) & (N_workers - 1)`  (requires `N_workers` power‑of‑two)
  - enqueue only into `/dev/shm/shadow_feed_ring_w{worker_id}_p{producer_id}`
- `IngressRecord` MUST be rigid and fixed‑size (no dynamic vectors). Recommended layout includes:
  - `signal_source`, `slot`, `signature`
  - `entity_id` (or allowlist key)
  - `venue_id` + `pattern_id`
  - decoded fields: `pool_pubkey`, `mint_in`, `mint_out`, `amount_in`
  - `flat_accounts_len: u8` and `flat_accounts: [Pubkey; 64]` (unused entries zeroed)
  - optional: fixed indices into `flat_accounts` for the exact accounts required by the pattern (so the worker never scans)

Failure policy:
- If required ALT expansion or decode fails, the ingress proxy MUST drop the record (do not forward “maybe‑decodable” work to the money worker).

The ingress path MUST be sized so it never becomes the throughput bottleneck. The shred capture path MUST use kernel bypass (AF_XDP / DPDK). The gRPC fallback path MAY use kernel networking (standard sockets) or a user‑space TCP stack (e.g., F‑Stack); this is outside the money‑worker loop.

#### 4.4.2 Egress proxy (worker envelopes → QUIC/Jito sessions)
- Maintains **pre‑authenticated, long‑lived** sessions:
  - QUIC/TLS 1.3 sessions to current/next leaders (and/or staked relays).
  - Jito Searcher API sessions (bundle submission).
- Reads `SignedTxEnvelope` from `/dev/shm/shadow_egress_ring_*`, performs QUIC framing + encryption (or Jito protocol framing), and transmits.
- MUST NOT perform per-send handshakes. Any session loss triggers an async reconnect; the send loop never blocks and never spins on handshakes. If no authenticated session is available, drop the envelope (fail-fast).

##### 4.4.2.1 “Kamikaze” QUIC mode (mandatory for latency auctions)
Vanilla QUIC stacks implement RFC congestion control (CUBIC/BBR), pacing, and polite retransmission behavior. On Solana TPU paths (packet loss + hostile jitter), “polite” behavior is equivalent to losing auctions.

For **STAKED_BLAST** traffic, the egress proxy MUST run in a “kamikaze” configuration:

- **No pacing / no intentional delay** on writes (disable any library pacing layer).
- **Congestion control must not throttle** our micro-bursts:
  - Use a custom/modified congestion controller that keeps the congestion window effectively unbounded for our message sizes (or an explicitly “unlimited cwnd” controller).
  - If the QUIC library cannot be configured this way, it is not acceptable for production Tier‑0 egress.
- **No delayed acknowledgement heuristics** where configurable (reduce ACK delay, ACK aggregation).
- Writes MUST be **fire-and-forget** from the egress worker perspective: never block the proxy worker waiting on retransmission timers.
- If a connection enters a throttled/backpressured state, the proxy SHOULD immediately fail over to alternate pre-warmed connections/endpoints rather than queueing.

##### 4.4.2.2 Redundant delivery via distinct SWQoS identities (loss bypass)
For critical `PUBLIC_GEYSER`-derived sends, the egress proxy SHOULD send **redundant copies** of the same transaction payload to improve delivery probability under packet loss.

**Why same-IP source port mutation is forbidden (SWQoS self-DDoS):**
Solana leaders enforce **Stake-Weighted Quality of Service (SWQoS)**. The TPU banking thread allocates a strictly bounded packet/byte allowance per connection identity (IP + staked node pubkey) proportional to delegated stake. Blasting N redundant copies from the same IP — even with distinct UDP source ports — amplifies our bandwidth footprint by N×. This instantly exhausts our stake-weighted rate limit, causing the leader's firewall to aggressively tail-drop our packets. Source port mutation on a single IP is literally a self-inflicted DDoS against our own delegated stake allocation.

**Multi-identity relay fan-out (mandatory for redundancy):**
Redundant copies MUST be routed through **geographically distinct staked relay nodes**, each possessing a different IP address and staked node identity:

- Each relay maintains its own SWQoS allocation with the leader, so redundant copies consume independent bandwidth budgets.
- **One transaction copy per SWQoS identity** (1 copy per relay). Never send multiple copies from the same relay/IP to the same leader.
- `relay_endpoints`: configurable list of staked relay endpoints (each with its own IP, node keypair, and pre-warmed QUIC session to the current/next leader).
- `max_relay_fanout` (default 3, max 5): number of distinct relay identities to fan out through per transaction.
- Relay selection SHOULD prefer relays with the highest confirmed SWQoS allocation and lowest observed latency to the current leader.
- If fewer than `max_relay_fanout` relays are healthy/available, send through however many are available (degrade gracefully; do not queue or block).

**Relay session management:**
- Each relay MUST maintain **pre-warmed, pre-authenticated** QUIC sessions to current and next leader TPU ports.
- Session loss triggers async reconnect; the send path never blocks on relay handshakes.
- Relay health is monitored continuously; unhealthy relays are rotated out of the active set.

- Copies MUST be bounded by a global burst limiter to avoid self-induced buffer overrun at the egress proxy.
- **TX ring sizing requirement (kernel bypass):** when multi-relay fan-out is enabled, the AF_XDP/DPDK/ef_vi TX descriptor ring MUST be sized to absorb worst-case bursts (relays × endpoints) without local drops. Recommended minimum: **4096 descriptors** (8192 for high fan-out). The proxy MUST expose `tx_ring_full_drops`; sustained non-zero values are a production paging condition.

This is not "retransmission"; it is deliberate geographic redundancy across independent SWQoS identities to bypass intermediate loss without exhausting any single identity's stake allocation.



#### 4.4.3 Kernel bypass (where it matters)
- **Money workers:** MUST NOT invoke kernel networking syscalls.
- **Egress proxy (STAKED_BLAST):** **MUST** support a kernel‑bypass TX/RX backend (AF_XDP / DPDK / ef_vi). A Tier‑0 system that “wins in the lab” but uses `sendmsg()` on production egress is a latency hallucination; the Linux UDP stack + qdisc jitter is large enough to lose first‑price auctions.
  - The QUIC implementation MUST be compatible with a user‑space datagram backend (i.e., it can emit encrypted UDP payloads into a kernel‑bypass TX ring without blocking syscalls).
- **Ingress (shred capture):** **MUST** use kernel bypass (AF_XDP / DPDK) for raw Turbine UDP shred reception. The gRPC fallback path MAY use the kernel stack, but it is not the primary Tier-0 ingestion path (Section 4.4.1).
- **Fallback (dev/test only):** kernel sockets are allowed for proxy bring‑up and debugging, but any p99 latency measurements or “Tier‑0 readiness” claims are invalid unless the production bypass backend is enabled.

Net: the money path does math; proxy cores do networking.

---

## 5. State daemon (`shadow-state`)

### 5.1 Responsibility
`shadow-state` maintains fork‑aware, near‑real‑time views of:
- Raydium pool accounts required for quoting
- vault token account balances (or the data needed to derive reserves)
- mint metadata (decimals, extensions, program owner)
- a subscribed set of Address Lookup Tables (ALTs)
- **index maps and arenas** required by Tier‑0:
  - `/dev/shm/shadow_pool_index_map` + `/dev/shm/shadow_pool_state` (Section 5.4)
  - `/dev/shm/shadow_mint_index_map` + `/dev/shm/shadow_mint_state` (used by risk/invariants; Section 6.7)

### 5.2 Subscription scope (profit‑first)
State scope MUST be curated to what Tier‑0 needs:
- Required pools and their vaults for supported venues
- Mints that appear in those pools
- ALTs needed to decode those venue transactions for observed targets

Everything else is out of scope.

### 5.3 Fork handling
`shadow-state` MUST:
- track slot + parent
- rollback on fork switch: revert affected pool hot state in `/dev/shm/shadow_pool_state` by writing the inactive slot and flipping `active_idx` (double‑buffer publish; Section 4.2.1)
- publish a monotonic `snapshot_slot` in `/dev/shm/shadow_health` so workers can detect discontinuities and staleness

### 5.4 Publishing model for Raydium pool state (no per‑worker diffs; no seqlock spins)

`shadow-state` maintains canonical decoded pool state. Tier‑0 money workers MUST read pool state in O(1) with **no diff draining**, **no spinning**, and **no timeouts**.

### 5.4.1 Data structures (preallocated; dynamic append)
`shadow-state` MUST pre‑allocate:
- `PoolArena` of size `max_pools` (default 262,144).
- `PoolIndexMap` (open‑addressed, fixed capacity, power‑of‑two; load factor < 0.7).

**PoolArena entry layout (per pool index):**
- `active_idx: ActiveIdx128B` (contains `active_idx: AtomicU8` and padding; `#[repr(C, align(128))]` so the index resides on its own cache sector)
- `hot[2]: PoolHotState64B` where each slot is exactly 64 bytes and aligned:
  - `#[repr(C, align(64))] struct PoolHotState64B { ... }`

**Mandatory tear guard inside `PoolHotState64B`:**
- `sequence: AtomicU32` (or aligned u32 with atomic semantics)
  - Even = stable
  - Odd = writer is mutating this slot

**Suggested `PoolHotState64B` layout (exactly 64 bytes):**
- `sequence: u32` (4)
- `pool_pubkey: [u8; 32]` (32)  → total 36
- `reserve_in: u64` (8)          → 44
- `reserve_out: u64` (8)         → 52
- `last_update_unix_ns: u64` (8) → 60
- `flags_or_fee_bps: u32` (4)    → 64

Notes:
- The full `pool_pubkey` is stored in the hot slot to allow a tear‑free verification step by readers.
- Additional cold fields (program id, vault pubkeys, authority, etc.) MAY exist in a separate cold arena; Tier‑0 quoting MUST rely only on hot fields.

**Index map entry layout (8‑byte atomic tag; no torn pubkeys):**
- Store **only** an atomic 8‑byte payload in the open‑addressed probe table:
  - `tag: u64 = (pubkey_hash40 << 24) | (pool_index_plus_one_u24)`
  - `pubkey_hash40` MUST be derived without SipHash:
    - `h64 = u64::from_le_bytes(pool_pubkey[0..8])`
    - `pubkey_hash40 = (h64 >> 24) & ((1<<40) - 1)`
- Readers:
  - atomically load the 8‑byte `tag`
  - compare `pubkey_hash40`
  - if it matches, use `pool_index` and verify full `pool_pubkey` from the hot slot (Section 5.4.3).

### 5.4.2 Writer protocol (shadow-state)
For each pool update:
1. Resolve `pool_index`:
   - If known: use existing index.
   - If new pool detected: append to arena:
     - `pool_index = arena_tail.fetch_add(1)` (bounded by `max_pools`)
     - initialize both hot slots with a known pubkey + zero reserves and even sequences.
2. Publish hot state using **double buffer with sequence guard** (Section 4.2.1):
   - `active = active_idx.load(Relaxed)`
   - `inactive = active ^ 1`
   - `seq = hot[inactive].sequence.load(Relaxed)`
   - `hot[inactive].sequence.store(seq + 1 /* odd */, Release)`
   - write payload fields into `hot[inactive]` (plain writes)
   - `hot[inactive].sequence.store(seq + 2 /* even */, Release)`
   - `active_idx.store(inactive, Release)`
3. Only after the hot slot flip is published, publish / update the index map tag for this `pool_pubkey`:
   - This ensures a reader that resolves the pool index will always see a valid hot slot pubkey to verify.

Fork handling:
- On fork switch, `shadow-state` MUST republish prior valid pool state via the same double‑buffer flip protocol (no reader spinning; correctness via publish ordering).

### 5.4.3 Reader protocol (money worker)
Money workers MUST do:
1. O(1) index lookup in `PoolIndexMap` (bounded probe; default `MAX_PROBES = 8`).
2. If a candidate `pool_index` is found, verify full pubkey inside the hot slot:
   - load `active_idx` (Acquire)
   - load `seq1` (Acquire); if odd → drop
   - copy 64 bytes to local
   - `compiler_fence(Acquire)`
   - load `seq2` (Relaxed); if `seq1 != seq2` or odd → drop
   - verify `local.pool_pubkey == expected_pool_pubkey`
3. If verification passes, use `reserve_in`, `reserve_out`, `flags_or_fee_bps`, and `last_update_unix_ns` from the local copy.

**No retries. No spin loops.** If any check fails (map miss, sequence mismatch, pubkey mismatch, staleness breach), the worker MUST drop the intent.

### 5.4.4 Genesis / new pool guarantee (money-grade)
Adding a new pool MUST NOT require a config change or restart. The dynamic append + index map publish protocol above is mandatory to avoid missing genesis blocks.

## 5.5 Staleness and health
`shadow-state` publishes `/dev/shm/shadow_health`:
- `state_is_healthy: bool`
- `last_pool_update_unix_ns`
- `last_mint_update_unix_ns`
- `last_alt_update_unix_ns`
- `snapshot_slot: u64` (monotonically increasing; for fork discontinuity detection)

Tier‑0 workers MUST drop trades if:
- `state_is_healthy=false`, or
- `ingress_record.unix_ns − last_pool_update_unix_ns > max_pool_staleness_ns`, or
- required mint/alt snapshots are stale beyond thresholds.

Per‑pool staleness is also checked inline via each pool hot slot’s `last_update_unix_ns` (Section 5.4, reader side).


### 5.6 Mint arena and index map (dynamic; genesis-safe)
Tier‑0 risk and mint-invariant checks require stable O(1) indexing for **new mints** (genesis tokens).

`shadow-state` MUST maintain:
- `/dev/shm/shadow_mint_index_map`: lock-free fixed-capacity map `mint_pubkey -> mint_index` using the **same 8‑byte atomic tagging** scheme as pools (Section 5.4.2/5.4.3), but with mint keys.
  - `h64 = u64::from_le_bytes(mint_pubkey[0..8])`
  - `mint_hash40 = (h64 >> 24) & ((1<<40) - 1)`
  - `mint_index_plus_one_u24 = (mint_index as u32 + 1)` stored in 24 bits
  - Tag packing (normative): `tag = (mint_hash40 << 24) | mint_index_plus_one_u24`
  - Tag stored as atomic `u64` (Release by writer; Acquire by readers)
  - Full 32-byte mint pubkey lives only in `/dev/shm/shadow_mint_state[mint_index]` and is verified there (tear-safe).
- `/dev/shm/shadow_mint_state`: fixed-capacity mint arena/snapshot keyed by `mint_index` (read‑mostly snapshot; append‑only publish).

On first observation of a mint (from pool creation, swap decode prerequisites, or account updates), `shadow-state`:
1. assigns a new `mint_index` (atomic append),
2. populates the mint metadata entry (decimals, owner program, Token-2022 extensions relevant to hard rejects),
3. then publishes the `(mint_hash40, mint_index_plus_one_u24)` tag into the mint index map (Release).

Tier‑0 workers:
- look up `mint_index` via bounded probing on the 8‑byte tags (Acquire),
- verify the full 32‑byte mint pubkey in the mint arena (tear-safe),
- use `mint_index` for inventory fencing and mint hard-reject gates,
- and drop on miss (fail closed).


---

## 6. Tier‑0 worker pipeline (`shadow-tier0` per‑worker loop)

### 6.1 High‑level loop (run‑to‑completion money workers)

Tier‑0 consists of `N` pinned money‑worker threads. Each worker executes a strict run‑to‑completion loop: dequeue → decide → sign → enqueue for egress. No allocations, no locks, no syscalls on the worker core.

**Ingress sharding (mandatory, DOS‑resistant):**
- `N` (`entry.workers`) MUST be a **power‑of‑two** (4/8/16/…). This avoids a hardware DIV in routing.
- At `shadow-tier0` boot, generate a per‑process `route_salt_u64` (CSPRNG; **not** in YAML; **not** in shared memory).
- Route every decoded `IngressRecord` to exactly one worker **by pool pubkey** using hardware CRC32 + bitmask:
  - `h32 = crc32_u64(route_salt_u64, u64::from_le_bytes(pool_pubkey[0..8]))`  (x86: `_mm_crc32_u64`)
  - `worker_id = (h32 as usize) & (N - 1)`
- Result: all activity for a given pool/mintpair is serialized on one core, preventing “swarm self‑competition” and enabling thread‑local forward‑projection state (Section 8.4).


**Feed rings (mandatory):**
- Each money worker owns **P** ingress rings (one per ingress producer thread):
  - `P = shred_capture_workers + grpc_workers`
  - Rings: `/dev/shm/shadow_feed_ring_w{worker}_p{producer}`
- The worker MUST poll its rings round‑robin (or weighted by source priority) and process whichever ring has data available. No ring drain loops that can starve the hot loop.

### Worker loop (strict O(1))
For each fixed‑size `IngressRecord` dequeued from any of the worker’s feed rings:

1. **Thread‑local signature dedupe (0‑alloc):**
   - Compute `sig_tag64 = u64::from_le_bytes(signature[0..8])`.
   - Check a thread‑local, cache‑resident dedupe filter (e.g., fixed ring of the last K `sig_tag64` values).
   - If seen: drop immediately.

2. **Fast staleness gate (cheap):**
   - Drop if required caches are stale/missing: pool state, mint state, tip LUT.
   - Drop if the record’s `slot` is too old vs. the current rolling blockhash window.

3. **Pool/mint resolution (O(1)):**
   - Resolve `pool_index` via `shadow_pool_index_map` and verify full `pool_pubkey` via the hot slot (Section 5.4.3).
   - Resolve `mint_out_index` via `shadow_mint_index_map` if needed for inventory fencing.

4. **Effective pool state (wait‑free read + thread‑local dirty overlay):**
   - Read `PoolHotState64B` using the wait‑free sequence‑guarded protocol (Section 5.4.3).
   - Apply the worker’s **thread‑local dirty overlay** for this pool (if present and not invalidated):
     - The overlay represents the worker’s own sequential forward‑projection of reserves from previously emitted intents on this pool that have not yet been reflected in `shadow-state`.
     - Overlay is safe because **this pool is single‑threaded** (pool‑sharded) — no cross‑core writes.
     - Overlay invalidation rules (mandatory; no clock syscalls):
       - Overlay carries `base_state_unix_ns` copied from `pool.last_update_unix_ns` when the overlay is seeded/reset (do not update it on optimistic emits).
       - **TTL expiry (ghost‑pump protection; anchored to true chain state):** if `ingress.unix_ns − overlay.base_state_unix_ns > 200_000_000` (200 ms), clear the overlay (re‑seed from shared state).
       - **Shared‑state freshness:** if `pool.last_update_unix_ns > overlay.base_state_unix_ns`, reset overlay to the shared state and set `overlay.base_state_unix_ns = pool.last_update_unix_ns`.

   **Policy:** overlay is intentionally pessimistic (assume previous intents land and move reserves). This reduces self‑induced revert bleed under swarms.

5. **Forward projection + alpha LCB:**
   - Using the effective reserves, compute:
     - projected reserves after the target (T+1) swap,
     - our backrun execution amount,
     - `min_out` for the backrun (tight but revert‑tolerant),
     - `alpha_lcb_lamports` (lower‑confidence bound of net alpha).
   - If `alpha_lcb_lamports < min_alpha_lcb_lamports`: drop.

6. **Tip selection (O(1) LUT):**
   - Index the fractional LUT using shifted rounding (Section 6.6.1).
   - `tip = (alpha_lcb_lamports * tip_fraction_u32) >> 32`.
   - Apply per‑route caps/guards (never by fixed ATR ratios).

7. **Risk reservation (inventory fence; lock‑free):**
   - Reserve inventory against `shadow_inventory_fence[mint_out_index]` using the **optimistic `fetch_add` + immediate rollback** protocol (Section 6.7).
   - If the post‑add value breaches the limit, the worker MUST `fetch_sub` to roll back and drop the intent.

8. **Build transaction template (deterministic bytes):**
   - Select route based on `signal_source` and policy (Section 8.3).
   - Build the lane‑specific deterministic template (Section 7.1.1):
     - If lane is `PUBLIC_GEYSER`: compute `cu_price_micro_lamports = ((tip_lamports as u128 * RECIPROCAL_Q32 as u128) >> 32) as u64` and include `set_compute_unit_price(cu_price_micro_lamports)`.
     - If lane is a bundle lane (`JITO_SEARCHER_API` / `PRIVATE_MEMPOOL`): include the lane tip instruction if required (e.g., Jito tip transfer) using the `jito_tip_pubkey` read wait‑free from `/dev/shm/shadow_blockhash` (Section 7.3). The egress proxy MUST NOT mutate the tip account post‑sign.
     - In **all** lanes: prepend `ShadowGuard::AssertReserveOutInRange { min_reserve_out, max_reserve_out }` (range; Section 7.1.1) as the first non‑budget instruction.


9. **Sign (nonce precompute) + enqueue for egress:**
   - Pop `(r,R)` from the in‑process locked nonce ring (Section 8.2.1).
   - Compute signature `s` and serialize the tx.
   - Destroy `r` with optimizer‑resistant zeroization (zeroize/volatile write); `panic = "abort"` (Section 8.2.1.2).
   - Enqueue `SignedTxEnvelope` into the appropriate egress ring (drop‑on‑full).
   - Emit `ExecutionRecord` for the Position Manager (pending table).

10. **Update thread‑local pool dirty overlay:**
   - If a tx was emitted (even if it may revert), update the pool overlay to the **projected post‑sequence reserves** for this pool:
     - apply the target swap projection, then
     - apply our backrun swap projection.
   - Do **NOT** extend TTL based on optimistic emits. `overlay.base_state_unix_ns` remains anchored to the shared pool snapshot timestamp used to seed the overlay (Step 4).
   - This makes intent #2 project on top of intent #1 for the same pool, reducing self‑competition and priority‑fee burn during swarms.

Workers MUST never perform:
- ALT expansion,
- protobuf parsing,
- Jito/QUIC/TLS framing,
- syscalls for networking,
- any diff‑draining loop.

### 6.2 Supported decode set (strict O(1), performed on ingress proxy cores)
Tier‑0 MUST support only patterns with fixed parsing cost. In V6.2.9, strict decoding occurs on **ingress proxy cores** (Section 4.4.1) and is emitted as a fixed-size `IngressRecord`.

Supported patterns:
- Raydium V4 `swap_base_in` (primary)
- (Optional) Pump/PumpSwap direct swap, only if adapter provides O(1) decode

Unsupported patterns are dropped immediately by the ingress proxy:
- Jupiter multi‑hop, aggregators, unknown programs
- arbitrary CPI chains

The money worker MUST NOT attempt dynamic decoding. It only validates `(venue_id, pattern_id)` and consumes the decoded fields.



### 6.3 ALT resolution (performed on ingress proxy cores)
ALT expansion is variable-length work and MUST NOT occur on the money-worker core.

- The ingress proxy expands ALTs and emits a flattened account list in `IngressRecord` (Section 4.4.1).
- On ALT miss: the ingress proxy MUST drop the record.
- Tier‑0 money workers MUST NOT fetch ALTs and MUST NOT perform ALT expansion.

`shadow-state` remains responsible for maintaining the ALT snapshot (`/dev/shm/shadow_state_alts`) that the ingress proxy consults.



### 6.4 Quote + slippage (two‑phase forward projection)
Tier‑0 uses deterministic integer math (no floats). Quoting MUST account for the fact that this is a **backrun**: the target transaction executes before ours, moving the AMM bonding curve. Quoting against the pre‑trade pool state produces a `min_out` that is guaranteed to fail after the target lands — every successful backrun reverts, and the system bleeds priority fees at 100% revert rate.

**Two‑phase O(1) forward projection (mandatory):**

1. **Project post‑target state (O(1)):**
   - Using the cached pool state (`reserve_in`, `reserve_out`, fee numerator/denominator) and the target's decoded `amount_in` / `mint_in` from `IngressRecord`:
   - `target_out = (reserve_out × target_amount_in × (fee_denom − fee_num)) / (reserve_in × fee_denom + target_amount_in × (fee_denom − fee_num))`
   - `projected_reserve_in  = reserve_in  + target_amount_in`
   - `projected_reserve_out = reserve_out − target_out`
   - This is the constant‑product AMM formula (Raydium V4); other venues substitute their own O(1) curve math. All arithmetic is integer (u128 intermediates to prevent overflow).

2. **Quote our backrun against the projected state (O(1)):**
   - `expected_out = (projected_reserve_out × our_amount_in × (fee_denom − fee_num)) / (projected_reserve_in × fee_denom + our_amount_in × (fee_denom − fee_num))`
   - Set `min_out` from `expected_out` using a conservative slippage buffer to absorb:
     - state staleness (our cached reserves may lag by one or two updates),
     - rounding differences between our integer math and the on‑chain program's integer math,
     - concurrent trades landing between the target and our backrun.

**Slippage buffer policy:**
- `min_out` MUST be tight enough to prevent "copy into toxic dump" scenarios (buying into a rug where our `expected_out` is stale garbage).
- `min_out` MUST NOT be so tight that normal pool variance causes universal reverts.
- Recommended default: `min_out = expected_out × (10_000 − slippage_bps) / 10_000` where `slippage_bps` is configurable (default 50–150 bps depending on venue and pool liquidity tier).

**Invariant:** Both phases are O(1) arithmetic with no branching, no iteration, and no network I/O. The two multiplies + divides add < 100 ns total.

### 6.5 Expected profit model input
Tier‑0 reads from shared memory (all published by `shadow-oracle`):
- `/dev/shm/shadow_oracle_targets`: per‑entity `Alpha_LCB` estimate (conservative), per‑entity caps and blacklist/whitelist signals
- `/dev/shm/shadow_tip_lut`: pre‑computed optimal tip LUT (Section 6.6; O(1) array index replaces all tip math on hot path)

Tier‑0 NEVER calls the oracle directly.

### 6.6 Tip selection (O(1) LUT with correct rounding and whale scaling)

Tier‑0 tip selection MUST be constant‑time and must not clamp away the convex tail.

#### 6.6.1 Fix truncation bias (bucket rounding)
Bucket selection MUST use **shifted rounding**, not truncation:

```
bucket = min((alpha_lcb + (1 << (BUCKET_SHIFT - 1))) >> BUCKET_SHIFT, lut_size - 1)
```

This prevents systematic under-bidding near bucket boundaries.

#### 6.6.2 Store fractions, not absolute lamports (u32 precision)
The LUT MUST store a **tip fraction** (fixed-point), not an absolute lamport value.

- `tip_lut[bucket] = tip_fraction_u32`
- `tip_fraction_u32 ∈ [0, 4_294_967_295]` where `4_294_967_295 = 100% of alpha_lcb`

Hot path computes:

```
tip = (alpha_lcb as u128 * tip_fraction_u32 as u128) >> 32
```

**Why u32, not u16:** A u16 fraction (65535 = 100%) forces a step size of `alpha / 65536`. For a 10 SOL alpha (~10B lamports), this yields ~152,000 lamport granularity. Jito bundle tie-breakers are frequently resolved by single-digit lamports. A competitor using finer precision outbids us by 1 lamport and captures the full alpha. The u16 "saves" zero clock cycles — on x86_64, `MUL r64` multiplies two 64-bit operands and stores the 128-bit result in `RDX:RAX`; a `>> 32` shift is simply reading the upper 64 bits from `RDX`. The cost is identical to a u16 multiply + `>> 16` shift, but u32 provides lamport-perfect granularity for any realistic alpha.

This keeps O(1) execution while ensuring out-of-distribution "whale" alpha automatically scales even when `bucket` is clamped to `lut_size-1`.

#### 6.6.3 Oracle responsibilities (off hot path)
`shadow-oracle` computes `tip_fraction_u32` per bucket by optimizing conservative NetEV under the desk's auction model:

`NetEV(tip) = p_win(tip_per_cu) × (alpha_lcb − auction_cost(tip, cu_limit))`

The oracle may impose regime-aware caps, but those caps are expressed as fractions (so scaling is preserved).

**Invariant:** Tier‑0 MUST NOT run any search or optimization for tips. It performs at most:
- one bucket computation,
- one LUT load,
- one 64×32 multiply + shift (single `MUL r64` instruction on x86_64).

#### 6.6.4 PUBLIC_GEYSER priority‑fee unit conversion (micro‑lamports/CU)
The tip LUT outputs an **absolute lamport budget** (`tip_lamports`) for an intent.

For `signal_source == PUBLIC_GEYSER` (Section 8.3, Case 1), that lamport budget is spent as a Solana **priority fee** via `ComputeBudget::set_compute_unit_price`, whose unit is **micro‑lamports per Compute Unit (CU)**.

**Unit rule (mandatory):** a lamport budget MUST NOT be passed directly as `set_compute_unit_price`.

Because Tier‑0 uses audited, fixed CU templates, `template_cu_limit` is a constant per template. Precompute at boot/compile‑time a **fixed‑point reciprocal** (one‑time DIV off‑path):

- `RECIPROCAL_Q32 = ceil((1_000_000 << 32) / template_cu_limit)`

Hot path (no division; exact CU pricing; no padding waste):

```
cu_price_micro_lamports = ((tip_lamports as u128 * RECIPROCAL_Q32 as u128) >> 32) as u64
```

**Overflow safety (mandatory):** the multiplication MUST be performed in 128‑bit space (`u128`) before shifting/casting back to `u64`, otherwise whale‑scale tips can overflow `u64` and silently wrap in release builds.

This yields a total priority fee of ≈ `tip_lamports` under the runtime rule `fee_lamports = (cu_price_micro_lamports × template_cu_limit) / 1_000_000`.

**No modulo constraint:** `template_cu_limit` MUST be the minimum audited CU limit for the template. It does **not** need to divide `1_000_000` (prevents bid‑density dilution).

### 6.7 Risk gating (O(1), atomic inventory fence)

**Problem with read‑only snapshot gating (eliminated):**
The prior model had N workers read a read‑only `/dev/shm/shadow_risk_snapshot` to check position limits. If the max position for a mint is 500 SOL and 5 workers spot the same opportunity, all 5 read "Position: 0, Limit: 500", all 5 pass the gate, all 5 fire. Result: 2500 SOL position, 5× limit breach, panic exit, strategy bankruptcy.

**Atomic inventory fence (mandatory):**
`/dev/shm/shadow_inventory_fence` is a cache‑line‑aligned array of atomic `i64` entries, **one per mint index**.
- `mint_index` is obtained via a wait‑free lookup in `/dev/shm/shadow_mint_index_map` (maintained by `shadow-state`).
- Each entry represents the **current committed position in lamports** for that mint.
- Fence entries MUST be **128B‑strided** (prefetcher‑safe) to prevent false sharing.

**Why a CAS spin loop is not acceptable (latency suicide under contention):**
A bounded `compare_exchange_weak` loop with `_mm_pause()` forces trailing workers to spin for up to 20 µs waiting for the cache line to quiesce. During a swarm where 8 workers hit the same mint's fence entry, 7 workers stall on L3 bus arbitration — sacrificing 20,000 ns of deterministic execution to prevent a counter from theoretically exceeding a limit for ~3 clock cycles. The CAS contention cost dwarfs the phantom overshoot it prevents.

**Risk check protocol (per worker, O(1), fail‑fast):**
1. Read risk limits from `/dev/shm/shadow_risk_snapshot` (read‑only; still published by `shadow-pm`):
   - `max_position_lamports_per_mint`
   - `max_notional_lamports_per_entity`
   - `global_kill_switch: bool`
   - `allowed_venues`
2. If `global_kill_switch == true` → drop immediately.
3. Compute `trade_size_lamports` for this intent.
4. Reserve inventory using **optimistic `fetch_add` + immediate rollback**:
   - `prev = fence[mint_index].fetch_add(trade_size_lamports, AcqRel)`
   - `new_val = prev + trade_size_lamports`
   - if `new_val > max_position_lamports_per_mint`:
     - **immediately** `fence[mint_index].fetch_sub(trade_size_lamports, AcqRel)` (rollback)
     - drop with `RISK_LIMIT_EXCEEDED`
   - otherwise: reservation succeeded, proceed.

   **Why this is correct:** the transient overshoot window (between `fetch_add` and the rollback `fetch_sub`) lasts ~3 clock cycles. During that window, a concurrent worker checking the same mint sees a value above the limit and drops — a **false negative**. This is strictly preferable to a 20 µs CAS spin stall (false positive for latency) because:
   - The overshoot is bounded to exactly `trade_size_lamports` above the limit, for at most one concurrent check.
   - The rollback is unconditional and immediate — no retry loop, no spin, no `_mm_pause()`.
   - In the worst case, one worker in a burst falsely drops a valid trade. In the CAS model, all trailing workers deterministically stall for thousands of nanoseconds, losing the latency auction.
   - At Tier‑0 speeds, a ~3‑cycle phantom rejection is infinitely cheaper than a 20 µs L3 bus stall.

5. After a successful reservation, proceed to build/sign/send.

**Rollback on send failure or revert:**
- If the worker drops the intent after reserving (template failure, deadline exceeded, egress ring full, etc.), it MUST `fetch_sub(trade_size_lamports)` to release the reservation before returning to polling.
- `shadow-pm` periodically reconciles `shadow_inventory_fence` against confirmed on‑chain position state. Reconciliation frequency: configurable (default every 500 ms).

  **Why blind `store(reconciled_value)` is forbidden (in‑flight reservation erasure):**
  A blind atomic store overwrites the fence with the chain‑confirmed position, silently erasing any in‑flight reservations that money workers accumulated between the PM's chain query and its write. Example: PM reads chain position = 100 SOL. Before the store lands, a worker CAS‑reserves +10 SOL (fence becomes 110). PM then executes `store(100)`, destroying the reservation. The next worker now sees headroom that doesn't exist and fires past the limit.

  **Delta‑based reconciliation protocol (mandatory):**
  1. PM snapshots `settled_position` from its own fill‑tracking database (the last value it knows was written to the fence during the previous reconciliation cycle).
  2. PM queries confirmed on‑chain position → `chain_position`.
  3. Compute `drift = chain_position − settled_position` (signed; drift may be positive from confirmed fills or negative from reverted trades / missed rollbacks).
  4. Apply drift atomically: `fence.fetch_add(drift, AcqRel)` (if `drift > 0`) or `fence.fetch_sub(drift.unsigned_abs(), AcqRel)` (if `drift < 0`).
  5. Update `settled_position = chain_position` in PM's local state for the next cycle.

  This preserves all in‑flight worker reservations accumulated since the last reconciliation. The fence converges toward truth without ever blindly overwriting concurrent state.

  **Why delta reconciliation alone is insufficient (revert blindness):**
  Delta reconciliation compares `chain_position` to `settled_position`. When a trade **reverts** on‑chain (slippage failure, CU exhaustion, etc.), the chain state does not change — the transaction is rolled back as if it never executed. Therefore `chain_position` remains unchanged, `drift = 0`, and the fence **permanently retains** the reservation for a trade that will never fill. During contested swarms with high revert rates, this silently exhausts inventory limits within minutes, starving the system of capital headroom.

  **Revert‑aware signature tracking (mandatory):**
  `shadow-pm` MUST actively track the terminal status of every signature the system submits and release leaked reservations when reverts are confirmed:

  1. **Ingest execution records:** PM subscribes to the `ExecutionRecord` log stream (drained by the `shadow-tier0` background thread — Section 6.8). For each record, PM stores `(signature, mint_out_index, trade_size_lamports, send_unix_ns)` in a **pending‑confirmation table** keyed by signature.

  2. **Poll signature status:** PM queries transaction status via RPC (`getSignatureStatuses`) or Geyser transaction notifications for all pending signatures. This is off the money‑worker path (PM runs on its own cores).

  2b. **Jito BundleState fast‑release (mandatory):** For `route_taken == JITO_BUNDLE`, failures often occur off‑chain in the Block Engine simulator and will never appear in RPC/Geyser. The egress proxy MUST subscribe to Jito `BundleState` updates and emit terminal outcomes (`Rejected` / `Dropped` / `NotLanded`) to PM. On receipt, PM MUST immediately treat the tx as terminal failure and release the reservation (Step 4), without waiting for `max_pending_age_s`.

  3. **On confirmed success (landed):** PM MUST treat a landed tx as terminal and clear the worker’s in‑flight reservation:

     - Execute `fence[mint_out_index].fetch_sub(trade_size_lamports, AcqRel)` to release the reservation.
     - Remove from pending table.

     The landed fill will be reflected in `chain_position` and will be re‑applied to the fence by the next delta reconciliation cycle as settled truth.

  4. **On confirmed failure (reverted / dropped):** PM MUST immediately execute `fence[mint_out_index].fetch_sub(trade_size_lamports, AcqRel)` to release the leaked reservation. Remove from pending table.

  5. **On expiry (blockhash expired, no status after `max_pending_age`):** treat as reverted — execute the same `fetch_sub` release. Default `max_pending_age`: 2.0 seconds (≈ 3–5 slots). If not landed by then, release capital aggressively and let reconciliation correct later.

  6. **Consistency invariant:** at steady state, `fence[mint] = confirmed_on_chain_position + sum(in_flight_reservations)`. Any deviation is a bug and MUST trigger an alert.

  **Ordering:** revert‑driven `fetch_sub` releases and delta‑based drift corrections are both applied to the same atomic fence entries. Both use `AcqRel` ordering. They are safe to run concurrently because they are additive adjustments (never blind stores). The fence converges from both directions: delta reconciliation corrects for confirmed fills, revert tracking corrects for failed sends.

**Per‑entity notional cap:**
- Per‑entity notional limits remain in `/dev/shm/shadow_risk_snapshot` (read‑only) and are checked against the entity's cumulative notional from oracle targets. This check is still read‑only because per‑entity exposure changes slowly (oracle‑driven) and a slight overshoot is tolerable — unlike per‑mint inventory.
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

#### 7.1.1 `ShadowGuard`‑guarded backrun templates (mandatory for all routes)
Tier‑0 backruns are state‑dependent, and **bundle atomicity does not guarantee upstream state change** (targets can gracefully catch slippage and still return `Ok()`).

Therefore, `ShadowGuard` MUST be included for **all** Tier‑0 backruns: `PUBLIC_GEYSER` TPU blasts, Jito bundles (`JITO_SEARCHER_API`), and private mempool lanes.

`ShadowGuard` is a minimal on‑chain assertion program (no routing, no CPI). It aborts the transaction **before** the swap if the pool state is outside the worker’s pre‑computed acceptable range for the upstream state transition.

Instruction order (templates are audited and fixed‑shape):

**A) `PUBLIC_GEYSER` (standalone TPU/QUIC blast):**
1. `ComputeBudget::set_compute_unit_limit(template_cu_limit)`
2. `ComputeBudget::set_compute_unit_price(cu_price_micro_lamports)`  (Section 6.6.4)
3. `ShadowGuard::AssertReserveOutInRange { min_reserve_out, max_reserve_out }`  ← Dual‑bound range guard (Section 7.1.1)
4. DEX swap instruction (fixed template) with strict `min_out`

**B) Bundle lanes (`JITO_SEARCHER_API` / `PRIVATE_MEMPOOL`):**
1. `ComputeBudget::set_compute_unit_limit(template_cu_limit)`
2. `ShadowGuard::AssertReserveOutInRange { min_reserve_out, max_reserve_out }`  ← Dual‑bound range guard
3. DEX swap instruction (fixed template) with strict `min_out`
4. Lane tip instruction (e.g., Jito tip transfer) if required by the lane, using the **preselected** Jito tip account published alongside the blockhash (Section 7.3)

**Pool‑State Guard contract (normative):**
- Program: `ShadowGuard` (minimal on‑chain assertion program; **no routing**, **no CPI**).
- Account (read‑only):
  - `reserve_out_vault`: SPL token account for the vault of the asset our backrun extracts as output (the swap’s output vault).
- Instruction variant (single discriminator; minimal program):
  - `AssertReserveOutInRange { min_reserve_out: u64, max_reserve_out: u64 }`
- Semantics:
  - `current = reserve_out_vault.amount`
  - require `min_reserve_out <= current <= max_reserve_out`, else revert.
- Bound computation (mandatory; off‑chain; 0 extra hot‑path ops):
  - Worker computes `expected_reserve_out_after_target` via the O(1) forward projection (Section 6.1 Step 5).
  - The two bounds serve **two distinct purposes**:
    - **Execution‑proof bound** (prevents the exit‑liquidity trap when the upstream target no‑ops/reverts):
      - If the upstream transition is expected to **decrease** `reserve_out_vault.amount` (e.g., copy‑trade buys, or any case where proving execution requires the output reserve to shrink), set a **ceiling**: `max_reserve_out ≈ expected_reserve_out_after_target (+ epsilon)`.
      - If the upstream transition is expected to **increase** `reserve_out_vault.amount` (proving execution requires the output reserve to grow), set a **floor**: `min_reserve_out ≈ expected_reserve_out_after_target (− epsilon)`.
    - **Sandwich / negative‑slippage bound** (prevents buying into pathological adverse movement that still satisfies the execution‑proof inequality):
      - For decrease‑direction proofs (`reserve_out` should shrink), set a **floor** `min_reserve_out` such that reserves cannot be *too* depleted (protects against sandwich pumps / “buying the top”).
      - For increase‑direction proofs, the default is to **not cap positive movement**: set `max_reserve_out = u64::MAX` unless an explicit strategy‑level ceiling is configured.
  - Bounds MUST include rounding/epsilon margin sufficient to avoid false reverts from integer AMM math, but MUST remain tight enough to prevent exit‑liquidity and sandwich traps.

**Rationale (profit):**
- Directional single‑bound guards prove upstream execution but can still execute into adversarial sandwiches (e.g., reserve collapses far below projection).
- A dual‑bound guard prevents both:
  - *Upstream no‑op* (target didn’t move state but our standalone tx would still execute), and
  - *Pathological adverse movement* (state moved “too far” in the harmful direction before we land).

### 7.2 Compute unit limits
Compute unit limit is part of bid density in builders. Therefore:
- templates MUST use the smallest safe CU limit for the instruction set
- calibration occurs off hot path; new template limits are rolled out explicitly

Tier‑0 MUST NOT run per‑intent simulation to choose CU.

For `PUBLIC_GEYSER` templates that express spend via `set_compute_unit_price`, `template_cu_limit` MUST be the minimum audited CU limit for the template (no padding). The lamport→micro‑lamport/CU conversion uses a fixed‑point reciprocal (Section 6.6.4); **no divisor constraint** exists.

### 7.3 Blockhash + Jito tip account snapshot (double‑buffered with tear detection)

Tier‑0 MUST maintain a rolling recent blockhash snapshot for transaction construction. Blockhash reads occur on the money worker hot path; therefore the blockhash MUST be published via the same wait‑free double‑buffer protocol used for pool state.

Additionally, **Jito bundles require a valid Jito tip account**, and the chosen tip account is part of the signed transaction bytes. The system MUST NOT “patch” the tip account after signing (that invalidates the signature). Therefore the same wait‑free snapshot MUST also publish the **preselected** tip account pubkey for bundle lanes.

### 7.3.1 Data structure
`/dev/shm/shadow_blockhash` contains:
- `active_idx: ActiveIdx128B` (contains `active_idx: AtomicU8` and padding; `#[repr(C, align(128))]` so the index resides on its own cache sector)
- `slot[2]: BlockhashSlot128B` (each exactly 128 bytes, `#[repr(C, align(128))]`)

`BlockhashSlot128B` MUST include:
- `sequence: AtomicU32` (even = stable, odd = writer mutating)
- `blockhash: [u8; 32]`
- `jito_tip_pubkey: [u8; 32]`  (one of the currently active Jito tip accounts)
- `blockhash_slot: u64`
- `last_update_unix_ns: u64`
- remaining bytes reserved/padded to 128

### 7.3.2 Writer protocol (background updater thread)
A background thread (NOT a money worker) refreshes:
- the recent blockhash (RPC), and
- the set of currently active Jito tip accounts (from a cold‑path source; e.g., Searcher API bootstrap + periodic refresh).

On each publish, the writer MUST select **one** tip pubkey from the active set (8 accounts) using a cheap local RNG (e.g., xorshift) and write it into the inactive slot alongside the blockhash.

Publish protocol (same sequence guard; no blocking):
1. `active = active_idx.load(Relaxed)`
2. `inactive = active ^ 1`
3. `seq = slot[inactive].sequence.load(Relaxed)`
4. `slot[inactive].sequence.store(seq + 1 /* odd */, Release)`
5. Write the payload into `slot[inactive]` (plain writes).
6. `slot[inactive].sequence.store(seq + 2 /* even */, Release)`
7. `active_idx.store(inactive, Release)`

### 7.3.3 Reader protocol (money worker; wait‑free; no spins)
Money workers MUST read the active slot using the sequence guard:

1. `active = active_idx.load(Acquire)`
2. `seq1 = slot[active].sequence.load(Acquire)`; if odd → drop intent
3. Copy 128 bytes into a stack local
4. `compiler_fence(Acquire)`
5. `seq2 = slot[active].sequence.load(Relaxed)`; if `seq1 != seq2` or odd → drop intent
6. Use the local `blockhash` for transaction build
7. For bundle lanes, copy the local `jito_tip_pubkey` into the template **before hashing and signing** (no post‑sign mutations).

Policy:
- No retries. No waiting.
- If blockhash is missing/stale, the worker MUST drop the intent.
- If `jito_tip_pubkey` is missing/uninitialized and the route selected is a bundle lane, the worker MUST drop the intent (do not guess).

## 8. Signing and sending (inline in `shadow-tier0` worker loop)

### 8.1 Signing authority
Every `shadow-tier0` worker thread holds a read‑only copy of the hot wallet Ed25519 private key in thread‑local storage (Section 2.1). There is no separate signing process. The key is loaded once at boot from an encrypted keyfile and cloned per worker.

### 8.2 Inline build → sign → enqueue (send is on proxy cores)
Within each money worker’s run‑to‑completion loop (Section 6.1):
- **Build**: populate template transaction with trade parameters, blockhash (wait‑free double‑buffer read from `/dev/shm/shadow_blockhash`; Section 7.3), and fee/tip fields. Zero allocations.
- **Sign**: Ed25519 sign the serialized message bytes using precomputed nonce material (Section 8.2.1).
- **Enqueue**: write a `SignedTxEnvelope` into `/dev/shm/shadow_egress_ring_*`. The egress proxy performs QUIC/Jito framing and transmission (Section 4.4).

Money workers do not run TLS/QUIC, do not encrypt, and do not perform network syscalls.

### 8.2.1 Asynchronous Ed25519 nonce precomputation (mandatory for <150 µs targets)
Ed25519 signing time is dominated by computing the curve point `R = r·B`. V6.2.9 removes this work from the money-worker loop by precomputing single-use nonce material on dedicated cores.

**Critical security rule:** the secret scalar nonce `r` MUST NEVER be placed in `/dev/shm` (tmpfs) or any other OS-visible IPC segment. If `r` leaks for any signed message, the private key can be recovered from the public signature.

#### 8.2.1.1 Nonce rings are in-process secret memory (not IPC)
Because crypto precompute threads and money workers live inside the same `shadow-tier0` process, nonce rings MUST be allocated as **anonymous, private memory** and locked into RAM:

- Allocate with `mmap(MAP_ANONYMOUS | MAP_PRIVATE)`.
- Lock pages with `mlock()` (no swap).
- Mark as non-dumpable: `madvise(MADV_DONTDUMP)` and set `RLIMIT_CORE=0` (or `prctl(PR_SET_DUMPABLE, 0)`).
- Rings MUST NOT be backed by tmpfs, files, memmaps of files, or `/dev/shm`.

#### 8.2.1.2 Producer/consumer behavior
- A pinned **crypto precompute thread** continuously generates single-use nonce material and pushes it into the in-process SPSC nonce ring for each worker (or worker-group).
- Each ring element contains at minimum:
  - `r_scalar` (secret scalar nonce; **MUST be zeroed after use**)
  - `R_encoded` (32-byte encoded group element)
  - `nonce_id` (monotonic; for debugging/anti-reuse)

Money-worker signing becomes:
- pop `(r, R)` from the ring,
- compute `h = H(R, A, M)` (SHA‑512),
- compute `s = r + h·a (mod L)` (fast scalar arithmetic),
- output signature `(R, s)`,
- immediately destroy `r_scalar` in-place using **optimizer‑resistant zeroization** (Section 0.5.1). Acceptable implementations include `libc::explicit_bzero()` (FFI) or the Rust `zeroize` crate. Normal overwrites are forbidden.
- after zeroizing, advance the consumer cursor; the ring slot MUST NOT retain usable secret material longer than necessary.

#### 8.2.1.3 Correctness and safety requirements
- Nonce material MUST be single-use. Reuse is catastrophic; enforce by construction (SPSC ring, no reuse, hard fail on duplicate `nonce_id`).
- The precompute thread MUST use a CSPRNG (OS `getrandom()` is acceptable on the precompute core). The money-worker thread MUST NOT call RNG syscalls.
- If the nonce ring is empty, the money worker MUST drop the intent (fail fast) rather than fall back to slow signing or blocking waits. Ring sizing must make empties practically impossible in steady-state.



### 8.3 Signal‑source‑aware multi‑path routing (mandatory)

Solana has no global public mempool. A target transaction observed on a public Geyser stream is already propagating natively to the leader's banking thread. Wrapping it into a Jito bundle adds ≥ 10–30 ms (Block Engine network hop + simulation); the raw target lands before the bundle is validated, and the bundle is rejected (duplicate signature). Routing MUST therefore be determined by `signal_source` (tagged at ingestion; Section 6.1), not by a blanket "bundle‑first" policy.

Money workers only **select** the route and enqueue an egress envelope; egress proxy threads perform QUIC/TLS or Jito protocol transmission (Section 4.4).

**Case 1 — `PUBLIC_GEYSER` (target already in flight):**
- Jito bundle wrapping is **forbidden**. The race against a natively propagating transaction is unwinnable with bundle overhead.
- Route: **direct staked QUIC/TPU blast** to the current and next leader's TPU ports via SWQoS‑aware staked connections.
- Fee strategy: allocate spend entirely as **priority fee** via `ComputeBudget::set_compute_unit_price(cu_price_micro_lamports)` (unit = micro‑lamports/CU). `cu_price_micro_lamports` MUST be derived from the lamport budget using the template’s fixed CU limit (Section 6.6.4). No Jito tip instruction (avoids tip donation to non‑Jito leaders).
- The transaction is a **standalone TPU blast** attempting to land sequentially after the target by virtue of high priority fee and fast delivery.

**Mandatory safety (ShadowGuard):**
- `ShadowGuard` is mandatory for **all** Tier‑0 lanes (Section 7.1.1).
- For `PUBLIC_GEYSER` standalone blasts (non‑bundle), `ShadowGuard` prevents the “naked backrun exit‑liquidity” failure mode where the upstream target aborts/no‑ops but our backrun still executes.
- The backrun template MUST include, as the first non‑budget instruction:
  - `ShadowGuard::AssertReserveOutInRange { min_reserve_out, max_reserve_out }` (range; Section 7.1.1)
- `min_reserve_out` / `max_reserve_out` are derived from the worker’s forward projection of the upstream state transition and include epsilon/rounding safety margin. `max_reserve_out` serves as the execution‑proof ceiling when proving a decrease‑direction transition; `min_reserve_out` serves as the execution‑proof floor when proving an increase‑direction transition, and as the sandwich/negative‑slippage floor when proving a decrease‑direction transition (Section 7.1.1).


Operational notes:
- Bounded retries with fee bumping; strict rate limits.
- Egress proxy MUST use "kamikaze" QUIC configuration and MAY fan out via distinct SWQoS relay identities (Section 4.4.2.1–4.4.2.2).

**Case 2 — `JITO_SEARCHER_API` (target held in Jito's pipeline):**
- The target is orderflow inside Jito's block‑building pipeline; it is not propagating publicly.
- Route: **Jito bundle** (target tx + our backrun tx), submitted to the Jito Block Engine.
- Fee strategy: allocate spend as **Jito bundle tip**, respecting configured minimum.
- **Tip account rotation (mandatory):** the money worker MUST read `jito_tip_pubkey` from the blockhash snapshot (Section 7.3) and build the tip transfer to that account **before hashing and signing**. The egress proxy MUST NOT patch tip accounts post‑sign.
- **Capital release (mandatory):** egress MUST subscribe to the Jito `BundleState` stream. On `Rejected` / `Dropped` / `NotLanded`, egress MUST emit an immediate terminal event to `shadow-pm` so the inventory reservation is released without waiting for RPC timeouts (Section 6.7).
- The backrun tx MUST include `ShadowGuard::AssertReserveOutInRange { min_reserve_out, max_reserve_out }` (range; Section 7.1.1). Bundle atomicity does not protect against upstream no‑op success.
- This is the only scenario where bundle backrunning is physically viable for copy‑trading.
- No TPU fallback: if the Jito leader slot passes, the opportunity is lost.

**Case 3 — `PRIVATE_MEMPOOL` (timing uncertain):**
- The target arrived via a private feed; timing exclusivity is not guaranteed.
- Route: **configurable** via `submit.private_mempool_strategy`:
  - `staked_blast` (default): treat as Case 1 (staked QUIC/TPU with priority fee).
  - `jito_bundle_if_compatible`: attempt Jito bundle only if the current/next leader is Jito‑compatible AND `submit.min_jito_timing_margin_ms` (default 15) is satisfied. Otherwise fall back to staked blast.
- Fee strategy follows the selected route.
- If the selected route is `JITO_BUNDLE`, egress MUST apply the same **BundleState‑driven fast release** as Case 2 (Section 6.7).
- Regardless of route selection, the backrun tx MUST include `ShadowGuard::AssertReserveOutInRange { min_reserve_out, max_reserve_out }` (range; Section 7.1.1).

**Relay fanout constraints (per signal source):**
- `PUBLIC_GEYSER`: fan out via staked relays / TPU only (Group B). Jito bundle endpoints forbidden.
- `JITO_SEARCHER_API`: fan out via Jito Block Engine endpoints only (Group A). Standalone TPU blast forbidden (target is not public).
- `PRIVATE_MEMPOOL`: follows the selected route's group rules.

**Dual‑variant racing** (broadcasting both Jito and non‑Jito variants for the same intent) remains **forbidden** by default (double‑buy risk / signature collision).

### 8.4 Duplicate prevention (pool‑sharded routing + thread‑local signature dedupe)

Duplicates happen. A Tier‑0 intent can be observed multiple times:
- via shred capture (UDP) and gRPC (TCP),
- via multiple relays/peers in the Turbine tree,
- via overlapping upstream providers.

The system MUST prevent duplicate processing **without** a global shared Exec Fence (no cross‑core CAS on a single address).

### 8.4.1 Mandatory ingress routing invariant (no broadcast)
Ingress MUST route each decoded `IngressRecord` to **exactly one** money worker by **pool pubkey**:

- `N_workers` MUST be a **power‑of‑two** (4/8/16/…).
- All ingress producers in the process share a single boot‑time `route_salt_u64` (CSPRNG; not in config).
- Compute worker assignment with CRC32 + bitmask (no DIV; salt makes mapping non‑deterministic to external adversaries):
  - `h32 = crc32_u64(route_salt_u64, u64::from_le_bytes(pool_pubkey[0..8]))`
  - `worker_id = (h32 as usize) & (N_workers - 1)`
- Enqueue only into that worker’s ring for this ingress producer:
  - `/dev/shm/shadow_feed_ring_w{worker_id}_p{producer_id}`

**Why pool sharding is mandatory (profit rationale):**
- During swarms, many different targets buy the same token in the same slot.
- If those intents are processed on different cores, each core forward‑projects on stale shared state and the desk competes against itself, paying priority fees for guaranteed reverts.
- Pool sharding serializes all activity for the pool on one core and allows thread‑local dirty reserve projection (Section 6.1), reducing self‑revert bleed.

### 8.4.2 Thread‑local signature dedupe (0 ns shared contention)
Each money worker MUST maintain a tiny fixed‑capacity, L1‑resident dedupe filter keyed by the target signature:

- `sig_tag64 = u64::from_le_bytes(signature[0..8])`
- If `sig_tag64` is already present in the local window: drop the intent.

Because duplicates of the same target signature necessarily touch the same pool for supported patterns, pool sharding guarantees that duplicates converge on the same worker, making thread‑local dedupe sufficient.

No global Exec Fence exists in V6.2.9.

### 8.5 Reserve floor (no "poverty engineering")
`shadow-tier0` MUST enforce a **native SOL reserve floor** sufficient for:
- base fees
- sustained tip/priority fee activity under normal operations

Default requirement:
- `reserve_floor_sol = 50 SOL`
- alert at `25 SOL`
- if below floor: disable new entries (exits still allowed) until replenished.

Workers MUST NOT mutate exit transaction shapes to "self‑fund" tips.

### 8.6 Revert-cost breaker (priority‑fee bleed prevention; wired to PagerDuty)
Solana charges **priority fees even when a transaction reverts**. `ShadowGuard` protects principal (prevents toxic fills), but it does not prevent **fee bleed** on repeated upstream failures (e.g., retail targets with terrible slippage).

Therefore, `shadow-tier0` MUST run a revert‑cost breaker that:
- detects deterministic revert streaks quickly,
- temporarily suppresses trading the offender (pool/target),
- and **pages on‑call immediately**.

`shadow-tier0` background thread tracks (aggregated across all workers and lanes):
- landed rate
- revert rate
- lamports spent per landed trade
- priority fee spend per slot (PUBLIC lanes)
- bundle tip spend per slot (bundle lanes)
- tip/CU efficiency vs win probability

#### 8.6.1 Consecutive‑revert breaker (default extremely tight)
Maintain per‑pool (and optionally per‑entity) counters for **standalone** `PUBLIC_GEYSER` blasts:
- `consecutive_reverts`: increment on any terminal failure that consumes fees (including `ShadowGuard` abort, swap revert, `min_out` failure); reset on landed success.
- Default trigger: `consecutive_reverts > 3`.

On trigger:
1. Write a **temporary blacklist** entry (pool_pubkey and/or target entity) with a short TTL (e.g., 30–120 seconds). Ingress proxies MUST consult this blacklist **before** any heavy decode work and drop early.
2. Signal the oracle to reduce aggressiveness for this pool/entity (e.g., lower the tip fraction cap in the next LUT recomputation).
3. **Page on‑call (PagerDuty) immediately** with:
   - pool_pubkey / mint
   - route (`PUBLIC_GEYSER`)
   - consecutive revert count
   - estimated lamports burned in the streak

Rationale: a tight breaker prevents “revert‑bleed bankruptcy” in the standalone TPU lane where upstream targets frequently fail and each revert still pays priority fees.

#### 8.6.2 Adaptive (non‑emergency) tuning
Breaker activates only when:
- revert rate is high **and**
- auction spend is high **and**
- reverts correlate with deterministic mismatch (e.g., consistently failing due to stale state)

When activated:
- tighten freshness requirements
- lower the aggressiveness of spend (oracle LUT)
- or temporarily disable the offending target/entity

Breaker MUST NOT target “normal” slippage reverts in healthy markets, but **the consecutive‑revert breaker above is intentionally tight** because repeated fee burn is a direct PnL leak even when principal is protected.

---

## 9. Position manager (`shadow-pm`)

### 9.1 Responsibility
`shadow-pm` owns inventory and exits:
- tracks fills from chain confirmations
- maintains per‑mint position
- triggers exit transactions when risk triggers occur
- **revert‑aware inventory fence management:** ingests the `ExecutionRecord` stream from `shadow-tier0`, tracks terminal status of every submitted signature (via RPC `getSignatureStatuses` or Geyser transaction notifications), and releases leaked fence reservations via `fetch_sub` when reverts or expiry are confirmed (Section 6.7)
- **delta‑based fence reconciliation:** periodically corrects drift between on‑chain confirmed position and the fence via `fetch_add`/`fetch_sub` (Section 6.7)

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
- LUT is published atomically (atomic pointer swap on the LUT header).

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
- **money-worker** latency p50/p95/p99 (feed-ring dequeue → egress-ring enqueue)
- **end-to-end** latency p50/p95/p99 (shred RX at NIC → signed tx on egress NIC TX ring) — this is the competitively meaningful metric
- ingress path latency p50/p95/p99 (shred/packet arrival → feed-ring enqueue) — must be tracked separately to detect ingress bottlenecks
- quote latency p99
- sign latency p99
- cache miss rates (pool/mint/ALT)
- state read anomaly rates (pool pubkey mismatch / staleness drops)
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
- Microbench: Tier‑0 p99 worker (dequeue→enqueue) ≤ 150 µs on production hardware
- Microbench: Tier‑0 p99 end‑to‑end (shred RX → NIC TX) ≤ 500 µs on production hardware
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
  latency_budget_us_p99: 150                 # worker dequeue → egress enqueue (Section 0.3)
  latency_budget_e2e_us_p99: 500             # wire (shred RX) → NIC TX (Section 0.4)
  kernel_bypass_backend: af_xdp              # af_xdp | dpdk | ef_vi (ingress shred capture + egress proxy; Section 4.4)
  hugepage_size_mb: 2                        # 2 or 1024 (1GB); must match kernel boot config

  proxies:
    ingress:
      primary: shred_capture                  # AF_XDP raw Turbine shred ingestion (Section 4.4.1)
      shred_capture_workers: 2                # pinned cores for AF_XDP shred RX + lookahead scan
      grpc_fallback: yellowstone_grpc         # TCP/gRPC fallback (non-Tier-0 + redundant secondary feed)
      grpc_workers: 1
    egress:
      quic_workers: 2                        # pre-warmed QUIC/TLS sessions
      jito_workers: 1                        # persistent Jito Searcher API sessions

  crypto:
    nonce_precompute:
      enabled: true
      ring_size: 8192                        # in-process; per worker (or per worker-group); drop if empty (Section 8.2.1)

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
    capacity: 262144                         # SHOULD match state.mint_state.max_mints
    reconcile_interval_ms: 500
    max_pending_age_s: 2.0                    # signatures older than this with no terminal status → treat as reverted (Section 6.7)
    signature_poll_interval_ms: 200          # how often PM polls getSignatureStatuses for pending signatures

state:
  max_pool_staleness_ms: 20
  max_mint_staleness_ms: 200
  max_alt_staleness_ms: 2000

  pool_state:
    max_pools: 262144                        # pre-allocated double‑buffer pool arena (Section 5.4)
    index_map_capacity: 524288               # pool_pubkey -> pool_index (power-of-two, load < 0.7)

  mint_state:
    max_mints: 262144
    index_map_capacity: 524288               # mint_pubkey -> mint_index (power-of-two, load < 0.7)

entry:
  workers: 8                                 # MUST be power‑of‑two (routing uses `& (N-1)`), N money-worker threads (1 pinned core each)
  drop_on_cache_miss: true
  supported_decode:
    - raydium_v4_swap_base_in

  tip:
    lut_bucket_shift: 20                     # alpha bucket width (lamports)
    lut_buckets: 1024
    lut_format: fraction_u32                 # u32 where 4_294_967_295 = 100% of alpha (Section 6.6.2)
    bucket_rounding: true                    # shifted rounding (Section 6.6.1)
    min_alpha_lcb_lamports: 10000000

submit:
  routing:
    default_public_geyser: staked_blast
    default_jito_searcher: jito_bundle
    private_mempool_strategy: staked_blast
    min_jito_timing_margin_ms: 15

  quic:
    kamikaze_enabled: true                  # disable pacing / avoid cwnd throttling (Section 4.4.2.1)
    max_relay_fanout: 3                     # 1-5; fan out via distinct SWQoS relay identities (Section 4.4.2.2)
    burst_limit_per_ms: 5000                # cap to avoid self-induced buffer overrun
  revert_bleed_breaker:
    enabled: true
    window_slots: 25
    max_spend_lamports_per_slot: 200000000
    max_revert_rate: 0.6

oracle:
  snapshot_interval_ms: 50
  tip_lut_recompute_interval_ms: 500
  attribution:
    enabled: true
    fast_match_enabled: true
    size_discount_on_unvalidated: 0.2
```

---

## 13. Implementation checklist (must match the spec; no copy‑paste drift)

1. **BIOS/OS determinism**
   - Disable C‑states, Turbo variability (lock frequency), and Hyper‑Threading.
   - Isolate Tier‑0 cores: `isolcpus`, `nohz_full`, `rcu_nocbs`.
   - Pin threads to the NIC‑local NUMA node; lock memory (`mlockall` where applicable).

2. **Shared memory layout**
   - Create `/dev/shm` segments exactly as specified.
   - Inventory fence entries MUST have **128‑byte stride** (avoid adjacent‑line prefetch false sharing).
   - Feed rings MUST be **one SPSC ring per worker per ingress producer**:
     - `/dev/shm/shadow_feed_ring_w{w}_p{p}`, total `N_workers × P_ingress_producers`.

3. **Ingress shred capture (AF_XDP)**
   - Parse Turbine headers and extract `(slot, shred_index)` for every shred.
   - Maintain **slot‑scoped** sliding fragment windows (`SlotFragCtx[4]`) with a bounded out‑of‑order buffer (cap 16):
     - Stitch `tail || prefix` only when contiguous (`shred_index == tail_shred_index + 1`) **within the same slot**; flush the slot context if index gap `> 16`.
   - Run allowlist/Bloom prefilter on **all signers** before ALT expansion.
   - Only after a hit: ALT expansion → strict pattern decode → fixed-size `IngressRecord`.

4. **Ingress routing (mandatory)**
   - Route each decoded intent to exactly one worker by **pool pubkey** (DOS‑resistant; no DIV):
     - `worker_id = crc32_u64(route_salt_u64, u64::from_le_bytes(pool_pubkey[0..8])) & (N_workers - 1)`
     - (`N_workers` MUST be power‑of‑two; `route_salt_u64` is boot‑time CSPRNG).
   - Enqueue into the **producer‑specific** ring:
     - `/dev/shm/shadow_feed_ring_w{worker_id}_p{producer_id}`.
   - No broadcast to all workers. No global Exec Fence.

5. **Pool state publishing**
   - Implement wait‑free double buffering with **sequence tear detection** (Section 4.2.1 / 5.4):
     - writer: seq odd → write inactive → seq even → flip active_idx.
     - reader: seq1 even → copy → fence → seq2 match → accept; else drop.

6. **Money worker**
   - Poll P ingress rings round‑robin.
   - Thread‑local signature dedupe: `sig_tag64 = first 8 bytes`.
   - Maintain thread‑local per‑pool dirty reserve overlay (pool‑sharded single‑thread guarantee).

7. **Risk + PM**
   - Worker reservation: `fetch_add(trade_size)` then rollback on immediate breach.
   - Position Manager MUST clear reservations on **all terminal states**:
     - landed OR reverted OR expired → `fetch_sub(trade_size)` + remove from pending.
   - Delta reconciliation then applies true chain drift.

8. **PUBLIC_GEYSER naked blast safety**
   - Implement the single‑instruction on‑chain `ShadowGuard` monotonic reserve bound and include it in **all** backrun templates (PUBLIC_GEYSER + bundles).

9. **Egress redundancy (must match Section 4.4.2.2)**
   - **Multi‑identity relay fanout via distinct SWQoS endpoints (no single‑IP source‑port mutation).**
   - Do not implement “5 identical clones from one IP with source‑port mutation.”

## 14. What "done" means (money-grade)

V6.2.9 is "done" only when:

- Tier‑0 **money-worker** p99 (`feed_ring_dequeue → egress_ring_enqueue`) ≤ **150 µs** on production hardware (Section 0.3).
- Tier‑0 **end-to-end** p99 (`shred RX at NIC → signed tx on egress NIC TX ring`) ≤ **500 µs** on production hardware (Section 0.4). If only the worker budget is measured while the ingress path launders hundreds of microseconds, the system is not production-ready.
- Zero blocking waits and zero kernel syscalls in the money-worker loop (verified via `strace` / `perf trace`).
- Crypto nonce rings do not underflow in steady-state (empty-pop rate effectively zero); signing p99 is consistent with the precompute design (Section 8.2.1).
- Dynamic pool + mint append works live: new pools/mints can be indexed and traded without restart (Section 5.4).
- Sharded routing + thread‑local dedupe occurs before inventory reservation; inventory fence contention remains bounded under burst tests (Section 6.1 / 8.4).
- Tip LUT uses shifted rounding and fractional scaling; whale opportunities do not clamp tips to a small absolute cap (Section 6.6).
- Realized net EV per slot is positive after auction costs in live conditions, with the revert bleed breaker preventing fee/tip hemorrhage under systemic mismatch.

---

## 15. Production host tuning (mandatory for ≤150 µs p99)

Tier‑0 latency targets are not achievable on a default Linux install. Production hosts MUST be tuned so the money-worker cores behave like a deterministic appliance.

### 15.1 BIOS / firmware
- Disable deep C‑states (keep cores in C0/C1 only).
- Disable frequency scaling jitter:
  - set CPU governor to `performance`, and
  - lock an all‑core frequency (avoid turbo oscillation under burst).
- Disable Hyper‑Threading / SMT for money-worker cores (shared L1 is EV suicide).
- Pin the NIC to a local NUMA node; avoid cross‑socket PCIe paths.

### 15.2 Linux kernel boot parameters (money cores)
For the set of money-worker cores (example `2-9`), production SHOULD use:
- `isolcpus=domain,managed_irq,2-9`
- `nohz_full=2-9`
- `rcu_nocbs=2-9`

Additionally, disable power saving states if not handled in BIOS:
- `intel_idle.max_cstate=0` / `processor.max_cstate=0` (platform dependent)

### 15.3 IRQ and scheduler isolation
- Disable `irqbalance`.
- Move all IRQs (especially NIC RX/TX) **off** money-worker cores.
- Use CPU sets / cgroups so only Tier‑0 threads run on isolated cores.
- `shadow-state` SHOULD run RT (`SCHED_FIFO`) on its own isolated core to minimize state staleness. Correctness does not rely on non‑preemption due to double‑buffer publish (Section 4.2.1). Proxies MAY also run RT if they remain well‑behaved.

### 15.4 Kernel bypass memory
If using DPDK/AF_XDP/ef_vi:
- pre-allocate hugepages (2MB or 1GB as supported) for packet buffers and rings,
- lock critical mappings (`mlockall`) for `shadow-state` and Tier‑0,
- monitor NIC ring occupancy and drops (RX/TX) as first-class SLOs.

### 15.5 Core dump and telemetry hygiene
- Enforce `RLIMIT_CORE=0` and `PR_SET_DUMPABLE=0` (Section 0.5.4).
- Ensure all telemetry agents and crash collectors are **disabled** on Tier‑0 hosts.