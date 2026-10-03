# SH Runbook

This is the persistent procedural guide for the SolanaHost track. Update it as each SH milestone produces real evidence or exposes a new failure mode so the next host starts from a known-good playbook instead of from memory.

For first bootstrap on any newly provisioned host, start with `host_run_guide.md` in the repo root, then use this runbook for SH-specific detail and milestone ordering.

## Core rules

- `external_geyser` is non-private.
- `local_validator_geyser` is non-private.
- `afxdp` is private-grade only when it is real AF_XDP packet ingress.
- Do not relabel local validator Geyser as private-grade just because it is local.
- Do SH milestones in order; do not skip ahead into AF_XDP, live send, or promotion work.

## Repository layout

- `SH\SH0-QC` - flattened QC pack for SH0 source-grade plumbing.
- `SH\SH1-local-agave-yellowstone` - SH1 lab pack for Agave + Yellowstone build and runtime rehearsal.
- `SH\SH2-latitude-host-audit` - SH2 host acceptance pack for fresh Latitude Tokyo hosts.
- `SH\SH3-lite-storage-firewall` - SH3-lite pack for NVMe storage correction, Solana-aware firewall hardening, and runtime evidence.
- `SH\SH4-QC` - SH4-lite QC pack for Yellowstone smoke status, evidence paths, and the listener-driven gate script.
- `SH\SH8-QC` - SH8-A AF_XDP bringup status for self-sourced Agave packet capture.
- `host_run_guide.md` - root-level new-host bootstrap guide covering dependencies, config generation, deployment paths, and common drift traps.
- `SH\SH_RUNBOOK.md` - this cumulative runbook.

## SH0 checkpoint before any new host

Before using any new host for validator work, confirm the repo still preserves source truth:

1. `grpc_source_grade` exists and defaults to `external_geyser`.
2. `external_geyser` and `local_validator_geyser` remain non-private in routing and reporting.
3. `signal_source` is exported into rollout/G12 artifacts.
4. Only AF_XDP may count as private-grade evidence.
5. The active host/bootstrap procedure still points at `host_run_guide.md`, not chat memory.

## Current continuation note

- SH8 observe-only AF_XDP private-grade proof remains green on `.57`.
- The current post-SH8 / post-R1-FIX-B blocker is **not** host provisioning, Agave bootstrap, ShadowGuard, AF_XDP ingress, or R1 admission.
- The latest `.57` R1-JITO-JSON no-send proof is green: AF_XDP/R1 reached route/actionable, R1 auto-admission, `process_intent`, no-send backend, final no-send publication, private-grade would-send evidence, and micro-market observation.
- Jito gRPC live is blocked on provider access, and Harmonic regional endpoints fail channel transport connect.
- Operator pivot 2026-05-06: private-lane-only routing is no longer the next live path. **R1-PUBLIC-A local foundation is now implemented**: explicit public RPC scout config/validation, public RPC egress wiring, and one-landed stop/reconcile behavior. The next practical live gate is the host trial under `0.05 SOL` input cap and `0.1 SOL` entity cap.
- R1-PUBLIC-B is direct TPU/QUIC public send with the same caps for landing/latency comparison. R1-PUBLIC-C is decoder coverage expansion across Raydium CPMM/CLMM, Orca, Meteora, and PumpSwap.
- Public execution must not be a hidden fallback. It must preserve AF_XDP edge, O(1) worker path, automatic admission, no manual wallet picking, ShadowGuard/bounds logic where applicable, inventory truth, fee-leak accounting, PM reconciliation, kill switches, and rung-by-rung scaling.
- Use `g12-qc\G12_REPORT.md` and `g12-qc\G12-QC.md` for the current R1/G12 blocker and QC artifact list.

## New host bootstrap procedure

### 1. Collect host facts immediately

Record:

- hostname
- distro and kernel
- CPU / RAM / disk shape
- whether `sudo -n true` works
- whether Rust, Cargo, CMake, pkg-config, clang, and `protoc` already exist

### 2. Use OOB first if the host is not yet trustworthy

If the provider exposes an OOB serial console, use it for first-boot and recovery work before relying on the network path.

Typical session shape:

```text
ssh <server_id>@<server_ip> -p 2222
[SOL Session operational. Use ~? for help]
```

Useful OOB capabilities:

- before boot: inspect hardware setup, change boot order
- during reboot: enter BIOS with `DEL` or `ESC`, view POST and GRUB
- after boot: press Enter to reach the Linux login prompt on `ttyS0`
- end the session with `~.`

### 3. Sync the SH2 audit pack first

Copy the entire `SH\SH2-latitude-host-audit` folder to the remote host before any Agave install.

Expected remote root:

```bash
/home/ubuntu/SH/SH2-latitude-host-audit
```

### 4. Run SH2 audit before any validator install

From the remote SH2 folder:

```bash
bash ./00_install_audit_utils.sh
bash ./10_collect_base_audit.sh
ip -br link
bash ./20_collect_nic_audit.sh <primary_public_nic>
bash ./30_secure_host_baseline.sh
```

Keep these artifacts:

- `~/shadow-host-audit/latitude_tokyo_audit_base.txt`
- `~/shadow-host-audit/latitude_tokyo_audit_nic.txt`

Stop after audit and baseline security. Do not install Agave yet.

### 5. If SH2 passes, sync the SH1 lab pack

Copy the entire `SH\SH1-local-agave-yellowstone` folder to the remote host, preserving LF shell scripts.

Expected remote root:

```bash
/home/ubuntu/SH/SH1-local-agave-yellowstone
```

### 6. Install build dependencies

From the remote SH1 folder:

```bash
bash ./00_install_build_deps.sh
```

Current known required packages include:

- `build-essential`
- `clang`
- `libclang-18-dev`
- `llvm`
- `cmake`
- `pkg-config`
- `libssl-dev`
- `protobuf-compiler`
- `git`
- `curl`
- `python3`
- `ca-certificates`

### 7. Ensure Rust is present

If `cargo` or `rustc` are missing, install rustup first and load `~/.cargo/env` before running the build scripts.

### 8. Build Agave

```bash
bash ./10_build_agave.sh
```

Evidence to keep:

- `agave-validator --version`
- `solana --version`
- `solana-test-validator --version`

### 9. Build Yellowstone

```bash
bash ./20_build_yellowstone.sh
```

Evidence to keep:

- plugin `.so` path
- successful `config-check`

Current expected plugin path:

```bash
~/SH/SH1-local-agave-yellowstone/lab/yellowstone-grpc/target/release/libyellowstone_grpc_geyser.so
```

### 10. Prepare identity and directories

```bash
bash ./30_prepare_identity_and_dirs.sh
```

Expected outputs:

- validator keypair
- `lab/ledger`
- `lab/accounts`
- `lab/logs`

### 11. Apply known host tuning before launch

Current known validator sysctls:

```bash
net.core.rmem_max = 134217728
net.core.wmem_max = 134217728
vm.max_map_count = 1000000
fs.nr_open = 1000000
```

Keep these in a dedicated file such as:

```bash
/etc/sysctl.d/21-agave-validator.conf
```

Then apply them with `sysctl`.

### 12. Launch the no-Geyser validator under systemd

Do not rely on a plain SSH shell for final launch because the default `nofile` limit may be too low.

Recommended shape:

```bash
systemd-run \
  --unit sh1-agave-no-geyser \
  --same-dir \
  --collect \
  -p WorkingDirectory=/home/ubuntu/SH/SH1-local-agave-yellowstone \
  -p LimitNOFILE=1000000 \
  -p LimitMEMLOCK=infinity \
  /bin/bash -lc 'source ~/.cargo/env && cd /home/ubuntu/SH/SH1-local-agave-yellowstone && bash ./40_start_agave_no_geyser.sh'
```

Expected validator shape:

- `--no-voting`
- `--private-rpc`
- `--rpc-bind-address 127.0.0.1`
- `--dynamic-port-range 8000-8025`
- devnet or testnet entrypoints
- expected genesis hash

For mainnet SH8-A service units, include:

```ini
[Service]
LimitNOFILE=1000000
LimitMEMLOCK=infinity
```

`LimitMEMLOCK` must be `infinity`; `.57` crash-looped with `unable to set memory lock limit` until this was added.

### SH8-A current-host AF_XDP notes

The SH8-A source model is self-sourced:

```text
Agave mainnet non-voting node
-> Solana UDP / Turbine / TVU on the host NIC
-> AF_XDP on that NIC
-> Shadow `SIGNAL_SOURCE_AFXDP`
```

Latitude does not provide the Solana feed. The feed source is our own Agave process.

Current `.57` findings:

- `eno2` has no carrier; ignore it until the provider wires it.
- `eno1` receives Solana UDP and can be used only as a constrained proof NIC with explicit rollback.
- Native XDP disrupted `eno1`; use `xdpgeneric` on the management NIC.
- Always arm `/home/ubuntu/rollback_xdp_eno1.sh` before attaching XDP to `eno1`.
- Pin `/sys/fs/bpf/shadow_xsks_map` after attaching the XDP program; otherwise AF_XDP workers bind but cannot receive redirects.
- Use the filtered XDP object from `g1-host-pack\xdp\shadow_rx_meta.bpf.c`, which redirects only destination UDP ports `8000-8025`, requires raw mainnet shred shape/version, and passes management/RPC traffic.
- Keep `grpc_workers=1`; current topology validation rejects `grpc_workers=0`.
- Keep `vm.nr_hugepages=4096` and remove stale `/dev/hugepages/shadow_umem_*` before retries.
- The old root-cause was storage: `/mnt/solana-ledger` and `/mnt/solana-accounts` must be mounted on the dedicated `3.5 TiB` NVMes, not left as rootfs directories on `/dev/md0`.
- Use `/home/ubuntu/sh8a_wait_ready_then_run.sh` to watch for replay/RPC readiness and trigger the AF_XDP proof immediately; stop doing manual polling once this watcher exists.
- On `.57`, `shred_capture_workers` must be `32`, not `48`; the valid AF_XDP queue range is `0-31`.
- A stale host binary shows up immediately as `afxdp_* = None` in `g12_gate_input.json` / `g12_gate_report.json`. Rebuild and redeploy before trusting any SH8 run that lacks those counters.
- Agave bootstrap is no longer the blocker on `.57`. The current first-green SH8-A source-proof run proved:
  - `artifact=/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_235006-af_xdp-32q-sh8-probe`
  - `afxdp_frames_seen=327`
  - `afxdp_udp_8000_8025_seen=327`
  - `afxdp_shred_parse_ok=8`
  - `afxdp_source_probe_emit_attempt=8`
  - `afxdp_source_probe_emit_ok=8`
  - `worker_signal_afxdp=2`
  - `worker_source_probe_afxdp_parsed_shred_drops=2`
- The SH8-B stage-counter rerun proved the next stop was not ingress or reassembly but tx extraction:
  - `artifact=/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_001337-af_xdp-32q-sh8b-counters`
  - `host_commit=1cfa2f7`
  - `afxdp_reassembly_insert_ok=7`
  - `afxdp_reassembly_complete_entries=5`
  - `afxdp_data_shred_payload_ok=5`
  - `afxdp_tx_extract_attempt=5`
  - `afxdp_tx_extract_ok=0`
  - `afxdp_tx_extract_err=5`
  - `worker_signal_afxdp=3`
  - `live_landed=0`
- If `afxdp_tx_extract_attempt > 0` and `afxdp_tx_extract_ok = 0`, debug payload/extractor assumptions first; do not go back to Agave bootstrap, XDP attach mode, or queue-count triage.

Current replacement-host `.221` findings (`259S001355`, Intel `i40e`):

- Rescue-mode inspection of the mounted failed root confirmed that `eno1` is not safe for XDP/AF_XDP on this host.
- The 2026-05-06 observe-only run attached `xdpgeneric` on `eno1` at `17:31:49 UTC`; the mounted kernel journal began logging repeated `i40e 0000:05:00.0` `AMD-Vi: IO_PAGE_FAULT` and `AMD-Vi: IOMMU Event log restarting` messages at `17:31:51 UTC`.
- The observe window itself still completed and produced non-zero gate evidence, so the failure was not a simple Tier0/app crash; the NIC path degraded under DMA/IOMMU faults until host reachability was eventually lost.
- Operational rule for `.221`: keep `eno1` as management / normal host / Agave traffic only. Do **not** attach XDP or AF_XDP to `eno1` again on this machine.
- Any future AF_XDP attempt on `.221` belongs on `eno2` only, and only if the provider wires carrier and the interface has a real usable traffic path.
- The corrected shred parser found that legacy DATA/CODE classification was reversed in our parser (`0xa5` is DATA, `0x5a` is CODE). Artifact `/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_004644-af_xdp-32q-sh8b-txdiag` proved the parser stopped fabricating DATA payloads from coding shreds.
- SH8-B first transaction-derived green is closed. The first-green artifact is `/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_031546-af_xdp-32q-sh8b-txdiag` from host commit `ebf0ee8`, with:
  - `afxdp_frames_seen=1493713`
  - `afxdp_shred_parse_ok=1493710`
  - `afxdp_reassembly_complete_entries=316431`
  - `afxdp_data_shred_payload_ok=293519`
  - `afxdp_tx_extract_ok=75814`
  - `afxdp_tx_record_emit_ok=2`
  - `afxdp_feed_enqueue_ok=2`
  - `worker_signal_afxdp=47`
  - `observe_candidates_private_grade=3`
  - `rollout_private_grade_admissible_opportunities=1`
  - `live_landed=0`
  - `worker_health_drops=0`
- The final SH8-B fix was not Geyser or live behavior. It combined destination-only/raw-shred XDP filtering, stable Agave disk-index startup on the constrained `.57` host, and stream extraction across DATA shred payload boundaries.
- SH8-C final longer soak is also green. The closure artifact is `/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak`, with:
  - `afxdp_frames_seen=8577897`
  - `afxdp_udp_8000_8025_seen=8577897`
  - `afxdp_shred_parse_data_ok=6406417`
  - `afxdp_tx_extract_ok=286568`
  - `afxdp_tx_record_emit_ok=15`
  - `afxdp_feed_enqueue_ok=15`
  - `worker_signal_afxdp=66`
  - `observe_candidates_private_grade=15`
  - `rollout_private_grade_admissible_opportunities=5`
  - `live_landed=0`
  - `worker_health_drops=0`
- SH8-A, SH8-B, and SH8-C are closed for observe-only AF_XDP private-grade proof. Broader G12 promotion is still governed by separate full-threshold gates; do not reopen XDP attach, 48-queue attempts, Agave bootstrap, Geyser relabeling, SH6, native XDP, or live send for this slice.

### 13. Watch bootstrap instead of guessing

Primary checks:

```bash
systemctl is-active sh1-agave-no-geyser.service
systemctl show -p MainPID,ActiveEnterTimestamp sh1-agave-no-geyser.service
tail -n 100 ~/SH/SH1-local-agave-yellowstone/lab/logs/agave-devnet-no-geyser.log
```

Healthy early signs:

- peer and RPC discovery
- known-validator selection
- `genesis.tar.bz2` download
- snapshot download
- root slot / catchup progress

### 13.5 If the validator is still writing to the OS mirror, stop and correct storage

On the current first host, the first no-Geyser run proved bootstrap but was still writing ledger and accounts under the SH1 lab root on `/dev/md0`. SH3-lite corrects that before the rehearsal is allowed to continue.

Required target layout:

```text
/mnt/solana-ledger
/mnt/solana-ledger/snapshots
/mnt/solana-accounts
```

Rules:

- do not leave ledger, snapshots, or accounts on the OS mirror once the data NVMe devices are available
- keep RPC bound to localhost
- keep the current working entrypoints/genesis/identity flags unchanged while fixing only storage and hardening
- use a Solana-aware firewall instead of the untouched SH2 fresh-host baseline if the host is already carrying validator traffic

Current first-host artifact pack:

- `SH\SH3-lite-storage-firewall\README.md`
- `SH\SH3-lite-storage-firewall\SH3_REPORT.md`

### 14. Only after stable no-Geyser proof, attach Yellowstone

```bash
bash ./50_write_yellowstone_config.sh
```

Then launch:

```bash
systemd-run \
  --unit sh1-agave-yellowstone \
  --same-dir \
  --collect \
  -p WorkingDirectory=/home/ubuntu/SH/SH1-local-agave-yellowstone \
  -p LimitNOFILE=1000000 \
  -p LimitMEMLOCK=2000000000 \
  /bin/bash -lc 'source ~/.cargo/env && cd /home/ubuntu/SH/SH1-local-agave-yellowstone && bash ./60_start_agave_with_yellowstone.sh'
```

### 15. Smoke the local Yellowstone endpoint

```bash
bash ./70_smoke_yellowstone_client.sh
```

Acceptance evidence:

- health check succeeds
- version query succeeds
- slot stream observed
- transaction stream observed

## Known pitfalls discovered so far

- `libclang-18-dev` matters on Ubuntu 24.04; runtime LLVM libraries alone were insufficient.
- Agave rejected `8000-8020`; use at least `8000-8025`.
- A plain SSH session started with `ulimit -n 1024`, which was too low.
- `systemd-run` was the clean fix for validator resource limits.
- `lib.sh` needed a nameref fix so cluster args were actually appended.
- Bootstrap can take a while before localhost RPC starts serving during snapshot download.
- On `.57`, AF_XDP queue count must match the host's valid queue range: `48` workers produced repeated `Invalid argument` binds, while `32` workers bound cleanly.
- A host-side repo whose `origin` points to a local bundle must be refreshed from that bundle rather than fetched from GitHub directly.
- Host `build.sh` may need an explicit PATH including `~/.cargo/bin` and the Solana install path; otherwise it fails with `cargo: command not found`.
- SH8 runs can still be using an old binary even after code is committed locally; trust the host artifact only if the raw AF_XDP counters are present in the gate JSON.
- A local Jito or Harmonic keypair file is not the same as provider authorization. Jito currently rejects `3i9N1VhsmKJBXB4STP4xdinmAXYCRecmNHBbLkSzwCW4`; Harmonic auth pubkey `9ZaQnhU56rxrw3mkGrVHaebTFXF1FgqAdvanTPUJMPwa` is staged, but configured regional endpoints fail transport connect.
- R1-JITO-JSON no-send is green. If public send is eventually used due to JSON-RPC tiny-live failure, keep it in a separately named public-tiny mode. Do not make public send a hidden fallback inside the private-lane Guerrila configuration.

## Evidence to capture for every host

- host facts
- OOB access pattern and any BIOS / boot-order findings
- base audit file
- NIC audit file
- toolchain versions
- Agave and Solana versions
- Yellowstone plugin path
- systemd unit name and active timestamp
- validator log path
- no-Geyser bootstrap proof
- Yellowstone smoke proof
- SH8 watcher log and trigger artifact path
- AF_XDP bind counts: `xskmap_registered`, `xsk_bound`, `bind_invalid_arg`
- AF_XDP raw counters from `g12_gate_input.json` / `g12_gate_report.json`
- any host-specific deviations or extra tuning

## Current first-host status

On the first constrained host, SH2 has already proven:

- the SH2 audit pack runs successfully on the host
- the primary public NIC is `eno1`
- the NIC is an Intel X550 on `ixgbe`
- hardware TX/RX timestamping is available
- the NIC exposes `48` combined channels
- the host has one NUMA node and direct public routing on a `/31`
- the storage shape is mirrored OS RAID plus two additional `3.5 TiB` NVMe devices
- the host is only `125 GiB` RAM, which is below the original `256 GiB` target

Important caveat on this shared host:

- `30_secure_host_baseline.sh` was intentionally not run after SH1 was already live, because the pack's fresh-host `ufw` policy would have broken current Solana P2P traffic. SH3-lite now replaces that skipped generic baseline with an explicit Solana-aware `ufw` policy for `8000-8025` plus OpenSSH while keeping RPC non-public.

On the first constrained host, SH1 reached:

- Agave built
- Yellowstone built
- validator identity created
- sysctls applied
- no-Geyser validator moved off the OS mirror and onto `/mnt/solana-ledger`, `/mnt/solana-ledger/snapshots`, and `/mnt/solana-accounts`
- no-Geyser validator running from a persistent `systemd` unit
- devnet genesis and snapshot bootstrap in progress from the corrected NVMe paths
- the first no-Geyser run proved more than 30 minutes of stable runtime and reached about `74%` snapshot progress before the storage migration stop
- the restarted NVMe-backed run is again downloading the snapshot at about `18 MB/s`
- about `122 GiB` remained available on the host with no swap in use after the restart
- dynamic Solana sockets bound across the expected `8000+` range
- the 2-hour no-Geyser runtime evidence completed and wrote `~/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt`
- the final evidence showed no swap, no kernel OOM lines, private RPC on `127.0.0.1`, dynamic port binds intact, and replay/root advancement under the NVMe-backed layout

The current SH3-lite report for this host lives in:

- `SH\SH3-lite-storage-firewall\SH3_REPORT.md`

Next-session disposition for this host:

- treat the first constrained host as finished constrained-host evidence, not as the continuing SH4 host
- shut it down before the next session
- reprovision a replacement host in the intended `256/384 GB` class

Resume sequence on the replacement host:

1. run `SH\SH2-latitude-host-audit`
2. sync `SH\SH1-local-agave-yellowstone`
3. replay the SH3-lite storage/firewall correction captured in `SH\SH3-lite-storage-firewall`
4. repeat the no-Geyser runtime proof
5. only then move to Yellowstone smoke

This is specifically to avoid starting from scratch: the audit pack, SH1 build pack, SH3-lite report, and checklist state are already the reusable handoff.

## Current replacement-host SH4 status

The current live SH4-lite continuation host has:

- the same constrained `125 GiB` RAM profile as the earlier host
- replayed back through SH2 audit plus SH3-lite substrate before Yellowstone work resumed

Current `.55` state:

- Agave pinned to `v3.1.14`
- Yellowstone pinned to `v12.3.0+solana.3.1.14`
- Yellowstone `config-check` passed
- `sh4-agave-yellowstone-smoke.service` is the active SH4-lite unit
- the host-side listener-driven gate script is `~/yellowstone/sh4_wait_until_green.sh`

Repo-side SH4-lite artifact pack:

- `SH\SH4-QC\README.md`
- `SH\SH4-QC\SH4_REPORT.md`
- `SH\SH4-QC\sh4_wait_until_green.sh`

Important current interpretation:

- SH4-lite is closed / green on `.55` as a constrained-host Yellowstone smoke proof
- the final artifact is `~/shadow-host-audit/sh4-yellowstone-smoke/yellowstone_smoke_final.txt`
- the archived repo copy is under `SH\SH4-QC\evidence\2026-04-28-sh4-green`
- next step is SH5-lite local-validator-Geyser Shadow ingest

## Current replacement-host SH5 status

Current `.55` SH5-A state:

- SH5-A is green / source-proof passed
- SH4 Yellowstone remains up and serving localhost gRPC/metrics
- `shadow-state` is publishing state/health/leader schedule
- `shadow-oracle` is publishing tip LUT segments
- `shadow-tier0` reaches `Startup complete` and emits G12 gate artifacts
- `grpc_connected = 1`
- `grpc_subscribe_sent = 1`
- `grpc_updates_transaction = 9154`
- `grpc_normalize_ok = 9154`
- `grpc_feed_enqueue_ok = 9125`
- `worker_records_seen = 9106`
- `worker_signal_local_validator_geyser = 9106`
- `worker_signal_afxdp = 0`
- `live_landed = 0`
- `observe_candidates_private_grade = 0`
- `rollout_private_grade_admissible_opportunities = 0`
- archived repo evidence lives under `SH\SH5-QC\evidence\2026-04-28-sh5a-source-probe-green`

Current interpretation:

- SH5-A proved that local Yellowstone / `local_validator_geyser` records can reach the Shadow money-worker source-counter path on the constrained Latitude Tokyo host
- the source-probe mode remains observe-only and no-op, so it does not create candidates, private-grade evidence, or live-send permission
- `observe_candidates_local_validator_geyser = 0` and `rollout_geyser_shadow_opportunities = 0` are good zeroes in the host run because SH5-A stayed separate from candidate proof
- SH5-B is now closed locally as a controlled supported-fixture software proof through `money_worker::tests::sh5_b_grpc_local_validator_fixture_reaches_observe_candidate_split`
- stronger host/mainnet replay remains a later strengthening step, and this still does not authorize SH6, SH8, `r1_tiny`, or live promotion

## Current SH7-C / ShadowGuard host state

SH7-C is green on `.57`, but future host moves still require recreating or restoring host-local ops
state. The repo contains the code, scripts, docs, and green evidence, but it intentionally does not
contain private keypair JSON files or host `/etc/shadow-engine` config.

Current `.57` SH7-C state:

- SH7-A final no-send proof is green.
- SH7-B micro-market proof is green.
- SH7-C is green for ShadowGuard ops truth.
- Solana CLI, Rust, cargo, cargo-build-sbf, Node.js, and npm were installed on `.57`.
- `/etc/shadow-engine/shadow-guard-deployer.json` exists on `.57` as `root:root` mode `600`.
- `/etc/shadow-engine/shadow-guard-program-keypair.json` exists on `.57` as `root:root` mode `600`.
- deployer pubkey: `4iYqgdyuU8YE6FkZRq3195SZU7m5E5MkaNeaHayfoiZJ`.
- deployed program ID: `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`.
- deploy signature: `39ZxpfbxLk4eyrWT6DaSSBwWiVx3HCg9dCkyJLTf5bsn74haZ6vs6KbWTWMM4zHAERLa7J57YcckoZDgtFrnFFiX`.
- configured guard vaults:
  - `HAULecjkcF2GHGSQ566yRBuwRoHxH24YGZs1n6B3QpAG`
  - `9mo6Dhx8RhrwNqxCBGcfqEZzmGPGr4hz1mfTdW8tpsq7`
- template proof was green.
- no-send simulation smoke proved pass and abort behavior on the configured vault layout.
- deployer balance after deploy was `0.31427784 SOL`.
- `shadow_guard_mainnet_deployed=true`, `shadow_guard_template_program_id_matches=true`, `live_landed=0`, and `promote_allowed=false`.
- G12-R0 long observe-only accumulation on `.57` reached `sample_slots=18401` and
  `observe_candidates=10335` with `rollout_would_send=3743`,
  `rollout_live_send_suppressed=3743`, `micro_market_observations=3743`,
  `worker_health_drops=0`, and `live_landed=0`.
- That specific G12-R0 accumulation artifact kept full G12 promotion false because it was Geyser-only:
  `private_lane_admissibility_ok=false`, `rollout_private_grade_admissible_opportunities=0`,
  `worker_latency_proof_present=false`, and `latency_ok=false`.
- Later `.57` SH8 evidence closes the private-grade and latency slices separately; full promotion still depends on the full current G12 gate bundle, not the historical Geyser-only artifact.

Critical future-host risk:

- If `/etc/shadow-engine/shadow-guard-program-keypair.json` is not securely preserved, the exact
  deployed program ID above cannot be upgraded from the new host. It is acceptable to deploy a
  replacement program ID, but then every config/template proof and SH7-C evidence line must use the
  new program ID.
- Do not commit keypair JSON files or failed-deploy recovery material to the repo.
- Do not preserve SH7-C green status from copied config alone after a host move. The new host must
  independently verify program deployment, template match, guard smoke, `live_landed=0`, and
  `promote_allowed=false`.

New-host resume sequence for SH7-C:

1. Read `host_run_guide.md` plus the current `g12-qc` docs before reusing host-local state.
2. Clone/pull `sh0-source-grade` and confirm the SH7-C green commit or newer is present.
3. Install or verify Solana CLI, Rust, cargo, and cargo-build-sbf.
4. Create `/etc/shadow-engine` and stage ShadowGuard keypairs by secure operator channel only.
5. Set keypair ownership/permissions to `root:root` and `600`.
6. If reusing the deployed program ID, verify `solana-keygen pubkey /etc/shadow-engine/shadow-guard-program-keypair.json` returns `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`.
7. If generating a new program ID, deploy it and update `execution.shadow_guard_program_id` plus `rollout.shadow_guard_program_id` in the active host configs before running proofs.
8. Wire real guard vault accounts, not token/sysvar/program placeholders.
9. Run `g1-host-pack/scripts/21_shadowguard_preflight.sh` and keep it red until the program is actually deployed or verified and smoke-tested.
10. Fund the deployer or choose a funded deployer path before any replacement deploy or upgrade.
11. Deploy, upgrade, or verify the ShadowGuard program on the intended mainnet-beta cluster.
12. Run guard abort/pass smoke and set `SHADOW_GUARD_SMOKE_OK=1` only for the final proof run.
13. Export G12 gate input/report and require `shadow_guard_mainnet_deployed=true`, `shadow_guard_template_program_id_matches=true`, `live_landed=0`, and `promote_allowed=false`.
