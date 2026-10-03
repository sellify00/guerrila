# PROJECT SHADOW ENGINE V6.2.2 — Profit‑First Tier‑0 Sniper System Specification (Unified)

**Status:** Locked engineering implementation specification (single authoritative document)  
**Supersedes:** V5, V5.1, V6.0, V6.1, V6.2, V6.2.1, and all prior drafts/addenda
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
**Hot path** in V6.2.2 is the deterministic, bounded **money-worker loop** from:

1) `IngressRecord` dequeued from the per-worker SPSC feed ring (`/dev/shm/shadow_feed_ring_*`), to  
2) `SignedTxEnvelope` enqueued into the per-worker egress ring (`/dev/shm/shadow_egress_ring_*`).

Stateful networking (TCP/gRPC ingest, TLS/QUIC egress, Jito Searcher API sessions) is **explicitly excluded** from the money-worker loop and is handled by dedicated proxy threads pinned to separate cores (Section 4.4). The money-worker thread never performs segment reassembly, HPACK, TLS, or QUIC framing.

### 0.4 Latency budget
Tier‑0 Sniper Lane **MUST** satisfy:

- **p99 money-worker latency ≤ 150 µs** from feed-ring dequeue to egress-ring enqueue (as defined in Section 0.3).
- **No blocking waits** in the worker loop.
- **No hot‑path network I/O** (RPC/HTTP) for decisions.
- **No kernel syscalls** inside the worker loop.

**Networking proxy constraint (separate cores):**
- Ingress and egress proxy threads MUST keep sessions **pre-warmed** and MUST NOT introduce backpressure into the money-worker loop; all worker→proxy handoff is drop-on-full ring buffers.

If the system cannot meet the p99 budget above on production hardware, it is not considered "production‑ready" regardless of architecture elegance.


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

#### 0.5.3 Cache‑line isolation (no false sharing)
- Any per‑mint/per‑pool atomic counter, seqlock entry, or fence entry **MUST** be isolated to its own cache line (64 bytes).
- Rust implementation **MUST** use explicit padding, e.g. `#[repr(C, align(64))]`, and **MUST NOT** use packed arrays like `[AtomicI64; N]` or `[AtomicU64; N]` without 64B‑stride padding.

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


2. **shadow-tier0** (Tier‑0 lane; latency‑critical **money workers**)
   - Single binary containing:
     - **N run‑to‑completion money-worker threads**, each pinned to an isolated core.
     - **Ingress proxy thread(s)** (TCP/gRPC / provider protocol termination), pinned to dedicated cores.
     - **Egress proxy thread(s)** (QUIC/TLS + Jito sessions), pinned to dedicated cores.
     - **Crypto precompute thread(s)** for Ed25519 nonce material, pinned to dedicated cores (Section 8.2.1).
   - Money workers execute the deterministic money loop on a single core:
     `IngressRecord` (**pre-flattened, pre-decoded**) → gate → quote → template build → sign → enqueue `SignedTxEnvelope`.
   - **Money workers are stateless-networking-free:** they never perform TCP reassembly, HTTP/2 HPACK decode, TLS, QUIC framing, or AES-GCM. Those occur only on proxy cores (Section 4.4).
   - **No variable-length parsing on the money core:** ALT expansion and swap decoding occur on ingress proxy cores (Section 4.4.1). After dequeue, the worker performs only fixed-cost operations until enqueue.
   - Each money-worker thread holds a **read‑only copy of the hot wallet Ed25519 private key** in thread‑local memory. Signature generation uses precomputed nonce material (Section 8.2.1).
   - Worker↔proxy communication is **lockless SPSC** (drop-on-full) rings:
     - ingress proxy → money workers: `/dev/shm/shadow_feed_ring_*`
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
- Nonce / recent‑blockhash coordination: each worker reads the rolling blockhash from a shared‑memory segment published by a background thread within `shadow-tier0`. No lock; atomic load of a cache‑line‑aligned struct.

**Cross‑worker synchronization (two atomic fences, no locks):**
- **Exec fence** (`/dev/shm/shadow_exec_fence`): a lightweight **per-target duplicate suppression fence** keyed by the **target transaction signature** (not by mint). Its only job is to ensure we do not process/send the **same target transaction** twice due to RSS duplication or multi-feed replay.
  - Worker computes `h64 = SipHash(signature)`.
  - Store/compare uses `sig_hash48 = (h64 & ((1<<48) - 1)) | 1` (non‑zero 48‑bit hash).
  - Fence entries include a 16‑bit rolling slot tag (`slot16 = slot as u16`) to enable bounded staleness and overwrite of old entries.
  - Entry is a single atomic `u64` tag (see Section 6.1) so it uses only native-width CAS (no `cmpxchg16b`).
  - **This CAS MUST be performed before touching shared risk limits** to minimize MESI cache-line bouncing under burst contention.
- **Inventory fence** (`/dev/shm/shadow_inventory_fence`): per‑mint atomic `i64` tracking committed position in lamports.
  - Reservation MUST use a bounded `compare_exchange_weak` loop (no `fetch_add` overshoot window), Section 6.7.
  - Release uses `fetch_sub` (O(1)).
- Fence entries MUST be cache‑line isolated (64B stride) to prevent false sharing.
- These are the only cross‑worker synchronization points on the money-worker loop. Both are lock‑free atomic operations.



### 2.2 CPU, NUMA, and NIC requirements
Tier‑0 **MUST** run on dedicated isolated cores:

- `shadow-tier0` (single binary, pinned cores):
  - N money-worker cores (1 per run‑to‑completion worker; decode→quote→sign→enqueue only)
  - 1+ ingress proxy cores (Yellowstone/gRPC and any other stateful provider protocol termination)
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
- O(1) quote from cached state.
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
- route through on‑chain assertion routers,
- add validation CPIs,
- run embedded simulators on the critical loop,
- invoke kernel syscalls for networking (Section 3.2) or logging.

---

## 4. Shared memory and IPC (zero‑copy)

### 4.1 Shared memory segments (all binary; no JSON)
All IPC uses fixed binary layouts in `/dev/shm`. No Python writes binary structs.

Required segments:
- `/dev/shm/shadow_feed_ring_*` (SPSC rings; one per money worker; **producer = ingress proxy**, consumer = worker)
- `/dev/shm/shadow_egress_ring_*` (SPSC rings; one per money worker; producer = worker, **consumer = egress proxy**)
- `/dev/shm/shadow_pool_index_map` (lock-free fixed-capacity **8-byte tag** map: `Hash40(pool_pubkey) -> pool_index_plus_one_u24`; full pubkey verified in arena; written by `shadow-state`, read by workers; Section 5.4)
- `/dev/shm/shadow_pool_state` (seqlock-protected **pool arena**; pre-allocated contiguous array of pool structs; written by `shadow-state`, read O(1) by workers; Section 5.4)
- `/dev/shm/shadow_mint_index_map` (lock-free fixed-capacity **8-byte tag** map: `Hash40(mint_pubkey) -> mint_index_plus_one_u24`; full pubkey verified in mint arena; written by `shadow-state`, read by workers; Section 5.6)
- `/dev/shm/shadow_mint_state` (read-mostly mint metadata arena/snapshot; fork-aware; written by `shadow-state`)
- `/dev/shm/shadow_state_alts` (read‑only snapshot)
- `/dev/shm/shadow_risk_snapshot` (read‑only snapshot)
- `/dev/shm/shadow_oracle_targets` (read‑only snapshot)
- `/dev/shm/shadow_tip_lut` (read‑only; pre‑computed optimal tip fractions published by oracle — Section 6.6)
- `/dev/shm/shadow_health` (read‑only snapshot)
- `/dev/shm/shadow_blockhash` (read‑only; cache‑line‑aligned rolling blockhash updated by `shadow-tier0` background thread)
- `/dev/shm/shadow_exec_fence` (atomic CAS fence for per-target/per-slot duplicate prevention across workers)
- `/dev/shm/shadow_inventory_fence` (atomic `i64` array; per‑mint committed position in lamports; bounded CAS reserve by workers + `fetch_sub` release, reconciled by `shadow-pm` — Section 6.7)
- `/dev/shm/shadow_tier0_stats` (read‑only snapshot; per‑worker send/sign/land counters)

**Secret nonce material:** Precomputed Ed25519 nonce material is **not** an IPC/shared-memory segment in V6.2.2. Because the crypto precompute threads and money workers run inside the same `shadow-tier0` process (threads), nonce rings MUST be allocated as **anonymous, private, locked memory** (Section 8.2.1) and MUST NOT be placed in `/dev/shm`.

Note: the prior per‑worker diff rings (`shadow_state_pool_diffs_*`) are eliminated. `shadow-state` writes pool state directly into the seqlock-protected pool arena, and Tier‑0 workers perform O(1) reads instead of draining diff queues (Section 5.4).

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
- Reader loads `seq` with `Acquire` ordering, copies the data, re‑loads `seq`. If `seq` changed or was odd during the read, the reader retries (spin).
- Each pool state entry MUST be **cache‑line‑aligned** (64 bytes or padded to 64‑byte boundary) to prevent false sharing between adjacent pool entries.
- The reader MUST NOT modify any byte in the seqlock segment. Reads are purely load‑only.
- No mutex, no futex, no syscall. Seqlock is entirely user‑space atomic operations.

**Writer preemption rule (no “spin of death”):**
- The writer MUST NOT be preempted or page-faulted while `seq` is odd.
- `shadow-state` MUST:
  - run the seqlock writer thread on a dedicated isolated core (no other daemons),
  - lock its memory (`mlockall(MCL_CURRENT|MCL_FUTURE)`),
  - and run under a real-time scheduler policy (`SCHED_FIFO`), so the writer cannot be involuntarily paused mid-write.
- The writer critical section (between `seq := odd` and `seq := even`) MUST contain only plain memory writes — **no syscalls, no heap allocs, no logging, no parsing**.

**Reader bounded retry (fail-fast):**
- Tier‑0 readers MUST cap seqlock retries (default `MAX_SEQLOCK_RETRIES = 8`).
- If the cap is exceeded, the worker MUST **drop the intent** with reason `SEQLOCK_SPINOUT` and immediately return to polling. Spinning indefinitely is a guaranteed auction loss.


### 4.3 Tier‑0 "no allocation / no lock / no syscall" contract
Inside the run‑to‑completion worker loop (ingest through send):
- **MUST NOT allocate** (no heap allocs)
- **MUST NOT lock** (no mutex; the only synchronization primitives are: atomic CAS on exec fence, bounded CAS reserve + `fetch_sub` release on inventory fence, and seqlock atomic loads for pool state reads — all are wait‑free / lock‑free)
- **MUST NOT syscall** — no `sendto`, `recvfrom`, `syslog`, `write`, `futex`, or any other kernel trap on the critical path. Workers enqueue to `/dev/shm/shadow_egress_ring_*`; proxy cores perform networking (Section 4.4).
- **MUST NOT poll or drain diff queues** — pool state is read directly via seqlock (Section 5.4)
- Logging MUST be async, sampled, and off the hot path

### 4.4 Stateful networking is off the money thread (proxy cores)

**Why:** Yellowstone is gRPC (HTTP/2 over TCP). Staked TPU delivery is QUIC (UDP + TLS 1.3 + connection state + AES‑GCM). Implementing TCP reassembly, HPACK, TLS, or QUIC inside the money-worker run-to-completion loop is incompatible with a 150 µs budget.

**Rule:** money workers MUST handle only fixed-size binary records and raw transaction bytes. All stateful networking and cryptography framing happens on dedicated proxy cores.

#### 4.4.1 Ingress proxy (TCP/gRPC termination → **pre-flattened, pre-decoded** feed rings)
The ingress proxy exists to keep **variable-length parsing** and **ALT expansion** off the money-worker core.

Ingress proxy responsibilities (pinned cores):
- Maintain provider connections (Yellowstone gRPC and any other stateful feeds).
- Parse protobuf messages and extract:
  - `signature`, `slot`, `unix_ns`, and the raw message bytes (if needed for auditing).
    - `unix_ns` SHOULD be stamped once in the proxy (TSC→ns or vDSO `clock_gettime`). Money workers MUST NOT call time syscalls.
  - `signal_source` tagging.
- **Expand Address Lookup Tables (ALTs)** and produce a deterministic **flattened account list**.
- **Strictly decode supported swap patterns** (Raydium V4 `swap_base_in`, etc.) and emit a fixed-size record so the money worker does zero pointer chasing and zero array expansion.

Ingress proxy output:
- Writes fixed binary `IngressRecord` structs into `/dev/shm/shadow_feed_ring_*` (SPSC, drop-on-full).
- `IngressRecord` MUST be rigid and fixed-size (no dynamic vectors). Recommended layout includes:
  - `signal_source`, `slot`, `signature`
  - `entity_id` (or allowlist key)
  - `venue_id` + `pattern_id`
  - decoded fields: `pool_pubkey`, `mint_in`, `mint_out`, `amount_in`
  - `flat_accounts_len: u8` and `flat_accounts: [Pubkey; 64]` (unused entries zeroed)
  - optional: fixed indices into `flat_accounts` for the exact accounts required by the pattern (so the worker never scans)

Failure policy:
- If required ALT expansion or decode fails, the ingress proxy MUST drop the record (do not forward “maybe-decodable” work to the money worker).

The ingress proxy MUST be sized so it never becomes the throughput bottleneck. It MAY use kernel networking (standard sockets) or a user-space TCP stack (e.g., F‑Stack); this is outside the money-worker loop.



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

##### 4.4.2.2 Redundant packet cloning (loss bypass)
For critical `PUBLIC_GEYSER`-derived sends, the egress proxy SHOULD send **redundant copies** of the same transaction payload:

- `redundant_copies_per_endpoint` (default 3, max 5)  
- Copies are separate QUIC packets (unique packet numbers) carrying identical transaction bytes at the application layer.
- Copies MUST be bounded by a global burst limiter to avoid self-induced socket buffer overrun.
- **TX ring sizing requirement (kernel bypass):** when cloning is enabled, the AF_XDP/DPDK/ef_vi TX descriptor ring MUST be sized to absorb worst-case bursts (endpoints × copies) without local drops. Recommended minimum: **4096 descriptors** (8192 for multi-endpoint blasting). The proxy MUST expose `tx_ring_full_drops`; sustained non-zero values are a production paging condition.

This is not “retransmission”; it is deliberate duplication to bypass intermediate loss.



#### 4.4.3 Kernel bypass (where it matters)
- **Money workers:** MUST NOT invoke kernel networking syscalls.
- **Egress proxy (STAKED_BLAST):** **MUST** support a kernel‑bypass TX/RX backend (AF_XDP / DPDK / ef_vi). A Tier‑0 system that “wins in the lab” but uses `sendmsg()` on production egress is a latency hallucination; the Linux UDP stack + qdisc jitter is large enough to lose first‑price auctions.
  - The QUIC implementation MUST be compatible with a user‑space datagram backend (i.e., it can emit encrypted UDP payloads into a kernel‑bypass TX ring without blocking syscalls).
- **Ingress proxy:** MAY use the kernel stack for TCP/gRPC feeds, but any **UDP datagram** ingest path intended to be competitive (private mempools, shred feeds) SHOULD use kernel bypass as well.
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
- rollback on fork switch: revert affected pool state entries in `/dev/shm/shadow_pool_state` using seqlock write protocol (Section 4.2.1); increment each affected entry's `seq` through odd→even cycle
- publish a monotonic `snapshot_slot` in `/dev/shm/shadow_health` so workers can detect discontinuities and staleness

### 5.4 Publishing model (dynamic appending arena + lock-free index map)

Tier‑0 cannot require human restarts to trade newly created pools. V6.2.2 uses a **pre‑allocated appending arena** plus a **lock‑free index map** that is safe against torn reads/writes.

#### 5.4.1 Shared segments
- `/dev/shm/shadow_pool_state`: a fixed-capacity **pool arena** of `max_pools` entries. Each entry is cache‑line aligned and protected by its own seqlock (Section 4.2.1).
- `/dev/shm/shadow_pool_index_map`: an open-addressed table of **atomic 8‑byte tags** mapping `Hash40(pool_pubkey) -> pool_index_plus_one_u24` (readers verify the full pubkey inside the seqlocked arena).

#### 5.4.2 Writer side (`shadow-state`): lock-free append
When `shadow-state` detects a new eligible pool (e.g., Raydium pool init / first observed swap / program-owned account matching the pool discriminator):

1. **Allocate an index**:
   - `pool_index = atomic_fetch_add(arena_tail, 1)`
   - If `pool_index >= max_pools`: stop appending, emit `POOL_ARENA_EXHAUSTED`, and drop new pools (operator action required).

2. **Populate the pool entry** using the per-entry seqlock write protocol:
   - Prepare all derived fields first (no parsing/logging/syscalls inside the critical section).
   - `seq := odd` (Release)
   - write the full fixed-size pool struct (including the full 32-byte `pool_pubkey`, `last_update_unix_ns`, and all quoting fields).
   - `seq := even` (Release)

3. **Publish the mapping** into `/dev/shm/shadow_pool_index_map` using **8‑byte atomic tagging**:
   - The index map **MUST NOT** attempt to atomically store `(Pubkey[32] + u32 index)` (36 bytes). That is not an atomic width on x86_64/ARM64 and will tear.
   - Each map slot stores a single **atomic `u64`** tag:

     `tag = [pubkey_hash40 | pool_index_plus_one_u24]`

     where:
     - `h64 = SipHash(pool_pubkey)` (or equivalent robust keyed hash),
     - `pubkey_hash40 = (h64 >> 24) & ((1<<40) - 1)` (40-bit),
     - `pool_index_plus_one_u24 = (pool_index as u32 + 1)` stored in 24 bits (so `0` is empty; supports up to 16,777,215 pools).
     - Packing (normative): `tag = (pubkey_hash40 << 24) | pool_index_plus_one_u24`.

   - Insertion uses open addressing (linear probing) and **atomic CAS** from `0 → tag` (Release on success; bounded probe).
   - The tag MUST be published **only after** the pool arena entry has been fully written and the seqlock has returned to an even value.

This append is off the money-worker loop; it may do bounded work per new pool without impacting Tier‑0 latency.

#### 5.4.3 Reader side (Tier‑0 money worker): wait-free lookup + seqlock read (tear-safe)
For every decoded trade:
1. Extract `pool_pubkey` from `IngressRecord`.
2. Compute `h64 = SipHash(pool_pubkey)` and `pubkey_hash40 = (h64 >> 24) & ((1<<40) - 1)`.
3. Probe `/dev/shm/shadow_pool_index_map` (bounded; `MAX_PROBES`, default 8):
   - Atomically load `tag` (Acquire). If `tag == 0`, treat as miss and drop.
   - If `tag.pubkey_hash40 != pubkey_hash40`, continue probing.
   - Otherwise, compute `pool_index = tag.pool_index_plus_one_u24 - 1`.
   - Read `/dev/shm/shadow_pool_state[pool_index]` via seqlock (bounded retries; Section 4.2.1).
   - **Verify** `pool_state.pool_pubkey == pool_pubkey`.
     - If it matches: return `pool_index`.
     - If it does not match: hash collision; continue probing.
4. On miss after `MAX_PROBES`: drop (pool state not available or map saturated).
5. Apply per‑pool staleness check using only data already in-cache:
   - define `now_unix_ns = ingress_record.unix_ns` (stamped by ingress proxy; the money worker MUST NOT call `clock_gettime`).
   - if `now_unix_ns - pool_state.last_update_unix_ns > max_pool_staleness_ns` → drop (fail closed).

This design guarantees the worker never observes a torn pubkey write: the map slot is an 8-byte atomic load, and the full 32-byte pubkey is only read from the tear-safe seqlocked arena.

#### 5.4.4 Capacity and cache behavior
- Default `max_pools` for production **SHOULD** be sized for genesis events (recommended: `262_144`), and stored in hugepage-backed shared memory.
- Hash size note: 40 bits of stored hash makes accidental collisions in a 262k‑entry universe effectively negligible; remaining collisions are handled by pubkey verification in the arena.
- The arena will not fit in L2 at this size. That is acceptable: hot workers touch only the small working set of pools currently trading. Cache pressure is managed by:
  - keeping entries compact and cache-line aligned,
  - reading exactly one pool entry per intent (plus mint entries),
  - and pinning the whole arena to the NIC-local NUMA node.

This removes the “restart to trade new pools” failure mode and prevents systematically missing genesis blocks.

### 5.5 Staleness and health
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

Per‑pool staleness is also checked inline via each seqlock entry's `last_update_unix_ns` (Section 5.4, reader side).


### 5.6 Mint arena and index map (dynamic; genesis-safe)
Tier‑0 risk and mint-invariant checks require stable O(1) indexing for **new mints** (genesis tokens).

`shadow-state` MUST maintain:
- `/dev/shm/shadow_mint_index_map`: lock-free fixed-capacity map `mint_pubkey -> mint_index` using the **same 8‑byte atomic tagging** scheme as pools (Section 5.4.2/5.4.3), but with mint keys.
  - `h64 = SipHash(mint_pubkey)`
  - `mint_hash40 = (h64 >> 24) & ((1<<40) - 1)`
  - `mint_index_plus_one_u24 = (mint_index as u32 + 1)` stored in 24 bits
  - Tag packing (normative): `tag = (mint_hash40 << 24) | mint_index_plus_one_u24`
  - Tag stored as atomic `u64` (Release by writer; Acquire by readers)
  - Full 32-byte mint pubkey lives only in `/dev/shm/shadow_mint_state[mint_index]` and is verified there (tear-safe).
- `/dev/shm/shadow_mint_state`: fixed-capacity mint arena/snapshot keyed by `mint_index` (seqlocked or read-mostly snapshot).

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
Each Tier‑0 money-worker thread executes the full pipeline on a single pinned core. No cross‑thread or cross‑process handoff occurs between dequeue and signature finalization.

For each fixed-size `IngressRecord` dequeued from the worker’s feed ring:
1. Read `signal_source` (tagged by the ingress proxy) and dedupe by signature (thread‑local). If duplicate: drop.
2. Fast reject if not from a tracked target entity / allowlist key.
3. Validate the record is a supported `(venue_id, pattern_id)`; otherwise drop.
4. **Exec fence (Step 1.5, fail‑fast):** claim the target signature across workers **before** touching any index maps, seqlocks, or quote math:
   - compute `h64 = SipHash(signature)` (64‑bit)
   - derive `sig_hash48 = (h64 & ((1<<48) - 1)) | 1` (non‑zero 48‑bit)
   - derive `slot16 = (slot as u16)` (rolling slot tag)
   - compute `tag = ((slot16 as u64) << 48) | sig_hash48`
   - attempt bounded‑probe atomic CAS insertion into `/dev/shm/shadow_exec_fence` (Section 6.1, Exec fence table format)
   - if the fence indicates `tag` is already present (duplicate signature in this slot window): drop immediately
5. Resolve indices (cache‑only):
   - `pool_index = lookup(pool_pubkey)` in `/dev/shm/shadow_pool_index_map` (bounded probe + pubkey verify; drop on miss).
   - `mint_out_index = lookup(mint_out)` in `/dev/shm/shadow_mint_index_map` (bounded probe + pubkey verify; drop on miss) — used for inventory fencing.
6. Read state (O(1)):
   - pool state via seqlock read from `/dev/shm/shadow_pool_state[pool_index]` and verify per‑pool staleness.
   - mint invariants as required from `/dev/shm/shadow_mint_state` (drop on hard reject).
7. Compute quote + `min_out` + conservative `alpha_lcb`.
8. Select bid spend (O(1)):
   - compute `bucket = min((alpha_lcb + (1 << (BUCKET_SHIFT - 1))) >> BUCKET_SHIFT, lut_size - 1)`
   - load `tip_fraction = tip_lut[bucket]` (u16 fixed-point; 65535 = 100%)
   - compute `tip = (alpha_lcb * tip_fraction) >> 16`
9. **Inventory fence (bounded CAS reserve):** reserve `trade_size_lamports` against `/dev/shm/shadow_inventory_fence[mint_out_index]` using a bounded `compare_exchange_weak` loop (Section 6.7).
   - If the reservation would exceed limits: drop immediately.
   - If the fence is too contended to reserve within `MAX_RISK_CAS_RETRIES`: drop (fail-fast).
10. Build transaction from template (fixed CU shape), using the rolling blockhash snapshot.
11. Sign using precomputed nonce material (Section 8.2.1).
12. Enqueue `SignedTxEnvelope` into `/dev/shm/shadow_egress_ring_*` for the appropriate egress proxy (route selected by `signal_source`, Section 8.3). Drop on full.
    - **If any failure occurs after Step 9** (deadline exceeded, template failure, ring full, etc.), the worker MUST `fetch_sub(trade_size_lamports)` to release the inventory reservation before returning to polling.
No step may block.

**Exec fence implementation note (required semantics):**
- The exec fence MUST be keyed by **target signature**, not by `(mint, slot)`. This ensures that during high-velocity launches (many profitable trades in the same mint within one slot), we still backrun **every** distinct target trade.  
- The fence exists only to prevent re-processing the *same* signature due to duplicated feed delivery.


**Exec fence table format (required):**
- `/dev/shm/shadow_exec_fence` MUST be a fixed-capacity, power-of-two open-addressed table of **cache-line-isolated atomic `u64` entries**.
- Each entry stores a single 64-bit `tag`:

  - `h64 = SipHash(signature)`  
  - `sig_hash48 = (h64 & ((1<<48) - 1)) | 1`  
  - `slot16 = (slot as u16)`  
  - `tag = ((slot16 as u64) << 48) | sig_hash48`

- The worker claims an entry via bounded-probe CAS (default `MAX_PROBES=8`), using only native-width atomics:
  - If it observes the same `tag` already present → duplicate → drop.
  - Otherwise it inserts `tag` into:
    - an empty slot (`old == 0`), or
    - a **stale** slot where `delta = slot16.wrapping_sub(old_slot16)` and `delta > stale_slots` (default `stale_slots=4`).
- The probe base index SHOULD be derived from `h64` (e.g., `idx0 = h64 & (cap-1)`) and linearly probed.
- Ordering:
  - loads: `Acquire`
  - successful CAS: `AcqRel`
  - failed CAS: `Acquire`
- On saturation (no slot found within `MAX_PROBES`), the worker MUST drop with reason `EXEC_FENCE_SATURATED` (fail-fast). A saturated fence is a capacity/config bug and must page ops.

**Rationale:** `AtomicU128` implies `LOCK CMPXCHG16B` on x86_64, which is materially slower and amplifies MESI contention under burst. A single `AtomicU64` CAS is the correct primitive for a Tier‑0 duplicate fence.

### 6.2 Supported decode set (strict O(1), performed on ingress proxy cores)
Tier‑0 MUST support only patterns with fixed parsing cost. In V6.2.2, strict decoding occurs on **ingress proxy cores** (Section 4.4.1) and is emitted as a fixed-size `IngressRecord`.

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

### 6.6 Tip selection (O(1) LUT with correct rounding and whale scaling)

Tier‑0 tip selection MUST be constant‑time and must not clamp away the convex tail.

#### 6.6.1 Fix truncation bias (bucket rounding)
Bucket selection MUST use **shifted rounding**, not truncation:

```
bucket = min((alpha_lcb + (1 << (BUCKET_SHIFT - 1))) >> BUCKET_SHIFT, lut_size - 1)
```

This prevents systematic under-bidding near bucket boundaries.

#### 6.6.2 Store fractions, not absolute lamports
The LUT MUST store a **tip fraction** (fixed-point), not an absolute lamport value.

- `tip_lut[bucket] = tip_fraction_u16`
- `tip_fraction_u16 ∈ [0, 65535]` where `65535 = 100% of alpha_lcb`

Hot path computes:

```
tip = (alpha_lcb * tip_fraction_u16) >> 16
```

This keeps O(1) execution while ensuring out-of-distribution “whale” alpha automatically scales even when `bucket` is clamped to `lut_size-1`.

#### 6.6.3 Oracle responsibilities (off hot path)
`shadow-oracle` computes `tip_fraction_u16` per bucket by optimizing conservative NetEV under the desk’s auction model:

`NetEV(tip) = p_win(tip_per_cu) × (alpha_lcb − auction_cost(tip, cu_limit))`

The oracle may impose regime-aware caps, but those caps are expressed as fractions (so scaling is preserved).

**Invariant:** Tier‑0 MUST NOT run any search or optimization for tips. It performs at most:
- one bucket computation,
- one LUT load,
- one 64×16 multiply + shift.

### 6.7 Risk gating (O(1), atomic inventory fence)

**Problem with read‑only snapshot gating (eliminated):**
The prior model had N workers read a read‑only `/dev/shm/shadow_risk_snapshot` to check position limits. If the max position for a mint is 500 SOL and 5 workers spot the same opportunity, all 5 read "Position: 0, Limit: 500", all 5 pass the gate, all 5 fire. Result: 2500 SOL position, 5× limit breach, panic exit, strategy bankruptcy.

**Atomic inventory fence (mandatory):**
`/dev/shm/shadow_inventory_fence` is a cache‑line‑aligned array of atomic `i64` entries, **one per mint index**.
- `mint_index` is obtained via a wait‑free lookup in `/dev/shm/shadow_mint_index_map` (maintained by `shadow-state`).
- Each entry represents the **current committed position in lamports** for that mint.
- Fence entries MUST be 64B‑strided (cache-line isolated) to prevent false sharing.

**Why `fetch_add` is not acceptable (temporary overshoot false negatives):**
A `fetch_add`+rollback pattern creates a transient overshoot window where the fence counter temporarily exceeds the limit, causing other workers to falsely reject valid trades. During memecoin swarms, this silently deletes EV.

**Risk check protocol (per worker, bounded, fail‑fast):**
1. Read risk limits from `/dev/shm/shadow_risk_snapshot` (read‑only; still published by `shadow-pm`):
   - `max_position_lamports_per_mint`
   - `max_notional_lamports_per_entity`
   - `global_kill_switch: bool`
   - `allowed_venues`
2. If `global_kill_switch == true` → drop immediately.
3. Compute `trade_size_lamports` for this intent.
4. Reserve inventory **without overshoot** using a bounded CAS loop on `shadow_inventory_fence[mint_index]`:
   - default `MAX_RISK_CAS_RETRIES = 6`
   - pseudocode (normative semantics):

     - `cur = fence.load(Acquire)`
     - repeat up to `MAX_RISK_CAS_RETRIES`:
       - if `cur + trade_size_lamports > max_position_lamports_per_mint` → drop `RISK_LIMIT_EXCEEDED`
       - attempt `compare_exchange_weak(cur, cur + trade_size_lamports, AcqRel, Acquire)`
       - on failure: set `cur = observed` and retry
   - if not reserved after retries: drop `RISK_FENCE_CONTENDED` (fail-fast; do not spin unboundedly).
5. After a successful reservation, proceed to build/sign/send.

**Rollback on send failure or revert:**
- If the worker drops the intent after reserving (template failure, deadline exceeded, egress ring full, etc.), it MUST `fetch_sub(trade_size_lamports)` to release the reservation before returning to polling.
- `shadow-pm` periodically reconciles `shadow_inventory_fence` against confirmed on‑chain position state. Drift (due to reverts, missed rollbacks, or confirmed fills) is corrected by atomically storing the reconciled value. Reconciliation frequency: configurable (default every 500 ms).

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

### 8.2 Inline build → sign → enqueue (send is on proxy cores)
Within each money worker’s run‑to‑completion loop (Section 6.1):
- **Build**: populate template transaction with trade parameters, blockhash (atomic load from `/dev/shm/shadow_blockhash`), and fee/tip fields. Zero allocations.
- **Sign**: Ed25519 sign the serialized message bytes using precomputed nonce material (Section 8.2.1).
- **Enqueue**: write a `SignedTxEnvelope` into `/dev/shm/shadow_egress_ring_*`. The egress proxy performs QUIC/Jito framing and transmission (Section 4.4).

Money workers do not run TLS/QUIC, do not encrypt, and do not perform network syscalls.

### 8.2.1 Asynchronous Ed25519 nonce precomputation (mandatory for <150 µs targets)
Ed25519 signing time is dominated by computing the curve point `R = r·B`. V6.2.2 removes this work from the money-worker loop by precomputing single-use nonce material on dedicated cores.

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
- Fee strategy: allocate spend entirely as **priority fee** (`SetComputeUnitPrice`). No Jito tip instruction (avoids Dragon 1 tip donation to non‑Jito leaders).
- The transaction is a standalone swap attempting to land sequentially after the target by virtue of high priority fee and fast delivery.
- Bounded retries with fee bumping; strict rate limits.
- Egress proxy MUST use “kamikaze” QUIC configuration and MAY send redundant copies (Section 4.4.2.1–4.4.2.2).

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
Workers MUST prevent duplicate sends for the same **target signature**:
- Exec fence is a fixed-capacity lock-free table in `/dev/shm/shadow_exec_fence` keyed by a rolling 64-bit `tag = (slot16<<48) | sig_hash48` derived from the target signature (Section 6.1).
- If the CAS/claim fails because the same `tag` is already present (duplicate signature within the slot window): drop immediately.
- Additional per‑worker dedup by signature hash (thread‑local, no synchronization) is still required to avoid hammering the shared fence on replay bursts.

The exec fence MUST NOT be keyed by `(mint, slot)`. Mint/position risk is handled exclusively by the inventory fence.



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
  latency_budget_us_p99: 150                 # worker dequeue → egress enqueue (Section 0.3)
  kernel_bypass_backend: af_xdp              # af_xdp | dpdk | ef_vi (used by egress proxy; Section 4.4)
  hugepage_size_mb: 2                        # 2 or 1024 (1GB); must match kernel boot config

  proxies:
    ingress:
      provider: yellowstone_grpc             # TCP/gRPC termination on proxy cores
      workers: 2
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

state:
  max_pool_staleness_ms: 20
  max_mint_staleness_ms: 200
  max_alt_staleness_ms: 2000

  pool_state:
    max_pools: 262144                        # pre-allocated seqlock pool arena (Section 5.4)
    index_map_capacity: 524288               # pool_pubkey -> pool_index (power-of-two, load < 0.7)

  mint_state:
    max_mints: 262144
    index_map_capacity: 524288               # mint_pubkey -> mint_index (power-of-two, load < 0.7)

entry:
  workers: 8                                 # N money-worker threads (1 pinned core each)
  drop_on_cache_miss: true
  supported_decode:
    - raydium_v4_swap_base_in

  tip:
    lut_bucket_shift: 20                     # alpha bucket width (lamports)
    lut_buckets: 1024
    lut_format: fraction_u16                 # u16 where 65535 = 100% of alpha (Section 6.6)
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
    redundant_copies_per_endpoint: 3        # 1-5; deliberate duplication to bypass loss (Section 4.4.2.2)
    burst_limit_per_ms: 5000                # cap to avoid self-induced buffer overrun
  exec_fence_capacity: 1048576               # signature-hash exec fence (power-of-two recommended)
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

## 13. Implementation checklist (ordered)

1. Implement **ingress proxy** (TCP/gRPC termination): maintain Yellowstone gRPC sessions, parse protobufs, and publish fixed `IngressRecord` structs into `/dev/shm/shadow_feed_ring_*` (drop-on-full; pinned cores).
2. Implement **egress proxies**:
   - QUIC/TLS sessions to TPU/staked relays (pre-warmed; no per-send handshake),
   - Jito Searcher API sessions for bundle submission,
   consuming `/dev/shm/shadow_egress_ring_*` and performing protocol framing + send on proxy cores (Section 4.4).
3. Implement **Ed25519 nonce precompute** thread(s) and in-process secret nonce rings (anonymous `mmap` + `mlock`); enforce single-use nonces and drop if rings empty (Section 8.2.1).
4. Implement shared-memory schemas: feed rings, egress rings, blockhash segment, exec fence, inventory fence, pool/mint index maps, pool/mint arenas.
5. Implement `shadow-state` fork-aware replication for Raydium V4 pools/vaults/mints + ALTs, including:
   - dynamic pool append + pool_index_map updates,
   - dynamic mint append + mint_index_map updates,
   - per-entry seqlock writes and rollback on fork switch.
6. Implement strict Raydium V4 O(1) decode adapter and deterministic account flattening.
7. Implement Tier‑0 money workers: dequeue → decode → state read → quote → tip (fraction LUT) → exec fence CAS → inventory reserve → template build → sign → enqueue (zero alloc, zero locks, zero syscalls).
8. Implement the risk reconciliation loop in `shadow-pm` to reconcile `/dev/shm/shadow_inventory_fence` against confirmed on-chain state.
9. Implement oracle snapshot publishing:
   - `alpha_lcb` inputs per entity/venue,
   - tip LUT as **fractions** with shifted rounding and whale scaling (Section 6.6),
   - deterministic fingerprint registry (optional).
10. Implement latency microbench + release gates:
   - worker p99 dequeue→enqueue,
   - seqlock retry rates under writer pressure,
   - exec fence contention under burst,
   - nonce ring empty-pop rate,
   - end-to-end profitability telemetry.

---
## 14. What "done" means (money-grade)

V6.2.2 is "done" only when:

- Tier‑0 **money-worker** p99 (`feed_ring_dequeue → egress_ring_enqueue`) ≤ **150 µs** on production hardware (Section 0.3).
- Zero blocking waits and zero kernel syscalls in the money-worker loop (verified via `strace` / `perf trace`).
- Crypto nonce rings do not underflow in steady-state (empty-pop rate effectively zero); signing p99 is consistent with the precompute design (Section 8.2.1).
- Dynamic pool + mint append works live: new pools/mints can be indexed and traded without restart (Section 5.4).
- Exec fence gating occurs before inventory reservation; inventory fence contention remains bounded under burst tests (Section 6.1).
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
- `shadow-state` seqlock writer MUST run RT (`SCHED_FIFO`) on its own isolated core (Section 4.2.1). Proxies MAY also run RT if they remain well-behaved.

### 15.4 Kernel bypass memory
If using DPDK/AF_XDP/ef_vi:
- pre-allocate hugepages (2MB or 1GB as supported) for packet buffers and rings,
- lock critical mappings (`mlockall`) for `shadow-state` and Tier‑0,
- monitor NIC ring occupancy and drops (RX/TX) as first-class SLOs.

### 15.5 Core dump and telemetry hygiene
- Enforce `RLIMIT_CORE=0` and `PR_SET_DUMPABLE=0` (Section 0.5.4).
- Ensure all telemetry agents and crash collectors are **disabled** on Tier‑0 hosts.

