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

## Habitat registry

Habitat selection is now durable host state, not a one-off report.

- Runtime registry: `/etc/shadow-engine/shadow_habitat_registry.json`
- Append-only audit registry: `/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_registry.jsonl`
- Decision ledger: `/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_decision_ledger.jsonl`
- Latest generated registry report: `/home/ubuntu/Guerrila/g1-host-pack/results/80_habitat_registry_latest/`

Use the registry before scheduling focused habitat windows. A volatile pool is not a trade signal by itself; volatility is only a habitat filter. Habitats must still be judged on policy cleanliness, venue support, source visibility, routeability, account or inventory readiness, PM reserve/release, and positive EV after Shadow's full execution costs.

Formal selection standard:

```text
habitat score chooses where to look
readiness score decides whether Shadow can convert
```

Do not promote a habitat to live-track unless it reaches `HUNT_LIVE_TRACK` with `usable_now_count > 0`. `WATCH` and `HUNT_RESEARCH` remain no-send research states until account/inventory, PM, policy, quote/min_out, and private-egress gates are all simultaneously green.

DirectArb now has an additional mode split that should stay explicit:

```text
BROAD_DISCOVERY:
  search broadly
  classify out-of-scope / non-convertible rows
  update registry
  no live-track claims

BOUNDED_HUNT:
  Pool Book selected habitats only
  readiness-qualified battlefield
  eligible for Run78B if a fresh candidate appears
```

Use the new DirectArb selectors before scheduling future strict DirectArb windows:

- `g1-host-pack/scripts/81_directarb_pool_book_selector.sh`
- `g1-host-pack/scripts/82_focused_directarb_poolbook_run78.sh`

Important rule: if broad search sees ATA/account/policy blockers again, treat them as classified out-of-scope discovery noise, not as a reopened strict DirectArb architecture regression.

## Current lane map and 19-day objective

As of 2026-05-29, `.15` is a lane-certification host through 2026-06-14. Do not use it for random live attempts. Use it to prove, lane by lane, whether Shadow is structurally ready or still architecture-blocked.

```text
Rebalancing:
  status = REBALANCING_EXECUTION_PLUMBING_READY
  latest proof = 66RG certificate reconciliation
  execution_plumbing_ready_rows = 21
  proposal_eligible_rebalancing_rows = 0
  split = 12 sender no-send, 9 bundle no-submit
  blocker = owner_selector_instrumentation_repair
  next = 66RG2 owner-selector instrumentation repair; proposal work closed

DirectArb:
  status = DIRECTARB_STRUCTURAL_HUNT_READY
  blocker = policy-clean roundtrip economics / habitat quality
  next = sentinel only; Pool Book selected windows only if a cleaner habitat appears

Backrun:
  status = BACKRUN_PREPROCESSED_PIPELINE_GREEN
  blocker = executable-shape density / exit-quality, not source timing
  latest proof = LS13A/ALT/LS13B + PREBR0 + fixed R1 factory smoke
  next = no-send Backrun factory density, not more ingest/selector surgery

Launch Continuation:
  status = LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED
  technical_read = LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN
  strategy_read = NOT_A_STRATEGY_DURATION_TEST
  pnl_read = SHORT_VALIDATION_COST_DOMINATED
  latest proof = 2026-06-01 hot R2 late-follow Sender acquisition landed, rescue flatten landed, final inventory 0
  primary blocker = primary PumpSwap flatten route shape failed with Custom 3007 before rescue
  next = Launch Continuation R2-HOLD: 22_launch13_r2_hold_live_execute.sh with 22_launch11 exit sentinel, hold after entry, exit by configured deterioration trigger, primary flatten without rescue

Post-shock / continuation:
  status = CONTINUATION_STRUCTURAL_HUNT_READY
  current_result = zero proposal rows in current resume5 and fresh windows
  blocker = policy / account / economics
  next = parked until a new habitat produces fresh proposal rows

Housekeeping:
  status = fresh window non-monetizable
  blocker = account / artifact surface, not executable tiny trades
  next = fresh tiny-trade factory or saved-loss certificate
```

Latest Rebalancing proof:

```text
66rg_rebalancing_certificate_owner_reconciliation_20260527
final_verdict=REBALANCING_PARKED_CERTIFICATE_AMENDED
66rb_status=REBALANCING_EXECUTION_PLUMBING_READY
66rb_meaning=execution_plumbing_structural_ready
rebalancing_rows_seen=3997
exact_positive_rows=91
quote_min_out_ready_rows=324
entry_rule_ready_rows=3451
exit_rule_ready_rows=2916
pm_ready_rows=72
account_ready_rows=47
inline_ata_possible_rows=9
template_ready_rows=69
sender_no_send_ready_rows=12
bundle_no_submit_ready_rows=9
roundtrip_positive_rows=91
structural_ready_rows=21
execution_plumbing_ready_rows=21
owner_mode_match_rebalancing_rows=0
pm_owner_lock_ready_rows=0
restoration_pressure_measured_rows=0
false_restoration_gate_input_ready_rows=0
proposal_eligible_rebalancing_rows=0
architecture_blocker_counts={}
wire_send=0
live_landed=0
send_transaction_called=false
sendBundle_called=false
```

AFBR1F/AFBR1G/AFBR1H must run only in native AF_XDP zero-copy posture:

```text
SHADOW_XDP_ATTACH_MODE=native
afxdp_bind_mode=zero_copy
SHADOW_AFXDP_SAFE_MODE=false
```

Before any AFBR1F/AFBR1G/AFBR1H lease, verify both Agave and `shadow-state` are healthy. The previous zero-sample AFBR1F failure was caused by missing/unhealthy `shadow_health`, which made workers reject decoded rows as health-unhealthy; it was not an AF_XDP transport failure. Green host posture is:

```text
local Agave getHealth=ok
processed slot lag acceptable
shadow-state running with /dev/shm/shadow_health state=1
eno1 has no XDP before the lease
/sys/fs/bpf/shadow_xsks_map absent before the lease
watchdog timer active
```

After every bounded native lease, return `eno1` to Agave ownership: stop Shadow datapath processes, detach XDP, remove XSKMAP, restore queues, and confirm Agave plus `shadow_health` are green before interpreting the run.

AFBR1G result (`2026-05-26`): the first audit over the 10 AFBR1F samples did not prove `.15` source-late. It found `overall_diagnosis=BUNDLE_READY_BUT_STATUS_LATE`, `arrival_timing_decision_counts={"BUNDLE_READY_BUT_STATUS_LATE":6,"STATUS_CHECK_LATE":4}`, `fresh_at_bundle_ready_count=0`, and `source_late_count=0`. Internal bundle construction stayed plausible (`target_seen_to_bundle_ready_us_p50=4429`, p90/p99 `46403`), but the first status check started late relative to bundle readiness (`bundle_ready_to_first_status_check_us_p50=671059`, p90/p99 `4747862`). AFBR1H then corrected the measurement path with a tiny `/dev/shm` hot bundle-ready probe row and immediate status probe. The `21_afbr1h_immediate_bundle_ready_freshness_batch.sh` run completed `10` valid native zero-copy no-send samples with `status=SOURCE_LATE_DOMINANT`, `freshness_class_counts={"SOURCE_LATE":10}`, `fresh_at_bundle_ready_count=0`, `invalid_preflight_count=0`, `invalid_post_cleanup_count=0`, `probe_nonzero_count=0`, and `unsafe_live_activity_count=0`. Distribution: `target_seen_to_bundle_ready_us_p50=148`, p90 `16017`, p99 `30562`; `bundle_ready_to_status_start_us_p50=997`, p90 `1377`, p99 `1587`; `monitor_seen_to_status_start_us_p50=37`; `provider_sim_latency_us_p50=67213`. Treat Backrun as structurally ready but `.15` AF_XDP source/fresh-target gated in this distribution. The next serious Backrun repair is earlier target availability, such as Helius preprocessed / Shred Delivery entitlement or another earlier target-source topology, not reopening projection, PM, provider auth, or bundle shape.

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
- **Native Run77 retry update (`2026-05-22`):** the `xdpgeneric` posture bug is now fixed in the host-pack path. `77_selected_numeric_safe_directarb_tpu_nosend.sh` now requires `SHADOW_XDP_ATTACH_MODE=native` and `afxdp_bind_mode=zero_copy`, proves local Agave green before native attach, and passes that pre-attach proof into Run40 so Run40 does not rerun freshness/reference RPC checks while Shadow owns `eno1`. Host retry `epoch8_auto_strict_directarb_quote_identity_run77_native_zero_copy_preattach_retry2` armed native XDP, pinned/registered `/sys/fs/bpf/shadow_xsks_map`, bound AF_XDP queue 0, started `shadow-oracle` and `shadow-egress-tpu`, and crossed the old post-attach preflight blocker. It is still **not accepted**: `shadow-tier0` then failed scorecard replay injection because it tried to fetch the selected transaction from the config Helius RPC during the native lease and the HTTPS request failed. Current blocker is now **pre-hydrate/cache the selected replay transaction before native attach, or make replay injection use a lease-safe already-fetched transaction source**, then rerun the same fresh Run76 `E9dp... / WSOL -> 456p...` class.
- **Post native Run77 retry handback (`2026-05-22`):** after the failed native retry, `.15` was manually handed back cleanly: Run77 runtime PIDs were stopped, native XDP detached from `eno1`, XSKMAP absent, queues restored to `Combined: 16`, Agave restarted, UDP listeners returned, local RPC recovered to `getHealth=ok`, and sampled slot reached `421339855`. Do not start another native lease until local Agave is green and the replay transaction fetch has been moved out of the native-attach window.
- **Run77B prehydrated replay build update (`2026-05-22`):** the next wrapper is now `g1-host-pack/scripts/77b_prehydrated_selected_numeric_safe_directarb_tpu_nosend.sh`. It loads the fresh Run76 scorecard, verifies the fixed selected signature `2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4` and class `E9dp... / WSOL -> 456p...`, performs `getTransaction` before any native XDP attach, writes `/home/ubuntu/shadow-host-audit/dpdk-runs/epoch8_auto_strict_directarb_quote_identity_run77_prehydrated/selected_replay_transaction.json`, and exports `SHADOW_RUN77_REPLAY_TX_PATH` / `SHADOW_RUN40_SCORECARD_REPLAY_TX_PATH` so Tier0 consumes the local artifact. It also supports `SHADOW_RUN77B_PREHYDRATE_ONLY=true` to stop immediately after artifact creation without touching `eno1`. This prehydrate-only mode has now passed on `.15` with `selection_loaded=1`, `prehydrated_replay_tx_loaded=1`, `replay_hydration_before_xdp=true`, `replay_hydration_rpc_during_lease=false`, hydrated slot `421267673`, and artifact size `14729` bytes; post-check stayed Agave-owned with no XDP, no XSKMAP, local RPC `getHealth=ok`, UDP listener count `30`, and sampled slot `421347394`. Tier0 now hard-fails when `SHADOW_RUN77B_REQUIRE_PREHYDRATED_REPLAY=true` and no artifact path is present, and validates artifact signature, slot, pool, mints, and `selected_route=tpu_quic_public` before decoding the base64 transaction. Run40/52A summaries now surface `replay_hydration_mode=prehydrated_artifact`, `replay_hydration_before_xdp=true`, `replay_hydration_rpc_during_lease`, and artifact-loaded fields. The wrapper now creates a Run77B temp config with `SHADOW_RUN77B_TPU_QUIC_WORKERS=2` by default, requires `quic_workers>=2`, and reports `blocked_quic_workers_zero` if the override is zero; the permanent Guerrila config remains unchanged. The wrapper still refuses non-native attach, non-`zero_copy`, missing `shadow-egress-tpu`, unsafe AF_XDP mode, and red pre-attach Agave.
- **Run77-family AF_XDP doctrine (`2026-05-22`):** canonical Run77 / Run77B proof on `.15` is **native XDP + AF_XDP zero-copy only**. `xdpgeneric` / copy mode is not a weaker success and not acceptable infrastructure evidence for this gate; it is a refused posture. The management-NIC transition risk is real, but the mitigation is OOB plus bounded Agave-first lease/handback, not downgrading to generic XDP. Both Run77 wrappers already enforce `SHADOW_XDP_ATTACH_MODE=native` and `afxdp_bind_mode=zero_copy`; keep that as a hard rule for the next native lease.
- **Sender milestone (`2026-05-23`):** Helius Sender is now structurally integrated as a route-aware egress lane, `helius_sender_dual`, for plain single-transaction private flow while true bundle-only / ordered flow remains on `jito_bundle`. Host no-send smoke on `.15` is green. Treat Sender as parked until the branch explicitly needs route-specific EV accounting, first live Sender proof, or bundle-vs-Sender comparison.
- **RunLS wrapper hardening (`2026-05-24`):** `env.g1.sh` now accepts `HELIUS_GATEKEEPER_URL`, `HELIUS_LASERSTREAM_URL`, and `HELIUS_LASERSTREAM_API_KEY` directly; generated host configs now carry `grpc_source_grade=laserstream`; and `20_g12_r0_observe_only.sh` now hard-fails RunLS posture if the rendered config is not actually LaserStream or if `SHADOW_PM_SHOCK_RESEARCH_EXPORT_PATH` is not window-local. The required row-level arbiter artifact is now `laserstream_vs_afxdp_matched_signature_stage_deltas.jsonl`.
- **RunLS6C config-hardened source-race truth (`2026-05-24`):** the first clean config-hardened `.15` source-race window now exists at `/home/ubuntu/shadow-host-audit/dpdk-runs/runls6c_config_hardened_20260524_e/window1/20_g12_r0_observe_only`. It finished with `cleanup_verified=true`, `grpc_source_grade_configured=laserstream`, `shock_research_export_path_is_window_local=true`, `arbiter_artifacts_window_local=true`, `manual_artifact_copy=false`, and `matched_row_count=15`. Median deltas on the matched sample were `raw_receive_delta_us=-20790`, `normalized_delta_us=-20791`, and `candidate_emit_delta_us=-21057` under the current `laserstream_minus_afxdp` basis, so LaserStream was earlier on that clean window. `trade_ready_delta_us` remained `null`, so there is still **no** live-architecture verdict. Current operating interpretation stays hybrid: AF_XDP remains the private-grade / live-sensitive hot trigger, while LaserStream is now a credible breadth / continuity / research source and possible future strategy-specific source.
- **RunLS7 / LS8 / LS9 branch truth (`2026-05-24`):** the dual-source question has now moved past plain matched-source timing. `RunLS7` showed `matched_near_trade_ready_count=0`, `only_laserstream_near_trade_ready_count=413`, and `only_afxdp_near_trade_ready_count=1`, so the overlap race stayed inconclusive while LaserStream-only usable coverage became strategically important. The offline `RunLS8` audit then proved those `413` LaserStream-only near-trade-ready rows were not junk: all `413` were quote-numeric, min-out positive, EV positive, template-ready, inventory/account ready, PM-reservable, and route-usable, with `laserstream_only_blocked_only_by_source_policy=413`; the split was `backrun=187`, `drop=226`, `directarb=0`, and route readiness was `jito_bundle=342` plus `helius_sender_dual=71`. `RunLS9` then promoted only the LaserStream **Backrun** slice into a no-send proof lane on `.15`: `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only` finished `run_status=completed`, `cleanup_verified=true`, `live_landed=0`, and `wire_send=0`, with `laserstream_backrun_seen=763`, `laserstream_backrun_no_send_eligible=124`, `laserstream_backrun_no_send_accept=124`, `laserstream_backrun_bundle_ready=83`, `laserstream_backrun_sender_ready=41`, `laserstream_backrun_pm_reserve_ok=83`, `laserstream_backrun_blocked_by_source_policy_before=124`, and `laserstream_backrun_blocked_by_source_policy_after=0`. Treat this as a real **LaserStream Backrun no-send promotion proof**, not as a live promotion and not as a DirectArb source promotion.
- **RunLS10 / RunLS9B / RunLS9C / RunLS9D follow-up truth (`2026-05-24`):** the next audits narrowed the meaning of that promotion. `RunLS10`, written beside the LS8 artifacts at `/home/ubuntu/Guerrila/g1-host-pack/results/20_g12_r0_observe_only/runls10_drop_semantics_summary.json`, shows that the LaserStream `drop` slice is **not** currently an executable money lane: `drop_total=226`, `drop_executable_count=0`, `drop_no_trade_count=226`, `drop_saved_loss_count=226`, `drop_requires_egress_count=0`, `drop_sender_ready=37`, `drop_bundle_ready=189`, `drop_pm_reserve_ok=226`, and `drop_realized_pnl_possible=0`. Treat those rows as non-trading saved-loss / suppression evidence even though they are route-usable. `RunLS9B`, written beside the LS9 artifacts at `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9b_laserstream_backrun_sender_pm_summary.json`, proved the LaserStream Backrun **Sender** subset was not ready for promotion: `sender_subset_total=41`, `sender_subset_no_send_accept_count=41`, but `sender_subset_pm_reserve_ok_count=0`, `sender_subset_accept_without_pm_reserve_count=41`, and `result=failed_pm_reserve_gate`. The repo/script-side quarantine is now explicit: `RunLS9D`, saved at `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9d_laserstream_backrun_sender_pm_summary.json`, reclassifies that Sender slice honestly as `sender_execution_no_send_accept_count=0`, `sender_research_only_accept_count=41`, `sender_blocked_pm_reserve_missing_count=41`, and `result=pass_pm_gate_honest`. The important new correction is that the bundle PM-release seam is now closed for no-send proof: the PM core now releases `ObserveOnlyNoSend` reservations, and the `.15` rerun with a `2s` post-window drain produced `runls9_laserstream_backrun_nosend_summary.json` with `laserstream_backrun_bundle_ready=153`, `laserstream_backrun_pm_reserve_ok=153`, `laserstream_backrun_no_send_accept=153`, `sender_execution_no_send_accept=0`, and `sender_research_accept_only=21`; the paired `RunLS9C` bundle audit now shows `laserstream_backrun_bundle_seen=153`, `laserstream_backrun_bundle_pm_ok=153`, `laserstream_backrun_bundle_envelope_built=153`, `laserstream_backrun_bundle_no_send_accept=153`, `worker_pm_reservation_created=153`, `pm_reservation_released=153`, and `laserstream_backrun_bundle_blocker=none`. So the clean branch reading is now: LaserStream Backrun no-send is real, the **bundle subset is the first fully reconciled no-send monetization lane**, the Sender subset remains quarantined until PM/accounting proof exists, and `drop` remains a research / suppression lane rather than an executable exit lane.
- **RunLS11D / AUTH0 follow-up truth (`2026-05-24`):** the refreshed scoped LaserStream Backrun private-bundle readiness gate at `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only` now proves the ordered bundle artifact can be assembled in no-submit mode from preserved target bytes. The fresh window stayed clean (`run_status=completed`, `cleanup_verified=true`, `wire_send=0`, `live_landed=0`, `grpc_source_grade_configured=laserstream`) and the selected artifact now shows `selection_result=selected_target_tx_ready`, `selected_target_tx_bytes_available=true`, `selected_target_tx_bytes_source=captured_at_ingress`, `selected_target_tx_message_hash=7111b899d2be4c6f1e7a0aa937a3efd6beb7212d96604a6d27c3047c7d08a74d`, `bundle_tx_count=2`, `bundle_envelope_built=true`, `selected_backrun_tx_template=jito_plain`, `selected_expected_out_raw=10329482098`, `selected_min_out_raw=10226187277`, `selected_net_ev_after_tip=2484055`, and `selected_tip_lamports=27012004`. `RunLS11D` then closes the target-capture question directly: `selected_target_tx_decodes_locally=true`, `selected_target_tx_decoded_len_bytes=743`, `selected_target_signature_matches=true`, `selected_target_pool_matches=true`, `selected_target_mint_in_matches=true`, `selected_target_mint_out_matches=true`, `selected_target_account_keys_count=15`, and `selected_target_program_ids_count=10`. Selected-scoped PM is also clean now: `selected_pm_reserve_created=true`, `selected_pm_reserve_released=true`, `selected_pm_release_reason=observe_only_no_send`, `selected_pm_pending_after=0`, and `pm_reserve_reconciled=true`. The remaining blocker is now singular rather than structural: provider reachability is green on both rerun `RunLS11C` and `RunAUTH0` (`provider_submit_probe=http_404`, `provider_status_probe=http_405`), but `provider_auth_material_present=false` with `provider=jito_direct`, `auth_source=jito_keypair`, and `jito_auth_path_present=false`. Operational rule after LS11D / AUTH0: **LaserStream Backrun bundle remains the leading monetization branch, target-byte preservation is now solved, and the next blocker before any controlled live proof is host private-bundle auth staging.**
- **RunAUTH1 / LS11C-provider rerun truth (`2026-05-24`):** the private-bundle auth blocker is now resolved for the first proof path by routing through Helius-mediated Jito bundles instead of waiting on direct Jito keypair auth. `RunAUTH1` wrote `runauth1_private_bundle_provider_matrix.json`, `runauth1_private_bundle_provider_rows.jsonl`, and `runauth1_selected_provider.json` in the LS11 artifact root, with `passing_provider_count=1`, `first_passing_provider=jito_via_helius`, `first_passing_provider_auth_source=helius_rpc_api_key`, `auth_material_present=true`, `tip_accounts_available=true`, `tip_accounts_count=11`, `status_api_reachable=true`, `simulate_bundle_available=true`, `send_bundle_disabled_in_test=true`, `send_bundle_called=false`, `wire_send=0`, and `live_landed=0`. The immediate `RunLS11C` provider rerun against `jito_via_helius` then showed `selected_target_tx_bytes_available=true`, `bundle_envelope_built=true`, `bundle_tx_count=2`, `tip_present=true`, `min_out_positive=true`, `net_ev_after_tip_positive=true`, `selected_pm_reserve_created=true`, `selected_pm_reserve_released=true`, `provider_auth_material_present=true`, `provider_status_api_reachable=true`, `simulate_bundle_available=true`, `send_bundle_disabled_in_mode=true`, `wire_send=0`, and `live_landed=0`. Direct Jito remains auth-red (`jito_auth_path_present=false`) and is no longer blocking the first controlled proof, but `RunLS12` is now explicitly blocked by the later `RunLS11E` freshness gate until the target transaction is unlanded and blockhash-valid at bundle-build time.
- **RunLS11E freshness gate (`2026-05-24`):** `21_runls11e_target_freshness_bundle_eligibility.sh` is now the hard pre-live gate for the selected LS11 artifact. It writes `runls11e_target_freshness_bundle_eligibility.json` and `runls11e_target_freshness_bundle_eligibility_rows.jsonl`, and it exits nonzero unless `selection_result=target_fresh_bundle_eligible`. The current `.15` rerun correctly fails closed with `selection_result=blocked_target_already_landed`, `target_tx_bytes_available=true`, `target_tx_decoded_len_bytes=743`, `target_signature_status_before_bundle.confirmationStatus=finalized`, `target_already_landed=true`, `target_recent_blockhash_valid=false`, `bundle_tx_count=2`, `target_tx_first=true`, `our_backrun_tx_second=true`, `tip_present=true`, `simulate_bundle_available=true`, `simulateBundle_pass=false`, `sendBundle_called=false`, `wire_send=0`, and `live_landed=0`. Operational rule: **do not run LS12 from processed LaserStream target rows**; the next build is either LaserStream preprocessed target feed or AF_XDP target trigger plus LaserStream enrichment, then rerun LS11E before any one-bundle live proof.
- **RunLS13 preprocessed target proof (`2026-05-24`):** `RunLS13` is now a standalone, no-submit preprocessed-subscription probe, not a live-pipeline promotion. The new binary is `runls13-preprocessed-probe` and the host wrapper is `g1-host-pack/scripts/21_runls13_laserstream_preprocessed_target_freshness.sh`. It writes `runls13_preprocessed_target_freshness_summary.json` and `runls13_preprocessed_target_freshness_rows.jsonl` under `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls13_laserstream_preprocessed_target_freshness/window1/`. The URL/token wiring has been verified against Helius docs and the dashboard shape: endpoint stays `https://laserstream-mainnet-ewr.helius-rpc.com`, while the API key is sent separately as the gRPC `x-token`; do **not** append `?api-key=` to the LaserStream gRPC URL. The direct LS13 probe now connects to the endpoint and fails specifically at `/geyser.Geyser/SubscribePreprocessed` with `error_code=PermissionDenied`, `error_message="Unsupported plan type"`, and `error_stage=subscribe_preprocessed_initial_request`. The same direct error appears for selected-pool, no-filter, and JUP-style account filters. Current conclusion: Helius-mediated bundle auth is still green, and standard LaserStream is still usable, but preprocessed transactions are plan/entitlement blocked for the current API key. Do not run LS12. Next step is to upgrade/enable Helius Shred Delivery / preprocessed transactions for this key, then rerun LS13; if that is not available, switch to the fallback branch `AF_XDP target trigger + LaserStream enrichment`.
- **RunLS13 rerun after Helius Professional upgrade (`2026-05-29`):** the old entitlement blocker is now cleared. On `.15`, the same wrapper was rerun with the host `.env` and wrote fresh artifacts under `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls13_laserstream_preprocessed_target_freshness_prof/`. The selected-context windows (`window1` at `30s`, `selected120_diag` at `120s`) both reached `direct_subscribe_probe.connect_ok=true`, `initial_request_ok=true`, and `error_code=null`, so `SubscribePreprocessed` is accepted for this key now; however both selected windows ended with `selection_result=blocked_no_preprocessed_target_seen`, `preprocessed_target_seen=false`, and `stream_error=stream_window_elapsed`. The unfiltered diagnostic (`nofilter_diag`, `60s`) proves the feed itself is live: `preprocessed_target_seen=true`, `target_tx_bytes_available=true`, `target_blockhash_valid=true`, `target_already_landed=false`, `target_tx_first=true`, `signature_status_rpc_reachable=true`, and `simulate_bundle_rpc_method_available=true`, while still keeping `sendBundle_called=false`, `wire_send=0`, and `live_landed=0`. Current conclusion: Helius preprocessed is now genuinely available on this key, but the old selected Backrun account filter did not observe a matching target in bounded windows. The next serious Backrun work is no longer entitlement repair; it is preprocessed-native target selection / join logic over the now-live feed, then rerunning the freshness gate from a real preprocessed hit.
- **RunLS13A / LS13U / LS13ALT / RunLS13B current preprocessed-native chain (`2026-05-29`):** the first `RunLS13A` / `RunLS13B` unfiltered pass on `.15` is now historical context, not the freshest blocker read. That first `90s` nofilter window produced `preprocessed_raw_tx_seen_count=6`, `target_not_landed_at_first_check_count=6`, `target_entity_ready_count=0`, `selector_decision_counts={"UNSUPPORTED_PROGRAM":6}`, and `status=PARK_SELECTOR_TOO_NARROW`. Follow-on `RunLS13U` unsupported-program census plus `RunLS13ALT` lookup-table hydration repaired that blocker on a fresh `120s` nofilter host run: `preprocessed_raw_tx_seen_count=8`, `target_not_landed_at_first_check_count=7`, `supported_program_candidate_count=4`, `rows_with_alt_lookups=3`, `lookup_table_fetch_ok_rows=3`, `alt_resolution_ready_rows=3`, `alt_resolution_blocked_count=0`, and `RunLS13B` now reports `target_entity_ready_count=3`, `selector_decision_counts={"TARGET_ENTITY_READY":3,"ALREADY_LANDED":1,"UNSUPPORTED_PROGRAM":2,"TX_DECODE_FAILED":2}`, `ready_venue_counts={"pumpfun_amm":3}`. Preprocessed ingest, ALT resolution, and target-entity linkage are now green; the blocker has moved forward.
- **PREBR0 / PREBR2 current executable-shape result (`2026-05-29`):** `21_runlsprebr0_preprocessed_projection_quote.sh` then produced the first real projected preprocessed Backrun candidate: `projected_post_target_state_count=1`, `projected_expected_out_positive_count=1`, `projected_min_out_positive_count=1`, and `projected_ev_after_tip_positive_count=1`. The inventory-aware executable-shape filter correctly parked it before bundle simulation with `status=PARK_EXECUTABLE_INPUT_NOT_HELD`, `projected_ev_positive_held_input_rows=0`, `projected_ev_positive_core_input_rows=0`, and `projected_ev_positive_unheld_token_rows=1`. Current Backrun blocker is not source timing, provider auth, or projection math. It is executable input shape / held inventory.
- **Pump.fun R1 inventory-band lesson (`2026-05-29`):** the first positive projected row led to a direct-route / R1 inventory-band proof path. `21_lsprebrpf_pumpfun_amm_direct_route_probe.sh` proved the direct Pump.fun route exists for that candidate, and `21_lsprebrinv_r1_one_mint_proposal.sh` produced an honest `R1_PROPOSAL_READY_ATA_PRECREATE_ONLY` package for mint `HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w`, pool `HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp`, with ATA rent explicitly priced. One ATA was precreated, the no-send package upgraded to `R1_PROPOSAL_READY_ONE_MINT_LIVE`, and the live executor then correctly aborted pre-send because the hypothetical exit quote was zero. The subsequent size ladder and autopsy proved all tested R1 sizes `0.005,0.01,0.025,0.05,0.1` ended `SIZE_EXIT_ZERO` with `reason_expected_out_zero=integer_rounding_dust_under_zero_fee_model`. The empty HXn ATA was then closed and rent recovered. Operational rule from here: do not precreate an ATA or promote an inventory-band proposal unless the exact intended size band has `exit_expected_out_raw > 0`, `exit_min_out_raw > 0`, and `exit_non_dust=true`.
- **Backrun R1 candidate factory (`2026-05-29`):** the first long R1 factory rooted at `/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_20260529T114758Z` is invalid as a market-density verdict. It launched child stages outside its run root and later exited in the direct-route probe with `IsADirectoryError: '.'`. `21_lsprebrinv_r1_candidate_factory.sh` now pins all child stage outputs under one run root, and `21_lsprebrpf_pumpfun_amm_direct_route_probe.sh` now hard-fails missing upstream paths instead of falling through to `Path(".")`. The fixed `.15` smoke at `/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_fix_smoke_20260529T_host` completed end-to-end with `proposal_ready_rows=1`, `ata_precreate_only_ready_rows=1`, `one_mint_live_ready_rows=0`, `projected_ev_positive_unheld_token_rows=1`, `wire_send=0`, and `live_landed=0`. A fresh fixed `6h` no-send rerun was then launched at `/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_fix_20260529T124709Z`. Treat the current Backrun question as surface density / conversion rate, not an ingest-plumbing failure.
- **Launch Continuation lane (`2026-05-29`, historical no-send baseline):** the new launch-follow / new-mint momentum R1 lane now exists as `22_launch0_new_mint_pool_census.sh` through `22_launch5_one_mint_live_execute.sh`, with exact compatibility entrypoints `22_launch0_preprocessed_birth_census.sh`, `22_launch1_buy_prefix_score.sh`, and `22_launch2_bonding_curve_entry_exit_ladder.sh`, plus the no-send factory wrapper `22_launchf_launch_continuation_candidate_factory.sh`. The lane is explicitly a tail-selection strategy, not a blind-buy strategy: it requires buy-prefix evidence, policy/creator-risk screening, exact-size nonzero exit ladders, PM reserve, ordered-entry bundle proof, and auto-flatten readiness before proposal. The rewired `.15` `120s` smoke at `/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_smoke_20260529T_rewire` completed the full `22_launch0 -> 22_launch4_entry_bundle_nosubmit -> 22_launch4_one_mint_live_proposal` funnel under one run root. That short window saw `launches_seen=1`, `buy_prefix_ready_rows=0`, `exit_nonzero_count=1`, `bundle_rows=1`, `entry_bundle_ready_no_submit_rows=0`, `proposal_ready_rows=0`, `one_mint_live_ready_rows=0`, and `first_blocker=buy_count_2s_below_threshold`, with `wire_send=0` and `live_landed=0`. The short threshold sweep on that smoke confirmed the same conclusion under all tested profiles (`r1_current`, `r1_5s3`, `r1_looser`): `buy_count_1s_ge_1_rows=1`, `buy_count_2s_ge_2_rows=0`, `buy_count_5s_ge_3_rows=0`, `unique_buyers_2s_ge_1_rows=1`, `unique_buyers_5s_ge_2_rows=0`, and `ready_rows=0`. Interpretation at that point: downstream ordered-entry bundle/proposal plumbing was real, and the short-window blocker was upstream buy-prefix density rather than broken later-stage wiring. `22_launch5_one_mint_live_execute.sh` is launch-native: live entry can be built as an ordered bundle `[target buy, our buy, tip]` through the Helius/Jito bundle endpoint, and flatten remains Sender-based. The launch live executor also enforces a per-day risk ledger (`SHADOW_R1_DAILY_*`): default `$60` profit-stop, `$25` loss-stop, `12` executed trades max, no new entry while any prior open exposure remains unresolved, and **live abort if `SHADOW_R1_DAILY_SOL_USD` is unset or nonpositive**. Historical long no-send launch factory: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_factory_6h_v2_20260529T164626Z`; this is superseded by the 2026-06-01 R2 live classification below.
- **Launch R2 live milestone (`2026-06-01`):** supersedes the no-send-only Launch read. Record the first hot R2 live sample as `LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED`: late-follow Sender acquisition landed, primary flatten failed with `flatten_sim_failed: Custom 3007`, rescue flatten landed, final inventory returned to `0`, and daily ledger open exposure returned to `0`. Treat this as `technical_status=LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN`, `strategy_status=NOT_A_STRATEGY_DURATION_TEST`, and `pnl_status=SHORT_VALIDATION_COST_DOMINATED`, not clean Launch success and not strategy failure. Next required proof is `R3_MONITORED_POSITION_PRIMARY_FLATTEN`: hold after entry, populate live adverse-move metrics, trigger exit only on configured deterioration/TTL/kill, and land primary flatten without rescue.
- **RunLS14 hybrid Backrun join proof (`2026-05-24`):** because preprocessed LaserStream is entitlement-blocked and processed LaserStream targets are stale for true live backrun, the active fallback branch is now explicit: `AF_XDP target trigger + LaserStream processed enrichment`. `shadow-tier0` now preserves decoded AF_XDP raw transaction bytes into `shadow_core::target_tx_cache`, so the fresh target side can feed the later bundle artifact while LaserStream supplies coverage/enrichment. The wrapper is `g1-host-pack/scripts/21_runls14_hybrid_backrun_join_proof.sh`; it runs a bounded dual-source observe-only window, writes `runls14_hybrid_backrun_join_rows.jsonl` and `runls14_hybrid_backrun_join_summary.json`, and never calls `sendBundle` (`wire_send=0`, `live_landed=0`). Passing `RunLS14` only means a hybrid Backrun candidate is ready for the `RunLS11E` no-submit freshness gate; it does **not** approve `RunLS12`.
- **RunLS15 / RunAFBR1 historical Backrun blocker (`2026-05-24`, superseded):** early AF_XDP-only Backrun projection failed closed on target relationship / projected quote coverage, not egress/provider/PM. That blocker has since been repaired through the AFBR1 target-entity/projection chain and AFBR-H now reports `BACKRUN_STRUCTURAL_HUNT_READY`. The current Backrun blocker is AFBR1H immediate target freshness / source timing: the `21_afbr1h_immediate_bundle_ready_freshness_batch.sh` run completed `10` valid native zero-copy no-send samples, all `SOURCE_LATE`, with `target_seen_to_bundle_ready_us_p50=148`, p90 `16017`, p99 `30562`, and `bundle_ready_to_status_start_us_p50=997`, p90 `1377`, p99 `1587`. It kept `sendBundle_called=false`, `wire_send=0`, and `live_landed=0`. Operational rule remains: do **not** run LS12, AFBR2, or any live proof from this state.
- **AFBR-H structural readiness rule (`2026-05-26`):** `g1-host-pack/scripts/21_afbrh_backrun_structural_hunt_readiness.sh` is now the Backrun certificate wrapper. It aggregates `RunLS9C`, `RunLS11D`, `RunAUTH1` / `RunLS11C`, `RunLS11E`, and `RunAFBR1`, then emits either `status=BACKRUN_STRUCTURAL_HUNT_READY` or `status=BLOCKED_ARCHITECTURE`. The current honest target state is structural-green but source/freshness-red: target relationship, projected post-target state, quote/min_out, PM reserve/release, provider path, and bundle shape are no longer the active blocker unless a future AFBR-H run regresses. Do not run `LS12` or `AFBR2` while immediate target freshness remains red.
- **Run65 corrected 5-minute full Shadow epoch (`2026-05-24`):** the fresh `.15` epoch `65_full_shadow_hybrid_private_epoch_20260524_5m_valuation` is now the current conversion-map reference, not old artifacts. It used the fresh 300s capture plus valuation-only rerun with private route simulation enabled (`allow_laserstream_private_simulation=true`, `allow_private_route_simulation=true`) and shows `observe_candidates=5096`, `continuous_candidates_seen=3953`, `candidate_deduped_count=3953`, `private_route_candidate_count=3829`, `private_route_executable_count=3829`, `jito_bundle_executable_count=3829`, `blocked_private_route_count=0`, and `blocked_route_unavailable_count=0`. Actual-inventory scheduling is still no-send only and produced `would_fire_count=44`, `captured_ev_sol_equivalent=6.414265985`, and `missed_ev_sol_equivalent=438.053171931`; the schedule split is `housekeeping=31`, `rebalancing=12`, `drop=1`, `directarb=0`, `backrun=0`. Treat the large missed number as modeled post-shock/rebalancing markout reservoir, not realized PnL. The current blocker stack is now execution readiness rather than route access: `account_ata_missing=2956`, `not_held_input=917`, `quote_missing=33`, `no_free_shard=3`. `g1-host-pack/scripts/65ab_epoch_unlock_planner.sh` now writes the blocker-ranked offline unlock artifacts at `/home/ubuntu/Guerrila/g1-host-pack/results/65ab_epoch_unlock_planner_20260524_5m/`; the first pass found no conservative ATA precreate approvals (`run65a_precreate_allowed_group_count=0`) but found `23` exit-qualified held-input overlay groups / `229` expected row delta in the strict row-level overlay. Next branch is `Run65A/Run65B` policy/liquidity unlock analysis plus `Run66` rebalancing executable rule proof. No live send from Run65.
- **Run65C / Run66 no-send conversion update (`2026-05-24`):** the next analysis pass is complete on `.15`. `Run65C` artifacts are at `/home/ubuntu/Guerrila/g1-host-pack/results/65c_policy_liquidity_unlock_audit_20260524_5m/`: `Run65A` remains no-precreate (`run65a_precreate_action_allowed_now_count=0`; top `AuRo...` is `PARK_NO_PRECREATE`), while `Run65B` has `21` `SIMULATE_ONLY` overlay candidates, `simulate_only_expected_would_fire_delta=212`, and `simulate_only_strict_ev_sol_equivalent=0.001902811`. Do not acquire those mints; they are one-mint simulation candidates only. `Run66` artifacts are at `/home/ubuntu/Guerrila/g1-host-pack/results/66_rebalancing_rule_proof_20260524_5m/`: `rebalancing_rows_seen=1660`, `rebalancing_rule_candidates=1116`, `rebalancing_pm_reserve_sim_ok=1640`, `rebalancing_positive_after_cost=1368`, but `rebalancing_numeric_quote_min_out=0` and `rebalancing_would_fire=0`. Current rebalancing blockers are `account_ata_missing=1096`, `exit_rule_missing=425`, `entry_rule_missing=119`, `inventory_not_ready=12`, and `quote_missing=8`. Treat this as a strong conversion map, not live PnL proof.
- **Run66Q / 66A / 66C / 66H current branch (`2026-05-24`):** quote/min_out visibility improved but live remains closed. `66Q` artifacts are at `/home/ubuntu/Guerrila/g1-host-pack/results/66q_rebalancing_quote_minout_preservation_20260524_5m/` with `rebalancing_numeric_quote_min_out=397`; the quote-overlay rerun of Run66 is at `/home/ubuntu/Guerrila/g1-host-pack/results/66_rebalancing_rule_proof_20260524_5m_quote_overlay/` with `rebalancing_quote_ready=406`, `rebalancing_numeric_quote_min_out=397`, and still `rebalancing_would_fire=0`. `66A` artifacts are at `/home/ubuntu/Guerrila/g1-host-pack/results/66a_rebalancing_account_ata_unlock_planner_20260524_5m/` and show one planner-only full-band-liquid ATA candidate, `HyDKNdnhZNVYQMruBevbNsUWruA9STmAQrS4srXApump`; no ATA was created. `66C` shows continuation rule shape but all rows are inventory-blocked; `66H` shows housekeeping rows are account-blocked; `65B` has `21` one-mint simulation candidates but no execute permission. Next route is no-send readiness repair, not live.
- **Run66Q2 / 66A-Q2 / 66C-Q / 65B-SIM1 current repair pass (`2026-05-24`):** the latest `.15` no-send artifacts are fresh and keep live closed. `66Q2` is at `/home/ubuntu/Guerrila/g1-host-pack/results/66q2_rebalancing_exact_quote_route_ev_20260524_5m/`: `rebalancing_numeric_quote_min_out=397`, `unique_safe_quote_match_count=397`, `net_ev_after_fee_tip_positive_count=397` under the configured route-cost model, but `quote_request_id_match_count=0`, `exact_quote_match_count=0`, and `matched_rollout_net_ev_after_fee_tip_positive_count=0`. The Q2-linked ATA planner is at `/home/ubuntu/Guerrila/g1-host-pack/results/66a_rebalancing_account_ata_unlock_planner_20260524_5m_q2/` and keeps exactly one planner-only review candidate, `HyDKNdnhZNVYQMruBevbNsUWruA9STmAQrS4srXApump`, with `top_ata_review_decision=ONE_ATA_PLAN_REVIEW`, `precreate_candidate_net_ev_after_fee_tip_positive_rows=2`, and no ATA creation. `66C-Q` is at `/home/ubuntu/Guerrila/g1-host-pack/results/66cq_continuation_quote_minout_preservation_20260524_5m/`: continuation now has `continuation_numeric_quote_min_out=116`, `continuation_min_out_positive=116`, and `continuation_net_ev_after_fee_tip_positive=116`, but `exact_quote_match_count=0`. `65B-SIM1` is at `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim1_top_mint_simulation_plan_20260524_5m/`: top three overlay mints are all `PLAN_ONLY_SIGNED_SWAP_SIM_NOT_RUN`, `simulation_ready_count=0`, `execute_mode_allowed=false`, `acquisition_allowed=false`, `live_send_allowed=false`. The active gates are exact quote identity, account/ATA readiness, inventory readiness, and signed swap simulation. No live send, no acquisition, no ATA creation.
- **Run66X bridge audit follow-up (`2026-05-24`):** `g1-host-pack/scripts/66x_exact_quote_identity_bridge_audit.sh` is now the next narrow no-send diagnostic for the exact-identity gate. It reads the fresh Run65 JSON, its `candidate_autopsy.jsonl`, and the paired `shock_research.jsonl`, then writes `run66x_exact_quote_identity_bridge_rows.jsonl` / `run66x_exact_quote_identity_bridge_audit.json` with row-level `quote_request_id_match`, `ingress_identity_match`, `candidate_id_exact_match`, `exact_quote_match`, `unique_safe_fallback_match`, `owner_mode_match`, and `first_exact_identity_blocker`. The Q2/CQ scripts were also patched to summarize `first_exact_identity_blocker_counts`, `owner_mode_match_on_numeric_count`, and `unique_safe_owner_mismatch_count`. Run order on `.15`: `66x_exact_quote_identity_bridge_audit.sh`, then rerun `66q2_rebalancing_exact_quote_route_ev.sh`, `66cq_continuation_quote_minout_preservation.sh`, and Q2-linked `66a_rebalancing_account_ata_unlock_planner.sh`. Do not promote unique-safe fallback rows to exact proof; acceptance still requires nonzero exact identity matches and route/rollout-positive EV. No live send, no acquisition, no ATA creation.
- **Run66X / 66R host rerun result (`2026-05-24`):** the host evidence rerun closed exact identity via `ingress_identity`, while `quote_request_id` remains technical debt. `66X` at `/home/ubuntu/Guerrila/g1-host-pack/results/66x_exact_quote_identity_bridge_audit_20260524_5m/` has `exact_quote_match_count=499`, `ingress_identity_match_count=499`, `quote_request_id_match_count=0`, and `matched_rollout_net_ev_after_fee_tip_positive_count=99`. Rebalancing Q2 bridge2 has `rebalancing_numeric_quote_min_out=409`, `exact_quote_match_count=387`, and `matched_rollout_net_ev_after_fee_tip_positive_count=59`; continuation bridge2 has `continuation_numeric_quote_min_out=125`, `exact_quote_match_count=112`, and `matched_rollout_net_ev_after_fee_tip_positive_count=40`. New script `66r_rebalancing_exact_positive_readiness_overlay.sh` writes `/home/ubuntu/Guerrila/g1-host-pack/results/66r_rebalancing_exact_positive_readiness_overlay_20260524_5m_bridge2/` and shows the current conversion ladder: `exact_positive_q2_rows=44`, `baseline_would_fire=0`, `synthetic_ata_overlay_would_fire=39`, `combined_synthetic_readiness_would_fire=39`, with baseline blockers `account_ata_missing=39`, `entry_rule_missing=1`, `exit_rule_missing=4`. Do not create HyDK: its exact-positive row is `entry_rule_missing`, so `hydk_synthetic_ata_would_fire_delta=0` and `hydk_review_result=SYNTHETIC_ATA_OVERLAY_FAIL_NOT_ATA_ONLY`. Next useful planner should rank the 39 ATA-only convertible exact-positive rows by mint/policy/liquidity; still no live send, no acquisition, no ATA creation.
- **Run66R-ATA planner result (`2026-05-24`):** `66r_ata_only_convertible_planner.sh` now ranks only the 39 exact-positive rows that 66R proved synthetically ATA-only convertible. Host artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66r_ata_only_convertible_planner_20260524_5m_bridge2/`. Result is no-action: `ata_only_convertible_rows=39`, grouped into `8` output mints, but `precreate_review_group_count=0`, `precreate_review_row_delta=0`, `decision_counts={"REJECT_POLICY":8}`, `wire_send=0`, `live_landed=0`. Top group `4D8qUHm334fxqeTauPvF8gQ7fYgrD4Mpmb1Wy6ftUSWR` has `row_count=11` but is `metadata_only_token2022_requires_explicit_allow`, exit-dust / exit-quote false, and negative after ATA rent. Do **not** create any ATA from this result. The next step is either explicit Token-2022 policy research plus repeatability/rent amortization evidence, or a fresh longer no-send epoch; do not promote synthetic 66R rows directly to account creation.
- **Run66T Token-2022 policy result (`2026-05-24`):** `66t_token2022_explicit_allow_research.sh` audits the 8 66R-ATA mint groups for Token-2022 extension risk, authority risk, exit dust, rent-adjusted EV, and explicit-allow possibility. Host artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66t_token2022_explicit_allow_research_20260524_5m_bridge2/`. Result is still no-action: `explicit_allow_possible_count=7` but `explicit_allow_candidate_count=0`, with `decision_counts={"REJECT_AUTHORITY_RISK":1,"REJECT_DUST_EXIT":3,"REJECT_RENT_NEGATIVE":4}`. The 7 Token-2022 groups are metadata-only and authority-clean, but fail economics/repeatability in this 5m window; `Es9v...` is rejected for mint/freeze authority risk. Do not loosen policy or create ATAs until repeated no-send evidence proves non-dust exit and rent-adjusted positive recurrence.
- **Run66U repeatability aggregate (`2026-05-24`):** the fresh `.15` 3-sample repeatability proof is now the current post-shock ATA-readiness reference. New scripts: `g1-host-pack/scripts/66u_run_repeatability_chain.sh` and `g1-host-pack/scripts/66u_ata_repeatability_aggregate.sh`. Final aggregate artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66u_ata_repeatability_aggregate_20260524_3x5m_retry3_final/`. Result: `status=run66u_ata_repeatability_aggregate_complete`, `source_row_count=15`, `mint_group_count=5`, `one_ata_plan_review_count=0`, `decision_counts={"REJECT_DUST_EXIT":1,"REJECT_RENT_NEGATIVE":4}`, `wire_send=0`, and `live_landed=0`. Top repeated mint `8FSVipViijpT4aE86e58PYRd73bQdZtyzng4FT2THSTT` / `DROPEE` appeared in all 3 samples with `total_synthetic_ata_delta_across_epochs=19`, non-dust full-band exit (`min=15.924327 USDC`, `median=23.770404 USDC`), but `total_rent_adjusted_ev=-5336272` lamports, so it remains `REJECT_RENT_NEGATIVE`. Current interpretation: the post-shock/rebalancing ATA-only conversion surface repeats, but there are still zero safe ATA creation candidates. Do not create ATAs, do not acquire inventory, and do not treat synthetic conversion as realized profit. Original epoch 3 was blocked by Agave/local RPC health; the replacement epoch produced fresh Run39/Run61 evidence but hung in post-window Agave restore, so the final Run65 pointer JSON was materialized from completed fresh artifacts before the 66 chain.
- **Run66C continuation readiness / Run65B-SIM2 update (`2026-05-24`):** continuation is now proven as an inventory-readiness branch, not an ATA branch. New script `g1-host-pack/scripts/66c_continuation_readiness_planner.sh` was run across the three fresh Run66U samples. Aggregate: `continuation_rows_seen=330`, `continuation_numeric_quote_min_out=243`, `continuation_net_ev_after_fee_tip_positive=242`, `exact_quote_match_count=226`, `matched_rollout_net_ev_after_fee_tip_positive_count=66`, `exact_positive_continuation_rows=63`, `baseline_would_fire=0`, `synthetic_ata_overlay_would_fire=0`, `synthetic_inventory_overlay_would_fire=58`, `combined_synthetic_readiness_would_fire=58`, `ata_only_convertible_exact_positive_rows=0`, and `inventory_only_convertible_exact_positive_rows=58`; all no-send. This means continuation has exact-identity/min_out/route-cost-positive rows, but the clean blocker is held-input inventory, not Token-2022/ATA rent. New script `g1-host-pack/scripts/65b_sim2_signed_swap_simulation.sh` then ran against `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim1_top_mint_simulation_plan_20260524_5m/` and wrote `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim2_signed_swap_simulation_20260524_5m/`: top 3 held-input overlay mints built Jupiter swap txs, signed locally for simulation, and passed `simulateTransaction` (`signed_swap_simulation_passed_count=3`, `simulation_ready_count=3`, `send_transaction_called=false`, `wire_send=0`, `live_landed=0`). They remain simulation-ready only: no acquisition and no live send.
- **Run66CI / Run65B-SIM3 inventory lifecycle split (`2026-05-24`):** new no-send scripts `g1-host-pack/scripts/66ci_continuation_held_input_inventory_planner.sh` and `g1-host-pack/scripts/65b_sim3_inventory_lifecycle_review.sh` were deployed and run on `.15`. `66CI` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66ci_continuation_held_input_inventory_planner_20260524_3x5m/`. It grouped the `58` continuation inventory-only rows into `12` held-input mints and failed closed: `inventory_sim_review_count=0`, `decision_counts={"REJECT_DUST_EXIT":2,"REJECT_LOW_EV":3,"REJECT_POLICY":7}`, `wire_send=0`, `live_landed=0`; broad continuation inventory remains real but not acquisition-reviewable under current policy/liquidity/EV checks. `65B-SIM3` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim3_inventory_lifecycle_review_20260524_5m/`. It reviewed the three SIM2-passed held-input mints and produced `lifecycle_review_count=3`, `signed_swap_simulation_passed_count=3`, `auto_flatten_available_count=3`, but `PM_reserve_sim_ok_count=0`, `acquisition_allowed=false`, `wire_send=0`, `live_landed=0`. Top lifecycle candidate is `vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump`: `expected_unlocked_would_fire_delta=61`, `max_inventory_at_risk_usdc=7.55475554`, `roundtrip_loss_usdc=0.02612854`, and `net_after_inventory_carry_cost_usdc=0.12078146`. This is plan-review evidence only; no acquisition is approved.
- **Run65B-SIM4 PM inventory reserve review (`2026-05-25`):** new no-send script `g1-host-pack/scripts/65b_sim4_pm_inventory_reserve_review.sh` was deployed and run on `.15` against the SIM3 lifecycle rows. Artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim4_pm_inventory_reserve_review_20260524_5m/`. Result: `row_count=3`, `pm_reserve_review_ready_count=3`, `pm_reserve_input_ok_count=3`, `pm_reserve_inventory_band_ok_count=3`, `pm_release_on_no_send_ok_count=3`, `pm_pending_after_zero_count=3`, `reserve_blocker_counts={"none":3}`, `release_blocker_counts={"none":3}`, `acquisition_allowed=false`, `live_send_allowed=false`, `wire_send=0`, `live_landed=0`. The earlier SIM3 PM gap is now narrowed to synthetic/no-send reserve evidence: all three top held-input mints can reserve and release the proposed post-acquisition inventory band with `pm_pending_after=0`. `vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump` remains the best small-exposure review candidate and now has `required_inventory_raw=286475131`, `required_acquisition_usdc=7.55475554`, `pm_reserve_inventory_band_ok=true`, and `pm_release_on_no_send_ok=true`. This still does **not** approve acquisition; next proof is one-mint no-send conversion replay plus explicit caps/TTL/flatten policy.
- **Run65B-SIM5 vRse one-mint replay (`2026-05-25`):** new no-send script `g1-host-pack/scripts/65b_sim5_one_mint_conversion_replay.sh` was deployed and run on `.15` for `vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump`. Artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim5_one_mint_conversion_replay_20260524_5m/`. Result is fail-closed for the continuation acquisition-review path: `status=run65b_sim5_one_mint_conversion_replay_blocked_no_vrse_continuation_rows`, `baseline_would_fire=0`, `synthetic_vRse_would_fire=61`, `would_fire_delta=61`, but `continuation_would_fire_delta=0`, `drop_would_fire_delta=61`, `exact_quote_match_count=0`, `min_out_positive_count=0`, and `matched_rollout_net_ev_after_fee_tip_positive_count=0`. Entry/exit/PM stayed green (`entry_quote_ok=true`, `exit_quote_ok=true`, `auto_flatten_available=true`, `pm_reserve_ok=true`, `pm_release_ok=true`, `pm_pending_after=0`), with `send_transaction_called=false`, `sendBundle_called=false`, `wire_send=0`, and `live_landed=0`. Interpretation: vRse is a valid 65B/drop overlay replay signal, not a continuation exact-positive replay signal; do **not** move vRse to `ONE_MINT_ACQUISITION_PLAN_REVIEW` from this run.
- **Run65B-SIM5 owner-gated top-3 replay (`2026-05-25`):** `65b_sim5_one_mint_conversion_replay.sh` now has a hard owner-mode gate: `ACQUISITION_REVIEW_ELIGIBLE`, `DROP_ONLY_RESEARCH`, `WRONG_OWNER_MODE`, `NO_EXACT_QUOTE_PROOF`, `NO_MIN_OUT_PROOF`, and `NO_MATCHED_ROLLOUT_EV`. Reruns for the top three SIM2/SIM3/SIM4 mints all failed closed as `DROP_ONLY_RESEARCH` for `target_owner_mode=continuation`: vRse artifact `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim5_one_mint_conversion_replay_20260524_5m_vRse_ownergate/` has `would_fire_delta=61`, `continuation_would_fire_delta=0`, `drop_would_fire_delta=61`; AeVz artifact `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim5_one_mint_conversion_replay_20260524_5m_AeVz/` has `would_fire_delta=13`, `continuation_would_fire_delta=0`, `drop_would_fire_delta=13`; 2ACE artifact `/home/ubuntu/Guerrila/g1-host-pack/results/65b_sim5_one_mint_conversion_replay_20260524_5m_2ACE/` has `would_fire_delta=8`, `continuation_would_fire_delta=0`, `drop_would_fire_delta=8`. All three remain no-send with `pm_reserve_ok=true`, `pm_release_ok=true`, `pm_pending_after=0`, `send_transaction_called=false`, `sendBundle_called=false`, `wire_send=0`, `live_landed=0`. Result: park 65B acquisition entirely for these mints; they are Drop-only research, not continuation acquisition candidates.
- **Run66V inline ATA lifecycle model (`2026-05-25`):** new no-send script `g1-host-pack/scripts/66v_inline_ata_lifecycle_model.sh` was deployed and run on `.15` against the bridge2 exact-positive rebalancing rows. Artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66v_inline_ata_lifecycle_model_20260524_5m_bridge2/`. Result: `source_exact_positive_synthetic_ata_rows=39`, `row_count=39`, `decision_counts={"EXPLICIT_ALLOW_REQUIRED_REVIEW_ONLY":27,"REJECT_INLINE_ATA_EV_NEGATIVE":11,"REJECT_POLICY_OR_CREATION_RISK":1}`, `inline_ata_creatable_count=38`, `inline_ata_template_ready_count=38`, `pm_reserve_input_ok_count=39`, `pm_reserve_rent_ok_count=39`, `net_ev_with_ata_burned_positive_count=0`, `net_ev_with_ata_recovered_positive_count=28`, `inline_ata_ev_positive_count=27`, `policy_actionable_now_count=0`, `explicit_allow_required_count=38`, `hard_policy_risk_count=1`, `wire_send=0`, `live_landed=0`, `ata_created=false`. Top row `AuRo...` has `net_ev_with_ata_burned=-1505220` and `net_ev_with_ata_recovered=534060`, but `policy_verdict=metadata_only_token2022_requires_explicit_allow` and `policy_actionable_now=false`. Interpretation: inline recovered-rent economics are interesting, but 66W template no-send is not opened because there are zero policy-actionable candidates and no explicit allow is approved.
- **Run66CI2 / 66V policy-clean scans (`2026-05-25`):** owner-aligned continuation instrument search and policy-clean inline ATA scan were added as no-send filters. `66ci2_owner_aligned_continuation_instrument_search.sh` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66ci2_owner_aligned_continuation_instrument_search_20260524_3x5m/`. It starts only from exact-positive continuation rows and shows `exact_positive_continuation_rows=63`, `owner_aligned_inventory_only_rows=58`, `instrument_group_count=12`, `decision_counts={"REJECT_DUST_EXIT":2,"REJECT_LOW_EV":3,"REJECT_POLICY":7}`, `continuation_instrument_candidate_count=0`, `wire_send=0`, `live_landed=0`; the top group has exact/min-out/matched-EV proof but `policy_verdict=unknown_token_program` and dust/no exit. `66v_policy_clean_inline_ata_scan.sh` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66v_policy_clean_inline_ata_scan_20260524_5m_bridge2/`. It hard-filters 66V to `policy_actionable_now=true`, `explicit_allow_required=false`, `hard_policy_risk=false` and finds `policy_clean_row_count=0`, `policy_clean_inline_ata_candidate_count=0`, `wire_send=0`, `live_landed=0`, `ata_created=false`. Current stuck point is confirmed: the engine has exact-positive Continuation/Rebalancing rows, but no policy-clean, owner-mode-correct execution instrument.
- **Run66CORE / 66EA / 66V2 usable-now scans (`2026-05-25`):** the new search basis was added and run on `.15` as no-send artifact filters. `66core_core_inventory_postshock_scan.sh` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66core_core_inventory_postshock_scan_20260524_3x5m/`. It finds exactly one core-pair exact-positive row (`WSOL -> USDT`, Raydium CLMM pool `3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF`) with `core_exact_positive_count=1`, exact quote/min-out/matched EV/PM all green, but `core_inventory_would_fire=0` because `first_core_inventory_blocker_counts={"account_ata_missing":1}`. `66ea_existing_account_postshock_scan.sh` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66ea_existing_account_postshock_scan_20260524_3x5m/`. It scans all `107` exact-positive post-shock rows and finds `existing_account_would_fire=0`, with blockers `inventory_not_ready=63`, `account_ata_missing=39`, `exit_rule_missing=4`, `entry_rule_missing=1`; no new account/inventory path is usable now. `66v2_policy_clean_inline_one_shot.sh` artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66v2_policy_clean_inline_one_shot_20260524_5m_bridge2/`. It hard-filters inline ATA rows to policy-actionable, no explicit allow, no hard policy risk, template/PM/EV-ready and finds `policy_clean_row_count=0`, `policy_clean_inline_one_shot_count=0`. Current truth: the artifact set contains post-shock exact-positive rows, including one core-pair row, but still zero no-new-mint / no-new-ATA / no-acquisition / no-explicit-allow execution candidates. Keep live closed.
- **Run66CORE-USDT plan-only review / Run77B prep repair (`2026-05-25`):** `66core_usdt_plan_only_review.sh` was added as the one remaining narrow post-shock review worth preserving. Artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/66core_usdt_plan_only_review_20260524_3x5m/`. Result: `usdt_core_row_count=1`, `usdt_ata_present=false`, `token_program_id=TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`, `ata_rent_lamports=2039280`, `core_ata_plan_review_count=0`, `park_low_ev_or_one_off_count=1`, `top_exact_quote_match=true`, `top_min_out_raw=18785531`, `top_matched_rollout_net_ev_after_fee_tip_raw=79889`, `top_net_ev_after_route_and_ata_burned_raw=-1959391`, `top_synthetic_usdt_ata_would_fire_delta=1`, `top_repeat_observed_count=1`, `top_pm_reserve_sim_ok=true`, `top_decision=PARK_LOW_EV_OR_ONE_OFF`, with `wire_send=0`, `live_landed=0`, `ata_created=false`. Interpretation: the only non-Token-2022 core-pair post-shock row is real but still too small / too one-off to justify ATA action under the current evidence window. Separately, local `77b_prehydrated_selected_numeric_safe_directarb_tpu_nosend.sh` had drifted into the inverse TPU/QUIC posture (`quic_workers=0` only); that repair is now landed locally and on `.15`: default `SHADOW_RUN77B_TPU_QUIC_WORKERS=2`, `quic_workers>=2` required, `blocked_quic_workers_zero` for `0`, and `blocked_quic_workers_too_low` for `1`. No native Run77B lease was armed as part of this repair.
- **Run77B native no-send acceptance (`2026-05-25`):** the fresh epoch8 `Run76` selector was restored into the `g1-host-pack/results` path, `Run77B` prehydrate-only was rerun under `SHADOW_RESULTS_DIR=/home/ubuntu/Guerrila/g1-host-pack/results`, and the bounded native AF_XDP zero-copy lease then passed on `.15`. A first native retry failed for a real proof-config reason rather than transport: the temporary `run40.config.yaml` inherited `system.proxies.ingress.grpc_source_grade=laserstream`, which the currently running parser on `.15` rejected. `77b_prehydrated_selected_numeric_safe_directarb_tpu_nosend.sh` now normalizes that field to `external_geyser` in the **temporary Run77B proof config only** while still forcing `quic_workers=2`; the permanent Guerrila config remains unchanged. Final artifact: `/home/ubuntu/Guerrila/g1-host-pack/results/epoch8_auto_strict_directarb_quote_identity_run77_prehydrated/` with inner `Run40` artifact `/home/ubuntu/Guerrila/g1-host-pack/results/epoch8_auto_strict_directarb_quote_identity_run77_prehydrated_run40/`. Final proof counters are `selection_loaded=1`, `selected_signature=2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4`, `selected_pool=E9dp...`, `selected_route=tpu_quic_public`, `replay_hydration_mode=prehydrated_artifact`, `replay_hydration_before_xdp=true`, `replay_hydration_rpc_during_lease=false`, `prehydrated_replay_tx_loaded=1`, `route_resolved_tpu_quic_public=1`, `tpu_quic_candidate_seen=1`, `tpu_quic_would_enqueue=1`, `tpu_quic_ring_enqueue_ok=1`, `tpu_quic_sender_dequeue_count=1`, `tpu_quic_no_send_accept_count=1`, `tpu_wire_send_count=0`, `live_landed=0`, `inner_status=tpu_quic_no_send_ready`, and `acceptance_pass=true`. Cleanup also closed green after a transient DNS wobble on the first Agave restart attempt: `cleanup_verified=true`, `eno1_xdp_lines=0`, `xsk_map_after_cleanup=absent`, `shadow_runtime_process_count_after_cleanup=0`, `udp_8000_8025_count=30`, and `agave_health_after_cleanup=ok`. Important doctrine note: this does **not** promote public TPU back to the main egress path. Current egress doctrine remains **private lanes**; `Run77B` uses `tpu_quic_public` only as a narrowly scoped replay/funnel proof artifact. Current classification: `Run77B strict DirectArb replay` is real no-send transport proof, while live/production egress policy remains private-lane-first.

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
