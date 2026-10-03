# Shadow Engine V6.4-Guerrila — Handoff Status Memo (2026-04-06)

**Status:** Updated handoff memo for the next engineering chat
**Supersedes:** `Shadow_Engine_V6.4_Guerrila_Handoff_Status_Memo_2026-04-01.md`
**Project posture:** Guerrila remains the active proving profile. Full public-war V6.4 is still **not** the near-term deployment target for a 150 SOL treasury.

---

## Current conclusion

The project is materially ahead of the 2026-04-01 memo. **G4 and G5 are both non-host-complete — the only remaining open items on both milestones require a physical commodity host (G1).**

The system is still **not** end-to-end runnable because G1 (host), G7 (Harmonic proxy), G8 (PM), G9 (oracle), G10 (template audits), G11 (Rosetta), and G12 (rollout) remain open. But the primary software body — from AF_XDP ingress through FEC recovery through the money-worker pipeline through Jito egress — is now fully built and tested on the software side.

The doctrine is unchanged:
- private-lane first,
- public execution amputated,
- preserve the 150 SOL treasury,
- do not reopen public-lane fee bleed,
- do not let optional cleverness derail the missing core body.

---

## What changed since the previous memo

The 2026-04-01 memo said "the primary AF_XDP/XSK ingress body is not live" and "G5 remains partial and not fully certified." Both of those statements are now obsolete.

### 1. AF_XDP ingress is fully built (G4 software-complete)

Since the previous memo, the following were completed:
- **Real XSK socket layer** (`xsk.rs`): raw Linux syscalls via libc, bind/prime/poll/drain/recycle, HW RX timestamps from XDP headroom, multi-queue UMEM partitioning. `#[cfg(target_os = "linux")]` gated.
- **Canonical shred parser** (G4-F1): `ShredMeta::parse()` in shadow-ingest is the single authoritative parser. Tier-0 delegates to it.
- **Reassembler-driven AF_XDP** (G4-F2): live path is parse_shred → entry_data trim → reassembler.ingest → collect_ready_data → extract_wire_txs → decode_and_emit. No per-shred shortcut.
- **FEC recovery engine** (G4-F3): Full GF(2^8) Reed-Solomon with bounded FEC-set store, compile-time log/exp tables, Vandermonde matrix, Gauss-Jordan inversion.
- **Recovered-data reinjection** (G4-F4): recovered shards re-enter the same contiguous/drain path in index order. Reassembler owns recovery as a zero-cost generic.
- **Timestamp provenance + seal** (G4-F5): `IngressRecord.unix_ns` proven to originate from RX metadata end-to-end. OOO buffer preserves original buffered HW timestamps. Recovered shreds carry the triggering frame's timestamp.

G4 is software-complete. The **only** remaining open item is the **real-host ingress performance seal** (G4-F5 item 12), which requires AF_XDP on a live NIC with XDP_ZEROCOPY, HW timestamps, and NUMA-pinned cores — i.e., it requires G1.

### 2. Money worker is non-host-complete (G5 software-complete)

Since the previous memo, the following were completed:
- **Thread bootstrap through Jito enqueue** (G1a–G1e): feed ring + Jito egress ring open, thread-local state (SigDedupe, PoolOverlay, NonceRing), early-drop gates (health, staleness, pool/mint tag-map + arena), route hydration bridge, pipeline execution wiring with inventory rollback.
- **Live loop seal** (G1f): NullEgressRing for Harmonic, Jito-only gate, 9 seal tests.
- **Real execution-deps boot** (G2): signer, TipLut, guard pubkeys with zero/placeholder rejection, inventory ceiling, nonce prefill clamped to 127.
- **Nonce provisioning** (G2-2): SharedNonceRing (lock-free SPSC), background producer thread per worker, worker drain before each process_intent.
- **Full success-path integration test** (G2-3): IngressRecord → all gates → hydrate → build → quote → policy → template → sign → Jito enqueue, end-to-end.
- **Pre-sign egress gate + dominance gate integration proofs** (G2-5).
- **Swap template validation** (G2-6): config-driven loading, fatal on empty/oversized.
- **G2 QC fixes**: nonce prefill 127, non-lossy drain, alignment-safe LUT loader, guard pubkey validation.
- **ShadowGuard BPF program** (G10 partial): implemented, 13 tests, builds with cargo-build-sbf, deployed to local validator. Mainnet deploy deferred.
- **Thread-local dirty overlay**: production overlay read-through wired in `pipeline.rs:268-281` — `overlay.get()` applies i128-clamped deltas before quoting; `overlay.apply()` fires only on successful egress enqueue. 7 integration tests prove miss/hit/accumulation/failed-enqueue/isolation paths.
- **No-alloc seal**: structural scan of 4 hot-path functions (13 banned patterns) + runtime saturation proof (SigDedupe, PoolOverlay, NonceRing all fixed-capacity).
- **Hot-path guard tests**: no leader pubkey map lookup, no Mutex/Condvar/RwLock/sleep in hot loop, lane-health is precomputed mmap.

G5 remaining open items are **all blocked on G1 commodity host**:
- Real strace smoke (structural regression guard already in place)
- Dequeue → egress microbench (needs live worker loop on pinned cores)

Other G5 open items are owned by later milestones:
- Taylor pre-filter: dormant, scalars owned by G3/G9
- Selective Death Clamp classification: owned by G8
- `harmonic_tactic_mask` full consumption: G8 classification
- Whale/apex tests: G9
- ATBA tests: dormant behind G3/CLMM

---

## Test counts

| Crate | Tests | Status |
|-------|-------|--------|
| shadow-core | 181 | all pass |
| shadow-ipc | 209 | all pass (7 ignored stress gates) |
| shadow-ingest | 125 | all pass |
| shadow-state | 152 | all pass |
| shadow-tier0 | 550 | all pass |
| shadow-guard | 13 | all pass |
| **Total** | **1,230** | **0 failures, 0 warnings** |

Run tests per-crate (`cargo test -p shadow-tier0`), NOT `--workspace`.

---

## Current milestone picture

### Done
- **G0** — workspace, feature gates, Guerrila guardrails
- **G2** — shared-memory primitives, rings, RCU, leader schedule, NACK
- **G3** — `shadow-state` on shared Geyser + state publishing (1-hour mainnet soak passed)
- **G6** — Jito egress proxy (real Searcher API client, per-worker rings, NACK, coarse health)

### Non-host-complete (blocked on G1)
- **G4** — AF_XDP ingress, canonical shred parser, reassembler, FEC recovery, timestamp provenance. All software done. Only open: real-host performance seal.
- **G5** — Money worker core. All non-host software done (overlay, no-alloc, hot-path guards, pre-sign gate, dominance gate). Only open: strace smoke + microbench.

### Partial
- **G10** — ShadowGuard BPF program implemented and tested. Template CU audits not started. Mainnet deploy deferred (needs funded keypair).

### Not started
- **G1** — commodity host (hardware procurement, BIOS tuning, AF_XDP bringup on real NIC)
- **G7** — Harmonic egress proxy
- **G8** — `shadow-pm` inventory truth and quarantine logic
- **G9** — `shadow-oracle` dual LUTs + Guerrila target filter
- **G9A** — additive quant tactics
- **G11** — `shadow-control` Rosetta Stone auto-heal
- **G12** — observe-only / tiny-cap live rollout / scale ladder
- **G13** — graduation criteria to full V6.4

---

## Crate map and key files

```
c:\Shadow Engine\Guerrila\
├── crates/
│   ├── shadow-core/src/
│   │   ├── pipeline.rs        — process_intent: overlay read → lookup → quote → policy → emit
│   │   ├── assembly.rs        — PoolOverlay, emit_prepared, template build, Ed25519 signing
│   │   ├── quote.rs           — CPMM forward projection + backrun quote
│   │   ├── policy.rs          — TipLut, fee clamp, dominance gate
│   │   ├── routing.rs         — RouteDecision, IntentKind, signal source
│   │   ├── routing_runtime.rs — derive_route_inputs_guerrila, GuerrilaRoutingConfig
│   │   ├── inventory.rs       — InventoryFencer (atomic fetch_add + rollback)
│   │   ├── lookup.rs          — resolve_required_state → LookupBundle
│   │   ├── config.rs          — ShadowConfig, IngressConfig, ExecutionConfig
│   │   └── pda.rs             — PDA + ATA derivation
│   ├── shadow-ipc/src/        — all shm segment layouts, rings, RCU, tag maps, arenas
│   ├── shadow-ingest/src/
│   │   ├── reassembler.rs     — slot-scoped shred reorder, contiguous/drain, recovery inject
│   │   ├── recovery.rs        — FecRecoveryEngine, GF(2^8) RS, bounded FEC-set store
│   │   └── shred_meta.rs      — canonical ShredMeta::parse()
│   ├── shadow-state/src/      — Geyser source, decoders, fork-aware store, publishers
│   ├── shadow-tier0/src/
│   │   ├── main.rs            — Tier-0 entry: boot_pipeline_deps, boot_execution_deps, AF_XDP + gRPC spawn
│   │   ├── money_worker.rs    — worker thread: early-drop → route → build_intent → process_intent
│   │   ├── boot_exec.rs       — signer, TipLut, guard pubkeys, nonce seeding, ExecutionDeps
│   │   ├── nonce_producer.rs  — SharedNonceRing (SPSC), background producer, drain
│   │   ├── sig_dedupe.rs      — SigDedupe fixed ring (256×u64, L1-resident)
│   │   ├── xsk.rs             — real XSK socket layer (Linux only, raw syscalls)
│   │   ├── afxdp_producer.rs  — shred → wire-tx extraction, AF_XDP normalization
│   │   └── grpc_normalize.rs  — gRPC fallback normalization (only file allowed to use SystemTime)
│   └── shadow-egress-jito/src/ — Jito proxy: session, dequeue, NACK, health
├── programs/shadow-guard/     — on-chain BPF program (AssertReservesInRange)
└── config.guerrila.yaml       — runtime config with execution section
```

---

## Architecture invariants the next chat must respect

1. **Clock isolation**: Only `grpc_normalize.rs` may use `SystemTime`/`Instant::now`/`chrono`. All other tier0 source files are scanned by structural tests.
2. **No-alloc hot path**: `money_worker_loop`, `build_worker_intent`, `process_intent`, `emit_prepared` must never allocate. Structural tests scan for 13 banned patterns. Runtime tests prove fixed-capacity saturation.
3. **Overlay-only-on-success**: `overlay.apply()` fires only inside `emit_prepared` after successful egress ring push. Failed intents leave the overlay unchanged.
4. **Nonce ring capacity**: 128 slots, 1 sentinel = 127 usable. `seed_nonce_ring` caps at 127. Boot clamps config values > 127.
5. **Guard pubkey validation**: Boot rejects all-zeros and all-three-identical guard pubkeys.
6. **No workspace-wide operations**: `cargo test --workspace` is forbidden. `cargo clean` (full) is forbidden (WDAC blocks subsequent builds). Use per-crate operations only (`cargo test -p shadow-tier0`, `cargo clean -p shadow-tier0`).
7. **Production overlay read-through**: `process_intent` at pipeline.rs:268-281 copies `lookup_bundle.pool`, applies `overlay.get()` with i128-clamped arithmetic, and feeds adjusted reserves into the quote path. This is live production code, not a test-only stub.

---

## What to build next

The critical path is now **G1 → G7 → G8 → G9 → remaining G10 → G11 → G12**.

### G1 unblocks G4 + G5 seal
Once a commodity host is provisioned:
- Run real AF_XDP on a XDP_ZEROCOPY NIC
- Run `strace -f -e trace=clock_gettime,gettimeofday,time` on worker threads → zero hits
- Run dequeue→egress microbench on pinned cores
- Stamp G4 and G5 as DONE

### G7 is the next software milestone
Harmonic egress proxy: regional gRPC clients, Death Clamp enforcement, coarse health publication. Mirrors what G6 did for Jito.

### G8 follows G7
`shadow-pm`: pending table, NACK fast-release, BundleState terminal paths, Selective Death Clamp classification, `harmonic_tactic_mask` publication.

### Do not build now
- Public-lane resurrection
- TPU rescue logic
- SWQoS / relay spend
- Write-lock saturation discounting
- Splinter-cell arbitrage
- Any cross-worker or shared-memory dominance coordinator

---

## What another chat should say if asked for current status

> Guerrila is still the active proving profile. G0/G2/G3/G6 are done. G4 (AF_XDP ingress, canonical shred parser, FEC recovery, timestamp provenance) and G5 (money worker: overlay, no-alloc, hot-path guards, dominance, pre-sign gate) are both non-host-complete — the only remaining open items require a physical commodity host (G1). G10 is partial (ShadowGuard BPF program done, template audits open). G7/G8/G9/G11/G12 are not started. 1,230 tests pass across 6 crates, 0 failures. The system is not end-to-end runnable until G1 host, G7 Harmonic proxy, G8 PM, G9 oracle, G10 audits, G11 Rosetta, and G12 rollout proof are completed.

And if asked what to build next:

> **Procure the G1 commodity host to unblock G4+G5 seal, then build G7 Harmonic proxy.**

---

## Authoritative documents

| Document | Purpose |
|----------|---------|
| `V6.4_Guerrila_Checklist_G4_UPDATED_2026-04-01.md` | Active checklist — source of truth for milestone status |
| `Instructions.md` | Guerrila spec / build instructions |
| `SubInstructions.md` | Sub-instructions for specific build tasks |
| `config.guerrila.yaml` | Runtime config with execution section |
| This memo | Handoff context for AI agents / supervisors |

---

## Business posture remains unchanged

Guerrila still exists to:
1. preserve capital,
2. prove the private-lane slice,
3. measure whether the 150 SOL private-lane-only weapon actually retains EV,
4. earn the right to scale only after evidence.

Do not promote full Shadow Engine to the near-term target.
Do not reopen public execution.
Do not let additive tactics become an excuse to skip the missing host and proxies.

Guerrila is still the proving weapon.
