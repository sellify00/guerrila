# PROJECT SHADOW ENGINE

## Addendum V3.6.1: HFT, MEV & Latency Survival Patches

**Status:** Mandatory Pre-Implementation Directives  
**Context:** The baseline V3.6 specification provides an institutional-grade defense against state failures, honeypots, and capital bleed. However, strict adherence to a few of its original constraints will result in lost races or missed alpha. The following 11 architectural patches resolve physical latency bottlenecks, CPU chokepoints, and smart-money evasion tactics.

---

## PILLAR 1: Execution Latency & Physics (The Speed War)

### Patch 1.1: Local Embedded SVM Simulation (Zero-RPC)

- **The Flaw (Module 6B):** The spec mandates pre-dispatch simulation to calculate compute limits, but implicitly relies on `simulateTransaction` RPC calls. Even against a bare-metal local validator, an HTTP/WS network call takes 2–10 milliseconds. In a pre-consensus race, a 5ms penalty guarantees you lose the blockspace.
- **The Detailed Fix:** **Ban all network-based simulation on the hot path.** Embed a lightweight Solana execution environment (e.g., `lite-svm`, `mollusk`, or native `solana-bpf-loader-program`) directly inside the Rust `shadow-submit` process. Feed it the account bytes already stored in `shadow-state`'s RAM. Forward-simulation time will drop from ~5,000 microseconds to **< 100 microseconds**.

### Patch 1.2: Asynchronous Blockhash Caching

- **The Flaw (Module 6 & 7):** Generating an intent requires a recent blockhash. Relying on the target’s blockhash is dangerous (it may expire in the exact slot your backrun lands), and fetching a new one synchronously during bundle construction adds fatal network latency.
- **The Detailed Fix:** `shadow-submit` must spawn a dedicated, asynchronous background thread that aggressively polls a trusted RPC or Yellowstone stream for the `LatestBlockhash` every ~400ms. It writes this hash to a lock-free atomic pointer (e.g., `Arc<ArcSwap<Hash>>` or an `RwLock`). The hot-path transaction builder thread reads this from RAM in ~2 nanoseconds, guaranteeing ultra-fresh blockhashes with zero I/O blocking.

### Patch 1.3: Parallelized Ed25519 Signing (Rayon Pool)

- **The Flaw (Module 7):** Centralizing signing authority in `shadow-submit` is structurally safe, but Ed25519 cryptography is CPU-bound (~20–50µs per signature). If the Entry Engine "sprays" an array of 50 bundle permutations (varying Jito tips to secure the auction), a single thread will block for 1.5–2.5 milliseconds doing pure math before dispatching.
- **The Detailed Fix:** The serialization and signing phase inside `shadow-submit` must be offloaded to a parallel thread pool using Rust’s `rayon`. Construct the byte arrays and sign the permutations concurrently across all available CPU cores (`par_iter().map(...)`), then funnel the finalized bytes back to the single QUIC/TCP socket dispatch thread.

---

## PILLAR 2: MEV Strategy & Alpha Capture (The Alpha War)

### Patch 2.1: "Net Intent" Extraction (Bypassing Target Evasion)

- **The Flaw (Module 5.1):** The spec mandates a strict "copyable profile gate" (only single-hop Raydium V4 swaps). However, "smart money" wallets actively evade MEV extractors by routing trades through Jupiter, Meteora, or custom splitters. Dropping complex transactions systematically blinds the engine to the highest-conviction alpha.
- **The Detailed Fix:** Decouple "Instruction Copying" from "Trading Intent." When a target routes through an aggregator, do not attempt to copy their exact 5-hop path. Instead, update your protocol adapters to parse the CPIs and extract the **Net Intent** (e.g., *"Target spent 500 USDC to receive Token X"*). Pass this raw intent to your local quoter, which calculates and builds the backrun using the most efficient, direct single-hop path available to *your* signer.

### Patch 2.2: Bare-Metal MEV Router (Stripping Anchor Overhead)

- **The Flaw (Module 12):** The spec recommends using the Anchor framework for the Phase 2 `shadow_mev_router`. Anchor adds 10,000–20,000 Compute Units (CUs) of overhead purely for instruction routing and account deserialization. In Jito bundle auctions, competitiveness is measured strictly in **Tip per CU**. High CU overhead mathematically compresses your profit margins and forces you to overpay to land blocks.
- **The Detailed Fix:** Write the `guarded_swap` program in **pure, bare-metal, `no_std` native Rust**. Hand-write the account resolution and CPI execution to eliminate all framework bloat. The compute overhead for atomic verification must remain strictly under **~3,000 CUs**.

---

## PILLAR 3: Concurrency & IPC (The Memory Layer)

### Patch 3.1: RCU / Triple-Buffered State (Anti-Seqlock Contention)

- **The Flaw (Module 2.1):** The V3.6 `/dev/shm` model relies on seqlocks. During extreme market volatility, `shadow-state` will update pool reserves thousands of times per second. Because the seqlock sequence is marked "odd" during writes, the Entry Engine's read attempts will constantly hit odd numbers, forcing it to spin-lock (wait) or fail CRC checks right when the market is moving fastest.
- **The Detailed Fix:** Upgrade high-frequency shared memory (like AMM pool reserves) from basic seqlocks to an **RCU (Read-Copy-Update) or Triple-Buffer** pattern. The writer constructs the update in an inactive "dirty" background buffer; when complete, it performs a single atomic pointer swap. The hot-path reader *never* spin-locks; it always has instant, lock-free access to the most recently completed "clean" frame.

### Patch 3.2: Zero-Copy Cross-Language IPC Alignment

- **The Flaw (Module 2.1):** Python (`shadow-oracle`) writing directly to `/dev/shm` for Rust to read assumes perfect C-ABI memory alignment. Python's `struct.pack` and Rust's `#[repr(C)]` frequently misalign on padding or endianness, leading to silent CRC failures or corrupted floating-point conviction scores.
- **The Detailed Fix:** Do not manually construct byte arrays across language boundaries. Use a strict, zero-copy serialization framework like `rkyv` or `FlatBuffers`, OR write a tiny Rust bridge library using `PyO3` that exposes an `update_oracle_state()` method to Python. Let Rust dictate the exact memory layout and emit the `std::sync::atomic::fence` barriers on both sides of the IPC boundary.

### Patch 3.3: Validator-Grade Fork DAG for `shadow-state`

- **The Flaw (Module 4B):** Solana propagates `AccountUpdate` shreds asynchronously from `SlotUpdate` network statuses. If `shadow-state` blindly applies account shreds as they arrive, your quoting math will be polluted by "ghost state" from micro-forks that never reach consensus.
- **The Detailed Fix:** `shadow-state` must function as a lightweight consensus engine. Maintain a strict **Directed Acyclic Graph (DAG)** of slots. Buffer all incoming account updates into speculative, un-rooted trees. Only promote them to the canonical "read view" (pushed to `/dev/shm`) when the RPC explicitly confirms the slot status as `Confirmed` or `Rooted`. Drop orphaned branches instantly.

---

## PILLAR 4: SVM Mechanics & Capital Safety (The Smart Contract War)

### Patch 4.1: Expanded Token-2022 Honeypot Filters

- **The Flaw (Module 5):** The V3.6 risk engine correctly rejects `FreezeAuthority` and `TransferHook`. This is incomplete for Token-2022 and leaves capital exposed to newer honeypot vectors.
- **The Detailed Fix:** Add two more mandatory hard-rejects to the `shadow-state` invariant cache and hot-path gates:
  1. **`InterestBearingConfig`:** Token amounts change dynamically based on timestamps, completely breaking exact-out quote math and forward-simulation.
  2. **`PermanentDelegate`:** A backdoor allowing the token creator to arbitrarily burn, seize, or transfer your tokens after you buy them.

### Patch 4.2: Immortal WSOL ATA (Compute Preservation)

- **The Flaw (Module 11):** The spec mandates closing empty token ATAs on full exit to reclaim rent. Doing this to the bot's Wrapped SOL (WSOL) ATA is a massive mistake. Re-creating a WSOL ATA on the very next entry trade costs valuable compute units and instruction bytes, severely handicapping your bundle competitiveness.
- **The Detailed Fix:** Add a hardcoded exemption to the cleanup routine: **never close the core WSOL ATA**. Keep exactly one WSOL ATA open permanently per process/signer. Reclaim rent only on target SPL tokens. For WSOL, simply sync the balance and use explicit wrap/unwrap instructions to manage liquidity.

### Patch 4.3: Whitelist-Driven Transfer Bypass

- **The Flaw (Module 11):** The "Transfer Bypass" lifeboat rule attempts to classify token destinations dynamically as "CEX" vs "Unknown EOA" vs "Staking Vault" to trigger emergency dumps. On Solana, everything is just a base58 address; heuristic guessing in real-time will result in massive false-positive liquidations when targets legitimately lock or stake tokens.
- **The Detailed Fix:** Maintain a hardcoded, frequently updated registry of known safe DeFi Program IDs (e.g., Kamino, Meteora, Raydium lock programs, PinkSale). If a target transfers tokens, only trigger the emergency liquidation if the destination owner is *not* on this strict whitelist.
