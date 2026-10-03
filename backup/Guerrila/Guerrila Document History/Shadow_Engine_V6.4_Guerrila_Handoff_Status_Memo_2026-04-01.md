# Shadow Engine V6.4-Guerrila — Handoff Status Memo (2026-04-01)

**Status:** Updated handoff memo for the next engineering chat  
**Supersedes:** `Shadow_Engine_V6.4_Guerrila_Handoff_Status_Memo_2026-03-14.md`  
**Project posture:** Guerrila remains the active proving profile. Full public-war V6.4 is still **not** the near-term deployment target for a 150 SOL treasury.

---

## Current conclusion

Guerrila is still the right project posture.

The doctrine is unchanged:
- private-lane first,
- public execution amputated,
- preserve the 150 SOL treasury,
- do not reopen public-lane fee bleed,
- do not let optional cleverness derail the missing core body.

The four redlines still govern all decisions:
1. do not hinder speed,
2. do not cause overhead,
3. do not bleed capital,
4. do not sacrifice major alpha.

The project is materially ahead of the 2026-03-14 memo, but it is still **not** end-to-end runnable.

---

## What changed since the previous memo

The biggest delta is that **G4 is no longer “not started” at the code/module level**.

### 1. G4-A is materially done
The fixed contract is now real:
- `IngressRecord` is frozen,
- `RawArchiveRecord` is frozen,
- `/dev/shm/shadow_feed_ring_w{worker}_p{producer}` is the fixed feed-ring path,
- `/dev/shm/shadow_raw_archive` is the fixed raw-archive segment,
- signal-source constants are now canonicalized in `shadow-ipc`.

### 2. G4-B is materially done
The pure ingest core exists:
- slot-scoped fragment tracking,
- bounded reorder buffers,
- same-slot-only crossover,
- late-duplicate handling,
- positive-gap handling,
- replay harness,
- drained payload handoff for formerly buffered shreds.

### 3. G4-C is materially done
The shared decode pipeline exists and is transport-neutral:
- Solana message parsing,
- all-signer allowlist prefilter,
- deterministic ALT expansion,
- flattening into fixed accounts,
- strict Raydium V4 decode,
- header-only obscurity scoring,
- mint resolution seam,
- deterministic worker routing,
- final `IngressRecord` emission.

### 4. G4-D is materially done at the module layer and much closer to live
The gRPC fallback side is no longer just a thin paper design:
- `grpc_normalize.rs` exists with a strict timestamp contract,
- `process_grpc_tx()` is the thin adapter into the shared G4-C pipeline,
- `grpc_producer.rs` owns topology validation, producer-id assignment, feed-ring emission, reconnect/backoff logic, and transport counters,
- `shadow-tier0/src/main.rs` now has a real Yellowstone fallback connect/subscribe/drain loop,
- Tier-0 now boots pipeline dependencies through `boot_pipeline_deps()` instead of assuming permanent `NoOp*` behavior.

### 5. New resolver sidecars now exist
A major integration seam is now real:
- `shadow-state` publishes a `pool_meta` sidecar,
- Tier-0 has `ShmAltResolver`,
- Tier-0 has `ShmMintResolver`,
- boot-time allowlist loading exists (`FileAllowlist` / `PassAllAllowlist`),
- there is also a file-backed mint-cache fallback for early bringup.

This matters because gRPC fallback can now, in principle, run the real decode path with real ALT/mint backing instead of a permanent `NoOp*` stack.

### 6. G4-E has real ownership/bootstrap/FDR scaffolding, but the primary AF_XDP path is still not live
The AF_XDP side is ahead architecturally, but it is still the main missing body:
- shared UMEM bootstrap exists,
- raw archive multi-lane segment exists,
- producer identity / queue ownership / archive publication / delay FIFO / survival flush code exist,
- startup wiring for AF_XDP resources exists in `shadow-tier0`.

But:
- there is still **no real XSK socket implementation**, no eBPF/XSK bind path, and no real RX polling loop on live packets,
- the AF_XDP thread in `main.rs` is still a bind-retry scaffold with placeholder hot-loop behavior,
- the AF_XDP thread still uses `NoOpAllowlist` / `NoOpAltResolver` / `NoOpMintResolver` in that scaffolded path,
- therefore the **primary Tier-0 ingress path is still not live**.

This is the single most important current limitation.

---

## Current milestone picture (practical, not checklist-drifted)

### Clearly done
- **G0** — workspace / feature gates / Guerrila guardrails
- **G2** — shared-memory primitives, rings, RCU, leader schedule, NACK foundation
- **G3** — `shadow-state` on shared Geyser + state publishing
- **G6** — Jito egress proxy

### Partially done but not sealed
- **G4** — substantial progress now exists:
  - **G4-A** contract freeze: materially done
  - **G4-B** ingest core + replay harness: materially done
  - **G4-C** shared decode pipeline: materially done
  - **G4-D** gRPC fallback producer: materially done, and now much closer to live runtime
  - **G4-E** AF_XDP transport/FDR shell: partial only
  - **G4-F/G/H** still open
- **G5** — worker core remains partial; logic is strong, certification is still open

### Still open
- **G1** host determinism / kernel-bypass bringup on the real host
- **G7** Harmonic egress proxy
- **G8** `shadow-pm`
- **G9** `shadow-oracle`
- **G9A** additive quant tactics milestone
- **G10** on-chain `ShadowGuard` + exact template audits
- **G11** `shadow-control` Rosetta auto-heal
- **G12** rollout ladder / observe-only / tiny-live proof
- **G13** graduation criteria to full public-war V6.4

---

## What is already strong

### Shared-memory/state spine is real
Still true from the prior memo, and stronger now:
- fixed-layout shm segments,
- wait-free RCU/double-buffer substrate,
- leader schedule + leader-class tags,
- pool/mint/ALT publish path,
- health snapshot publication,
- pool metadata sidecar for ingest mint resolution.

### `shadow-state` is real and now publishes the extra sidecar ingest needed
Still materially true:
- rooted canonical store,
- live fee freshness,
- ALT publication,
- leader-class publication,
- dynamic append behavior,
- read-only soak proof.

New integration value:
- pool metadata sidecar now exists to support Tier-0 mint resolution.

### Jito private lane remains real
Still materially true:
- Jito proxy crate,
- Searcher API session,
- per-worker egress rings,
- exact post-sign byte preservation,
- NACK path,
- coarse egress health.

### gRPC fallback is no longer just a dead shell
This is the most important new runtime improvement outside AF_XDP.

The newer `shadow-tier0/src/main.rs` now:
- boots pipeline dependencies,
- builds a Tokio runtime for each gRPC ingest thread,
- connects to Yellowstone,
- subscribes to transaction updates,
- normalizes them,
- and feeds them into the shared pipeline/emit path.

That said, this is **fallback/redundancy**, not the primary Tier-0 body.

---

## What is not true yet

Do **not** let the next chat describe the project as:
- complete,
- deploy-ready,
- fully integrated end-to-end,
- observe-only ready,
- tiny-live ready,
- or “G4 done.”

That is still false.

The best current one-paragraph description is:

> Guerrila remains the active proving profile. The project is materially stronger than the 2026-03-14 handoff: G4-A/B/C are now real in code, G4-D gRPC fallback is substantially built and wired into Tier-0 startup, and shared-memory ALT/mint resolver sidecars now exist. But the system is still not end-to-end runnable because the primary AF_XDP/XSK ingress body is not live yet, G5 is not fully certified, and G7/G8/G9/G10/G11/G12 remain open.

---

## The single most important current blocker

**Real XSK/AF_XDP ingress is now the next build target.**

Do not let the next chat get distracted.

At this point, the correct interpretation is:
- gRPC fallback should continue to be improved because it is now close to live and useful for redundancy/audit/warm-start,
- but the **real next organ** is still the missing AF_XDP/XSK body.

The AF_XDP module is currently honest scaffolding, not a live primary ingress path.

---

## Exact next move

### Build now
1. **Finish the gRPC path functionally** by ensuring the Tier-0 runtime uses the real pipeline dependency stack everywhere the new main intends to use it.
2. **Implement real XSK/AF_XDP now**:
   - real XSK socket type / bind path,
   - shared UMEM registration,
   - fill/completion ring management,
   - RX batch polling,
   - hardware RX timestamp extraction,
   - AF_XDP frame normalization into the existing ingest core,
   - feed-ring emission,
   - raw-archive publication.
3. **Only after real AF_XDP exists**, move to **G4-F** for broad-spectrum shred matrix and FEC/recovery.
4. Then continue against the live ingress path with:
   - remaining **G5 certification gates**,
   - then **G7 / G8 / G9 / G10 / G11 / G12** in the normal private-lane order.

### Do not build now
- public-lane resurrection,
- TPU rescue logic,
- SWQoS/public relay work,
- broad public-war optimizations,
- optional quant cleverness that distracts from the missing primary ingress body.

---

## Interpretation of the latest Tier-0 main

There are multiple uploaded `main.rs` revisions from different review passes.

**Treat the authoritative `shadow-tier0/src/main.rs` as the later patched version** — the one that includes:
- `mod file_allowlist;`
- `mod pool_mint_cache;`
- `mod shm_alt_resolver;`
- `mod shm_resolvers;`
- `PipelineDeps`
- `boot_pipeline_deps()`
- `spawn_grpc_ingest_workers(..., deps)`

That is the version that reflects the latest integration direction.

Do **not** anchor on the older `main.rs` variant that still hard-wires `NoOpAllowlist` / `NoOpAltResolver` / `NoOpMintResolver` into the gRPC thread body.

For AF_XDP, however, even the latest main is still only a scaffold until XSK exists.

---

## What another chat should say if asked for current status

Use this:

> Guerrila is still the active proving profile. G0/G2/G3/G6 are done. G4 has moved materially forward in code: A/B/C are effectively built, D (gRPC fallback) is substantially wired, and E has real ownership/FDR scaffolding. But G4 is not actually finished because the primary AF_XDP/XSK path is still not live. G5 remains partial and not fully certified. The system is still not end-to-end runnable until real AF_XDP ingress, Harmonic, PM, oracle, ShadowGuard audit, Rosetta control, and rollout proof are completed.

And if asked what to build next:

> **Implement real XSK/AF_XDP now. Do not drift into optional tactics or public-lane complexity.**

---

## Business posture remains unchanged

The business truth did **not** change just because the code got better.

Guerrila still exists to:
1. preserve capital,
2. prove the private-lane slice,
3. measure whether the 150 SOL private-lane-only weapon actually retains EV,
4. earn the right to scale only after evidence.

Do not promote full Shadow Engine to the near-term target.
Do not reopen public execution.
Do not let additive tactics become an excuse to skip the missing ingress body.

Guerrila is still the proving weapon.
