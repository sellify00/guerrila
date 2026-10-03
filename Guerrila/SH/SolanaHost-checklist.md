# SolanaHost Checklist — Latitude Tokyo Self-Sourced Ingest Path

**Document type:** implementation checklist + staged test plan  
**Applies to:** Shadow Engine / Guerrila after the decision to stop depending on an external private feed being pre-mapped to `eno2`  
**Primary objective:** create our own Solana data source on Latitude Tokyo bare metal, use local Geyser/RPC first, and eventually graduate to raw AF_XDP shred / TVU packet capture from our own Solana node path.  
**End goal:** raw AF_XDP shred capture on Latitude Tokyo, feeding Shadow Engine through the existing fixed-width `IngressRecord` / feed-ring model. Public execution may only run through explicit R1-PUBLIC tiny-capped modes, never as a hidden fallback.
**Current practical target:** non-voting Agave RPC/Geyser node, not a voting validator.  
**Status:** SH0 is complete; SH2 practical host audit and SH3-lite runtime proof were completed on the first constrained host; SH4-lite Yellowstone smoke and SH5-A local-validator-Geyser ingest are green on the replayed replacement host; SH7-A final no-send, SH7-B micro-market proof, SH7-C ShadowGuard ops proof, and G12-R0 long observe-only safety soak are green. Runtime latency proof is green. SH8 is now closed green on `.57` for observe-only AF_XDP private-grade proof: SH8-A proved parsed-shred source receipt, SH8-B proved transaction-derived AF_XDP records, and SH8-C final soak artifact `/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak` produced `afxdp_tx_extract_ok=286568`, `afxdp_tx_record_emit_ok=15`, `afxdp_feed_enqueue_ok=15`, `worker_signal_afxdp=66`, `observe_candidates_private_grade=15`, `rollout_private_grade_admissible_opportunities=5`, `live_landed=0`, and `worker_health_drops=0`. Full G12 promotion remains governed by separate full-threshold gates. The replacement host is still only about `125 GiB` RAM, so this remains a constrained-host proof rather than final production acceptance. Current post-SH8 continuation is no longer a host-substrate blocker: the latest `.57` R1-JITO-JSON no-send proof is green, with AF_XDP/R1 reaching route/actionable, R1 auto-admission, `process_intent`, no-send backend, final no-send publication, private-grade would-send evidence, and micro-market observation. Jito gRPC live is blocked on provider auth and Harmonic endpoints fail channel transport connect. The 2026-05-06 operator pivot made R1-PUBLIC-A the next practical live gate, and that local foundation is now implemented: explicit public RPC scout config/validation, public RPC egress wiring, and one-landed stop/reconcile behavior. The remaining step is host deployment and first landed proof under `0.05 SOL` input cap and `0.1 SOL` entity cap.

---

## 0. Decision summary

The earlier assumption was that the rented bare-metal host might receive an external Solana private feed on a dedicated NIC such as `eno2`. Current evidence suggests that generic Latitude bare metal should be treated as normal compute/network unless a feed source is explicitly delivered by someone else.

This checklist pivots to a self-sourced model:

```text
Latitude Tokyo bare metal
→ our own Agave non-voting node / RPC / Geyser stack
→ local transaction stream
→ Shadow Engine ingest
→ later raw TVU/shred packet capture via AF_XDP/DPDK
```

This path does not require Latitude to already have a Solana private feed. It requires Latitude to provide a host and network capable of running a Solana node.

---

## 1. Ground rules

### 1.1 What we are building first

- [ ] A **non-voting Agave RPC/Geyser node** in Tokyo.
- [ ] A local Geyser/Yellowstone stream that Shadow Engine can consume.
- [ ] A timing comparator between Triton gRPC and our local node stream.
- [x] Source-grade telemetry so we know whether records came from Triton, local validator Geyser, or future AF_XDP.

### 1.2 What we are not building first

- [ ] Not a voting validator.
- [ ] Not a stake-seeking validator business.
- [ ] Not raw AF_XDP shred capture on day one.
- [ ] Not DPDK/FPGA yet.
- [ ] Not any public-lane execution fallback.

### 1.3 Required truth labels

Every event source must remain honestly labeled:

| Source | Truth label | May satisfy private-grade AF_XDP proof? | Notes |
|---|---:|---:|---|
| Triton gRPC | `external_geyser` / public-provider gRPC | No | Good enough for software proof and benchmarking. |
| Local Agave Geyser | `local_validator_geyser` | No, unless explicitly promoted by a later gate | Self-sourced and stronger than generic public fallback, but not raw AF_XDP. |
| AF_XDP local TVU/shred tap | `afxdp_shred` / private-grade packet ingress | Yes, if proven | Final target. |
| DPDK local TVU/shred tap | `dpdk_shred` | Yes, if proven | Optional later upgrade. |

Do not relabel Triton gRPC or local Geyser as AF_XDP/private-grade just to pass a gate.

---

## 2. Milestone map

| Milestone | Name | Purpose | Exit condition |
|---|---|---|---|
| SH0 | Local planning + source-grade changes | Prevent false attribution before host work | Shadow can distinguish Triton, local Geyser, and AF_XDP. |
| SH1 | Local toolchain lab | Build Agave/Yelllowstone/client pieces locally | Tools build; plugin config can be validated. |
| SH2 | Latitude host qualification | Prove the Tokyo host can run a Solana node | Hardware/network/ports pass. |
| SH3 | Non-voting Agave node | Join the Solana network without voting | Node runs, catches up, stable logs. |
| SH4 | Local Geyser/Yellowstone stream | Turn our node into a data source | Slots/transactions stream locally. |
| SH5 | Shadow ingest integration | Feed Shadow from local node | `worker_signal_local_validator_geyser > 0`. |
| SH6 | Triton-vs-local benchmark | Decide whether local node improves timing | Same-signature timing report with p50/p95/p99/p999. |
| SH7 | G12 software continuation | Use gRPC/local Geyser to finish software gates | final no-send, micro-market, ShadowGuard paths tested. |
| SH8 | Raw AF_XDP shred capture | Capture local Solana network packets directly | AF_XDP producer emits valid `IngressRecord`s. |
| SH9 | G13/G14 graduation | Decide whether AF_XDP is enough or DPDK needed | AF_XDP baseline timing/loss known; DPDK considered only if justified. |

**Current milestone state:** SH0 is closed. SH2 audit evidence and SH3-lite runtime proof were collected on the first constrained host using `SH/SH2-latitude-host-audit` and `SH/SH3-lite-storage-firewall`. That host is now historical rehearsal evidence only. Live continuation moved to constrained replacement hosts, where SH2 plus SH3-lite substrate replay are complete, SH4-lite Yellowstone smoke is closed / green, SH5-A local-validator-Geyser ingest is green / source-proof passed, SH7-A final no-send proof is green, SH7-B micro-market proof is green, and SH7-C ShadowGuard ops proof is green on `.57` with deployed program `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`. G12-R0 long observe-only accumulation reached full sample/candidate volume, latency proof is green, SH8 is closed green for observe-only AF_XDP private-grade proof, and R1-JITO-JSON no-send proof is green through `process_intent`, no-send backend, final no-send publication, private-grade would-send evidence, and micro-market observation. Broader G12 promotion remains separate and must still honor all full-threshold gates. Jito gRPC/Harmonic private egress is blocked, so the current practical path is R1-JITO-JSON tiny-live approval before any public-tiny fallback. For any newly provisioned host, bootstrap from `host_run_guide.md` first; this checklist remains the staged SH reference, not the root dependency/config guide.

---

## 3. SH0 — Shadow source-grade preparation (closed)

### 3.1 Add explicit source grades

- [x] Add `SIGNAL_SOURCE_EXTERNAL_GEYSER`.
- [x] Add `SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER`.
- [x] Keep `SIGNAL_SOURCE_AFXDP` reserved for actual AF_XDP packet ingress.
- [x] Keep `SIGNAL_SOURCE_PUBLIC_GEYSER` / external gRPC visibly separate from local validator Geyser.
- [x] Add the gRPC source-grade config field:

```yaml
system:
  proxies:
    ingress:
      grpc_source_grade: external_geyser
      grpc_workers: 1
      shred_capture_workers: 0
```

When pointing Shadow at a local Yellowstone endpoint later, flip the endpoint/token and set:

```yaml
system:
  proxies:
    ingress:
      grpc_fallback: "${LOCAL_VALIDATOR_GEYSER_ENDPOINT}"
      grpc_fallback_token: "${LOCAL_VALIDATOR_GEYSER_TOKEN}"
      grpc_source_grade: local_validator_geyser
```

### 3.2 Add non-gating diagnostics

- [x] `worker_signal_external_geyser`
- [x] `worker_signal_local_validator_geyser`
- [x] `worker_signal_afxdp`
- [x] `worker_signal_unknown`
- [x] `observe_candidates_total`
- [x] `observe_candidates_external_geyser`
- [x] `observe_candidates_local_validator_geyser`
- [x] `observe_candidates_private_grade`
- [x] `rollout_private_grade_admissible_opportunities`
- [x] `rollout_geyser_shadow_opportunities`

### 3.3 Preserve Guerrila firewalls

- [x] Public execution remains impossible in the private-lane Guerrila config.
- [ ] Public QUIC workers remain `0`.
- [ ] gRPC/local Geyser observe-only may produce shadow/no-send diagnostics only when explicitly enabled.
- [x] Live promotion may not use external gRPC evidence as private-grade evidence.
- [x] AF_XDP/private-ingress proof remains a distinct later gate.

SH0 stops at truthful source labels/counters plus AF_XDP-only private-grade accounting. A distinct observe-only/no-send route class for local validator Geyser is still deferred to later SH5/SH7 work.

### 3.4 Tests

- [x] External Geyser record increments external-Geyser counters only.
- [x] Local validator Geyser record increments local-validator counters only.
- [x] AF_XDP record increments AF_XDP/private-grade counters only.
- [ ] Public/external Geyser cannot reach live send in any private-lane config. R1-PUBLIC-A may use explicit public RPC only under its own mode/config, with `0.05 SOL` input cap, `0.1 SOL` entity cap, one-landed-tx stop/reconcile behavior, and no private-grade relabeling.
- [ ] Observe-only/no-send mode can allow Geyser records to test the no-send pipeline without satisfying private-grade proof.

---

## 4. SH1 — Local toolchain lab

### 4.0 Repo lab pack

- [x] `SH/SH1-local-agave-yellowstone/env.sh.example`
- [x] `SH/SH1-local-agave-yellowstone/lib.sh`
- [x] `SH/SH1-local-agave-yellowstone/00_install_build_deps.sh`
- [x] `SH/SH1-local-agave-yellowstone/10_build_agave.sh`
- [x] `SH/SH1-local-agave-yellowstone/20_build_yellowstone.sh`
- [x] `SH/SH1-local-agave-yellowstone/30_prepare_identity_and_dirs.sh`
- [x] `SH/SH1-local-agave-yellowstone/40_start_agave_no_geyser.sh`
- [x] `SH/SH1-local-agave-yellowstone/50_write_yellowstone_config.sh`
- [x] `SH/SH1-local-agave-yellowstone/60_start_agave_with_yellowstone.sh`
- [x] `SH/SH1-local-agave-yellowstone/70_smoke_yellowstone_client.sh`

### 4.1 Local OS assumptions

Preferred local environment:

- [ ] Ubuntu 24.04 bare metal, VM, or WSL2 for build rehearsal.
- [ ] Rust installed.
- [ ] LLVM/Clang/build tools installed.
- [ ] `protobuf-compiler`, `pkg-config`, `libssl-dev`, and related build dependencies installed.

### 4.2 Install Agave/Solana tools locally

- [ ] Install current Agave/Solana CLI release.
- [ ] Verify:

```bash
solana --version
agave-validator --version
solana-test-validator --version
```

### 4.3 Build Yellowstone gRPC locally

- [ ] Clone Yellowstone gRPC.
- [ ] Build release artifacts.
- [ ] Locate plugin `.so`.
- [ ] Locate sample config.
- [ ] Run config checker if available.

Example:

```bash
git clone https://github.com/rpcpool/yellowstone-grpc.git
cd yellowstone-grpc
cargo build --release
find target/release -name '*.so'
cargo run --bin config-check -- --config yellowstone-grpc-geyser/config.json
```

### 4.4 Local plugin sanity

- [ ] Confirm the plugin config points to the correct `.so` through `libpath`.
- [ ] Confirm transaction notifications are enabled.
- [ ] Confirm account notifications are either disabled or tightly filtered if not needed.
- [ ] Confirm port, token, and TLS settings are known.

### 4.5 Local client sanity

- [ ] Build or run a Yellowstone client.
- [ ] Subscribe to slots.
- [ ] Subscribe to transactions.
- [ ] Confirm protobuf schema matches Shadow’s normalizer.
- [ ] Confirm raw transaction reconstruction path compiles.

---

## 5. SH2 — Latitude Tokyo host qualification

### 5.0 Provisioning stance

- [x] SH2 may be provisioned now as an acceptance host.
- [x] Treat SH2 as hardware/network qualification, not yet as the production validator.
- [x] Stop after audit plus baseline security posture.
- [x] Do not install Agave during SH2.

Provisioning target:

```text
OS: Ubuntu 24.04
Billing: shortest practical commitment / hourly if available
Access: SSH key only
Public IP: yes
NAT: no, if selectable
Disk mode: raw local NVMe / no managed abstraction if selectable
Name/tag: shadow-solana-tokyo-sh2
```

Avoid managed firewall or DDoS layers that silently filter high-volume UDP unless they can be disabled.

### 5.1 Required host class

Minimum realistic target:

- [x] Ubuntu 24.04 support.
- [x] Dedicated public IP.
- [x] No NAT if possible.
- [ ] 1Gbps symmetric minimum.
- [ ] 10Gbps preferred.
- [ ] Inbound TCP/UDP dynamic Solana P2P range allowed, normally `8000-10000` or a narrower chosen range.
- [x] At least 12 cores / 24 threads for validator-style operation.
- [x] Prefer 16 cores / 32 threads or more for RPC/Geyser.
- [ ] 256GB RAM minimum.
- [ ] 512GB RAM preferred, especially if account indexes are enabled later.
- [ ] NVMe disks, preferably separated by role:
  - [ ] accounts disk
  - [ ] ledger disk
  - [ ] snapshots disk
  - [ ] OS disk

### 5.2 Strong recommended SKU

Recommended first serious Latitude shape:

```text
CPU:      AMD EPYC / Intel Ice Lake or newer, high clock preferred
RAM:      512GB preferred, 256GB minimum
Network:  10Gbps preferred
Disk:     3× NVMe if possible
OS:       Ubuntu 24.04
IP:       dedicated public IP, no NAT
Ports:    inbound TCP/UDP 8000-10000 or selected dynamic range
```

### 5.3 Ask Latitude / provider before renting

- [ ] Can this host run a Solana/Agave non-voting node?
- [ ] Are inbound UDP and TCP ports `8000-10000` allowed?
- [ ] Is there any DDoS/filtering layer that drops high-volume UDP?
- [ ] Is the public IP static for the rental period?
- [ ] Is the host behind NAT?
- [ ] Is 10Gbps actually available and not shared/oversubscribed in a way that breaks P2P?
- [ ] Are NVMe devices local, not network-backed?
- [ ] Can we use a private network between two Latitude hosts in Tokyo if we split node and Shadow?

### 5.4 Host preflight commands

Run before installing the node:

```bash
uname -a
lsb_release -a || cat /etc/os-release
lscpu
free -h
lsblk -o NAME,MODEL,SIZE,TYPE,MOUNTPOINT,FSTYPE
ip -br addr
ip -br link
ethtool <iface>
ss -ulpn | head
```

Network probe:

```bash
iperf3 --version
mtr -rwzc 100 entrypoint.mainnet-beta.solana.com || true
```

Port openness will require external validation from another host.

Reusable repo pack:

- [x] `SH/SH2-latitude-host-audit/README.md`
- [x] `SH/SH2-latitude-host-audit/env.sh.example`
- [x] `SH/SH2-latitude-host-audit/lib.sh`
- [x] `SH/SH2-latitude-host-audit/00_install_audit_utils.sh`
- [x] `SH/SH2-latitude-host-audit/10_collect_base_audit.sh`
- [x] `SH/SH2-latitude-host-audit/20_collect_nic_audit.sh`
- [x] `SH/SH2-latitude-host-audit/30_secure_host_baseline.sh`

Expected audit artifacts:

- [x] `latitude_tokyo_audit_base.txt`
- [x] `latitude_tokyo_audit_nic.txt`

### 5.5 OOB / serial console use

- [ ] Confirm whether the provider OOB console is active.
- [ ] If active, record the console command shape: `ssh <server_id>@<server_ip> -p 2222`.
- [ ] Use OOB for pre-boot inspection, boot-order changes, BIOS entry, and recovery if the network path is broken.
- [ ] Press Enter after boot to reach the Linux login prompt on the serial console.
- [ ] Exit the OOB session with `~.`.

### 5.6 Host qualification exit

- [ ] Host has enough RAM/CPU/NVMe.
- [ ] Network meets minimum requirements.
- [x] Dynamic P2P port range is open.
- [x] No NAT or filtering surprise.
- [x] Decision made: one-host combined node+Shadow or two-host split.

Current first-host SH2 findings:

- The audit pack was run on the first constrained host using `eno1` as the primary public NIC.
- The host exposes an Intel X550 NIC on `ixgbe`, hardware timestamping, `48` combined channels, and one NUMA node.
- The disk shape is workable: mirrored OS/root plus two additional `3.5 TiB` NVMe devices.
- The running validator already demonstrates real gossip traffic and snapshot bootstrap on the same host.
- The current host is below the desired RAM target at roughly `125 GiB`, so treat it as a workable combined-host rehearsal box rather than the ideal long-term mainnet validator shape.
- The default SH2 `ufw` baseline was intentionally not applied after SH1 was already live; SH3-lite now replaces it with an explicit Solana-aware policy that allows OpenSSH plus `8000-8025` TCP/UDP and keeps `8899` / `8900` denied publicly.

---

## 6. SH3 — Non-voting Agave node on Latitude

Reusable repo pack for the first live SH3-lite host:

- [x] `SH/SH3-lite-storage-firewall/README.md`
- [x] `SH/SH3-lite-storage-firewall/SH3_REPORT.md`

### 6.1 Node identity and directories

- [x] Create non-root user, e.g. `sol`.
- [x] Create identity keypair.
- [ ] Create directory structure:

```text
/mnt/accounts
/mnt/ledger
/mnt/snapshots
/home/sol/bin
/home/sol/logs
/home/sol/geyser
```

- [x] Mount NVMe disks with stable `/etc/fstab` entries.
- [ ] Avoid putting accounts and ledger on the same disk when possible.

### 6.2 System tuning

- [ ] Set CPU governor to performance.
- [ ] Set high file descriptor limits.
- [ ] Set appropriate sysctl network buffers.
- [ ] Disable swap or keep it effectively unused.
- [x] Configure time sync.
- [ ] Confirm host clock sanity.
- [x] Configure firewall to allow chosen dynamic port range.
- [x] Keep RPC ports private unless intentionally exposed.

Example checks:

```bash
ulimit -n
chronyc tracking || timedatectl
sysctl net.core.rmem_max net.core.wmem_max
```

### 6.3 Install Agave

- [x] Install current Agave release.
- [x] Verify versions.
- [x] Record exact version in runbook.
- [ ] Keep install script and release channel in artifact notes.

### 6.4 Mainnet non-voting node script

Create a `validator.sh` / `agave-rpc.sh` script with:

- [x] `--identity`
- [x] `--no-voting`
- [x] `--full-rpc-api` only if needed
- [x] `--private-rpc`
- [x] `--ledger`
- [x] `--accounts`
- [x] `--snapshots` if using separate path
- [x] `--dynamic-port-range`
- [ ] mainnet entrypoints
- [ ] expected genesis hash
- [ ] known validators / only-known-rpc if desired
- [ ] `--limit-ledger-size`
- [ ] `--geyser-plugin-config` once SH4 begins

Important: first bring the node up without complex plugin configuration if necessary, then add Geyser after base node stability is proven.

### 6.5 Node service

- [x] Create systemd unit.
- [x] Logs to known path.
- [ ] Restart policy conservative.
- [x] Disk paths validated before start.
- [x] Environment variables captured.

### 6.6 Node runtime checks

- [x] Process starts.
- [x] Ledger initializes.
- [x] Gossip works.
- [x] Catchup progresses.
- [ ] No constant repair storms.
- [ ] No disk saturation.
- [ ] No memory exhaustion.
- [ ] No OOM kills.
- [x] No firewall/NAT warnings.

Current SH3-lite state on the first constrained host:

- The original no-Geyser run was intentionally stopped after reaching about `74%` snapshot progress because it was still writing ledger/accounts to the OS mirror.
- The validator now runs from `/mnt/solana-ledger`, `/mnt/solana-ledger/snapshots`, and `/mnt/solana-accounts`.
- The service is now a persistent `systemd` unit rather than a transient launch.
- `ufw` now allows only OpenSSH plus `8000-8025` TCP/UDP and denies public `8899` / `8900`.
- The 2-hour runtime evidence completed and wrote `~/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt`.
- Final evidence showed no swap, no kernel OOM lines, dynamic Solana ports still bound, RPC kept private on `127.0.0.1`, and replay/root advancement after the NVMe-backed restart.
- This host should now be treated as a completed constrained-host proof box and replaced next session before SH4-lite continues.

Useful commands:

```bash
solana gossip | head
solana catchup <IDENTITY_PUBKEY>
tail -f /home/sol/logs/solana-rpc.log
journalctl -u agave-rpc -f
```

### 6.7 SH3 exit

 - [x] Non-voting node is stable for at least 1 hour.
 - [x] Node joins cluster/gossip.
 - [x] Catchup is healthy or improving.
 - [x] Host resource usage is understood.
 - [x] No Shadow integration yet required.

---

## 7. SH4 — Local Geyser / Yellowstone stream

Current live SH4-lite pack:

- `SH/SH4-QC/README.md`
- `SH/SH4-QC/SH4_REPORT.md`
- `SH/SH4-QC/sh4_wait_until_green.sh`

### 7.1 Plugin build on host

- [x] Build Yellowstone gRPC on the host, or copy a compatible release build.
- [x] Confirm `.so` path.
- [x] Confirm plugin config is readable by validator process.
- [x] Run config checker.

### 7.2 Plugin config

- [x] `libpath` points to the actual plugin `.so`.
- [x] Bind address/port chosen.
- [x] Authentication decision documented for localhost-only scope.
- [x] TLS decision documented.
- [x] Transaction streaming enabled.
- [x] Slot streaming enabled.
- [x] Account streaming disabled or filtered unless needed.
- [x] Prometheus/metrics enabled if available.

### 7.3 Attach plugin to node

- [x] Add `--geyser-plugin-config /home/ubuntu/SH/SH1-local-agave-yellowstone/lab/yellowstone-config.json`.
- [x] Restart node.
- [x] Confirm plugin loads successfully.
- [x] Confirm no plugin panic/abort behavior.
- [x] Confirm plugin does not make the node fall behind.

### 7.4 Stream test

- [x] Client connects locally.
- [x] Slot updates observed.
- [x] Transaction updates observed.
- [ ] Raydium filter works if using account include.
- [ ] Timestamps are captured at adapter boundary.
- [ ] Transactions can be reconstructed into Solana wire format.

### 7.5 SH4 exit

- [x] Local node emits a live Geyser/Yellowstone stream.
- [x] Stream can be consumed on localhost or private LAN.
- [x] Stream survives 30 minutes without disconnect loops.
- [x] Resource overhead is acceptable.

---

## 8. SH5 — Shadow integration with local validator Geyser

**Current SH5-A status:** green / source-proof passed.

What passed:

- SH4 local Yellowstone validator is alive.
- `shadow-state` publishes state, health, and leader schedule.
- `shadow-oracle` publishes tip LUTs.
- `shadow-tier0` reaches `Startup complete`.
- G12 input/report artifacts are emitted.
- `grpc_connected = 1`.
- `grpc_subscribe_sent = 1`.
- `grpc_updates_transaction = 9154`.
- `grpc_normalize_ok = 9154`.
- `grpc_pipeline_drop_unsupported_program = 0`.
- `grpc_feed_enqueue_ok = 9125`.
- `worker_records_seen = 9106`.
- `worker_signal_local_validator_geyser = 9106`.
- `worker_signal_afxdp = 0`.
- `live_landed = 0`.
- `observe_candidates_private_grade = 0`.
- `rollout_private_grade_admissible_opportunities = 0`.

What remained outside SH5-A on the `.55` host rerun:

- `observe_candidates_local_validator_geyser = 0`.
- `rollout_geyser_shadow_opportunities = 0`.

Current diagnostic split from `.55`:

- Yellowstone metrics advanced during the same bounded run (`grpc_bytes_sent`, `grpc_message_sent_count`, slot gauges, and subscribe counters all increased).
- `shadow-tier0` connected its gRPC ingest thread (`grpc-ingest-0: connected (conn_id=1)`).
- the patched SH5 counters now prove `grpc_updates_transaction > 0`, `grpc_normalize_ok > 0`, `grpc_pipeline_drop_unsupported_program = 0`, and `grpc_feed_enqueue_ok > 0`.
- the gated `grpc_source_probe_mode: unsupported_program_noop` path converts unsupported devnet transactions into no-op source-proof records under local-validator-Geyser observe-only rules.
- worker-side source counters are now non-zero (`worker_records_seen = 9106`, `worker_signal_local_validator_geyser = 9106`).

Next boundary:

- SH5-A is complete; do not reopen host, Yellowstone, or producer-path triage for this source-proof gate.
- SH5-B is now closed locally as a controlled supported-fixture software proof via `money_worker::tests::sh5_b_grpc_local_validator_fixture_reaches_observe_candidate_split`.
- stronger host/mainnet local-validator replay remains separate if later evidence needs to be stronger than software proof.
- do not start SH6, SH8, `r1_tiny`, or any live send path from this result.

### 8.1 Config

- [ ] Add local Geyser endpoint to host env.
- [ ] Add source label `local_validator_geyser`.
- [ ] Keep Triton endpoint available for comparator/fallback.
- [ ] Ensure `envsubst` expansion cannot accidentally point at stale config.

Example shape:

```yaml
system:
  proxies:
    ingress:
      grpc_fallback: "${LOCAL_VALIDATOR_GEYSER_ENDPOINT}"
      grpc_fallback_token: "${LOCAL_VALIDATOR_GEYSER_TOKEN}"
      grpc_source_grade: local_validator_geyser
      grpc_source_probe_mode: unsupported_program_noop
      grpc_workers: 1
      shred_capture_workers: 0

rollout:
  mode: observe_only
  no_send_backend: true
  r0_allow_geyser_observe_shadow: true
```

### 8.2 Source tagging

- [x] Local Geyser records are emitted as `SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER`.
- [x] Triton records are emitted as `SIGNAL_SOURCE_EXTERNAL_GEYSER` or equivalent.
- [x] Neither is emitted as `SIGNAL_SOURCE_AFXDP`.
- [x] Gate export includes source-grade counters.

### 8.3 Shadow state path

- [ ] `shadow-state` still hydrates pools/mints/ALTs.
- [ ] Demand ALT hydrator remains active.
- [ ] Hot-pool hydrator remains active.
- [ ] State health remains green.
- [ ] Pool hit/miss diagnostics stay visible.

### 8.4 Observe-only routing policy

- [x] In R0 observe-only/no-send, local Geyser may reach the no-send pipeline if explicitly enabled.
- [ ] In live modes, local Geyser route permissions must be reviewed separately.
- [x] External gRPC cannot become live private execution by default.
- [x] Public execution remains unreachable.

### 8.5 SH5-A run target

SH5-A is the source-grade ingest gate, not the candidate-generation gate.

```text
worker_signal_local_validator_geyser > 0
worker_signal_afxdp = 0
observe_candidates_private_grade = 0
rollout_private_grade_admissible_opportunities = 0
live_landed = 0
```

If `worker_signal_local_validator_geyser=0`, debug the Geyser/client/normalizer/feed-ring path before touching money-worker routing.

### 8.6 SH5-B run target

Candidate-generation proof is a later observe-only gate:

```text
observe_candidates_local_validator_geyser > 0
rollout_geyser_shadow_opportunities > 0
worker_health_drops = 0
live_landed = 0
```

Status:

- software proof is now closed locally via `money_worker::tests::sh5_b_grpc_local_validator_fixture_reaches_observe_candidate_split`
- host/mainnet replay remains an optional strengthening step, not permission for SH6, SH8, `r1_tiny`, or live promotion

---

## 9. SH6 — Triton vs local-node timing comparator

### 9.1 Purpose

Decide whether local node Geyser is better than Triton gRPC and whether further raw AF_XDP work is justified.

### 9.2 Comparator design

For every normalized transaction:

- [ ] signature
- [ ] source grade
- [ ] slot
- [ ] provider timestamp if available
- [ ] adapter receive timestamp
- [ ] decode success/failure
- [ ] route candidate/drop outcome

Join by signature:

```text
same signature:
  local_validator_geyser_arrival_ns
  triton_geyser_arrival_ns
  delta_ns = local - triton
```

Interpretation:

```text
delta_ns < 0  => local node saw it earlier
delta_ns > 0  => Triton saw it earlier
```

### 9.3 Metrics

- [ ] matched signatures count
- [ ] local-only count
- [ ] Triton-only count
- [ ] p50 delta
- [ ] p90 delta
- [ ] p95 delta
- [ ] p99 delta
- [ ] p999 delta if enough samples
- [ ] max delta
- [ ] source disconnect count
- [ ] decode success ratio per source
- [ ] slot lag per source

### 9.4 Acceptance thresholds

Initial practical target:

- [ ] At least 10,000 matched signatures over a test window.
- [ ] Local stream is not materially worse than Triton.
- [ ] Local stream has lower or comparable p99 jitter.
- [ ] Local stream has acceptable disconnect behavior.

Strong local-node win:

- [ ] local p50 earlier than Triton by meaningful margin.
- [ ] local p99 no worse than Triton.
- [ ] local decode success no worse than Triton.

### 9.5 Decision rule

| Result | Decision |
|---|---|
| Local beats Triton clearly | Promote local Geyser as main observe source. |
| Local equals Triton but is stable | Keep local for redundancy and future raw capture. |
| Triton beats local | Keep Triton primary; improve node connectivity/peering before more spend. |
| Local unstable | Split node and Shadow onto separate hosts or reduce plugin load. |
| Local better but noisy | Tune host, disks, cores, and plugin filters. |

---

## 10. SH7 — Continue G12 software proof under Geyser source

### 10.1 Why this exists

G12 software gates should not be blocked by absence of external AF_XDP feed. Use Triton/local Geyser to finish software truth while keeping private-grade proof separate.

### 10.2 Gates to finish

- [x] Final no-send execution evidence:
  - [x] `rollout_would_send > 0`
  - [x] `rollout_live_send_suppressed > 0`
  - [x] `live_landed = 0`
- [x] Micro-market observations:
  - [x] `micro_market_observations > 0`
  - [x] `micro_market_uplift_ok=true` after real comparator logic
- [ ] ShadowGuard ops proof:
  - [x] mainnet program deployed: `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`
  - [x] template program ID matches deployed ID
  - [x] deployer key staged when needed
  - [x] guard pass/abort smoke completed by simulation on configured Raydium V4 vaults
  - [x] current SH7-C blocker cleared on `.57`: `shadow_guard_mainnet_deployed=true`, `shadow_guard_template_program_id_matches=true`, `live_landed=0`, `promote_allowed=false`.
  - [x] new-host continuity handled: `.55` host-local program keypair was not preserved, so `.57` generated and deployed a new program ID. Future hosts must securely preserve or intentionally supersede `/etc/shadow-engine/shadow-guard-program-keypair.json`.
- [x] PM rollout ledger truth:
  - [x] would-send flags counted correctly
  - [x] live-suppressed flags counted correctly
  - [x] source-grade split visible
- [x] G12-R0 long observe-only accumulation:
  - [x] `sample_slots=18401`
  - [x] `observe_candidates=10335`
  - [x] `worker_health_drops=0`
  - [x] `live_landed=0`
  - [x] `promote_allowed=false`
- [ ] Full G12 promotion:
  - [x] private-grade admissibility proof
  - [x] worker latency proof

### 10.3 Promotion rule

Even if G12 software path becomes green under Geyser source, do not claim AF_XDP/private-ingress proof. The report must say:

```text
Software no-send path: green
Micro-market accounting: green/blocked as measured
ShadowGuard proof: green/blocked as measured
AF_XDP private-grade ingress: closed green for SH8 observe-only proof
Full G12 promotion: false until all separate full-threshold gates pass
```

### 10.4 New-host SH7-C continuity checklist

If `.57` is deprovisioned after ShadowGuard ops proof, the repo is enough to continue the code/doc
side, but not enough to preserve host-local upgrade authority by itself.

- [ ] Read `host_run_guide.md` plus current `g12-qc` docs before recreating host state.
- [ ] Pull `sh0-source-grade` at the SH7-C green commit or newer.
- [ ] Reinstall/verify Solana CLI, Rust, cargo, and cargo-build-sbf.
- [ ] Recreate `/etc/shadow-engine` on the new host.
- [ ] Securely restore or regenerate:
  - [ ] `/etc/shadow-engine/shadow-guard-deployer.json`
  - [ ] `/etc/shadow-engine/shadow-guard-program-keypair.json`
- [ ] Keep keypair files out of git and set `root:root` plus mode `600`.
- [ ] If preserving the deployed program ID, verify the program keypair pubkey is `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`.
- [ ] If generating a new program keypair, deploy it, update active host configs, and future evidence to the new program ID.
- [ ] Restore real guard vault IDs:
  - [ ] `HAULecjkcF2GHGSQ566yRBuwRoHxH24YGZs1n6B3QpAG`
  - [ ] `9mo6Dhx8RhrwNqxCBGcfqEZzmGPGr4hz1mfTdW8tpsq7`
- [ ] Run `g1-host-pack/scripts/21_shadowguard_preflight.sh`.
- [ ] Fund or choose a funded deployer path before deploy/upgrade.
- [ ] Deploy, upgrade, or verify the ShadowGuard program on mainnet-beta.
- [ ] Run guard abort/pass smoke and set `SHADOW_GUARD_SMOKE_OK=1` only for the final proof run.
- [ ] Export G12 gate input/report with:
  - [ ] `shadow_guard_mainnet_deployed=true`
  - [ ] `shadow_guard_template_program_id_matches=true`
  - [ ] `live_landed=0`
  - [ ] `promote_allowed=false`

---

## 11. SH8 — Raw AF_XDP shred / TVU capture target

### 11.1 Purpose

After local node is stable, capture the Solana node’s live network packet flow directly and produce Shadow `IngressRecord`s without relying on Geyser serialization latency.

Target path:

```text
Solana P2P / TVU / shred UDP traffic
→ NIC RX queue
→ XDP program
→ AF_XDP socket / UMEM
→ shred/frame parser
→ reassembly / decode
→ Shadow IngressRecord
→ feed ring
→ money worker
```

### 11.2 Precondition

- [x] Local Agave node stable enough for replay-triggered SH8 proof on `.57`.
- [ ] Geyser stream stable.
- [ ] Triton-vs-local timing comparator complete.
- [ ] Host resource usage known.
- [x] Network port/interface for Solana traffic identified.
- [x] Packet capture confirms relevant UDP traffic on interface with XDP detached.

### 11.3 Raw packet visibility

Before AF_XDP:

```bash
sudo tcpdump -ni <iface> -e -vv udp and portrange 8000-10000
ip -s link show <iface>
ethtool -S <iface> | grep -Ei 'rx|drop|err|queue'
```

Pass condition:

- [ ] RX counters move.
- [ ] tcpdump sees Solana P2P/Turbine/repair-like UDP traffic.
- [ ] Traffic remains visible over a real runtime window.

### 11.4 AF_XDP bring-up

- [x] Choose dedicated interface or queue.
- [ ] Confirm NIC/driver supports AF_XDP path.
- [ ] Pin RX queue to ingress core.
- [x] Attach XDP program.
- [x] Register XSK into XSKMAP.
- [ ] Confirm XDP redirect count increases.
- [x] Confirm AF_XDP user process receives frames.
- [ ] Confirm no packet loss under normal Solana node traffic.

### 11.5 Shred/parser work

- [ ] Identify packet format actually visible from local node path.
- [x] Parse UDP/IP/Ethernet headers.
- [x] Classify TVU/shred/repair/gossip traffic.
- [x] Ignore irrelevant traffic.
- [x] Extract slot/shred metadata where available.
- [x] Reassemble enough payload to produce transaction candidates.
- [ ] Preserve RX timestamp.
- [ ] Emit raw archive records for replay/debug.

### 11.6 Shadow integration

- [x] AF_XDP producer emits fixed-width `IngressRecord`.
- [x] `signal_source = SIGNAL_SOURCE_AFXDP` only for actual AF_XDP packets.
- [x] Feed-ring output shape matches existing money worker expectations.
- [ ] ALT/mint/pool resolution paths unchanged.
- [ ] State freshness gates unchanged.
- [ ] Public execution remains unreachable.

Current `.57` SH8 closure truth:

```text
artifact: /home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak
shred_capture_workers: 32
afxdp_frames_seen=8577897
afxdp_udp_8000_8025_seen=8577897
afxdp_shred_parse_data_ok=6406417
afxdp_tx_extract_ok=286568
afxdp_tx_record_emit_ok=15
afxdp_feed_enqueue_ok=15
worker_signal_afxdp=66
observe_candidates_private_grade=15
rollout_private_grade_admissible_opportunities=5
live_landed=0
worker_health_drops=0
latency_ok=true
worker_latency_proof_present=true
```

### 11.7 AF_XDP acceptance

- [x] `afxdp_frames_seen > 0`
- [x] `afxdp_udp_8000_8025_seen > 0`
- [x] `afxdp_shred_parse_ok > 0`
- [x] `afxdp_shred_parse_data_ok > 0`
- [x] `afxdp_shred_parse_coding_ok > 0`
- [x] `afxdp_reassembly_insert_ok > 0`
- [x] `afxdp_reassembly_complete_entries > 0`
- [x] `afxdp_data_shred_payload_ok > 0`
- [x] `afxdp_source_probe_emit_ok > 0`
- [x] `afxdp_tx_extract_ok > 0`
- [x] `afxdp_tx_record_emit_ok > 0`
- [x] `afxdp_feed_enqueue_ok > 0`
- [x] `worker_signal_afxdp > 0`
- [x] `observe_candidates_private_grade > 0`
- [x] `worker_health_drops = 0`
- [x] `worker_pool_state_hit > 0`
- [x] `rollout_private_grade_admissible_opportunities > 0` if route policy allows.
- [x] SH8-C longer soak confirms the transaction-derived AF_XDP path stays green.
- [ ] Same-signature AF_XDP vs local Geyser timing comparator exists; this is later benchmarking, not SH8 closure.
- [ ] AF_XDP p99 and p999 are better than or equal to local Geyser path; this is later benchmarking, not SH8 closure.
- [ ] Loss rate is characterized for production sizing; this is later strengthening, not SH8 closure.

---

## 12. SH9 — DPDK/SmartNIC optional escalation

DPDK is not a prerequisite for the self-sourced node path. It becomes relevant only after AF_XDP is working and measured.

### 12.1 Evaluate DPDK only if

- [ ] AF_XDP p999 jitter is too high.
- [ ] AF_XDP loss appears under bursts.
- [ ] XSKMAP/driver behavior is unstable.
- [ ] Multi-queue scaling is blocked.
- [ ] DPDK can emit the same `IngressRecord` shape without redesigning the worker.

### 12.2 DPDK guardrails

- [ ] DPDK remains ingress-only.
- [ ] It does not alter route legality.
- [ ] It does not change PM truth.
- [ ] It does not reopen public execution.
- [ ] AF_XDP remains rollback path.

---

## 13. One-host vs two-host deployment

### 13.1 One-host combined mode

```text
Latitude host:
  Agave node
  Yellowstone plugin
  Shadow state/tier0/PM/oracle
```

Pros:

- [ ] simpler rental
- [ ] localhost stream
- [ ] no private LAN dependency

Cons:

- [ ] Agave node is noisy
- [ ] disk/network/replay can disturb Shadow latency
- [ ] harder CPU/IRQ isolation
- [ ] more failure coupling

### 13.2 Two-host split mode

```text
Host A:
  Agave node + Geyser
Host B:
  Shadow Engine
```

Pros:

- [ ] cleaner isolation
- [ ] easier performance attribution
- [ ] Shadow cores not disturbed by AccountsDB/replay

Cons:

- [ ] more cost
- [ ] private LAN required
- [ ] additional network hop
- [ ] more deployment complexity

### 13.3 Decision rule

Start one-host only for lab/proof if host is large enough. Move to two-host split if:

- [ ] Shadow p99 regresses.
- [ ] Agave replay causes CPU spikes.
- [ ] disk IO saturates.
- [ ] Geyser plugin load makes node fall behind.
- [ ] AF_XDP capture competes with Agave networking.

---

## 14. Failure-mode matrix

| Symptom | Likely cause | First action |
|---|---|---|
| Node cannot join gossip | blocked ports / NAT / bad entrypoints | Check firewall, dynamic port range, public IP. |
| Node falls behind constantly | undersized host / disk IO / network | Check CPU steal, iowait, ledger/accounts disks. |
| Geyser plugin crashes node | bad plugin config / plugin overload | Validate config, reduce filters, inspect plugin logs. |
| Local stream has slots but no transactions | filter too narrow or transaction notifications disabled | Check Yellowstone subscription and plugin config. |
| Shadow sees Geyser but no candidates | normalizer/decode/filter issue | Inspect decode drops and source tags. |
| Local Geyser slower than Triton | node propagation position worse than provider | Keep Triton primary; tune node/peering later. |
| tcpdump sees no Solana UDP on node host | wrong interface/filter or node not connected | Check dynamic ports and Agave logs. |
| AF_XDP receives packets but no records | parser/reassembly/decode gap | Add producer-side packet/shred counters. |
| AF_XDP p99 worse than Geyser | batching/queue/core tuning wrong | Tune queues, IRQs, cores, UMEM, XDP mode. |

---

## 15. Minimum acceptance bundle per milestone

### SH1 local lab bundle

- [ ] tool versions
- [ ] build logs
- [ ] Yellowstone config-check output
- [ ] local client stream proof

### SH2 host qualification bundle

- [ ] `lscpu`
- [ ] `free -h`
- [ ] `lsblk`
- [ ] NIC speed and link
- [ ] port-open proof
- [ ] disk mount plan

### SH3 Agave node bundle

- [ ] Agave version
- [ ] node script
- [ ] systemd unit
- [ ] log excerpt
- [ ] gossip proof
- [ ] catchup proof
- [ ] resource snapshot

### SH4 Geyser bundle

- [x] plugin commit/version
- [x] config JSON/JSON5
- [ ] plugin load log
- [ ] slot stream proof
- [ ] transaction stream proof

### SH5 Shadow bundle

- [ ] expanded config
- [ ] source-grade counters
- [ ] short R0 run report
- [ ] decode/drop diagnostics

### SH6 benchmark bundle

- [ ] matched signature count
- [ ] delta histogram
- [ ] p50/p95/p99/p999
- [ ] local-only/Triton-only counts
- [ ] recommendation

### SH8 AF_XDP bundle

- [x] XDP attach proof
- [x] XSKMAP registration proof
- [x] packet RX counters
- [x] AF_XDP frame counters
- [x] parser/reassembly counters
- [x] Shadow `worker_signal_afxdp` proof
- [ ] timing comparator vs local Geyser

---

## 16. Security and operational notes

- [ ] Do not expose RPC publicly unless explicitly intended.
- [ ] Use `--private-rpc` for internal node use.
- [ ] Restrict SSH to known IPs.
- [ ] Keep keys off shared paths.
- [ ] Non-voting node does not need vote account.
- [ ] Do not fund vote fees unless intentionally becoming a voting validator.
- [ ] Keep validator identity backed up.
- [ ] Keep plugin auth token secret.
- [ ] Keep Shadow execution keys separate from node identity keys.
- [ ] Record every host run in a handoff note.

---

## 17. References / external anchors

These are external docs to re-check before the real Latitude build because version details change:

- Anza — Agave Validator Requirements: https://docs.anza.xyz/operations/requirements
- Anza — Setup an Agave RPC Node: https://docs.anza.xyz/operations/setup-an-rpc-node
- Anza — Solana Validator Geyser Plugins: https://docs.anza.xyz/validator/geyser
- Triton One / rpcpool — Yellowstone gRPC: https://github.com/rpcpool/yellowstone-grpc

---

## 18. Current recommended next actions

1. [x] Keep Triton gRPC as the short-term ingest path for G12 software progress.
2. [x] SH0 complete: truthful source-grade counters landed, and AF_XDP is the only private-grade source in PM rollout evidence.
3. [ ] Build Yellowstone locally and validate plugin/client mechanics.
4. [ ] Rent one strong Latitude Tokyo host only for SH2/SH3 qualification, after SH1 toolchain rehearsal is complete.
5. [ ] Run a non-voting Agave node; do not attempt voting.
6. [ ] Add Yellowstone Geyser to the node.
7. [ ] Feed Shadow from local Geyser.
8. [ ] Benchmark local Geyser vs Triton by signature.
9. [ ] Only after local node/Geyser is stable, start SH8 raw AF_XDP shred capture.
