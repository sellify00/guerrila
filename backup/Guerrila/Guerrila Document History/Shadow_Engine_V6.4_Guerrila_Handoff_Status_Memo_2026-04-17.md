# Shadow Engine V6.4-Guerrila — Handoff Status Memo (2026-04-17)

**Status:** Final post-G1-closure handoff
**Supersedes:** `Shadow_Engine_V6.4_Guerrila_Handoff_Status_Memo_2026-04-06.md` (archived to Document History)
**Project posture:** Guerrila remains the active proving profile. Full public-war V6.4 is still **not** the near-term deployment target for a 150 SOL treasury.

---

## Current conclusion

G0 through G6 are done. G1 commodity host is sealed and shut down. G7 (Harmonic proxy), G8 (shadow-pm), and G9 (shadow-oracle) are implemented with QC passed. The entire hardware + software body — from commodity host through AF_XDP ingress through FEC recovery through the money-worker pipeline through Jito+Harmonic egress through PM reconciliation through oracle LUT publishing — is built, tested, and host-sealed.

The system is still **not** end-to-end runnable because G10 (template audits), G11 (Rosetta), and G12 (rollout) remain open.

**Next milestone: G10** (template CU audits, ShadowGuard mainnet deploy).

The doctrine is unchanged:
- private-lane first,
- public execution amputated,
- preserve the 150 SOL treasury,
- do not reopen public-lane fee bleed,
- do not let optional cleverness derail the missing core body.

---

## Milestone status

| Gate | Status | Notes |
|------|--------|-------|
| G0 | **DONE** | Workspace, feature gates, config guardrails |
| G1 | **DONE** | g1-host-a sealed 2026-04-17, shut down. Intel X710 (`i40e`), AF_XDP zero-copy capable. See `g1-host-pack/G1_CLOSURE_BUNDLE.md` |
| G2 | **DONE** | Shared memory, rings, RCU, leader schedule, NACK |
| G3 | **DONE** | `shadow-state` on shared Geyser + state publishing (1-hour mainnet soak) |
| G4 | **DONE** | AF_XDP ingress, shred parser, reassembler, FEC recovery, timestamp provenance. Real-host seal: 4/4 scenarios pass (baseline, reorder, FEC-degraded, archive-pressure). Rerun confirmed reproducible. |
| G5 | **DONE** | Money worker core. Strace: zero banned syscalls. Microbench: p99=8,979ns on host (13.6ns process_intent on Windows). Well under 150us budget. |
| G6 | **DONE** | Jito egress proxy (session, dequeue, NACK, health) |
| G7 | **IMPLEMENTED** | Harmonic egress proxy (QC passed). shadow-harmonic crate: multi-region gRPC, Death Clamp, health with hysteresis, NACK, result ring. 195 tests. |
| G8 | **IMPLEMENTED** | shadow-pm (QC fixes #2-#17 done). PendingTable, reconciler, fence coupling, PM loop, live evidence plane, orphan classification, rejection threading, revert discrimination, UUID threading, rejection hints, lane-neutral bundle source. |
| G9A | **NOT STARTED** | UUID threading piece merged into G8 QC Fix #15, but actual G9A deliverables (CU-density optimization, exhaust scavenging, tick-boundary sabotage telemetry) not implemented. Depends on G10 template audits. |
| G9 | **IMPLEMENTED** | shadow-oracle (QC passed, fixes #6-#13). Dual banked LUTs, baseline+competition-adjusted bank builders, 3-way target policy (ATBA deferred pending CLMM sidecar), crowd heat with slot-based decay, telemetry wired into tick loop, venue_id+archetype threading. Source defaults aligned to Guerrila minimum patch. 52 oracle tests, 1,902 total. |
| G10 | **PARTIAL** | ShadowGuard BPF program done (13 tests). Template CU audits open. Mainnet deploy deferred. |
| G11 | NOT STARTED | shadow-control Rosetta Stone auto-heal |
| G12 | NOT STARTED | Observe-only / tiny-cap live rollout / scale ladder |
| G13 | NOT STARTED | Graduation criteria to full V6.4 |

---

## Test counts (verified 2026-04-20)

| Crate | Tests | Status |
|-------|-------|--------|
| shadow-core | 418 | all pass |
| shadow-ipc | 259 | all pass (7 ignored stress gates) |
| shadow-ingest | 152 | all pass |
| shadow-state | 152 | all pass |
| shadow-tier0 | 575 | all pass |
| shadow-oracle | 52 | all pass |
| shadow-harmonic | 177 | all pass |
| shadow-egress-jito | 104 | all pass (1 ignored smoke) |
| shadow-guard | 13 | all pass |
| **Total** | **1,902** | **0 failures** |

Run tests per-crate (`cargo test -p shadow-tier0`), NOT `--workspace`.

---

## G4 real-host seal details

All 4 scenarios passed on g1-host-a, then confirmed reproducible in a second full rerun:

| Scenario | Frames | Parse | Data | Coding | FEC Recovered | Archive OK | Drop | Checks |
|----------|--------|-------|------|--------|---------------|------------|------|--------|
| Baseline | 128 | 100% | 64 | 64 | 64 | 128 | 0 | 7/7 PASS |
| Reorder | 128 | 100% | 64 | 64 | — | 128 | 0 | 7/7 PASS |
| FEC-degraded | 124 | 100% | 60 | 64 | 48 | 124 | 0 | 8/8 PASS |
| Archive pressure | 6022 | 100% | 3014 | 3008 | — | 32 | 5990 | 10/10 PASS |

Archive pressure: 401 PPS sustained, ring cap=32, `ingress_nonblocking: true`.

### Key production fixes found during seal
1. **ETH+IP+UDP header stripping** — AF_XDP delivers full Ethernet frames; `strip_udp_headers()` added to `afxdp_runtime.rs`
2. **observe_rx_timestamp** — moved to fire for ALL frames, not just parsed shreds
3. **Survival flush as recycle evidence** — DelayFifo (1M depth) never primes in short runs; survival_flush_count is valid
4. **Replay sender off-by-one** — `break` before `send` skipped last packet
5. **XDP_UMEM_COMPLETION_RING** — kernel 6.8 requires it even for RX-only sockets
6. **XSKMAP + bpf_redirect_map** — required for zero-copy bind path

### Host hardware
- AMD EPYC 9254 24-Core, 377 GB RAM, kernel 6.8.0-106
- Intel X710 10GBASE-T (`i40e` driver) — **AF_XDP zero-copy capable**
- Seals ran on veth pair (copy mode) for deterministic loopback replay; production uses real NIC with XDP_ZEROCOPY
- 4096 hugepages, single NUMA node

---

## G5 host seal details

| Test | Result | Detail |
|------|--------|--------|
| Strace smoke | **PASS** | `strace -f -e trace=clock_gettime,gettimeofday,time` on money-worker threads — zero hits |
| Dequeue-to-egress microbench | **PASS** | p99 = 8,979 ns (9.0 us) on pinned core, 150 us budget |

Local (Windows, no core pinning) microbench for reference:
- `process_intent` raw: p50=1,700ns, p95=3,400ns, p99=13,600ns
- All massively under 150 us budget

---

## G1 host status

**g1-host-a is SHUT DOWN as of 2026-04-17.**

- SSH user: `ubuntu` (key in `ssh_file.txt`, ed25519)
- Seal results archived: `g1-host-pack/g4_seal_results/` (26 files, 4 scenario dirs)
- Binaries on host: `~/Guerrila/target/release-guerrila/` (shadow-tier0 7.2M, g4-ingress-seal, g4-shred-replay)
- G5 strace/microbench results: on host at `~/Guerrila/g1-host-pack/results/11_g5_strace_smoke/` and `12_g5_dequeue_egress/` (dirs exist but results from prior session not persisted to files — pass/fail evidence is in session transcript only)
- XDP program was loaded on eno2 (prog id 555) and seal_rx (prog id 565)
- veth pair seal_rx/seal_tx was active in seal_ns namespace
- Hourly billing stopped

To reprovision: power on via hosting provider, SSH in with the current provider-supplied host address. All binaries and config remain on disk.

---

## G9 status: shadow-oracle (IMPLEMENTED, QC passed 2026-04-20)

G9 is the `shadow-oracle` binary crate. All deliverables implemented:

1. **Dual banked LUTs** — `shadow_tip_lut_base` (Harmonic/Vanilla) and `shadow_tip_lut_jito` (Jito tax-compensated), keyed by `leader_class_id` (8 classes). Double-buffer publication with Acquire/Release ordering.
2. **Guerrila target filter** — 3-way target policy: `PlainWithinCap`, `ApexPartial` (clamped to 15 SOL), `Atba` (deferred pending CLMM sidecar). 17 tests.
3. **Competition-adjusted strategy mode** — crowd heat ingestion via SPSC ring from PM, per-class penalty collapse, slot-based half-life decay. `primary_strategy_mode` defaults to `baseline`, `shadow_strategy_mode` defaults to `competition_adjusted`.
4. **Obscurity discounting** — header-only, 0-3 scoring, conservative initial floor (4500 bps = 45%). 5 tests.
5. **Telemetry** — per-class baseline/adjusted comparison, shadow PnL counters, sparse class tracking. JSON emission. 10 tests.
6. **Venue_id + archetype threading** — full 8-layer pipeline from IngressRecord through PM heat emission. `archetype_from_lamports()` for size-based classification.
7. **ATBA candidacy** — infrastructure in place, intentionally dormant until CLMM sidecar publishes `dist_to_tick_lamports`.

52 shadow-oracle tests. QC fixes #6-#10 applied.

### What to build next: G10

G10: template CU audits, ShadowGuard mainnet deploy. ShadowGuard BPF program already done (13 tests), deploy and template audits remain open.

---

## Architecture invariants

1. **Clock isolation**: Only `grpc_normalize.rs` may use `SystemTime`/`Instant::now`/`chrono`. Structural test enforces.
2. **No-alloc hot path**: 4 hot-path functions scanned for 13 banned patterns. Runtime saturation tests prove fixed-capacity.
3. **Overlay-only-on-success**: `overlay.apply()` fires only after successful egress ring push.
4. **Nonce ring capacity**: 128 slots, 1 sentinel = 127 usable. Boot clamps.
5. **Guard pubkey validation**: Boot rejects all-zeros and all-three-identical.
6. **No workspace-wide ops**: `cargo test --workspace` forbidden. `cargo clean` (full) forbidden (WDAC). Per-crate only.
7. **Production overlay read-through**: `process_intent` at pipeline.rs:268-281 is live production code.

---

## Crate map

```
crates/
  shadow-core/       — pipeline, assembly, quote, policy, routing, inventory, lookup, config, PDA
  shadow-ipc/        — all shm segment layouts, rings, RCU, tag maps, arenas
  shadow-ingest/     — reassembler, FEC recovery, canonical shred parser
  shadow-state/      — Geyser source, decoders, fork-aware store, publishers
  shadow-tier0/      — Tier-0 entry, money worker, AF_XDP, gRPC, boot, nonce, XSK
  shadow-oracle/     — Oracle binary: dual banked LUTs, competition adjustment, target filter
  shadow-harmonic/   — Harmonic egress proxy (multi-region gRPC, Death Clamp, health)
  shadow-egress-jito/ — Jito egress proxy (session, dequeue, NACK, health)
  shadow-guard/      — on-chain BPF program (AssertReservesInRange) [programs/]
```

---

## Authoritative documents

| Document | Purpose |
|----------|---------|
| `V6.4_Guerrila_Checklist_G4_UPDATED_2026-04-01_competition_strategy_patch.md` | Active checklist — source of truth for milestone status |
| `g1-host-pack/G1_CLOSURE_BUNDLE.md` | G1/G4/G5 host seal artifacts and evidence |
| `g1-host-pack/G1_RUNBOOK.md` | Host provisioning and validation runbook |
| `Instructions.md` | Guerrila spec / build instructions |
| `SubInstructions.md` | Sub-instructions for specific build tasks |
| `config.guerrila.yaml` | Runtime config with execution section |
| This memo | Handoff context for AI agents / supervisors |

---

## What another chat should say if asked for current status

> Guerrila is the active proving profile. G0-G6 done, G1 host sealed and shut down (Intel X710, AF_XDP zero-copy capable). G4 real-host ingress seal: 4/4 scenarios pass, confirmed reproducible. G5 host seals: strace zero syscalls, microbench p99=9us. G7 (Harmonic proxy), G8 (shadow-pm), and G9 (shadow-oracle) implemented with QC passed. G9A not started (UUID piece merged into G8, but actual quant tactics — CU density, exhaust scavenging, tick-boundary — not implemented). G10 partial (ShadowGuard BPF done, template audits open). 1,902 tests across 9 crates, 0 failures. Next milestone: G10 (template CU audits, ShadowGuard mainnet deploy).

And if asked what to build next:

> **Build G10: template CU audits and ShadowGuard mainnet deploy. ShadowGuard BPF program done (13 tests), deploy and template audits remain open.**

---

## Business posture remains unchanged

Guerrila still exists to:
1. preserve capital,
2. prove the private-lane slice,
3. measure whether the 150 SOL private-lane-only weapon actually retains EV,
4. earn the right to scale only after evidence.

Do not promote full Shadow Engine to the near-term target.
Do not reopen public execution.
Do not let additive tactics become an excuse to skip the missing control plane.

Guerrila is still the proving weapon.
