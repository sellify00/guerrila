# Shadow Engine V6.4-Guerrila — Handoff Status Memo

**Date:** 2026-05-08

**Purpose:** Bring the next agent up to speed after G12 observe-only safety proof, SH8 AF_XDP private-grade proof, AF_XDP-vs-Triton latency comparison, R1-FIX-B no-send proof, R1 tiny-live private-lane diagnostics, the R1-JITO-JSON no-send host proof, and the 2026-05-06 operator pivot to explicit public tiny-live.

**Doctrine update 2026-05-08:** Shadow Engine is now framed as a **micro-impulse propagation engine**, not a rare-shock backrun bot. Immediate Backrun remains a primitive, but the active research frontier is always-on post-impulse scoring across Continuation, Housekeeping, Rebalancing, Scout/Press/Kill, Flatten, and Drop. The speed advantage is AF_XDP/DPDK plus O(1) worker latency observing propagation before slower systems classify it. This loosens trigger admission and observation volume only; it does not loosen live-send caps, provenance, source-grade truth, route legality, ShadowGuard, inventory, fee floors, or PM reconciliation.

**Current next step:** Private access is gatekept/sparse enough that private-lane-only routing is no longer the primary live path. R1-JITO-JSON no-send is green, and the **R1-PUBLIC-A** local foundation is now implemented: explicit public RPC scout config/validation, explicit `sendTransaction` egress wiring, first-landed stop/reconcile behavior, and machine-enforced class-D floors (`min_retained_ev_lamports > 0`, `min_net_after_fee_lamports > 0`) that fail closed before real send while leaving observe-only / no-send paths intact. But the fresh `.51` PS-P0 rerun changes the research decision: the recorder is now good enough, and the current external-geyser/public slice is **NEGATIVE_CALIBRATION_A_ONLY**. It produced `3477` rollout observations and `3470` forward-markout observations with `0` positive `t0` retained EV, `0` positive forward markout, `0` positive after-fee markout, and `0` D-class candidates. So do **not** attack `.51` as-run, do **not** fee-tune this slice, and do **not** promote pools `439504`, `705258`, or `705282`; keep them as negative examples of “busy but worthless” structure. Cross-pool / route-lag markouts are now landed locally as additive `ps_p0_route_lag_markout` records keyed by venue + route family and excluding same-pool reuse. The first decoder expansion slice is also landed locally: Raydium CPMM exact-input decode in `shadow-ingest`, Raydium CPMM pool classification/decode in `shadow-state`, and observe configs widened so CPMM enters the live/bootstrap surface. The first offline expanded-surface report path is also landed locally as `run\ps_p0_abcd_report.js`; it reads the PM shock-research JSONL and emits A/B/C/D splits by venue, motif, tactic family, route family, explicit tape `pool_class` (with fallback for older tapes), surface, and pool. Host `.51` is reachable as `ubuntu`, but because its RAM class is still only about `125 GiB`, Agave bootstrap was intentionally stopped at about `49.9%` snapshot download and local RPC is now down again; this host should be treated as another constrained rehearsal/transfer box, not the final combined target. Keep R1-PUBLIC-A ready, but only arm live send from class-D PS-P0 candidates on the replacement host path. AF_XDP/private-grade proof and the first expanded-surface tape now belong on the next properly sized host, not on `.51`. Separately, rescue-mode inspection of replacement host `.221` (`259S001355`, Intel `i40e`) confirmed that using XDP/AF_XDP on `eno1` is unsafe there: the 2026-05-06 observe-only run attached `xdpgeneric` on `eno1` at `17:31:49 UTC`, the mounted journal began logging repeated `i40e 0000:05:00.0` `AMD-Vi: IO_PAGE_FAULT` / `IOMMU Event log restarting` at `17:31:51 UTC`, and the host later lost reachability even though the observe window itself completed and emitted gate evidence. On `.221`, reserve `eno1` for management / normal Agave traffic only; any future AF_XDP attempt belongs on `eno2` only if carrier is present. DPDK on `.221` is now materially farther along: the standalone harness and the in-process Tier0 DPDK path both build and run under guarded VFIO windows on `eno1`, and the host survives the restore path. But the latest live sample on `eno1` is still only tiny UDP gossip (`132`-byte packets; no packets `>1200` bytes in a 20s sample), so the full DPDK producer -> feed-ring -> money-worker counters remain `0`. The per-source latency comparison foundation is now also landed locally: `shadow-tier0` exports `g12_latency_by_source.json` with `public_geyser`, `external_geyser`, `local_validator_geyser`, `afxdp`, `dpdk`, and aggregate `triton_grpc` buckets across `source_to_feed`, `source_to_worker`, `source_to_process_intent`, `source_to_nosend`, and `source_to_final_shadow_event` plus the older stage-local buckets. That means gRPC end-to-end comparison is now measurable immediately, while DPDK source-to-worker / source-to-no-send remains correctly empty until DPDK emits real `IngressRecord`s. Latitude has now prepared the next host: `109.94.99.15` (`G-12`, Intel X550 / `ixgbe`). Real routed-`eno1` AF_XDP zero-copy bind is now proven stable there: `g4-ingress-seal` reached zero-copy XSK bind and, after fixing pinned XSKMAP registration in `crates/shadow-tier0/src/xsk.rs`, XSKMAP registration too, with no host hang, no IOMMU faults, and clean `eno1` restore. Guarded DPDK on the same routed `eno1` is also now green enough for host bring-up: the standalone harness completed with `dpdk_rx_to_classify_p999_ns=740`, `dpdk_kernel_faults=0`, and clean `ixgbe` restore. But both AF_XDP and DPDK are still packet-dark for Solana UDP there right now (`tcpdump` saw `0` UDP `8000-8025`, and the DPDK harness saw `27` packets total but `0` matching Solana UDP), so meaningful producer/feed-ring proof still needs Agave/bootstrap or replay traffic. Treat `.221` as the completed i40e feasibility/archive box and `.15` as the active X550 proving host.



**Current research pivot:** Clean backrun-only samples have shown sparse/flat economics so far, while older class-D evidence was not cleanly reproducible under isolated per-run provenance. The next proof path is **Impulse/Continuation P0**: treat any relevant on-chain market event as an impulse, record sub-slot and event/slot markouts, and report per-owner A/B/C/D by durable pool pubkey and mint pair. Public send remains blocked until clean per-run positive net-after-fee evidence exists. External/local geyser data may be used for observation and public-scout research, but remains non-private-grade.

**Impulse P0 required outputs:** `impulse_id`, durable `pool_pubkey`, venue, mint pair, impulse type/tier (`S0_micro_impulse`, `S1_reserve_displacement`, `S2_same_direction_burst`, `S3_route_lag_impulse`), direction, size, price impact, reserve displacement, `50ms/100ms/250ms/500ms` markouts, `+1/+2/+3` same-pool event markouts, `+1/+2` slot markouts, same-direction follow-through, absorption, route-gap decay, reserve recovery half-life, Backrun/Continuation/Housekeeping/Rebalancing/Drop scores, selected owner, fallback owner, and net-after-fee.

---

## 1. Executive state

G12 is **in progress**, not complete.

What is now green:

- G12-R0 long observe-only / no-send safety soak is green.
- SH7-A/B/C are green: final no-send publication, micro-market observation, and ShadowGuard ops proof are closed.
- SH8-A/B/C are green for observe-only AF_XDP private-grade proof.
- AF_XDP vs Triton gRPC latency A/B is complete and strongly favors AF_XDP.
- Standalone and in-process DPDK feasibility on `.221` is green enough to continue as an optional post-G12 ingress candidate: release `shadow-tier0 --features dpdk` now builds on-host, a guarded `eno1` VFIO observe-only run boots the full Tier0 process, and the rollback path restores host reachability.
- Replacement host `109.94.99.15` (`G-12`, Intel X550 / `ixgbe`) now proves routed-`eno1` AF_XDP zero-copy stability: real `g4-ingress-seal` zero-copy bind and XSKMAP registration succeed there, the host stays reachable, and kernel logs show no IOMMU/watchdog fault pattern.
- Standalone DPDK host bring-up is now also green on `109.94.99.15`: the guarded `eno1` VFIO harness completes with `dpdk_rx_to_classify_p999_ns=740`, `dpdk_kernel_faults=0`, and clean `ixgbe` restore.
- The comparable latency foundation is now landed locally: `g12_latency_by_source.json` exports source-split p9/p50/p90/p95/p99/p999/max/count snapshots for gRPC/AF_XDP/DPDK stages without changing the fixed IPC record contracts.
- R1 `auto_private_grade` remains the intended live gate; wallet allowlists are fallback-only.
- R1-FIX-B no-send proof is green on host `.57` under the corrected packaged artifact.
- R1 AF_XDP hot path is proven under temporary Jito no-auth diagnostics through route/actionable, R1 auto-admission, `process_intent`, template build, and pipeline success.
- R1-JITO-JSON no-send proof is green on host `.57` through no-send backend, final no-send publication, private-grade would-send evidence, and micro-market observation.
- Jito/Harmonic session diagnostics and retry safety are improved: Jito unauthorized auth now backs off, and Harmonic regional channel failures are explicitly logged.
- PS-P0 forward-markout recording is now landed and validated. The fresh `.51` rerun is decisive negative calibration: structurally active, but `0` positive forward markouts and `0` positive after-fee records.

What is not green:

- Full G12 promotion is still a separate gate.
- Productive live landing is **not** proven.
- Jito rejects the staged auth pubkey, and Harmonic endpoints fail channel connect.
- R1-PUBLIC-A is implemented locally, including machine-enforced positive retained-EV and positive net-after-fee floors for real send, but the latest `.51` PS-P0 rerun is A-only negative calibration. Keep the public scout ready, but do not arm it from this slice; only class-D PS-P0 candidates may trigger send.
- R1-PUBLIC-C-style surface expansion is the next research task; cross-pool / route-lag markouts are now landed locally in the PM research tape, the first decoder expansion slice (Raydium CPMM exact-input + pool-state support) is in, explicit `pool_class` is now emitted in PS-P0 tape rows, and the offline A/B/C/D analyzer path is now available as `run\ps_p0_abcd_report.js`. Broader decoder coverage beyond that, plus the first real host-generated expanded-surface report, is still open.
- Host `.57` is currently powered off after the R1-JITO-JSON no-send proof; `.51` has now also been reclassified as a constrained transfer host and had Agave bootstrap intentionally stopped at about `49.9%` snapshot download. Future host-side work now requires reprovision onto the intended `256/384 GiB` class.
- Full Tier0 DPDK p99/p999 is **not** green yet. The `.221` guarded `eno1` DPDK observe-only run boots and the `.15` standalone harness completes, but neither current host window carries shred-sized Solana ingress; on `.15`, plain `tcpdump` saw `0` UDP `8000-8025` and the harness saw `0` matching Solana UDP, so `worker_signal_dpdk` and all `dpdk_*` feed/worker latency counters remain `0` for real traffic-shape reasons.
- The new per-source latency export has not yet been exercised on a shred-bearing DPDK window. On current `.221` and `.15` data, the DPDK source-to-* buckets are correctly empty because no DPDK records crossed the feed boundary.

---

## 2. Milestone board

| Gate | Status | Notes |
|---|---:|---|
| G9 | IMPLEMENTED | `shadow-oracle`, dual LUT banks, competition-adjusted shadow path. |
| G9A | IMPLEMENTED / USER-SIGNED-OFF | CU density, deterministic Jito-only exhaust, tick-boundary shadow telemetry. |
| G9B | PARTIAL / OBSERVE-ONLY FOUNDATION IMPLEMENTED | A-E landed; live F/G/H still deferred. |
| G10 | IMPLEMENTED / OPS-PROVED | ShadowGuard software proof plus mainnet deploy/smoke proof are green. |
| G11 | IMPLEMENTED / SOFTWARE-SIGNED-OFF | Rosetta auto-heal complete. |
| G12 | IN PROGRESS | R0 observe-only safety, SH8 AF_XDP private-grade proof, latency proof, R1-FIX-B no-send proof, R1 hot-path diagnostic proof, and R1-JITO-JSON no-send proof are green; R1-PUBLIC-A local foundation and PS-P0 forward-markout recorder are implemented, and the fresh `.51` rerun is decisive A-only negative calibration (`3470` forward-markout observations, `0` positive before fees, `0` positive after fees). The next move is surface expansion / motif discovery, while AF_XDP/private-grade source truth on `.51` remains a separate host gate before any live proof. |
| G13 | NOT STARTED | Graduation after sustained live proof. |

### 2.1 SolanaHost side board

| Gate | Status | Notes |
|---|---:|---|
| SH0 | CLOSED | Source-grade truth plumbing remains fail-closed. |
| SH1 | CLOSED FOR CONSTRAINED HOST | Agave / Yellowstone build + lab pack replayed. |
| SH2 | PARTIAL ACCEPT / CONSTRAINED HOST | Earlier constrained host was useful for replay but not final production sizing. |
| SH3-lite | CLOSED | Storage / firewall rehearsal repeatable on fresh hourly hosts. |
| SH4-lite | CLOSED | Localhost Yellowstone smoke passed. |
| SH5-A/B | CLOSED | Local-validator-Geyser source proof and supported-fixture software proof passed without private/live contamination. |
| SH7-A/B/C | CLOSED | Final no-send, micro-market uplift accounting, and ShadowGuard ops proof are green. |
| SH8-A/B/C | CLOSED FOR OBSERVE-ONLY AF_XDP PROOF | AF_XDP source probe, transaction-derived records, and final private-grade soak are green. |

---

## 3. Latest verified G12 truth

### 3.1 G12-R0 long observe-only safety soak

Artifact:

```text
/home/ubuntu/shadow-host-audit/g12-r0-observe/20260429_084358-long-accumulation
```

Key counters:

```text
status=threshold_reached
sample_slots=18401
observe_candidates=10335
rollout_would_send=3743
rollout_live_send_suppressed=3743
micro_market_observations=3743
micro_market_uplift_ok=true
shadow_guard_mainnet_deployed=true
shadow_guard_template_program_id_matches=true
worker_health_drops=0
live_landed=0
all_gates_green=false
promote_allowed=false
```

This closed the long no-send safety soak. It did not close private-grade admission because it was Geyser-only.

### 3.2 SH8 AF_XDP private-grade proof

Decisive artifacts:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_235006-af_xdp-32q-sh8-probe
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_031546-af_xdp-32q-sh8b-txdiag
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak
```

SH8-C final soak counters:

```text
afxdp_tx_extract_ok=286568
afxdp_tx_record_emit_ok=15
afxdp_feed_enqueue_ok=15
worker_signal_afxdp=66
observe_candidates_private_grade=15
rollout_private_grade_admissible_opportunities=5
live_landed=0
worker_health_drops=0
```

Interpretation: AF_XDP observe-only private-grade ingress is proven on `.57`; full G12 promotion remains a separate threshold decision.

### 3.3 AF_XDP vs Triton latency A/B

Valid artifact:

```text
/home/ubuntu/shadow-host-audit/g12-latency-ab/20260430_095649-afxdp-rerun-vs-triton-valid
```

Result:

```text
AF_XDP end-to-end p999=76700 ns
Triton gRPC end-to-end p999=128661054 ns
AF_XDP producer-to-feed p999=105140 ns
Triton gRPC producer-to-feed p999=142475814 ns
live_landed=0
worker_health_drops=0
```

Decision: keep AF_XDP primary for G12 and for any current private-grade proof, but DPDK is now an active **optional** post-G12 / AF_XDP-instability path across two host families: `.221` proves `i40e` VFIO + DPDK RX viability where AF_XDP on routed `eno1` is unsafe, while `109.94.99.15` proves X550 / `ixgbe` routed-`eno1` AF_XDP zero-copy stability plus guarded DPDK host bring-up.

### 3.3a DPDK feasibility on `.221`

Standalone DPDK RX-loop proof on `.221` (`eno1`) is green again:

```text
/home/ubuntu/shadow-host-audit/21_dpdk_rx_latency/dpdk-rx-latency.json
```

Latest 30-second result:

```text
dpdk_packets_seen=107
dpdk_udp_8000_8025_seen=73
dpdk_rx_to_classify_p99_ns=920
dpdk_rx_to_classify_p999_ns=950
dpdk_kernel_faults=0
```

The in-process Tier0 DPDK path is also now real on the same host:

```text
/home/ubuntu/shadow-host-audit/22_dpdk_tier0_observe_only/
```

What is proven:

- release `shadow-tier0 --features dpdk` now builds on Linux `.221`;
- guarded VFIO takeover on `eno1` can boot the full Tier0 process with `private_packet_backend=dpdk`;
- the host survives the run and returns to SSH reachability after restore;
- `shadow_blockhash` fallback/reuse is now needed on a management NIC because outbound RPC/DNS disappears during VFIO ownership.

What is **not** proven yet:

```text
worker_signal_dpdk=0
dpdk_producer_to_feed_count=0
dpdk_feed_to_worker_count=0
dpdk_worker_to_nosend_count=0
dpdk_end_to_end_observe_count=0
```

Interpretation: DPDK RX itself is alive, but the current `eno1` sample is not a valid full-Tier0 proof window. A live `tcpdump` sample on `.221` showed only `132`-byte UDP packets to ports `8000/8001`, and a 20-second `greater 1200` sample saw `0` packets. So the current blocker is **traffic shape**, not DPDK bootstrap or money-worker wiring.

### 3.3b Replacement host `.15` (`109.94.99.15`, X550 / `ixgbe`)

The newly prepared X550 host is now the active proving box for routed-`eno1` packet-path work:

- hostname `G-12`, dual Intel X550 (`8086:1563`) on `ixgbe`, with `eno1` carrying the public route;
- routed-`eno1` AF_XDP zero-copy is stable there: `g4-ingress-seal` reached zero-copy XSK bind, and after the pinned XSKMAP `EINVAL` fix in `crates/shadow-tier0/src/xsk.rs`, XSKMAP registration too;
- the host stayed reachable before and after the AF_XDP probe, `eno1` restored cleanly, and kernel logs showed normal XDP queue toggling rather than the `.221`-style IOMMU/watchdog fault pattern;
- the guarded standalone DPDK harness now also runs on the same routed `eno1` and restores cleanly.

Current DPDK artifact:

```text
/home/ubuntu/shadow-host-audit/dpdk-runs/21_dpdk_rx_latency/dpdk-rx-latency.json
```

Latest 20-second result:

```text
dpdk_packets_seen=27
dpdk_udp_8000_8025_seen=0
dpdk_rx_to_classify_p99_ns=740
dpdk_rx_to_classify_p999_ns=740
dpdk_kernel_faults=0
```

Interpretation: the X550 host now proves both AF_XDP zero-copy host safety and guarded DPDK VFIO bring-up on routed `eno1`, but it is still not a packet-bearing Solana ingress window. A plain `tcpdump` sample on `.15` saw `0` UDP packets in `8000-8025`, and the DPDK harness saw only `27` total packets with `0` matching Solana UDP. So the next blocker on `.15` is again **traffic shape / feed arrival**, not packet-path stability.

### 3.4 R1-FIX-B no-send proof

Meaningful host outcome under the stronger pool-cap corrected no-send artifact:

```text
/home/ubuntu/shadow-host-audit/r1-damm-v2-nosend/20260501_040001-poolcap-1m-nosend-afxdp
```

```text
worker_signal_afxdp=5991
r1_afxdp_private_grade_route_actionable=2037
r1_afxdp_private_grade_process_intent_entered=2037
worker_no_send_backend_called=21748
worker_final_no_send_event_published=28964
rollout_afxdp_private_grade_would_send=2689
rollout_afxdp_private_grade_live_send_suppressed=2689
micro_market_afxdp_private_grade_observations=2689
live_landed=0
worker_health_drops=0
```

Interpretation:

- the pre-`process_intent` R1 choke point is fixed;
- the no-send proof reaches the intended downstream evidence surfaces;
- R1 admission, deployed-size capping, and final no-send publication are no longer the active blocker.

### 3.4a R1-JITO-JSON no-send host proof

R1-JITO-JSON was synced, built, and config-validated on `.57`, then run in observe-only/no-send with Harmonic disabled and public fallback still amputated:

```text
/home/ubuntu/shadow-host-audit/r1-jito-json/20260501_062327-r1-jito-json-nosend-afxdp
```

```text
afxdp_tx_extract_ok=62764
afxdp_tx_record_emit_ok=7227
worker_signal_afxdp=5441
r1_afxdp_private_grade_route_actionable=4702
r1_afxdp_private_grade_process_intent_entered=4702
worker_no_send_backend_called=56970
worker_final_no_send_event_published=74213
rollout_afxdp_private_grade_would_send=5999
micro_market_afxdp_private_grade_observations=5999
live_landed=0
worker_health_drops=0
```

Operational note: native XDP attach transiently dropped carrier on this host; the green run used `xdpgeneric`, and XDP/XSKMAP were detached/removed after the window.

### 3.5 Tiny-live private-lane diagnostics

The first tiny-live scout exposed producer-side unsupported-program/decode coverage. That was no longer the final blocker after subsequent DAMM/route/hydration work and a larger no-send proof. The key later diagnostic is temporary Jito no-auth, Jito-only, with the private-lane health recovery fix:

```text
/home/ubuntu/shadow-host-audit/r1-tiny-live/20260501_044401-r1-tiny-live-jito-only-noauth-healthfix-afxdp
```

```text
worker_route_actionable=1204
r1_afxdp_private_grade_route_actionable=1204
r1_afxdp_private_grade_process_intent_entered=1187
r1_auto_admission_attempt=1204
r1_auto_admission_admit=1187
worker_pipeline_attempts=1187
worker_pipeline_success=1187
worker_process_intent_ok=1187
worker_template_build_ok=1187
live_landed=0
worker_health_drops=0
```

Interpretation:

- AF_XDP ingress, route/actionability, R1 admission, `process_intent`, template build, and pipeline success are no longer the current blocker.
- This diagnostic used `jito.auth_mode: no_auth`; it is not an acceptable live landing configuration.
- Real private-lane live remains blocked by provider access.

Current private-lane auth/connectivity evidence:

```text
/home/ubuntu/shadow-host-audit/r1-tiny-live/20260501_051013-r1-tiny-live-dual-lane-authdiag-afxdp

worker_signal_afxdp=3039
afxdp_tx_extract_ok=47899
afxdp_tx_record_emit_ok=3232
worker_route_actionable=0
r1_afxdp_private_grade_route_actionable=0
worker_route_reason_no_viable_route=1277
worker_route_resolved_jito=0
worker_route_resolved_harmonic=0
live_landed=0
worker_health_drops=0
jito_auth_failed_log_count=2
harmonic_failed_log_count=69
```

Jito rejects staged auth pubkey `3i9N1VhsmKJBXB4STP4xdinmAXYCRecmNHBbLkSzwCW4` with `PermissionDenied`. Harmonic auth pubkey `9ZaQnhU56rxrw3mkGrVHaebTFXF1FgqAdvanTPUJMPwa` is staged, but configured `fra/ams/nrt.harmonic.xyz` endpoints fail at channel transport connect. The worker signer `ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX` was also tested as Jito auth and rejected.

---

## 4. R1 status and current blocker

The intended R1 product shape is still:

```yaml
r1:
  live_admission:
    mode: auto_private_grade
    source_required: afxdp
    require_private_grade: true
    require_shadow_guard: true
    require_micro_market_uplift: true
    require_pre_sign_egress_admission: true
    max_live_trade_input_sol: 1.0
    max_position_sol_per_mint: 1.0
    max_notional_sol_per_entity: 2.0
```

Important implementation facts:

- default R1 mode is `auto_private_grade`;
- manual signer allowlist remains explicit fallback only;
- `pass_all` is rejected for R1 live admission;
- tier0 bypasses signer identity prefilter only for R1 auto mode so the money worker can gate on candidate truth;
- rollout/G12 exports include R1 auto-admission counters.

Current private-lane blocker:

```text
AF_XDP records work
-> R1 hot path works
-> Jito JSON-RPC no-send path is green
-> Jito auth is unauthorized
-> Harmonic endpoints fail transport connect
-> JSON-RPC tiny-live send/landing remains unproven
```

So the active blocker is **live send/landing proof**, not host bootstrap, Agave bootstrap, ShadowGuard, AF_XDP ingress, R1 admission, or the JSON-RPC no-send path.

Because the user does not have practical provider contacts/access, but Jito JSON-RPC may not require the same gRPC token path, the next live path is now:

```text
R1-JITO-JSON
```

This must keep Harmonic disabled and public execution amputated:

```yaml
submit.routing.default_jito_searcher: jito_json_bundle
submit.routing.enable_harmonic_lane: false
jito.json_rpc.enabled: true
jito.json_rpc.endpoint: "https://tokyo.mainnet.block-engine.jito.wtf"
jito.json_rpc.mode: bundle_only
```

The next approval gate is JSON-RPC tiny-live. If Jito JSON-RPC tiny-live fails due auth/rate/landing, public-tiny becomes the fallback. Public-tiny must be a separate mode/config, may not be described as private-lane R1 green, and may not silently weaken the existing private-lane configuration.

---

## 5. Important negatives

Do **not** treat the current state as:

- permission to use a wallet allowlist as the normal R1 live gate;
- permission to use `pass_all` for R1;
- permission to relabel Geyser evidence as private-grade;
- permission to silently reopen public execution inside the private-lane Guerrila config;
- proof that productive live R1 tiny is green;
- proof that full G12 promotion is complete;
- proof that a new host alone fixes the current R1 blocker.

The correct production direction remains:

```text
AF_XDP packet
-> fixed-width decode/classify/quote
-> ShadowGuard/freshness/inventory/micro-market gates
-> R1 auto_private_grade admission
-> Jito JSON-RPC Block Engine send before public
-> explicit public-tiny fallback only if JSON-RPC tiny-live fails and it is deliberately configured/labeled
```

Not:

```text
observe traffic
-> dump wallets
-> manually choose wallets
-> trade against a static allowlist
```

---

## 6. Resume plan

When work resumes:

1. treat Jito gRPC/Harmonic private-lane R1 as blocked on provider access;
2. treat R1-JITO-JSON no-send as green, but do not keep private-lane-only routing as the next live path;
3. treat the fresh `.51` PS-P0 rerun as **NEGATIVE_CALIBRATION_A_ONLY** and preserve pools `439504`, `705258`, and `705282` as do-not-target negative examples for the current slice;
4. stop rerunning the same external-geyser surface expecting profit; the current answer is no;
5. move next research to R1-PUBLIC-C-style surface expansion and motif discovery: Raydium CPMM/CLMM, Orca, broader Meteora, PumpSwap, and cross-pool / route-lag markouts;
6. keep R1-PUBLIC-A local code/config work ready, but only arm the explicit public RPC scout under `0.05 SOL` input cap and `0.1 SOL` entity cap when PS-P0 yields class-D candidates;
7. keep AF_XDP source requirement, ShadowGuard/bounds logic where applicable, R1 auto-admission, inventory truth, fee-leak accounting, PM reconciliation, kill switches, micro-market uplift, and tiny caps intact;
8. Latitude has now prepared the next host; start there from `host_run_guide.md`, then `SH\SH_RUNBOOK.md`, then `SH\SolanaHost-checklist.md`;
9. before shutting down `.221`, preserve `~/shadow-host-audit/`, the current repo/config state, and any generated `g12_gate_input.json`, `g12_gate_report.json`, and `g12_latency_by_source.json` artifacts;
10. keep public execution out of private-lane configs; it must be explicit R1-PUBLIC mode, not a hidden fallback.

---

## 7. Files most relevant now

- `host_run_guide.md`
- `SH\SH_RUNBOOK.md`
- `SH\SolanaHost-checklist.md`
- `crates\shadow-tier0\src\afxdp_runtime.rs`
- `crates\shadow-tier0\src\afxdp_producer.rs`
- `crates\shadow-tier0\src\ingress_control.rs`
- `crates\shadow-tier0\src\main.rs`
- `crates\shadow-tier0\src\grpc_normalize.rs`
- `crates\shadow-tier0\src\grpc_producer.rs`
- `crates\shadow-tier0\src\tail_latency.rs`
- `crates\shadow-tier0\src\money_worker.rs`
- `crates\shadow-tier0\src\rollout_gate_export.rs`
- `crates\shadow-control\src\rollout_gate.rs`
- `crates\shadow-ingest\src\pipeline.rs`
- `crates\shadow-ingest\src\route.rs`
- `crates\shadow-egress-jito\src\session.rs`
- `crates\shadow-egress-jito\src\proxy.rs`
- `crates\shadow-harmonic\src\session.rs`
- `crates\shadow-harmonic\src\proxy.rs`
- `g1-host-pack\config.guerrila.yaml`
- `g12-qc\G12_REPORT.md`
- `g12-qc\G12-QC.md`

---

## 8. One-line resume point

**Resume from surface expansion, not another `.51` replay: AF_XDP/private-grade ingress, R1 no-send proof, R1 hot-path processing, JSON-RPC-selected no-send publication, and the explicit public-RPC scout foundation are green locally; the PS-P0 forward-markout recorder is also green, and the fresh `.51` rerun is decisive A-only negative calibration (`3477` rollout observations, `3470` forward-markout observations, `0` positive before fees, `0` positive after fees, `0` D-class candidates), so do not attack `.51` as-run or promote pools `439504`, `705258`, or `705282`; instead broaden surface coverage and add cross-pool / route-lag markouts, while keeping R1-PUBLIC-A ready but gated on class-D PS-P0 candidates.**
