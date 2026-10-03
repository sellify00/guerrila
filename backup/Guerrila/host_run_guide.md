# Host Run Guide

This guide is the **new-host continuation map** for Guerrila. It is not a bug handoff for the current issue; it is the baseline procedure so the next agent can provision, shape, configure, validate, and run a fresh host without guessing.

Use this file first when a replacement or additional host is provisioned.

## Canonical references

Read these before touching the host:

1. `Shadow_Engine_V6.4_Guerrila_Handoff_Status_Memo_CURRENT_SPEED_MONETIZATION_UPDATE.md` - current cross-project truth.
2. `V6.4_Guerrila_Checklist_Unified_G9A_OPSO_Rebalancing_DPDK_2026-04-27_SPEED_MONETIZATION_UPDATE.md` - milestone and gate checklist.
3. `SH/SH_RUNBOOK.md` - cumulative SolanaHost procedure and hard-won host lessons.
4. `SH/SolanaHost-checklist.md` - staged SH milestone map and acceptance criteria.
5. `g1-host-pack/G1_RUNBOOK.md` - commodity-host / AF_XDP / XDP bringup procedure.
6. `g12-qc/00_SUPERVISOR_BRIEF.txt` plus the mirrored handoff/checklist/runtime docs in `g12-qc` - current local QC bundle for blockers, architecture gaps, and next-run context.

## What a new host is for

A new host is typically used for one or more of:

- replaying the SH host qualification path;
- re-establishing Agave + local source infrastructure;
- rebuilding and redeploying `shadow-*` binaries;
- regenerating `/etc/shadow-engine/config.guerrila.yaml`;
- rerunning observe-only, AF_XDP, or later R1 gates.

Do **not** assume a new host should immediately run live. Recreate the evidence chain from the checklist and current handoff state.

## Core truth rules

These rules must stay intact on every host:

- `external_geyser` is **not** private-grade.
- `local_validator_geyser` is **not** private-grade.
- only real `afxdp` packet ingress may satisfy private-grade AF_XDP proof.
- do not silently reopen public execution inside the private-lane Guerrila config; public execution is allowed only in an explicitly named R1-PUBLIC mode.
- **do not normalize manual wallet-pick / generated JSON allowlist flow as the default R1 path.**
- for R1, `auto_private_grade` is the intended admission path; manual allowlist is fallback/testing only.
- **R1-JITO-JSON no-send is green on `.57`**: AF_XDP/R1 reached `process_intent`, no-send backend, final no-send publication, and private-grade would-send evidence with Harmonic disabled and public fallback still amputated.
- **Operator pivot 2026-05-06:** public send is now the next explicit tiny-live route, not a hidden fallback: R1-PUBLIC-A public RPC scout first, then R1-PUBLIC-B TPU/QUIC comparison, then R1-PUBLIC-C decoder coverage expansion.
- **Current local state:** R1-PUBLIC-A foundation is implemented locally: default-disabled `public_rpc` / `r1.public_scout` config, fail-closed validation, explicit public RPC `sendTransaction` egress, and PM stop-after-first-landed behavior. The first R1-PUBLIC-C local slice is also in: Raydium CPMM exact-input decode, Raydium CPMM pool-state ingest/classification, explicit `pool_class` in PS-P0 tape rows, and the offline A/B/C/D analyzer at `run\ps_p0_abcd_report.js`. Host `.51` proved useful for software/bootstrap rehearsal and AF_XDP helper setup, but because it still only exposes about `125 GiB` RAM it has now been demoted to another constrained transfer host. Agave bootstrap there was intentionally stopped at about `49.9%` snapshot download, local RPC is down again, and the gate input still has `worker_signal_afxdp=0`, `afxdp_frames_seen=0`, and `observe_candidates_private_grade=0`. The next host-side step is no longer “wait for `.51`”; it is reprovisioning the next `256/384 GiB` host and replaying SH2/SH3-lite plus AF_XDP/private-grade proof there. The source-split latency export is also now landed locally as `g12_latency_by_source.json`, so new-host runs can archive comparable `source_to_feed`, `source_to_worker`, `source_to_process_intent`, `source_to_nosend`, and `source_to_final_shadow_event` snapshots without changing any fixed IPC contracts.
- **Operator correction 2026-05-14:** the foreground path has since moved again. `Run38` / public-RPC blackhole is now regression-only for CLMM quote, PM reserve/release, and one-shot latch proof; it is not the main business-path proof anymore. `Run39` broad-surface discovery is the active foreground path on `.15`, and `Run40` is a blocked scaffold until `shadow-tier0` gains a real TPU/QUIC sender. Broad discovery currently spans Raydium V4, Raydium CPMM, Raydium CLMM, Pump.fun AMM / PumpSwap, and Meteora DAMM v2, but broad end-to-end proof is still blocked for non-CLMM because they currently share the single configured Raydium V4 execution template.
- **Replacement host state:** `109.94.99.15` (`G-12`, dual Intel X550 / `ixgbe`) remains the active X550 proving host, but the current operator rule is now stricter than the earlier note. Historical `.15` artifacts include both shred-bearing windows on routed `eno1` and traffic-dark / zero-counter windows, so do **not** treat any saved harness result as proof that the host is currently packet-ready. The missing prerequisite behind the earlier zero-counter AF_XDP run was that `.15` was not actually running a live local Agave validator at the time: there were no UDP `8000-8025` listeners, `worker_signal_afxdp=0`, `observe_candidates=0`, and the blackhole census produced no meaningful ingress evidence. As of `2026-05-11`, `.15` is back on the correct SH1 path: `sh1-agave-no-geyser.service` is running `mainnet-beta` with ledger on `/mnt/solana-ledger`, accounts on `/mnt/solana-accounts`, RPC bound to `127.0.0.1`, and dynamic Solana ports `8000-8025`. The later guarded `eno1` AF_XDP rerun with rebuilt XDP diagnostics still proves the narrower feed-truth point: the XDP path was alive, but that sampled live window was not shred-bearing. During the 120s observe-only window, the XDP diag map saw packets on `eno1` and saw UDP hits in the Solana port range, while concurrent `tcpdump` only saw tiny `132`-byte UDP to local `8000`; the shred-variant, shred-version, and redirect counters all stayed `0`. Treat that as feed-arrival evidence, not an AF_XDP/XSKMAP bug. A direct `2026-05-14` host check then closed the stale capability question explicitly: `env.g1.sh` on `.15` resolves to `SHADOW_NIC_IFACE=eno1` and `SHADOW_XDP_ATTACH_MODE=native`, and a manual `ip link set dev eno1 xdp obj ... sec xdp` succeeded on the live host. The stock `03_afxdp_functional.sh` wrapper still printed a failure, but only because `xdp-loader` was absent and the script does not surface the successful native `ip link xdp` path clearly. The important follow-up nuance is now proven too: the real hazard on routed management `eno1` is the **native attach/detach transition**, not steady-state native AF_XDP itself. On `.15`, `ixgbe` drops carrier and reinitializes PHC / XDP queues when native XDP is entered or torn down; the first manual attach+detach sequence left `eno1` with only IPv6LL and no default route until OOB recovery restarted `systemd-networkd`. After that recovery, a second native Run38 arm under OOB showed the more precise steady-state truth: native attach caused one more short carrier flap (`11:12:12` lost carrier, `11:12:30` gained carrier), but once the link settled and XDP stayed attached, `eno1` remained routable, `afxdp_bind_mode: zero_copy` stayed active, and normal SSH from the operator IP resumed while native `xdp` remained live. Treat native AF_XDP on `.15` as **host-capable and steady-state compatible with management once settled**, but still **transition-risky on the routed management NIC** unless you have OOB coverage and avoid unnecessary XDP detach/reload churn.
- **Late regression correction 2026-05-14:** after the rescue/reboot cycle and later host work, `.15` drifted off the SH1 prerequisite path again even though older notes still described it as corrected. By the time the real DPDK Run38 timeout and the real regular AF_XDP safe/copy Run38 timeout were sampled, `.15` again had **no** local RPC on `127.0.0.1:8899`, **no** UDP listeners on `8000-8025`, and **no** live persistent SH1 unit. Those matching zero-counter runs therefore do **not** distinguish DPDK from AF_XDP; they are missing-local-source evidence.
- **Countermeasure now landed:** the main host-pack AF_XDP/DPDK/scout entrypoints fail closed on a local-Agave preflight before they arm ingress. That preflight requires a live SH1 service/process (`sh1-agave-no-geyser.service`, `sh1-agave-no-geyser-2.service`, or transient `sh1-agave-no-geyser`), local RPC on `127.0.0.1:8899`, acceptable slot lag versus the configured bootstrap/reference RPC, and bound `8000-8025` UDP listeners.
- **Always-live `.15` rule (`2026-05-15`):** for the AF_XDP operating mode, local Agave and its watchdog are baseline host hygiene, not a run-only repair step. Treat `.15` as an always-live local-validator host: keep `sh1-agave-no-geyser-2.service` enabled, keep `g1-local-agave-watchdog.timer` enabled, and treat any drift away from green local-Agave preflight as a host fault to repair before interpreting AF_XDP/DPDK/scout results. The watchdog's storage guard is part of that baseline too: it requires `--limit-ledger-size`, records the active full/incremental snapshot-retention flags, and fails closed if ledger or accounts free space drops below the configured floor (`100 GiB` each in the current repo defaults).
- **Current `.15` watchdog state (`2026-05-15`):** the watchdog timer is installed, enabled, and firing on the live host every `3 min`; the latest run correctly observed bootstrap-in-progress and left the validator running instead of flapping it. Disk growth is currently bounded and healthy on `.15` (`/mnt/solana-ledger` about `621 GiB` used / `2.9 TiB` free, `/mnt/solana-accounts` about `564 GiB` used / `3.0 TiB` free), so the immediate blocker is **not** disk pressure. The real current blocker is that local Agave is still bootstrap-red on the transient `sh1-agave-no-geyser.service`: local RPC on `127.0.0.1:8899` is still down while a newer incremental snapshot is being fetched/applied. Until bootstrap finishes and the host returns to green preflight on the preferred persistent path, do not treat `.15` as currently ready for native proof runs.
- **Current foreground state (`2026-05-15`):** Run39 broad-surface discovery is now genuinely working on `.15`; the current archived slice `/home/ubuntu/shadow-host-audit/dpdk-runs/39_broad_surface_discovery_current` finished `status=scorecard_generated` after the wrapper was fixed to arm directly from a successful prelaunch UDP probe and to log every failed watch probe. That scorecard showed `candidate_rows=10`, `held_input_candidate_rows=8`, `would_admit_rows=5`, `top_routes=[["jito_bundle", 10]]`, and `held_input_public_rpc_scout_rows=0`. Treat that as proof that the current surface is held-input `jito_bundle`, not evidence that "no candidates exist."
- **Current bounded-window truth (`2026-05-18`):** Run54 is now just what it should be: an offline all-venue current-scorecard autopsy, not a live runner. The honest live replacement is Run55, `g1-host-pack/scripts/55_all_venue_bounded_live_readiness.sh`, which is now host-proven on `.15`. Run55 starts from green local Agave preflight, runs a bounded `600s` native AF_XDP zero-copy Run39 window, classifies `public_rpc_live_ready`, `tpu_quic_no_send_ready`, and `tpu_quic_tiny_live_ready`, and only attempts live send if an actually implemented route clears. The latest host proof finished the full window but ended `status=blocked_no_implemented_live_route_ready_candidate` with `public_live_ready_count=0`, `tpu_no_send_ready_count=0`, and `tpu_tiny_live_ready_count=0`. The active blocker is therefore no longer “can `.15` run a bounded native AF_XDP proof window?”; it is current-candidate quality / source-grade / implemented-route truth.
- **Run64D inventory lesson (`2026-05-20`):** the bounded metadata-only Token-2022 acquisition executor is now real on `.15`, but the first one-mint experiment also proved the need for an explicit exit-liquidity gate. `Coinini` (`8Nh2DSKq...`) was acquired successfully with microscopic spend, yet the full balance currently exits for only `0.000001 USDC`. Treat this as a dust-test artifact, not useful held-input inventory. Run64D now blocks future acquisitions with `exit_quote_dust` unless a full-balance exit quote clears both `0.10 USDC` minimum value and `50%` of acquisition cost. Never scale metadata-held-input acquisition without both entry and exit quotes.
- **Run64D post-rerun truth (`2026-05-20`):** the first bounded Coinini acquisition has now been measured through the fresh `Run61 -> Run62 -> Run63` no-send chain, and it did **not** move live DirectArb conversion. Final counters were `directarb_inventory_ready_delta=0`, `directarb_would_fire_delta=0`, and `remaining_directarb_main_blocker=not_held_input`, even though the broader modeled unlock still reports `directarb_would_fire_if_policy_safe_held=682` and `deployable_by_us_delta=784`. Treat this as proof that the one-mint dust-band experiment was useful for executor validation and inventory-risk learning, but not as evidence that metadata-only acquisition has become a profitable live lane. Do **not** acquire the second metadata mint yet.
- **Run64D post-acquisition host rule (`2026-05-20`):** the Agave-first rule on `.15` has now been re-proven again. After any AF_XDP/observe-only window, stop `shadow-tier0` / `shadow-oracle`, detach XDP from `eno1`, restart `sh1-agave-no-geyser-2.service`, and confirm `local_health=ok`, low slot lag, live UDP listeners, and `eno1 LOWER_UP` with no XDP attached before interpreting the host as healthy. One current footgun remains: if the host does not have a real bootstrap/reference RPC override, the repo preflight can still fail closed on `reference_rpc_unconfigured` even when local Agave is green. Until that default is fixed, export explicit `SHADOW_BOOTSTRAP_RPC_URL` and `SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL` for serious `.15` runs.
- **Run41 state (`2026-05-15`):** the next proof family is targeted Raydium CLMM + Jito no-send, centered on pool `8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj`, `WSOL -> USDC`, at a conservative cap of `0.1 WSOL` (`100000000` raw). The worker signer still holds about `12.5 SOL` natively, but the current fixed CLMM template uses the pre-funded WSOL ATA from `/etc/shadow-engine/signer-token-inventory.json`; current WSOL ATA balance is `0.2 WSOL`, which is already enough for the `0.1 WSOL` proof cap, and `auto_wrap_supported=false` remains the rule.
- **Immediate `.15` blocker (`2026-05-15`):** the Run41 smoke harness is now built and deployed, but the latest smoke ended `INNER_PRECHECK_FAILED` because local Agave preflight was red (`local_rpc_unhealthy`, processed/finalized slot lag about `16190`). Do **not** treat any current Run41 `NO_TARGET_HIT` style result as market evidence until local Agave / local RPC health is green again.
- **Historical Run76 selector result (`2026-05-21` earlier):** the first selector on the old `88` historical numeric rows picked pool `GqyUGfUXjQWtks8C4xf9yBgbwT3WvdPat5iisFVc23NZ`, `WSOL -> BkK43JimdWEHnkBB7miFTUXre4h23n3qseQaqBp3AZAF`. Keep that result only as historical context for the old artifact set; it is **not** the freshest Run77 input anymore.
- **Fresh Epoch8 Run76 selector result (`2026-05-21` latest):** the fresh selector against the `44` numeric-safe Epoch8 rows now supersedes the old BkK4 pick. Selected class: pool `E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4`, `WSOL -> 456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o`, `selected_pair_count=8`, `selected_deduped_ev_usd=14.481405`, `selected_input_amount_raw=1000000000`, `selected_expected_out_raw=438884922`, `selected_target_out_raw=7297936851`, `selected_min_out_raw=434496072`, `selected_min_out_positive=true`, `selected_template_id=1`, `selected_quote_match_mode=unique_slot_pool_mint_amount_template`, `excluded_ambiguous_count=25`, `excluded_min_out_nonpositive_count=0`, and `excluded_nonunique_composite_count=0`. Treat this `E9dp... / WSOL -> 456p...` class as the current approved Run77 replay input.
- **Fresh Epoch8 Run77 replay/no-send update (`2026-05-21` latest):** the fresh replay did use the correct new scorecard target and `Run40` proved replay injection for signature `2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4` on pool `E9dp...`. But this latest attempt must be classified correctly: it armed `eno1` in **`xdpgeneric`**, not native AF_XDP zero-copy, so it is **infrastructure-only replay evidence**, not canonical AF_XDP zero-copy no-send proof.
- **Current Run77 blocker (`2026-05-21` latest-latest):** the fresh Epoch8 replay is still blocked after injection. `tpu_summary.txt` stayed empty and no final `tpu_quic_candidate_seen / would_enqueue / ring_enqueue / sender_dequeue / no_send_accept` proof was produced. The live config printed `runtime route set: {drop, jito_bundle, harmonic_bundle}`, `public execution route: unreachable`, and `quic_workers=0`, so the current boundary remains **post-injection worker-to-TPU conversion / finalization**. Treat the latest Run77 as blocked and **non-canonical** because it was `xdpgeneric` rather than AF_XDP zero-copy.

## Mandatory zero-candidate liveness ladder

Every future `0 candidates`, `0 observe_candidates`, or `0 process_intent` investigation must begin with this ladder and stop at the first red level. Do **not** jump into CLMM, WSOL, route, policy, or economics analysis before `L0-L5` are green.

```text
L0 host/service alive
L1 Agave/live source alive
L2 UDP/shred-sized traffic visible
L3 XDP/DPDK counters moving
L4 feed ring push/pop moving
L5 worker records_seen moving
L6 route/pool/mint gates moving
L7 process_intent moving
L8 rollout/no-send/live-send evidence
```

- `L0` = host reachable, service/unit alive, expected process not crash-looping.
- `L1` = the intended live source is actually up (`sh1-agave-no-geyser.service`, external source, or other named producer), not assumed.
- `L2` = same-window packet proof exists; for `.15`, this means real large UDP on `eno1`, not tiny `132`-byte gossip/bootstrap packets.
- `L3` = XDP/AF_XDP/DPDK stage counters move; zero here means do not debug worker logic yet.
- `L4` = producer/feed ring push-pop path moves; otherwise do not debug routing/policy.
- `L5` = worker-side `records_seen` / ingress-consumption counters move.
- `L6-L8` = only after ingress and worker liveness are proven should route filters, pool/mint gates, `process_intent`, and final rollout evidence be investigated.

## Critical product-shape rule: no manual candidate pipeline

This is a hard rule for every new host and every next agent: **the production R1 path is automatic hot-path admission, not operator-curated candidate selection.**

The production process must **not** become:

```text
watch traffic for a long time
-> export JSON
-> pick wallets / candidates
-> generate allowlist
-> maybe trade later
```

That flow is allowed only for:

- audit artifacts;
- debugging;
- replay;
- QC evidence.

It is **not** the product and must not be treated as the normal live path. If a host setup or runbook step requires manual wallet picking or generated candidate JSON for ordinary live admission, stop and fix the hot-path coverage instead.

The intended private-lane live path is:

```text
AF_XDP packet arrives
-> shred/tx decode
-> candidate classification
-> pool/mint/state lookup
-> quote / micro-market / ShadowGuard / egress-health checks
-> R1 auto-admission
-> build/sign/send through Jito/Harmonic private egress
```

No public fallback inside this private-lane mode. No manual wallets. No operator wait-loop. No dynamic worker-side search. The hot path must remain fixed-shape and O(1)-oriented.

### Current practical route: R1-PUBLIC-A/B/C

As of the current R1 state, Jito gRPC/Harmonic private-lane live is blocked by provider access, and private flow is too gatekept/sparse to remain the only live route:

```text
Jito auth pubkey 3i9N1VhsmKJBXB4STP4xdinmAXYCRecmNHBbLkSzwCW4 -> unauthorized
Harmonic auth pubkey 9ZaQnhU56rxrw3mkGrVHaebTFXF1FgqAdvanTPUJMPwa -> staged, but fra/ams/nrt endpoints fail channel connect
```

The **R1-JITO-JSON no-send host proof is green**:

```text
AF_XDP candidate
-> R1 auto_private_grade
-> process_intent
-> no-send backend / final no-send event
-> no Harmonic
-> no public RPC/TPU fallback
-> tiny caps
```

Green no-send artifact:

```text
/home/ubuntu/shadow-host-audit/r1-jito-json/20260501_062327-r1-jito-json-nosend-afxdp
```

The operator decision is to move live proof to explicit public execution with tiny caps while preserving the engine discipline:

```text
AF_XDP candidate
-> R1 auto_private_grade
-> bounds / ShadowGuard where applicable
-> inventory truth
-> fee-leak accounting
-> PM reconciliation
-> explicit public send
-> stop on first landing or kill condition
```

R1-PUBLIC-A:

```text
public RPC scout
max_live_trade_input_sol: 0.05
max_notional_sol_per_entity: 0.1
success condition: one landed transaction
then stop and reconcile
```

Current status 2026-05-14: **implemented, but no longer the foreground proof path**. `Run38` / public-RPC blackhole is retained as a regression harness only. The current foreground path is `Run39` broad-surface discovery on `.15`. `Run40` exists only as a blocked scaffold until `shadow-tier0` has a real TPU/QUIC sender.

R1-PUBLIC-B:

```text
direct TPU/QUIC public send
same 0.05 SOL / 0.1 SOL caps
compare landing and latency against R1-PUBLIC-A
```

Ordinary direct TPU/QUIC does **not** require Jito/Harmonic-style private authorization. It does require:

```text
current slot / leader schedule
current + next leader identities
validator TPU / QUIC contact info
pre-warmed QUIC connections where possible
fanout policy: current leader / next leader / next-N
signed transaction bytes
separate status / reconcile path
```

Treat SWQoS / staked-validator peering as a later landing-quality optimization, not a prerequisite for the first TPU/QUIC proof.

R1-PUBLIC-C:

```text
decoder coverage expansion
Raydium CPMM / CLMM / Orca / Meteora / PumpSwap
increase recognition surface without manual wallet picking
```

Current wired discovery surface:

```text
discovery-ready now:
  Raydium V4
  Raydium CPMM
  Raydium CLMM
  Pump.fun AMM / PumpSwap
  Meteora DAMM v2

not yet wired / not yet first-class:
  Orca Whirlpool
  Meteora DLMM
  explicit thin-SOL bucketing
  explicit migration / launchpad bucketing
```

Broad-surface discovery is **not** the same thing as broad end-to-end proof. CLMM has venue-specific runtime template and exact-quote handling. Non-CLMM venues still fall through to the single configured execution template, which is currently a Raydium V4 template.

Never make public send a hidden fallback from the private-lane config. If public send is used, it must be explicit, tiny-capped, and labeled as R1-PUBLIC evidence. Do not jump to `1 SOL`; the first public live is for truth, not profit.

Why R1-PUBLIC-A stays first:

```text
public RPC blackhole proof
  = safer proof harness
  = controlled transport failure / NACK
  = easier PM reserve/release evidence
  = lower accidental-send risk

TPU/QUIC
  = target production route
  = better final route shape
  = more leader / connection / fanout complexity
```

Current foreground posture:

```text
Run39 broad-surface discovery
  = foreground no-send / live-scout classifier
  = deployed on `.15`
  = waits for shred-bearing UDP on `eno1`
  = emits `surface_scorecard.json`

Run38 any-HUNT CLMM public-blackhole
  = regression-only
  = proves CLMM/public-RPC/PM/latch mechanics
  = idle windows prove CLMM public-scout scarcity only

Run40 TPU/QUIC tiny-live
  = blocked until `shadow-tier0` has a real TPU/QUIC sender path
```

Operationally, `g1-host-pack/scripts/38_any_hunt_publicrpc_blackhole_once.sh` should now be used only to keep the CLMM/public-blackhole regression harness healthy. Foreground discovery should come from `g1-host-pack/scripts/39_broad_surface_discovery.sh`, and broad execution proof must wait until non-CLMM venues have venue-correct execution templates.

Target long-run dependency map:

```text
Hot ingest:
  local Agave / AF_XDP / DPDK primary

State hydration / account reads:
  local RPC + shadow-state primary

Submission:
  direct TPU/QUIC primary

Reconcile:
  local RPC/status first

External paid RPC/gRPC:
  fallback / comparison / emergency only
```

The engine still needs RPC-like functions somewhere:

```text
latest blockhash
leader schedule / current leader
validator TPU contact info
signature status
account reads for hydration gaps
```

But those functions should converge toward our own local node/state infrastructure rather than premium providers in the hot path.

The 30-minute / 2-hour runs are **proof windows**, not the production opportunity loop. They exist to prove counters and safety:

```text
AF_XDP records are real
private-grade counters move
no-send path works
micro-market observes
ShadowGuard matches
latency proof is present
live_landed=0 in observe mode
```

For live opportunities, expected local decision time is microseconds to low hundreds of microseconds, with the existing target still:

```text
worker dequeue -> egress enqueue <= 150us p99
packet receive -> worker decision -> egress enqueue < 1ms p999
```

Network landing time is separate and depends on private-lane RTT, leader timing, bundle acceptance, and slot variance.

If AF_XDP traffic is being extracted but worker/actionability counters are not moving, do **not** invent a JSON allowlist as the fix. The right product-shape fix is decoder/actionability expansion:

```text
unsupported-program histogram
-> route_program() entries for top DEX families
-> exact decoders for the dominant families
-> pool/mint/state hydration
-> no-send proof
-> only then tiny-live
```

## Expected host shape

The repo has operated on constrained hosts, but the intended serious host shape is:

- Ubuntu 24.04
- dedicated public IP
- no NAT if selectable
- at least 12 cores / 24 threads, preferably 16 / 32+
- ideally 256 GB+ RAM
- separate NVMe roles for ledger / accounts / snapshots when possible

See `SH/SolanaHost-checklist.md` for the fuller target and staged acceptance criteria.

## Required software and dependencies

At minimum, expect to need:

- `git`
- `curl`
- `python3`
- `build-essential`
- `clang`
- `llvm`
- `cmake`
- `pkg-config`
- `libssl-dev`
- `protobuf-compiler`
- `bpftool`
- `ethtool`
- `iproute2`
- `numactl`
- `stress-ng`
- kernel headers / Linux tools

Known important detail from prior hosts: **`libclang-18-dev` was required on Ubuntu 24.04 for Agave builds.**

Also expect to need:

- Rust toolchain via `rustup`
- Solana / Agave CLI
- optional Yellowstone build dependencies if replaying SH1/SH4 work

## Host directories that matter

Standardize on these paths unless there is a strong reason not to:

```text
/home/ubuntu/Guerrila
/home/ubuntu/SH
/etc/shadow-engine
/home/ubuntu/shadow-host-audit
```

Important runtime/config artifacts:

```text
/etc/shadow-engine/config.guerrila.yaml
/etc/shadow-engine/env.g1.local.sh
/etc/shadow-engine/signer-token-inventory.json
/sys/fs/bpf/shadow_xsks_map
/home/ubuntu/Guerrila/run/g12_gate_input.json
/home/ubuntu/Guerrila/run/g12_gate_report.json
```

## Active proving host `.15` operator map

When current instructions say "work on `.15`", assume this host unless a newer handoff explicitly replaces it:

```text
host: 109.94.99.15
user: ubuntu
role: active X550 / ixgbe proving host
primary NIC: eno1
validator/source doctrine: local Agave first, AF_XDP gets a bounded lease only
```

Do not burn time rediscovering the layout. Start here:

```bash
ssh ubuntu@109.94.99.15
cd /home/ubuntu/Guerrila
cd /home/ubuntu/shadow-host-audit/dpdk-runs
```

Windows operator note:

```text
preferred identity path: C:\Users\alung\.ssh\g1_host_key
if rescue mode or a rebuild changed the host key, remove the stale 109.94.99.15 entry from C:\Users\alung\.ssh\known_hosts before reconnecting
```

### `.15` directories and files that matter most

```text
/home/ubuntu/Guerrila
  active repo root

/home/ubuntu/shadow-host-audit/dpdk-runs
  canonical host result root for run wrappers and epoch artifacts

/etc/shadow-engine/config.guerrila.yaml
  active runtime config actually consumed by host runs

/etc/shadow-engine/env.g1.local.sh
  host-local env overrides

/etc/shadow-engine/signer-token-inventory.json
  signer token-account / WSOL inventory truth

/mnt/solana-ledger
/mnt/solana-accounts
  local Agave ledger / accounts storage
```

### `.15` host-side Guerrila topology

There are **two** Guerrila layouts in practice, and the next agent must not confuse them:

```text
Windows operator workspace
  C:\Shadow Engine\Guerrila
  local edit / search / validation workspace
  not the thing the host is currently running

host repo clone
  /home/ubuntu/Guerrila
  the repo copy .15 actually builds and launches from

host wrapper-local outputs
  /home/ubuntu/Guerrila/g1-host-pack/results
  rendered configs, wrapper-local logs, helper outputs
  useful during an active run, but not the main durable archive

host live runtime scratch
  /home/ubuntu/Guerrila/run
  live gate files, shared-memory-adjacent runtime scratch, active run state

host durable result archive
  /home/ubuntu/shadow-host-audit/dpdk-runs
  canonical durable truth for epochs, run summaries, scorecards, shock_research, and post-run forensics

host runtime config truth
  /etc/shadow-engine/config.guerrila.yaml
  the config the host wrappers actually consume

host runtime inventory truth
  /etc/shadow-engine/signer-token-inventory.json
  signer inventory and WSOL/account truth used by host-side gating

host local Agave runtime
  /home/ubuntu/SH/SH1-local-agave-yellowstone/lab
  validator identity, startup scripts, and agave logs
```

Operator rule:

```text
local repo truth != host durable truth

if you want to know what .15 actually did,
read /home/ubuntu/shadow-host-audit/dpdk-runs first,
not C:\Shadow Engine\Guerrila\g1-host-pack\results
```

### `.15` services and timers to check first

```text
sh1-agave-no-geyser-2.service
  preferred persistent local validator service

g1-local-agave-watchdog.timer
  keeps the validator on the always-live SH1 path

shadow-state
  state/hydration surface when the current task depends on it
```

Quick status probes:

```bash
systemctl status sh1-agave-no-geyser-2.service g1-local-agave-watchdog.timer --no-pager
sudo ss -lupn | grep -E '127\.0\.0\.1:8899|:(800[0-9]|801[0-9]|802[0-5])'
ip -br link show eno1
sudo ip -details link show dev eno1
```

Green `.15` means:

```text
local Agave healthy
127.0.0.1:8899 answering
UDP listeners bound on 8000-8025
eno1 LOWER_UP
no stale XDP attachment unless the bounded AF_XDP window is intentionally active
```

### `.15` source / packet ownership topology

The most important `.15` rule is operational, not cosmetic:

```text
default healthy state:
  eno1 belongs to local Agave
  local Agave catches up / stays healthy
  Shadow is not attached to eno1

bounded AF_XDP proof state:
  Shadow borrows eno1 temporarily
  XDP is attached on eno1
  /sys/fs/bpf/shadow_xsks_map may exist
  shadow-oracle + shadow-tier0 run
  local Agave may fall behind during the lease

post-window recovery state:
  stop shadow-tier0 / shadow-oracle
  detach XDP from eno1
  remove stale XSKMAP if present
  restart / verify sh1-agave-no-geyser-2.service
  do not interpret host health again until local RPC is back green
```

This is the practical `.15` topology:

```text
internet / Solana UDP
  -> eno1
  -> local Agave by default

bounded AF_XDP lease only when a proof run explicitly needs it
  -> XDP/XSKMAP on eno1
  -> shadow-tier0 / shadow-oracle
  -> no assumption that Agave can keep catching up during the lease
```

If the user says Agave needs `eno1` back, believe that immediately. On `.15`, Shadow should not keep `eno1` attached while waiting for the validator to recover.

### `.15` quick navigation map

Use these first so you do not have to rediscover the host layout:

```text
/home/ubuntu/Guerrila/g1-host-pack/scripts
  host wrappers, run launchers, and operator entrypoints

/home/ubuntu/Guerrila/g1-host-pack/results
  rendered per-run configs and wrapper-local outputs

/home/ubuntu/Guerrila/run
  live gate files and active runtime scratch

/home/ubuntu/Guerrila/g1-host-pack/artifacts/release-guerrila
  binaries that the host wrappers expect to launch

/home/ubuntu/shadow-host-audit/dpdk-runs
  canonical durable run/epoch result archive

/home/ubuntu/SH/SH1-local-agave-yellowstone/lab/logs/agave-mainnet-beta-no-geyser.log
  current local Agave bootstrap / replay / catch-up log
```

Useful first commands on `.15`:

```bash
cd /home/ubuntu/Guerrila
source g1-host-pack/env.g1.sh
test -f /etc/shadow-engine/env.g1.local.sh && source /etc/shadow-engine/env.g1.local.sh
systemctl status sh1-agave-no-geyser-2.service g1-local-agave-watchdog.timer --no-pager
ls -1 /home/ubuntu/shadow-host-audit/dpdk-runs | tail
```

### Canonical `.15` artifact roots for the current foreground branch

```text
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch6_6jus_approval_propagation_test_canonical_monetization
  fresh epoch proving that the 6jUs approval override reached real Run39 runtime

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch6_6jus_approval_propagation_test_canonical_monetization_run55_run39
  paired Run39 row/autopsy artifact root for that same epoch

/home/ubuntu/shadow-host-audit/dpdk-runs/run73_epoch6_wsol_entry_meteora_ata_input_capacity
  current ATA + WSOL capacity planner output for the repeated WSOL-entry Meteora DirectArb rows

/home/ubuntu/shadow-host-audit/dpdk-runs/73b_metadata_output_ata_topup_epoch
  durable Run73B prep artifacts: ATA execution, WSOL top-up, inventory refresh, comparison plan

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch7b_run73b_metadata_output_ata_retry_canonical_monetization
  fresh retry epoch after ATA + WSOL prep

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch7b_run73b_metadata_output_ata_retry_post_run66_forensic
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch7b_run73b_metadata_output_ata_retry_post_run69_strict_surface
  strict-vs-fixup and strict-surface outputs for the post-Run73B fresh retry

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch7b_run74_strict_directarb_live_readiness
  strict DirectArb live-readiness forensic root

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch7b_run73b_metadata_output_ata_retry_canonical_monetization_run55_run39/shock_research.jsonl
  raw rollout tape currently used for Run39/Run75 quote rehydration work

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch7b_run73b_metadata_output_ata_retry_post_run69_strict_surface/directarb_strict_rows.jsonl
  strict DirectArb rows whose numeric quote preservation is the current blocker

/home/ubuntu/shadow-host-audit/dpdk-runs/75_strict_directarb_quote_preservation_proof
  current Run75 numeric quote/min-out preservation artifact root

/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_run55_run39
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_run62
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_post_run66_forensic
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_post_run69_strict_surface
/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_run75_quote_identity_proof
  fresh Epoch8 chain after the ingress-identity bridge deploy
```

### Current `.15` foreground truth as of 2026-05-21

The `6jUs` branch is no longer the critical path:

```text
approval propagation = PASS
fresh recurrence = ABSENT
```

Run73B is now materially complete on `.15`:

```text
7 output ATAs created for the repeated WSOL-entry Meteora DirectArb outputs
WSOL topped from 0.999241658 to 1.000260001
inventory refreshed from host RPC truth
```

The fresh retry epoch after that prep came back much stronger:

```text
deployable_by_us_opportunity_count = 226
would_fire_total = 219
would_fire_directarb = 136
would_fire_continuation = 20
would_fire_rebalancing = 22
would_fire_housekeeping = 41
strict_current_state_would_fire = 159
strict_current_state_ev_usd = 290.169953
directarb_strict_would_fire = 133
```

The repeated target scope moved from fixup-backed to strict:

```text
target_scope_missing_output_ata_rows: 14 -> 0
target_scope_input_balance_not_ready_rows: 14 -> 0
target_scope_strict_rows: 0 -> 126
target_scope_strict_ev_usd_delta = +3870.193832
route_leak_count = 0
jito_bundle_executable_count = 0
```

Fresh Run73 now reports `policy_verdict=none` on the seven target mints, but `precreate_allowed=false` because the signer ATAs already exist; there is nothing left to approve or create on that narrow ATA branch.

Run74 now sharpens the strict DirectArb lane itself:

```text
strict_directarb_rows = 133
strict_directarb_unique_rows = 133
strict_directarb_deduped_net_ev_usd = 287.47512
strict_directarb_deduped_retained_ev_usd = 4101.998934
all routes = tpu_quic_public
all sources = afxdp
venue split = meteora_damm_v2 127, raydium_clmm 6
size profile = all rows at 1.0 WSOL / 1000 USD equiv
```

Every strict DirectArb row is currently:

```text
quote_ready = true
inventory_ready = true
approved_held_input = true
output_account_ready = true
template_ready = true
tpu_or_public_usable = true
would_enqueue_tpu = true
```

The older `0 preserved quote` caveat is no longer the freshest truth. After the bridge deploy, `.15` now also has a **fresh Epoch8 rerun**:

```text
Run61 / fresh epoch:
  surface_rows = 9268
  deployable_by_us_opportunity_count = 111
  would_fire_total = 106
  would_enqueue_tpu_count = 106
  owner_mode_directarb_count = 222
  would_fire_directarb = 74
  jito_bundle_executable_count = 0

Run66:
  strict_current_state_would_fire = 83
  strict_current_state_ev_usd = 154.017528
  ata_fixup_would_fire = 23
  fixup_allowed_total_would_fire = 106

Run69:
  strict_row_count = 83
  directarb_strict_rows = 74
  directarb_deduped_ev_usd = 152.916773
  route_leak_count = 0
  jito_bundle_executable_count = 0

Run75 fresh:
  strict_directarb_rows = 74
  quote_numeric_preserved_count = 44
  min_out_numeric_preserved_count = 44
  quote_numeric_unique_slot_pool_mint_count = 44
  quote_numeric_ambiguous_slot_pool_mint_count = 25
  template_id_preserved_count = 44
  quote_request_id_preserved_count = 0
  all_selected_candidates_min_out_positive = false
```

Current reading:

```text
fresh numeric quote/min_out preservation is real
fresh template_id preservation is real
exact ingress quote identity is still incomplete
do not treat Epoch8 as the final exact-identity close
```

So the next agent should still not re-open the `6jUs` thread, should not rerun ATA creation, and should not chase more weird inventory by default. The strict-lane blocker has narrowed again: it is now **fresh exact quote identity / quote_request_id preservation**, not broad discovery.

### Run75 blocker diagnosis and current local fix state

The numeric-quote gap is now understood more precisely than the raw `0` counters imply:

```text
numeric quote fields do exist in raw rollout tape
but they live on the later final no-send rollout rows
while the old strict-row join keyed off the original ingress shell row
```

That means the old Run39 / Run75 join was landing on zero-filled shell events rather than on the populated final no-send event for the same ingress candidate.

The local fix is now landed:

```text
new rollout bridge fields:
  template_id
  quote_request_id
  ingress_unix_ns

patched consumers:
  g1-host-pack/scripts/39_broad_surface_discovery.sh
  g1-host-pack/scripts/75_strict_directarb_quote_preservation_proof.sh
```

Current rule for the next agent:

```text
do not treat historical artifacts as retroactively repairable
the new join needs a fresh rerun whose shock_research.jsonl contains the ingress bridge fields
acceptance is:
  quote_numeric_preserved_count > 0
  min_out_numeric_preserved_count > 0
```

### Run75 current result on `.15`

Run75 has now been executed directly against the existing `epoch7b` artifacts on `.15`, and the result is no longer all-zero:

```text
strict_directarb_rows = 133
quote_numeric_preserved_count = 88
min_out_numeric_preserved_count = 88
quote_numeric_unique_slot_pool_mint_count = 88
quote_numeric_ambiguous_slot_pool_mint_count = 35
quote_request_id_preserved_count = 0
template_id_preserved_count = 0
```

Interpretation:

```text
88 strict rows now have numeric expected_out / target_out / min_out evidence
those 88 came from a unique historical fallback on:
  slot + pool + mint_in + mint_out
35 rows are still ambiguous in the old artifact set and remain non-numeric
```

Important safety rule:

```text
historical slot/pool/mint fallback is acceptable only when it is unique
do not pretend the 35 ambiguous rows are exact
```

Current operational meaning:

```text
Run75 is now positive enough to pick a candidate class from the 88 numeric rows
but the future ingress-identity bridge rerun is still required to remove ambiguity
and to restore quote_request_id / template_id on fresh artifacts
```

### Epoch8 fresh ingress-identity rerun result on `.15`

The fresh rerun after deploying the ingress bridge is now complete. This is the authoritative fresh chain:

```text
epoch8_auto_strict_directarb_quote_identity
epoch8_auto_strict_directarb_quote_identity_run62
epoch8_auto_strict_directarb_quote_identity_post_run66_forensic
epoch8_auto_strict_directarb_quote_identity_post_run69_strict_surface
epoch8_auto_strict_directarb_quote_identity_run75_quote_identity_proof
```

Fresh result summary:

```text
strict_current_state_would_fire = 83
strict_current_state_ev_usd = 154.017528
directarb_strict_rows = 74
directarb_deduped_ev_usd = 152.916773
route_leak_count = 0
jito_bundle_executable_count = 0

quote_numeric_preserved_count = 44
min_out_numeric_preserved_count = 44
quote_numeric_unique_slot_pool_mint_count = 44
quote_numeric_ambiguous_slot_pool_mint_count = 25
template_id_preserved_count = 44
quote_request_id_preserved_count = 0
all_selected_candidates_min_out_positive = false
```

Important interpretation:

```text
Epoch8 is a real fresh improvement over the old all-zero historical join
but it is still only a partial ingress-identity proof

template_id now survives on 44 fresh rows
quote_request_id still does not survive
25 rows remain ambiguous
not all preserved rows have positive min_out
```

So the next exact blocker is not "find more candidates". It is:

```text
make quote_request_id preservation first-class in the fresh strict DirectArb artifact path
and remove the remaining 25-row ambiguity
```

### Epoch8 fresh Run76 selected class on `.15`

The fresh selector against the Epoch8 numeric-safe subset is now complete and supersedes the older historical `BkK4...` selection:

```text
selected_pool = E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4
selected_mint_in = WSOL
selected_mint_out = 456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o
selected_pair_count = 8
selected_deduped_ev_usd = 14.481405
selected_input_amount_raw = 1000000000
selected_expected_out_raw = 438884922
selected_target_out_raw = 7297936851
selected_min_out_raw = 434496072
selected_min_out_positive = true
selected_route = tpu_quic_public
selected_source = afxdp
selected_template_id = 1
selected_quote_match_mode = unique_slot_pool_mint_amount_template
excluded_ambiguous_count = 25
excluded_min_out_nonpositive_count = 0
excluded_nonunique_composite_count = 0
```

Selection reading:

```text
this is the freshest replay candidate
it comes from the 44 positive-min_out numeric-safe rows
it supersedes the old historical BkK4 selection
```

### Epoch8 fresh Run77 replay attempt on `.15`

The fresh replay attempt did inject the correct selected candidate:

```text
selected signature = 2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4
pool = E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4
mint_out = 456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o
Run40 log = PASS scorecard replay injected
```

But this must be recorded honestly:

```text
the attempt armed eno1 in xdpgeneric
not native AF_XDP zero-copy
```

So this Run77 attempt is:

```text
partial infrastructure replay evidence
not canonical AF_XDP zero-copy no-send proof
```

And the replay is still blocked deeper in the funnel:

```text
tpu_summary.txt = empty
no final candidate_seen / would_enqueue / ring_enqueue / sender_dequeue / no_send_accept proof
runtime route set = {drop, jito_bundle, harmonic_bundle}
public execution route = unreachable
quic_workers = 0
```

Host cleanup after the attempt:

```text
eno1 was handed back to Agave
XDP/XSKMAP were detached
validator service was restarted
latest follow-up check is green:
  sh1-agave-no-geyser-2.service = active
  local RPC 127.0.0.1:8899 = listening
  getHealth = ok
  UDP listeners on 8000-8025 = present
  eno1 = LOWER_UP with no XDP attached
  /sys/fs/bpf/shadow_xsks_map = absent
  eno1 channels restored to Combined: 16
```

### `.15` bounded AF_XDP lease / recovery commands

Before a native AF_XDP window, have OOB ready and verify Agave first. After the window, return `eno1` to Agave immediately:

```bash
sudo pkill -f 'shadow-tier0' || true
sudo pkill -f 'shadow-oracle' || true
sudo ip link set dev eno1 xdp off || true
sudo systemctl restart sh1-agave-no-geyser-2.service
```

For serious `.15` runs, keep these RPC overrides explicit until the repo default is cleaned up:

```bash
export SHADOW_BOOTSTRAP_RPC_URL=https://api.mainnet-beta.solana.com
export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL=https://api.mainnet-beta.solana.com
```

## First-pass procedure on a new host

### 1. Audit the host before installing everything

Replay SH2 first. The repo already has the audit pack:

```text
SH/SH2-latitude-host-audit
```

From that folder, the expected order is:

```bash
bash ./00_install_audit_utils.sh
bash ./10_collect_base_audit.sh
ip -br link
bash ./20_collect_nic_audit.sh <primary_public_nic>
bash ./30_secure_host_baseline.sh
```

Keep the resulting audit artifacts under `~/shadow-host-audit/`.

If the previous host is about to be shut down, preserve these first:

- `~/shadow-host-audit/`
- the active repo/config state under `/home/ubuntu/Guerrila` and `/etc/shadow-engine`
- any generated `run/g12_gate_input.json`, `run/g12_gate_report.json`, and `run/g12_latency_by_source.json`

### 2. Clone or sync the repo

Expected working tree:

```bash
cd /home/ubuntu
git clone <repo> Guerrila
cd /home/ubuntu/Guerrila
```

If deploying from a local bundle instead of pulling from origin, keep the host tree clean enough that the executed artifacts and checked-out source do not silently drift.

### 3. Build or copy the host pack artifacts

The host pack lives here:

```text
g1-host-pack/
```

Important files:

- `g1-host-pack/build.sh`
- `g1-host-pack/config.guerrila.yaml`
- `g1-host-pack/env.g1.sh`
- `g1-host-pack/scripts/run_all.sh`
- `g1-host-pack/scripts/21_shadowguard_preflight.sh`

You can either:

1. build on the host, or
2. build on Linux elsewhere and copy the produced binaries into `g1-host-pack/artifacts/release-guerrila/`

Do **not** forget that the executed host binary may come from:

```text
g1-host-pack/artifacts/release-guerrila/
```

not from `target/release-guerrila/` directly. That drift has already caused false debugging before.

### 4. Install Rust and load it in-shell

If Rust is missing:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

Many host scripts and release builds assume Cargo is available in `PATH`.

### 5. Install Solana / Agave tooling when the host needs validator or ShadowGuard work

Do this when the host will:

- run Agave,
- run validator/Yellowstone milestones,
- execute ShadowGuard preflight/deploy/smoke,
- or perform Solana CLI checks.

Keep version evidence:

```bash
solana --version
agave-validator --version
solana-test-validator --version
```

## Config generation

The host config template is:

```text
g1-host-pack/config.guerrila.yaml
```

It contains `${VARIABLE}` placeholders and **must** be expanded before use.

The environment source of truth is:

```text
g1-host-pack/env.g1.sh
```

with host-local overrides typically stored at:

```text
/etc/shadow-engine/env.g1.local.sh
```

### Required rule

`ShadowConfig::from_file` does **not** expand env vars. Always use `envsubst`.

Example:

```bash
cd /home/ubuntu/Guerrila/g1-host-pack
source ./env.g1.sh
mkdir -p /etc/shadow-engine
envsubst < config.guerrila.yaml > /etc/shadow-engine/config.guerrila.yaml
```

Then verify no placeholders remain:

```bash
grep 'CHANGE_ME\|\${\|${' /etc/shadow-engine/config.guerrila.yaml
```

That command should produce no output.

## Host-local secrets and credential files

Typical files expected under `/etc/shadow-engine`:

- `jito-auth-keypair.json`
- `harmonic-auth-keypair.json`
- `worker-signer.json`
- `shadow-guard-deployer.json`
- `jito_tip_lut.bin`
- `allowlist.bin` when required by the current mode

Lock them down:

```bash
chmod 600 /etc/shadow-engine/*.json /etc/shadow-engine/*.bin
```

Do not commit host secrets into the repo.

## Runtime modes to keep straight

### Observe-only / no-send

This is the normal first proving step on a new host.

Typical shape:

- `rollout.mode: observe_only`
- `rollout.no_send_backend: true`
- public execution still amputated
- evidence comes from counters, no-send publication, and gate reports

### Tiny-live

Do this only after the active handoff/checklist says the prior gates are satisfied.

Current intended R1 shape is still:

- `r1.live_admission.mode: auto_private_grade`
- source required: `afxdp`
- ShadowGuard required
- pre-sign egress admission required
- micro-market uplift required
- tiny caps (`1 SOL` input, `1 SOL` per mint, `2 SOL` per entity) unless later docs explicitly supersede this

Private-lane tiny-live currently cannot use the Jito gRPC auth path or Harmonic endpoints. Do not keep retrying unauthorized Jito gRPC auth or unreachable Harmonic endpoints. R1-JITO-JSON no-send is green; the next explicit gate is a tiny-live JSON-RPC run with Harmonic disabled and public execution still amputated. Prepare public-tiny only if that JSON-RPC tiny-live path fails too.

## Validation before runtime

Always run config validation before starting the service:

```bash
/home/ubuntu/Guerrila/g1-host-pack/artifacts/release-guerrila/shadow-tier0 \
  --check-config /etc/shadow-engine/config.guerrila.yaml
```

Also verify the resulting config still preserves:

- `grpc_source_grade` truth labels
- public QUIC disabled unless explicitly approved
- live send disabled for observe-only runs
- AF_XDP/private-grade counters not satisfied by Geyser sources
- `auto_private_grade` retained for intended R1 mode

## AF_XDP / XDP host notes

If the host is expected to run AF_XDP:

- confirm the real active NIC first (`ip -br link`, `ethtool`, audit artifacts)
- do not assume the secondary NIC has carrier
- ensure the XDP program is attached to the intended interface
- ensure `/sys/fs/bpf/shadow_xsks_map` is pinned and readable
- remember that a management NIC may need `xdpgeneric` rather than native XDP for conservative wrapper scripts, but that is a harness policy, not a statement that the host cannot do native AF_XDP
- on `109.94.99.15` (X550 / `ixgbe`), AF_XDP and DPDK capability are already proven on the real host path: prior zero-copy/XSKMAP AF_XDP proof succeeded, `.15` remains the active DPDK proving host, and a direct `2026-05-14` manual native `ip link ... xdp obj ...` attach on routed `eno1` also succeeded
- the current `Run29-C` / `Run38` wrapper is intentionally stricter than the host itself: it force-validates `SHADOW_XDP_ATTACH_MODE=xdpgeneric` and `afxdp_bind_mode=copy`, so a native-attach refusal there should be read as **wrapper safe-profile policy**, not as proof that `.15` cannot run native AF_XDP
- the practical operator nuance is now explicit: native AF_XDP on `.15` is **capability-proven**, but on the routed management `eno1` the risky part is the **transition** into or out of native XDP, where `ixgbe` can briefly drop carrier and reinitialize queues
- once native XDP is attached and the link has settled, `.15` can keep `eno1` routable and accept normal SSH while `afxdp_bind_mode: zero_copy` remains live; the thing to avoid is repeated `xdp off` / reattach churn on the management NIC
- clear stale hugepage / UMEM leftovers before retries when relevant
- keep artifact evidence for queue count, XDP attach mode, and XSKMAP presence

Read these for the detailed procedure:

- `g1-host-pack/G1_RUNBOOK.md`
- `SH/SH_RUNBOOK.md`
- `SH/SH8-QC/SH8_REPORT.md`

## DPDK host notes

If the host is expected to run DPDK:

- `109.94.99.15` is still the current X550 / `ixgbe` DPDK proving host: the guarded `eno1` VFIO harness is green there with real shred-bearing traffic (`dpdk_rx_packets_total=2213`, `dpdk_udp_8000_8025_seen=2108`, `dpdk_large_udp_8000_8025_seen=623`, `dpdk_rx_to_classify_p999_ns=1160`, `dpdk_kernel_faults=0`)
- the current `.15` DPDK blocker is no longer “RX-dark NIC”; the 120s Tier0 DPDK run saw only `82` packets and classified all `82` as `dpdk_classify_drop_non_udp`, so the live gap is now in the in-process classify bridge, not the host traffic source
- on Ubuntu 24.04, make sure a working `dpdk-devbind.py` exists on `PATH`; `.15` needed a host-local helper at `/usr/local/bin/dpdk-devbind.py` because the packaged name was not present
- the detached DPDK launchers use `systemd-run`; ensure the unit gets `SHADOW_NIC_IFACE`, `SHADOW_RESULTS_DIR`, and any other required `SHADOW_*` values explicitly, because a sourced shell alone may not populate the detached unit environment

Read:

- `g1-host-pack/G1_RUNBOOK.md`
- `Shadow_Engine_V6.4_Guerrila_Handoff_Status_Memo_CURRENT.md`

## Agave / SolanaHost notes

If the host is expected to run local Agave / Yellowstone:

- replay SH1, SH2, and SH3-lite in order
- do not skip directly into AF_XDP if validator substrate is not stable
- verify ledger/accounts/snapshots are on the intended disks, not silently left on rootfs
- prefer `systemd` for final validator launch
- use the known sysctls and `LimitNOFILE` / `LimitMEMLOCK` settings from `SH/SH_RUNBOOK.md`
- do not count tiny bootstrap/gossip UDP (`~132-byte` packets to `8000`) as usable shred/TVU proof; wait until snapshot/bootstrap progresses far enough that real raw shred traffic appears and local RPC becomes usable

Read:

- `SH/SH1-local-agave-yellowstone/README.md`
- `SH/SH3-lite-storage-firewall/README.md`
- `SH/SH_RUNBOOK.md`

## Is Agave bootstrap necessary on a new host?

Not always. Decide this before starting a long bootstrap.

Agave bootstrap **is necessary** when the host must provide its own Solana data source:

- SH1/SH3/SH4 local validator or Yellowstone work;
- SH8-style self-sourced AF_XDP proof;
- any test that depends on local TVU/shred packets from the host's own non-voting Agave node;
- any comparison between local validator/Geyser and external provider data.

Agave bootstrap is **not automatically necessary** when the host is only being used for:

- code build/deploy validation;
- config generation and `--check-config`;
- ShadowGuard preflight or template checks that only need Solana CLI/RPC;
- external Geyser observe-only/no-send software proof;
- reviewing or replaying already captured artifacts.

For the current G12/R1 state, do **not** assume Agave bootstrap is the next fix by default. SH8 already proved observe-only AF_XDP private-grade ingress on `.57`, and AF_XDP/R1 hot-path processing has since been proven through route/actionable, `process_intent`, no-send backend, final no-send publication, and private-grade would-send evidence. The next egress step is JSON-RPC tiny-live approval, not Agave bootstrap. Bootstrap Agave on a new host only if the goal is to recreate the self-sourced AF_XDP substrate or rerun SH/AF_XDP evidence from scratch.

## Host run outputs to archive every time

At minimum, keep:

- host audit outputs
- generated `/etc/shadow-engine/config.guerrila.yaml`
- build/deploy commit or bundle identity
- `shadow-tier0 --check-config` output
- runtime logs
- `run/g12_gate_input.json`
- `run/g12_gate_report.json`
- any script-produced artifact directory under `~/shadow-host-audit/`

When a host run is important, mirror the relevant truth back into:

- `g12-qc/`
- the current handoff memo
- any milestone-specific QC folder (`SH/SH7-QC`, `SH/SH8-QC`, etc.)

## Common ways to get confused

Avoid these failure modes:

1. **Editing the template but not regenerating `/etc/shadow-engine/config.guerrila.yaml`.**
2. **Rebuilding `target/release-guerrila/*` but forgetting the executed binary is under `g1-host-pack/artifacts/release-guerrila/`.**
3. **Treating Geyser evidence as private-grade AF_XDP evidence.**
4. **Starting live before observe-only/no-send proof is re-established on the new host.**
5. **Assuming NIC names, queue counts, or XDP mode from an older host.**
6. **Leaving validator disks mounted on rootfs directories instead of real NVMe volumes.**
7. **Trusting memory or prior chat context instead of the current markdown handoff/checklist docs.**
8. **Confusing a local Jito/Harmonic auth keypair with provider authorization.**
9. **Going public before trying the Jito JSON-RPC Block Engine path.**
10. **Counting public-tiny fallback evidence as private-lane R1/G12 green.**

## Recommended order for the next agent on a new host

1. Read the canonical docs listed at the top of this file.
2. Run the SH2 host audit and keep the artifacts.
3. Establish dependencies: system packages, Rust, Solana/Agave as needed.
4. Build or sync `g1-host-pack` artifacts.
5. Fill host-local env values and generate `/etc/shadow-engine/config.guerrila.yaml` with `envsubst`.
6. Stage secrets into `/etc/shadow-engine` with locked permissions.
7. Run `shadow-tier0 --check-config`.
8. Recreate the expected observe-only or milestone-specific proof run first.
9. Archive outputs into `~/shadow-host-audit/`, including `g12_latency_by_source.json` when present.
10. Reflect the outcome back into repo docs/QC folders so the next handoff starts from files, not memory.
11. Only after artifacts and docs are synced should the previous proving host be shut down.
