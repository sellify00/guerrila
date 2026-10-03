# G1 Runbook — Commodity Host Determinism & Kernel-Bypass Bringup

## Prerequisites

| Requirement | Minimum |
|-------------|---------|
| CPU | AMD EPYC-class (or Ryzen with NUMA) |
| RAM | 128 GB |
| NIC | XDP_ZEROCOPY capable: Mellanox ConnectX-4+, Intel E810, or Intel X710 |
| OS | Ubuntu 22.04+ LTS (kernel >= 6.1 recommended; kernel 6.8+ requires completion ring for RX-only AF_XDP — our `xsk.rs` handles this) |
| Packages | `build-essential`, `clang`, `llvm`, `libbpf-dev`, `bpftool`, `ethtool`, `numactl`, `stress-ng`, `linux-tools-$(uname -r)`, `curl`, `bc` |

```bash
apt update && apt install -y \
  build-essential clang llvm libbpf-dev \
  ethtool numactl stress-ng linux-tools-generic \
  curl bc iproute2 net-tools bpftool linux-headers-$(uname -r)
```

For OOB serial-console access and rescue-driven password recovery, see:

- `g1-host-pack\G1_OOB_RECOVERY_GUIDE.md`

## Current `.15` Lane-Certification Posture

As of 2026-05-29, host `109.94.99.15` is being used to certify lane readiness through 2026-06-14. The goal is not random live attempts. The goal is to separate architecture/system blockers from market, source-timing, speed, and strategy blockers.

Current lane map:

```text
Rebalancing:
  REBALANCING_EXECUTION_PLUMBING_READY
  latest proof = 66RG certificate reconciliation
  execution_plumbing_ready_rows = 21
  proposal_eligible_rebalancing_rows = 0
  split = 12 sender no-send, 9 bundle no-submit
  remaining blocker = owner_selector_instrumentation_repair

DirectArb:
  DIRECTARB_STRUCTURAL_HUNT_READY
  remaining blocker = policy-clean roundtrip economics / habitat quality
  role = sentinel only until a cleaner habitat appears

Backrun:
  preprocessed ingest / selector / projection are green
  remaining blocker = executable-shape density / exit-quality, not source timing
  latest proof = fixed R1 Backrun factory smoke + active 6h no-send rerun

Launch Continuation:
  LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED
  role = tail-selection of early launch momentum, not blind-buy
  technical read = LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN
  strategy read = NOT_A_STRATEGY_DURATION_TEST
  pnl read = SHORT_VALIDATION_COST_DOMINATED
  latest proof = 2026-06-01 hot R2 live sample acquired, rescue-flattened, final inventory 0
  remaining blocker = monitored-position primary flatten proof
  next proof = Launch Continuation R2-HOLD via 22_launch13_r2_hold_live_execute.sh and 22_launch11 exit sentinel

Post-shock / continuation:
  structurally real
  current result = zero proposal rows in current resume5 and fresh windows
  remaining blocker = policy / account / economics
  role = parked until a new habitat produces fresh proposal rows

Housekeeping:
  current fresh window = account / artifact surface only
  next proof = fresh tiny-trade factory or saved-loss certificate
```

Latest Rebalancing certificate reconciliation:

```text
script=66rg_rebalancing_certificate_owner_reconciliation_20260527
artifact=/home/ubuntu/Guerrila/g1-host-pack/results/66rg_rebalancing_certificate_owner_reconciliation_20260527/final_verdict.json
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

Interpretation: `66RB` proved execution plumbing only. The corrected wording is: `21` rows passed structural execution plumbing, but `0` rows passed Rebalancing owner-mode eligibility. Rebalancing proposal work is closed until `66RG2` proves owner-selector instrumentation, PM owner lock or split-rule readiness, restoration pressure, and false-restoration inputs. This certificate does not authorize live execution, ATA creation, acquisition, Token-2022 allow, or bundle submission.

AFBR1F was restored on `.15` after `shadow-state` finished bootstrap and `/dev/shm/shadow_health` turned green. The useful batch result was:

```text
valid_afbr1f_sample_count=10
attempts_completed=13
no_candidate_window_count=3
invalid_preflight_count=0
invalid_post_cleanup_count=0
local_rpc_unhealthy_count=0
agave_behind_after_cleanup_count=0
freshness_race_classification_counts={"ALREADY_PROCESSED_AT_FIRST_CHECK": 10}
```

AFBR1G over that batch sharpened the diagnosis:

```text
overall_diagnosis=BUNDLE_READY_BUT_STATUS_LATE
arrival_timing_decision_counts={"BUNDLE_READY_BUT_STATUS_LATE":6,"STATUS_CHECK_LATE":4}
source_late_count=0
fresh_at_bundle_ready_count=0
target_seen_to_bundle_ready_us_p50=4429
target_seen_to_bundle_ready_us_p90=46403
bundle_ready_to_first_status_check_us_p50=671059
bundle_ready_to_first_status_check_us_p90=4747862
```

AFBR1H then corrected the proof path by writing a tiny `/dev/shm` hot bundle-ready probe row before heavy artifact construction and probing status from that hot row:

```text
script=21_afbr1h_immediate_bundle_ready_freshness_batch.sh
status=SOURCE_LATE_DOMINANT
valid_afbr1h_sample_count=10
freshness_class_counts={"SOURCE_LATE": 10}
fresh_at_bundle_ready_count=0
status_unknown_count=0
probe_nonzero_count=0
unsafe_live_activity_count=0
no_candidate_window_count=5
invalid_preflight_count=0
invalid_post_cleanup_count=0
reference_rpc_unreachable_count=0
local_rpc_unhealthy_count=0
agave_behind_after_cleanup_count=2

target_seen_to_bundle_ready_us_p50=148
target_seen_to_bundle_ready_us_p90=16017
target_seen_to_bundle_ready_us_p99=30562
bundle_ready_to_status_start_us_p50=997
bundle_ready_to_status_start_us_p90=1377
bundle_ready_to_status_start_us_p99=1587
monitor_seen_to_status_start_us_p50=37
provider_sim_latency_us_p50=67213
sendBundle_called=false
wire_send=0
live_landed=0
```

Interpretation: AFBR1G proved the saved freshness/status measurement was late. AFBR1H fixed that proof-path timing: across `10` valid native zero-copy no-send samples, Shadow reached bundle-ready quickly, started the immediate status probe at about `1ms` p50 / `1.6ms` p99 after bundle-ready, and every sampled target was already processed while no live activity occurred. Treat Backrun as structurally HUNT-ready but `.15` AF_XDP Backrun as source/fresh-target gated in the current sample distribution. The next serious Backrun source work is earlier target availability, such as Helius preprocessed / Shred Delivery entitlement or another earlier target-source topology, not reopening projection, PM, provider auth, or bundle shape.

Do not rerun AFBR1H/AFBR1F/AFBR1G unless both Agave and `shadow-state` are green:

```text
local Agave getHealth=ok
slot lag acceptable
/dev/shm/shadow_health present and state=1
shadow-state full-mode publisher running
eno1 has no XDP before attach
/sys/fs/bpf/shadow_xsks_map absent before attach
g1-local-agave-watchdog.timer active
```

AF_XDP posture for Backrun and strict DirectArb proofs on `.15` is native zero-copy only:

```text
SHADOW_XDP_ATTACH_MODE=native
afxdp_bind_mode=zero_copy
SHADOW_AFXDP_SAFE_MODE=false
```

`xdpgeneric` / copy mode is not acceptable evidence for these proof families. After any bounded lease, hand `eno1` fully back to Agave and verify no Shadow datapath processes, no XDP, no XSKMAP, restored queues, local Agave green, and `shadow_health` green.

---

## Step 1: Transfer the Pack

```bash
# From your dev machine:
scp -r g1-host-pack/ user@host:/opt/shadow/g1-host-pack/
ssh user@host
cd /opt/shadow/g1-host-pack
```

---

## Step 2: Fill Environment Variables

```bash
cp env.g1.sh env.g1.local.sh
vi env.g1.local.sh   # fill in all CHANGE_ME values
```

Required values:
- `SHADOW_GEYSER_ENDPOINT` / preferred alias `HELIUS_LASERSTREAM_URL` — gRPC URL for the live external stream; for current RunLS work this should be the Helius LaserStream endpoint, not Triton
- `SHADOW_GEYSER_TOKEN` / preferred alias `HELIUS_LASERSTREAM_API_KEY` — auth token for the live external stream
- `SHADOW_GRPC_FALLBACK_WORKERS` — tier0 gRPC fallback ingest workers (`0` disables tier0 gRPC ingest while preserving `shadow-state` Triton state)
- `SHADOW_RPC_URL` — HTTP RPC for `shadow-state` bootstrap / hydration / targeted reads (default this to Helius, not Triton)
- `SHADOW_BOOTSTRAP_RPC_URL` — optional high-throughput HTTP override for bootstrap / hot-pool hydration (defaults to `SHADOW_RPC_URL`)
- `SHADOW_SOLANA_RPC_URL` — HTTP RPC for PM blockhash / signature-status polling (default this to Helius)
- `SHADOW_PUBLIC_RPC_URL` / preferred alias `HELIUS_GATEKEEPER_URL` — HTTP RPC for explicit `public_rpc` `sendTransaction` (prefer Helius Gatekeeper when available)
- `SHADOW_GRPC_SOURCE_GRADE` — leave this unset unless intentionally overriding it; `env.g1.sh` now infers `laserstream` automatically from the Helius LaserStream endpoint and the RunLS wrappers fail closed if the rendered grade is wrong
- `SHADOW_NIC_IFACE` — NIC interface name (run `ip link` to find)
- `SHADOW_MONEY_CORES` — isolated cores for money workers (check `lscpu` for NUMA)
- Credential paths — copy keypairs to `/etc/shadow-engine/`

Legacy aliases `HELIUS_GATEKEEPER`, `LASERSTREAM_GRPC_URL`, and `LASERSTREAM_X_TOKEN` still work, but the current repo-standard names are the `HELIUS_*` variables above.

---

## Step 3: Build Binaries

Build must run on Linux (the host itself, or a Linux build machine with the same target).

```bash
# Option A: Build on the host (requires Rust toolchain)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
cd /path/to/guerrila-workspace
./g1-host-pack/build.sh

# Option B: Build on a separate Linux machine, then scp binaries
./g1-host-pack/build.sh
scp g1-host-pack/artifacts/release-guerrila/shadow-* user@host:/opt/shadow/g1-host-pack/artifacts/release-guerrila/
```

Produces:
- `artifacts/release-guerrila/shadow-tier0`
- `artifacts/release-guerrila/shadow-egress-harmonic`
- `artifacts/release-guerrila/shadow-oracle`
- `artifacts/release-guerrila/shadow-state`
- `artifacts/release-guerrila/g4-ingress-seal`
- `artifacts/release-guerrila/g4-shred-replay`
- `artifacts/release-guerrila/build-g4-corpus`

The standalone DPDK latency harness is built separately with Meson/Ninja on Linux:

```bash
apt update && apt install -y meson ninja-build pkg-config dpdk-dev libsystemd-dev
cd tools/dpdk-rx-latency
meson setup builddir
meson compile -C builddir
```

This produces `tools/dpdk-rx-latency/builddir/dpdk-rx-latency`, which is the guarded
userspace p999 probe for `rte_eth_rx_burst -> UDP classify`.

> **Note:** Script 12 (`g5_dequeue_egress`) requires the Rust toolchain on the host because it runs the ignored raw-nanosecond dequeue→egress benchmark on pinned cores. If building off-host, either install Rust on the host for script 12, or run the benchmark locally with `cargo test -p shadow-tier0 --release -- --exact money_worker::tests::g5_dequeue_to_egress_raw_ns --ignored --nocapture`.

---

## Step 4: BIOS Configuration

Enter BIOS setup and apply:

1. **CPU Governor**: Lock to `Performance` (disable EIST/SpeedStep or set fixed P-state)
2. **C-States**: Disable C6 and deeper. Allow C1 only if needed for halt.
3. **SMT**: Disable on money cores (or globally if acceptable)
4. **AVX Negative Offset**: Set to zero only if thermal headroom confirms no throttling
5. **NPS (NUMA Per Socket)**: NPS4 if EPYC, to maximize locality between NIC and money cores

---

## Step 5: Kernel Boot Parameters

Edit `/etc/default/grub`:

```
GRUB_CMDLINE_LINUX="isolcpus=4-11 nohz_full=4-11 rcu_nocbs=4-11 \
  default_hugepagesz=2M hugepagesz=2M hugepages=4096 \
  intel_iommu=on iommu=pt"
```

Adjust core ranges to match your NUMA topology. Then:

```bash
update-grub
reboot
```

After reboot, verify:

```bash
cat /proc/cmdline | grep isolcpus
grep HugePages_Total /proc/meminfo
```

---

## Step 6: NIC Setup

```bash
# Set combined queue count
ethtool -L $SHADOW_NIC_IFACE combined 12

# Increase ring buffers
ethtool -G $SHADOW_NIC_IFACE rx 4096 tx 4096

# Pin NIC IRQs away from money cores
# (script 07_g4_host_seal.sh will verify this)
for irq in $(grep $SHADOW_NIC_IFACE /proc/interrupts | awk '{print $1}' | tr -d ':'); do
    echo "$SHADOW_UTILITY_CORES" > /proc/irq/$irq/smp_affinity_list
done

# Enable hardware timestamps
ethtool -T $SHADOW_NIC_IFACE  # check capability first
hwstamp_ctl -i $SHADOW_NIC_IFACE -r 1 2>/dev/null || \
  ethtool --set-priv-flags $SHADOW_NIC_IFACE rx-timestamps on 2>/dev/null || true
```

---

## Step 6a: veth Loopback for Deterministic Seal Testing

The G4 seal uses deterministic corpus replay, which requires a local loopback path (send shreds to ourselves). A veth pair with a network namespace provides this. veth forces copy mode — this is expected and only affects the seal harness, not production.

**Production traffic uses `eno2` (or your NIC) with XDP_ZEROCOPY — no `--copy-mode` flag.**

```bash
# Create veth pair with a network namespace
ip link add seal_rx type veth peer name seal_tx
ip netns add seal_ns
ip link set seal_tx netns seal_ns

# Assign IPs
ip addr add 10.99.2.1/24 dev seal_rx
ip netns exec seal_ns ip addr add 10.99.2.2/24 dev seal_tx

# Bring up
ip link set seal_rx up
ip netns exec seal_ns ip link set seal_tx up
ip netns exec seal_ns ip link set lo up

# Load XDP on the RX side (Step 7 must be done first)
ip link set dev seal_rx xdp obj xdp/shadow_rx_meta.bpf.o sec xdp
bpftool map pin name xsks_map /sys/fs/bpf/shadow_xsks_map
```

Then run the seal with `--copy-mode` and `--iface seal_rx`. The replay sender runs from inside the namespace:

```bash
# Seal (main namespace)
sudo ./g4-ingress-seal --iface seal_rx --queue 0 --pin-core 2 \
  --scenario corpus/baseline.json --copy-mode \
  --xskmap-pin /sys/fs/bpf/shadow_xsks_map --out-dir /tmp/results &

# Sender (seal_ns namespace)
sudo ip netns exec seal_ns ./g4-shred-replay \
  --scenario corpus/baseline.json --target-ip 10.99.2.1 --pps 500 \
  --out-dir /tmp/results
```

> **Limitations:** veth does not support zero-copy or hardware timestamps. XDP metadata timestamps come from `bpf_ktime_get_ns()` (software clock). Copy-mode throughput is lower than real NIC zero-copy. These are acceptable for correctness and functional seals. Production runs on the real NIC (`eno2`) with XDP_ZEROCOPY for full performance.

---

## Step 6b: R1-AFXDP-ENO1-SAFE

Use this guarded profile when AF_XDP must run on the routed `eno1` lane of an i40e / AMD-Vi host for observe-only or no-send evidence:

```bash
source g1-host-pack/env.g1.sh
source g1-host-pack/env.r1-afxdp-eno1-safe.sh
envsubst < g1-host-pack/config.r1-afxdp-eno1-safe.yaml > /etc/shadow-engine/config.guerrila.yaml
sudo g1-host-pack/scripts/20_g12_r0_observe_only.sh 60
```

If the validator is still moving between bootstrap phases and large shred-bearing UDP is intermittent, use the shred-gated detached launcher instead of babysitting the timing manually:

```bash
source g1-host-pack/env.g1.sh
source g1-host-pack/env.r1-afxdp-eno1-safe.sh
export SHADOW_AFXDP_SHRED_WAIT_SECONDS=21600
export SHADOW_AFXDP_OBSERVE_WINDOW_SECONDS=300
sudo g1-host-pack/scripts/23_afxdp_wait_for_shreds_then_observe_launch.sh
```

That wrapper waits for `tcpdump` on the live AF_XDP interface to see `udp portrange 8000-8025` with packet size `>1000`, then launches `20_g12_r0_observe_only.sh` automatically under the same safe `eno1` posture.

For current RunLS / dual-source work, `20_g12_r0_observe_only.sh` now hard-fails before arming if any of these are false:

```text
grpc_source_grade=laserstream
grpc_fallback points at Helius LaserStream
state.source.geyser_endpoint points at Helius LaserStream
SHADOW_PM_SHOCK_RESEARCH_EXPORT_PATH is window-local
```

The expected per-window arbiter artifacts now include:

```text
laserstream_source_arbiter_report.json
laserstream_vs_afxdp_signature_first_seen.jsonl
laserstream_vs_afxdp_candidate_join_report.json
source_grade_candidate_summary.json
laserstream_vs_afxdp_matched_signature_stage_deltas.jsonl
```

This profile locks the stable baseline:

```text
eno1
one queue
xdpgeneric
AF_XDP copy mode
observe-only / no-send
explicit management-NIC override
offload reductions before XDP load
```

Do not widen queue count or move back to zero-copy until the previous soak at the safer setting has passed.

---

```bash
cd /opt/shadow/g1-host-pack/xdp

# If vmlinux.h is missing (needed for BTF-enabled BPF):
bpftool btf dump file /sys/kernel/btf/vmlinux format c > vmlinux.h

make
# Produces: shadow_rx_meta.bpf.o
```

Load the XDP program and pin the XSKMAP:

```bash
# Load XDP program on the NIC interface
ip link set dev $SHADOW_NIC_IFACE xdp obj shadow_rx_meta.bpf.o sec xdp

# Pin the XSKMAP so the seal binary can register XSK socket fds
# The map name must match what's in shadow_rx_meta.bpf.c (xsks_map)
bpftool map pin name xsks_map /sys/fs/bpf/shadow_xsks_map

# Verify
bpftool prog show | grep shadow_rx_meta
ls -la /sys/fs/bpf/shadow_xsks_map
```

> **XSKMAP is required.** The XDP program uses `bpf_redirect_map(&xsks_map, rx_queue_index, XDP_PASS)` to deliver frames to AF_XDP sockets. Without the pinned map, `g4-ingress-seal --xskmap-pin` cannot register the socket fd and XDP will pass all frames to the kernel stack instead.

---

## Step 8: Expand Config

`ShadowConfig::from_file` does **not** expand environment variables.
Use `envsubst` to produce the final config:

```bash
source env.g1.local.sh
mkdir -p /etc/shadow-engine
envsubst < config.guerrila.yaml > /etc/shadow-engine/config.guerrila.yaml
```

Verify placeholder expansion:

```bash
grep 'CHANGE_ME\|\${\|${' /etc/shadow-engine/config.guerrila.yaml
# Should produce no output
```

---

## Step 9: Deploy Credentials

```bash
# Copy keypairs to the paths referenced in config
cp jito-auth-keypair.json    /etc/shadow-engine/
cp harmonic-auth-keypair.json /etc/shadow-engine/
cp worker-signer.json        /etc/shadow-engine/
cp jito_tip_lut.bin          /etc/shadow-engine/

chmod 600 /etc/shadow-engine/*.json /etc/shadow-engine/*.bin
```

---

## Step 10: Run Validation Suite

The full suite runs 13 scripts in order:

| # | Script | What it does |
|---|--------|-------------|
| 00 | `host_facts` | CPU, NUMA, NIC, memory, governor audit |
| 01 | `nic_xdp_cap` | XDP_ZEROCOPY + driver capability check |
| 02 | `private_lane_rtt` | RTT to Jito + Harmonic endpoints |
| 03 | `afxdp_functional` | Synthetic UDP injection, zero corruption |
| 04 | `hw_timestamp` | RX hardware timestamp verification |
| 05 | `rx_latency` | p50/p99 RX descriptor latency |
| 06 | `thermal_stability` | AVX workload + clock lock |
| 07 | `g4_host_seal` | isolcpus, hugepages, IRQ, NUMA alignment |
| 08 | `g5_microbench` | Binary readiness + NUMA memory check |
| 09 | `shape_host` | Apply governor, hugepages, IRQ steering, NIC queues |
| 10 | `g4_ingress_perf_seal` | AF_XDP ingress PPS + CPU on pinned core |
| 11 | `g5_strace_smoke` | No file/network/clock syscalls in hot path |
| 12 | `g5_dequeue_egress` | Dequeue-to-egress p50/p95/p99 on pinned core |

**Recommended run order**: 00-08 first (read-only audit), then 09 (shapes host), then 10-12 (active benchmarks). `run_all.sh` executes this sequence automatically.

```bash
source env.g1.local.sh
sudo -E bash scripts/run_all.sh
```

Or run individual scripts:

```bash
sudo -E bash scripts/00_host_facts.sh    # no root needed
sudo -E bash scripts/01_nic_xdp_cap.sh
sudo -E bash scripts/03_afxdp_functional.sh
# etc.
```

Use `--continue` to run all scripts even if some fail:

```bash
sudo -E bash scripts/run_all.sh --continue
```

---

## Step 11: Review Results

```bash
ls -la results/*/
cat results/00_host_facts/host_facts.txt
cat results/07_g4_host_seal/g4_host_seal.txt
```

---

## Exit Criteria

All three must hold to sign off G1:

- [ ] **AF_XDP stable on the commodity host** — scripts 01, 03, 04, 05, 10 all pass
- [ ] **Host audit passes cleanly** — scripts 00, 07 all pass, no IRQs on money cores, hugepages mounted, governor locked
- [ ] **No reason remains to buy Equinix before software proof** — RTT (script 02) acceptable, thermals (script 06) stable

G5 host seals (scripts 11, 12) must also pass before G5 is signed off:
- [ ] **Strace smoke** — script 11 shows zero banned syscalls on money-worker threads
- [ ] **Dequeue-to-egress microbench** — script 12 shows p99 inside the 150 us Guerrila budget

When all 13 scripts pass with zero failures, G1 + G4 host seal + G5 host seal are done.

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `No hugepages configured` | Check `/etc/default/grub` hugepages param, reboot |
| `NIC NUMA mismatch` | Move money cores to NIC-local NUMA node, or swap NIC slot |
| `XDP program load fails` | Check kernel version >= 6.1, install `libbpf-dev`, rebuild `.bpf.o`. If `vmlinux.h` missing: `bpftool btf dump file /sys/kernel/btf/vmlinux format c > vmlinux.h` |
| `AF_XDP bind EINVAL` | Kernel 6.8+ requires both fill AND completion ring even for RX-only sockets. Our `xsk.rs` handles this — if using a custom XSK setup, add `XDP_UMEM_COMPLETION_RING` |
| `AF_XDP 0 frames received` | Check XSKMAP is pinned and XDP program calls `bpf_redirect_map()`. Without it, frames go to kernel stack. Verify with `bpftool prog show` and `bpftool map show`. On `.15`, also dump `shadow_xdp_diag`: if `total_packets` / `port_range` move but `shred_variant` and `redirect` stay `0`, the wire window is only tiny gossip/bootstrap UDP, not raw shreds. |
| `0% parse rate on received frames` | AF_XDP delivers full Ethernet frames (14+20+8=42 byte headers). `parse_shred()` expects raw shred bytes. Our `strip_udp_headers()` in `afxdp_runtime.rs` handles this — if you see parse failures, check the stripping path |
| `socat not delivering packets in netns` | Use `/dev/udp` bash built-in instead: `echo HELLO > /dev/udp/10.99.2.1/9000`. socat may have path issues inside network namespaces |
| `AF_XDP socket creation failed` | Need root, kernel `CONFIG_XDP_SOCKETS=y` |
| `Thermal throttling` | Check BIOS C-state/AVX offset settings, improve cooling |
| `IRQ violations on money cores` | Re-run NIC IRQ affinity script (Step 6) |
| `envsubst leaves ${}` | Ensure all vars are exported (`set -a` in env.g1.sh) |
| `Pressure seal: 0 recycles` | DelayFifo (1M depth) never primes in short runs. `survival_flush_count` is valid recycle evidence — the verdict logic accounts for this |
| `sudo: unable to resolve host` | Harmless warning. Add hostname to `/etc/hosts`: `echo "127.0.1.1 $(hostname)" >> /etc/hosts` |

---

## G12 Observe-Only Reprovision Notes

These notes were learned later during G12 host work and matter for the next hourly reprovision even though they are not part of the original G1 host seal.

### Current known-good startup posture

- Upload the current `g1-host-pack` plus the current G12-critical workspace files from local/GitHub, not an older host tarball.
- The working host tree is `/home/ubuntu/Guerrila` and is not a git checkout, so host sync must be verified by copying files or comparing hashes.
- Generate `/etc/shadow-engine/config.guerrila.yaml` with `envsubst` from `g1-host-pack/config.guerrila.yaml`; do not run from placeholder YAML or the older top-level config.
- Keep using the dashed rpcpool hostname. The dotted variant served the wrong certificate and caused endpoint failures.

### Stale config trap — do this exactly

This was the repeated fresh-host startup mistake during G12.

- **Do not** generate `/etc/shadow-engine/config.guerrila.yaml` from the repo-root `config.guerrila.yaml` during current G12 reprovision.
- For current Tier0/G12 routing surfaces, the repo-root config is older and can miss required fields such as `submit.routing.pre_sign_egress_admission_gate`.
- The correct source for fresh-host reprovision is:
  - `/home/ubuntu/Guerrila/g1-host-pack/config.guerrila.yaml`

Use this exact pattern on a new host:

```bash
cd /home/ubuntu/Guerrila
set -a
source g1-host-pack/env.g1.local.sh
set +a

envsubst < g1-host-pack/config.guerrila.yaml | sudo tee /etc/shadow-engine/config.guerrila.yaml > /dev/null

./g1-host-pack/artifacts/release-guerrila/shadow-tier0 --check-config /etc/shadow-engine/config.guerrila.yaml
```

Quick proof that the right config source was used:

```bash
grep -n "pre_sign_egress_admission_gate" /etc/shadow-engine/config.guerrila.yaml
```

- If that field is missing, assume the host was expanded from the wrong config source and regenerate `/etc/shadow-engine/config.guerrila.yaml` before debugging anything else.
- If `shadow-tier0 --check-config` fails on routing fields after reprovision, treat stale config generation as the first suspect.

### Required host tools

- Install Rust and run `source ~/.cargo/env`. Script 12 and some rebuild/debug flows require `cargo` on the host PATH.
- Install Solana CLI as well. ShadowGuard preflight now depends on `solana`.

### Required credentials and paths

- Stage the normal auth/signer material under `/etc/shadow-engine/`.
- If ShadowGuard ops proof is in scope, also stage `/etc/shadow-engine/shadow-guard-deployer.json`. Without it, preflight fails closed even if Solana CLI is installed.

### Host pitfalls rediscovered during G12

- Keep hugepages at `4096`; lower values caused UMEM allocation failure.
- After copying shell scripts from Windows, normalize them back to LF on-host. CRLF broke G12 runner scripts.
- When using `sudo`, source the environment inside the elevated shell if the script depends on `env.g1.sh`.
- Keep the management/default-route NIC and the dedicated ingress NIC separate. On the failed hourly host, `eno1` carried normal traffic while the candidate ingress NIC `eno2` stayed at `rx_packets=0`.

### Mandatory zero-candidate liveness ladder

Before investigating any `0 candidates`, `0 observe_candidates`, or `0 process_intent` result, walk this ladder in order and stop at the first red level:

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

Operator rule:

- do **not** debug CLMM, WSOL, route filters, mint gates, or policy before `L0-L5` are green
- do **not** interpret `0 candidates` as market coldness until `L2-L5` are proven in the same runtime window
- on `.15`, `L2` requires real large UDP on `eno1`; tiny `132`-byte gossip/bootstrap packets do **not** satisfy the ladder
- if `L3` is red but `L2` is green, debug XDP/DPDK/XSKMAP/VFIO/runtime wiring
- if `L4` is red but `L3` is green, debug producer/feed-ring/export plumbing
- if `L5` is red but `L4` is green, debug worker ingest/records consumption
- only after `L6-L8` fail with `L0-L5` green should strategy or economics be blamed

### Reject a bad hourly host fast — before a long G12 run

This is the most important cost-saving lesson from the rejected hourly G12 host.

- The AF_XDP software path can be completely correct and still produce `worker_signal_afxdp=0` if the dedicated NIC is not receiving the upstream feed.
- Before spending time on script 20 or deeper G12 debugging, prove that the dedicated ingress NIC sees packets **even with XDP detached**.
- If the dedicated NIC stays at `rx_packets=0` / `rx_bytes=0` during a short control capture, reject the host or escalate to the provider immediately.

Use this exact control proof on a new host before any long G12 runtime:

```bash
cd /home/ubuntu/Guerrila

export IFACE="$SHADOW_NIC_IFACE"

sudo ip link set dev "$IFACE" xdp off || true
sudo rm -f /sys/fs/bpf/shadow_xsks_map || true

echo "== link =="
ip -br link show "$IFACE"
cat /sys/class/net/"$IFACE"/address
ethtool "$IFACE" | egrep 'Speed|Duplex|Auto-negotiation|Link detected' || true

echo "== before =="
ip -s link show "$IFACE"

echo "== tcpdump 30s =="
sudo timeout 30 tcpdump -ni "$IFACE" -e -vv -c 20 'udp or ip' || true

echo "== after =="
ip -s link show "$IFACE"
sudo ethtool -S "$IFACE" 2>/dev/null | grep -Ei 'rx|xdp|drop|queue_0|queue0' | head -200 || true
```

Reject the host before deeper debugging if:

- `ip -s link show "$IFACE"` still reports `rx_packets=0` and `rx_bytes=0`;
- `tcpdump` captures zero packets on the candidate ingress NIC;
- the management NIC clearly sees normal traffic while the candidate ingress NIC stays silent.

Quick wrong-NIC comparison:

```bash
for IFACE in eno1 eno2; do
  echo
  echo "===== $IFACE ====="
  ip -br link show "$IFACE"
  ip -s link show "$IFACE" | sed -n '1,8p'
  sudo timeout 5 tcpdump -ni "$IFACE" -c 5 'udp or ip' 2>&1 | head -20 || true
done
```

- If another interface sees packets and the chosen ingress NIC does not, either point `SHADOW_NIC_IFACE` at the real feed NIC or have the provider move the feed.
- Do **not** relabel gRPC fallback as private to paper over this.
- Do **not** chase `process_intent()` / post-route no-send diagnostics until a real private-grade source reaches the worker.
- Treat this host-rejection proof as the `L2` entry gate in the mandatory liveness ladder above.
- Once the NIC does show real ingress, the current script 20 helper will rebuild/load the XDP program, pin `/sys/fs/bpf/shadow_xsks_map`, and clamp the NIC to one combined queue for the single AF_XDP worker.

### Recommended first passes on a fresh G12 host

1. install packages, Rust, and Solana CLI;
2. expand config into `/etc/shadow-engine/config.guerrila.yaml` from `g1-host-pack/config.guerrila.yaml`, not the repo-root config;
3. verify hugepages and endpoint/env posture;
4. run `shadow-tier0 --check-config /etc/shadow-engine/config.guerrila.yaml` immediately after expansion so stale config is caught before service startup;
5. run the XDP-detached control proof above and reject the host immediately if the dedicated NIC still has zero RX;
6. only after the dedicated NIC shows real traffic, run script 12 to regenerate the worker latency artifact;
7. then run script 20 for a 120-second G12 `r0_observe_only` diagnostic;
8. if `sample_slots` / `observe_candidates` stay zero, re-check that the current `crates/shadow-tier0/src/money_worker.rs` zero-mint observe-only fix is present before rebuilding Tier0;
9. if `worker_signal_afxdp` is still zero after the NIC already proves real ingress, then debug AF_XDP/XSKMAP/runtime wiring;
10. if private-grade signals are non-zero but `rollout_would_send` and `micro_market_observations` stay zero, the blocker has moved downstream to a silent post-route no-send failure rather than hydration, health, or LUT startup;
11. do not attempt a longer acceptance run until one of those two branches is understood.

### DPDK RX latency harness

Use the standalone harness before any Tier0 DPDK integration when you need an honest
DPDK-side p50/p95/p99/p999 for `rte_eth_rx_burst -> UDP 8000-8025 classify`.

Build on the Linux host:

```bash
cd /home/ubuntu/Guerrila/tools/dpdk-rx-latency
meson setup builddir
meson compile -C builddir
```

Run the guarded wrapper directly on a non-management NIC:

```bash
source /home/ubuntu/Guerrila/g1-host-pack/env.g1.local.sh
sudo -E bash /home/ubuntu/Guerrila/g1-host-pack/scripts/21_dpdk_rx_latency.sh
```

For a management NIC like `eno1`, launch it detached so the run survives VFIO takeover:

```bash
source /home/ubuntu/Guerrila/g1-host-pack/env.g1.local.sh
sudo -E bash /home/ubuntu/Guerrila/g1-host-pack/scripts/21_dpdk_rx_latency_launch.sh
```

Operational note from `109.94.99.15`:

- if Ubuntu 24.04 does not provide `dpdk-devbind.py` on `PATH`, install or stage an equivalent helper before using the wrappers; the current X550 proving host uses `/usr/local/bin/dpdk-devbind.py`
- because the detached launcher uses `systemd-run`, make sure `SHADOW_NIC_IFACE`, `SHADOW_RESULTS_DIR`, and any DPDK override vars are injected into the unit environment explicitly if a sourced shell alone does not propagate them

Key env overrides:

- `SHADOW_DPDK_LATENCY_IFACE` — NIC to bind (defaults to `SHADOW_NIC_IFACE`)
- `SHADOW_DPDK_LATENCY_SECONDS` — capture window
- `SHADOW_DPDK_LATENCY_LCORES` — DPDK EAL lcore mask/range
- `SHADOW_DPDK_LATENCY_QUEUE_ID` — queue to poll
- `SHADOW_DPDK_LATENCY_JSON` — explicit JSON output path

The wrapper restores the original kernel driver, IPv4 address, and default route, then
adds `dpdk_kernel_faults` plus the saved kernel log path to the emitted JSON.

### DPDK Tier0 observe-only wrapper

After the standalone harness is green, the guarded full-path DPDK Tier0 window is:

```bash
cd /home/ubuntu/Guerrila
source /home/ubuntu/Guerrila/g1-host-pack/env.g1.local.sh
source ~/.cargo/env
cargo build -p shadow-tier0 --release --features dpdk --bin shadow-tier0
sudo -E bash /home/ubuntu/Guerrila/g1-host-pack/scripts/22_dpdk_tier0_observe_only.sh
```

On Ubuntu 24.04, `shadow-tier0 --features dpdk` also needs `libsystemd-dev`; otherwise the release link step fails on `-lsystemd`.

For a management NIC like `eno1`, prefer the detached launcher:

```bash
cd /home/ubuntu/Guerrila
source /home/ubuntu/Guerrila/g1-host-pack/env.g1.local.sh
sudo -E bash /home/ubuntu/Guerrila/g1-host-pack/scripts/22_dpdk_tier0_observe_only_launch.sh
```

The wrapper:

- stops the current Tier0 process;
- writes a temporary observe-only/no-send config with `private_packet_backend: dpdk`;
- binds the target NIC to `vfio-pci`;
- runs release Tier0 with DPDK enabled;
- restores the original NIC driver/IP/default route;
- restarts the baseline Tier0 binary after the DPDK window.

Important management-NIC notes from `.221`:

- if the same NIC carries management/default-route traffic, expect outbound RPC/DNS to fail while VFIO owns the port;
- the DPDK Tier0 path therefore needs an already-seeded `/dev/shm/shadow_blockhash`, or a preseed step before the handoff;
- gRPC fallback DNS failures during the VFIO window do **not** prove the DPDK RX loop is broken.

Key env overrides:

- `SHADOW_DPDK_TIER0_IFACE` — NIC to bind (defaults to `SHADOW_NIC_IFACE`)
- `SHADOW_DPDK_TIER0_SECONDS` — runtime window
- `SHADOW_DPDK_TIER0_BIN` — explicit Tier0 binary path
- `SHADOW_DPDK_TIER0_CONFIG_SRC` — baseline config to clone before patching to DPDK
- `SHADOW_RESULTS_DIR` — artifact root

Current `.221` / `.15` interpretation:

- historical `.15` artifacts include both shred-bearing `eno1` windows and traffic-dark windows; do **not** assume the current wire state from any saved artifact alone
- the latest zero-counter AF_XDP blackhole census on `.15` was operator-error, not proof of a hot-path classifier bug: there was no live local Agave validator at the time, no UDP `8000-8025` listeners, `worker_signal_afxdp=0`, and no meaningful ingress reached Tier0
- on `2026-05-11`, `.15` was corrected back onto the SH1 path: `sh1-agave-no-geyser.service` now runs `mainnet-beta`, keeps RPC on `127.0.0.1`, uses dynamic ports `8000-8025`, and stores ledger/accounts on `/mnt/solana-ledger` and `/mnt/solana-accounts`
- late `2026-05-14` regression correction: after rescue/reboot and later host work, `.15` fell off that SH1 prerequisite path again; by the time the real DPDK Run38 timeout and the real regular AF_XDP safe/copy Run38 timeout were sampled, there was no local RPC on `127.0.0.1:8899`, no UDP listeners on `8000-8025`, and no live persistent SH1 unit. Those equal-zero runs are therefore missing-local-source evidence, not AF_XDP-vs-DPDK backend falsification
- with that validator active, `.15` can show both transient shred-sized bootstrap windows and tiny-gossip-only windows on routed `eno1`; do **not** promote one earlier `1225-1232` sample into a standing truth for later runs
- therefore, before every new `.15` AF_XDP/DPDK run, verify both:
  - one of the SH1 validator units is active (`sh1-agave-no-geyser.service`, `sh1-agave-no-geyser-2.service`, or the transient `sh1-agave-no-geyser`)
  - `tcpdump -ni eno1 'udp portrange 8000-8025'` during the same runtime window shows large shred-bearing packets, not just tiny `132`-byte gossip/bootstrap UDP or silence
- the main host-pack ingress/scout entrypoints now enforce this fail-closed before they arm packets or scouting:
  - local Agave must be running under one of the expected SH1 units (`sh1-agave-no-geyser.service`, `sh1-agave-no-geyser-2.service`, or the transient `sh1-agave-no-geyser` unit)
  - local RPC at `127.0.0.1:8899` must answer
  - local slot lag versus `SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL` must stay within the configured threshold
  - local UDP listeners must still be bound in `8000-8025`
- current operating rule (`2026-05-14` late): in AF_XDP mode, `.15` should keep local Agave live continuously rather than bringing it up only before a run. Persistent `sh1-agave-no-geyser-2.service`, the watchdog timer, and the storage/preflight guardrails are part of the baseline host posture, not optional recovery steps
- current `.15` restore state (`2026-05-14` late): transient `sh1-agave-no-geyser.service` is running again, persistent `sh1-agave-no-geyser-2.service` is installed and enabled for boot, the watchdog timer is active, UDP `8000-8025` listeners have returned, local RPC has now progressed from unreachable to reachable-but-unhealthy during catch-up, and the validator is actively replaying into readiness. The guarded native zero-copy Run38 waiter is armed behind both local-Agave preflight and explicit OOB arming. Until that preflight passes cleanly, hold new `.15` AF_XDP/DPDK/scout windows
- native zero-copy Run38 on routed `eno1` should now be armed through `g1-host-pack/scripts/38_any_hunt_publicrpc_blackhole_native_wait.sh`, not by blindly swapping the ordinary waiter:
  - it waits for green local-Agave preflight
  - it also waits for explicit OOB arming at `/home/ubuntu/shadow-host-audit/dpdk-runs/run38_native_oob_ready.flag`
  - only after both conditions hold does it launch the native zero-copy Run38 regression harness
- for the AF_XDP operating mode, also install the watchdog timer so the host re-checks SH1 health continuously and starts the preferred persistent validator unit if Agave is absent:
  - script: `g1-host-pack/scripts/24_local_agave_watchdog.sh`
  - units: `g1-host-pack/systemd/g1-local-agave-watchdog.service` and `.timer`
  - default preferred service: `SHADOW_LOCAL_AGAVE_WATCHDOG_SERVICE=sh1-agave-no-geyser-2.service`
  - the watchdog also enforces bounded-storage guardrails:
    - validator runtime must still include `--limit-ledger-size`
    - runtime summary records the active `--maximum-full-snapshots-to-retain` / `--maximum-incremental-snapshots-to-retain` values
    - ledger/accounts free space must stay above the configured GiB floor
- install on host:
  - `sudo install -m 0755 g1-host-pack/scripts/24_local_agave_watchdog.sh /home/ubuntu/Guerrila/g1-host-pack/scripts/24_local_agave_watchdog.sh`
  - `sudo install -m 0644 g1-host-pack/systemd/g1-local-agave-watchdog.service /etc/systemd/system/g1-local-agave-watchdog.service`
  - `sudo install -m 0644 g1-host-pack/systemd/g1-local-agave-watchdog.timer /etc/systemd/system/g1-local-agave-watchdog.timer`
  - `sudo systemctl daemon-reload`
  - `sudo systemctl enable --now g1-local-agave-watchdog.timer`
  - leave the timer enabled; in the always-live `.15` posture it should remain on continuously, not only during proof windows
  - watchdog summaries land under `${SHADOW_RESULTS_DIR}/24_local_agave_watchdog/`
- override knobs live in `env.g1.sh`:
  - `SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT`
  - `SHADOW_LOCAL_AGAVE_SERVICE_NAMES`
  - `SHADOW_LOCAL_AGAVE_RPC_URL`
  - `SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL`
  - `SHADOW_LOCAL_AGAVE_MAX_PROCESSED_SLOT_LAG`
  - `SHADOW_LOCAL_AGAVE_MAX_FINALIZED_SLOT_LAG`
  - `SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MIN`
  - `SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MAX`
  - `SHADOW_LOCAL_AGAVE_REQUIRE_DYNAMIC_PORTS`
  - `SHADOW_LOCAL_AGAVE_WATCHDOG_SERVICE`
  - `SHADOW_LOCAL_AGAVE_REQUIRE_LIMIT_LEDGER_SIZE`
  - `SHADOW_LOCAL_AGAVE_MIN_LEDGER_FREE_GIB`
  - `SHADOW_LOCAL_AGAVE_MIN_ACCOUNTS_FREE_GIB`
- latest guarded `eno1` AF_XDP rerun with rebuilt XDP diagnostics proved the hot path is not the blocker by itself:
  - the XDP program saw packets and Solana-port UDP on `eno1`
  - `shadow_xdp_diag` showed `shred_variant=0`, `shred_version=0`, and `redirect=0`
  - concurrent `tcpdump` in that same window only saw tiny `132`-byte UDP to local `8000`
- interpret that result as upstream feed-arrival / validator-traffic-phase failure, not as an XSKMAP or AF_XDP runtime bug
- if those checks fail, interpret zero `worker_signal_*` / zero `observe_candidates` as missing feed-arrival prerequisites, not as valid evidence against the Tier0 ingest path
- `eno2` remains dark on the current host; do not silently pivot to it unless routing/carrier facts change
- also treat routed `eno1` native AF_XDP on `.15` as a **transition-risk** rather than a blanket steady-state ban: the first native attach / detach cycle can cut carrier and knock out normal SSH until the link and `systemd-networkd` re-settle, so require OOB coverage for the first native arm; once native XDP is attached and settled, normal SSH and native zero-copy AF_XDP can coexist on `eno1`
- fresh dual-source truth (`2026-05-24`): the first clean config-hardened LS6C window on `.15` is `/home/ubuntu/shadow-host-audit/dpdk-runs/runls6c_config_hardened_20260524_e/window1/20_g12_r0_observe_only`; it finished with `cleanup_verified=true`, window-local shock-research / arbiter artifacts, and `matched_row_count=15`
- current source-race interpretation (`2026-05-24`): that clean LS6C window showed `raw_receive_delta_us=-20790`, `normalized_delta_us=-20791`, and `candidate_emit_delta_us=-21057` under the current `laserstream_minus_afxdp` basis, so LaserStream was earlier on that matched sample; `trade_ready_delta_us` stayed `null`, so keep the hybrid operating rule rather than promoting LaserStream to the live trigger
- current RunLS env rule (`2026-05-24`): for the hardened LaserStream windows, `g1-host-pack/env.g1.sh` only resolves the intended values if either the repo `.env` exists **or** `/etc/shadow-engine/env.g1.local.sh` is readable by the operator shell. On `.15`, the working posture is now to keep `/etc/shadow-engine/env.g1.local.sh` readable by `ubuntu` (currently `root:ubuntu`, mode `640`) so `SHADOW_GRPC_SOURCE_GRADE=laserstream`, `SHADOW_GEYSER_ENDPOINT`, and `SHADOW_PUBLIC_RPC_URL` resolve correctly before wrapper preflight. If those resolve blank, stop and fix env readability before trusting any RunLS result.
- current LaserStream Backrun proof (`2026-05-24`): `RunLS9` remains green at `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only`, but the scoped branch has now advanced one level further. The fresh PM-fixed rerun kept `grpc_source_grade_configured=laserstream`, `shock_research_export_path_is_window_local=true`, `cleanup_verified=true`, `live_landed=0`, and `wire_send=0`, while proving a real Backrun no-send lane with the current tape shape: `laserstream_backrun_seen=701`, `laserstream_backrun_no_send_eligible=174`, `laserstream_backrun_no_send_accept=153`, `laserstream_backrun_bundle_ready=153`, `laserstream_backrun_sender_ready=21`, `laserstream_backrun_pm_reserve_ok=153`, `laserstream_backrun_blocked_by_source_policy_before=174`, and `laserstream_backrun_blocked_by_source_policy_after=0`. Treat that as a LaserStream **Backrun-only no-send** promotion proof; do not widen it to DirectArb or live/private-grade routing.
- current LaserStream follow-up audits (`2026-05-24`): `RunLS10`, `RunLS9B`, `RunLS9C`, and `RunLS9D` are now saved next to the LS8/LS9 artifacts and should be read before any live-promotion talk. `RunLS10` (`/home/ubuntu/Guerrila/g1-host-pack/results/20_g12_r0_observe_only/runls10_drop_semantics_summary.json`) says the `drop` slice is non-trading saved-loss / suppression, not an executable egress lane: `drop_total=226`, `drop_executable_count=0`, `drop_no_trade_count=226`, `drop_saved_loss_count=226`, `drop_requires_egress_count=0`, `drop_realized_pnl_possible=0`. `RunLS9B` (`/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9b_laserstream_backrun_sender_pm_summary.json`) says the Sender-ready Backrun subset is PM/accounting-red: `sender_subset_total=41`, `sender_subset_no_send_accept_count=41`, but `sender_subset_pm_reserve_ok_count=0`, `sender_subset_accept_without_pm_reserve_count=41`, and `result=failed_pm_reserve_gate`. The quarantine is now made explicit by `RunLS9D` (`/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9d_laserstream_backrun_sender_pm_summary.json`): `sender_execution_no_send_accept_count=0`, `sender_research_only_accept_count=41`, and `sender_blocked_pm_reserve_missing_count=41`. The important new correction is that the bundle lane is no longer waiting on a PM-core logic gap: after the `ObserveOnlyNoSend` release fix and a `2s` post-window drain in `20_g12_r0_observe_only.sh`, `RunLS9C` (`/home/ubuntu/Guerrila/g1-host-pack/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9c_laserstream_backrun_bundle_nosend_summary.json`) now shows `laserstream_backrun_bundle_seen=153`, `laserstream_backrun_bundle_pm_ok=153`, `laserstream_backrun_bundle_envelope_built=153`, `laserstream_backrun_bundle_no_send_accept=153`, `worker_pm_reservation_created=153`, `pm_reservation_released=153`, and `laserstream_backrun_bundle_blocker=none`. Operational rule: LaserStream Backrun no-send is proven, the **bundle side is now the first fully reconciled no-send monetization lane**, the Sender side stays research-only until PM reserve is honest, and `drop` is suppression-only.
- current LaserStream private-bundle live-readiness gate (`2026-05-24`): the refreshed `RunLS11` / `RunLS11D` / `RunAUTH0` chain is now saved at `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only`. The scoped window stayed safe (`run_status=completed`, `cleanup_verified=true`, `wire_send=0`, `live_landed=0`) and the selected no-submit artifact in `runls11_selected_backrun_bundle.json` now proves a real ordered no-submit bundle artifact on the Shadow side: `selection_result=selected_target_tx_ready`, `selected_route_family=jito_bundle`, `selected_target_tx_bytes_available=true`, `selected_target_tx_bytes_source=captured_at_ingress`, `bundle_envelope_built=true`, `bundle_tx_count=2`, `selected_backrun_tx_template=jito_plain`, `selected_expected_out_raw=10329482098`, `selected_min_out_raw=10226187277`, `selected_net_ev_after_tip=2484055`, and `selected_tip_lamports=27012004`. `RunLS11D` closes the target-capture question directly: `selected_target_tx_decodes_locally=true`, `selected_target_tx_decoded_len_bytes=743`, `selected_target_signature_matches=true`, `selected_target_pool_matches=true`, `selected_target_mint_in_matches=true`, `selected_target_mint_out_matches=true`, `selected_target_account_keys_count=15`, and `selected_target_program_ids_count=10`. Selected-scoped PM is also green: `selected_pm_reserve_created=true`, `selected_pm_reserve_released=true`, `selected_pm_release_reason=observe_only_no_send`, `selected_pm_pending_after=0`, and `pm_reserve_reconciled=true`. `RunLS11C` and `RunAUTH0` now agree on the network posture too: Jito reachability is present (`provider_submit_probe=http_404`, `provider_status_probe=http_405` count as reachable in no-submit mode), tip accounts are present, but the host is still auth-red with `provider_auth_material_present=false`, `provider=jito_direct`, `auth_source=jito_keypair`, and `jito_auth_path_present=false`. Operational rule after the refreshed LS11 chain: **LaserStream Backrun bundle remains the leading monetization branch, target-byte preservation is solved, and the next blocker before any controlled live bundle proof is host Jito auth staging.**
- current private-bundle provider matrix (`2026-05-24`): `RunAUTH1` now supersedes the direct-Jito-only blocker for the first proof route. The matrix artifacts are saved beside the LS11 selected bundle as `runauth1_private_bundle_provider_matrix.json`, `runauth1_private_bundle_provider_rows.jsonl`, and `runauth1_selected_provider.json`. The selected route is `jito_via_helius` with `provider_auth_source=helius_rpc_api_key`, `passing_provider_count=1`, `auth_material_present=true`, `tip_accounts_available=true`, `tip_accounts_count=11`, `status_api_reachable=true`, `simulate_bundle_available=true`, `send_bundle_disabled_in_test=true`, `send_bundle_called=false`, `wire_send=0`, and `live_landed=0`. The follow-up `RunLS11C` provider rerun against the selected Helius path is green for the live-readiness fields: `selected_target_tx_bytes_available=true`, `bundle_envelope_built=true`, `bundle_tx_count=2`, `tip_present=true`, `min_out_positive=true`, `net_ev_after_tip_positive=true`, `selected_pm_reserve_created=true`, `selected_pm_reserve_released=true`, `provider_auth_material_present=true`, `provider_status_api_reachable=true`, `simulate_bundle_available=true`, `send_bundle_disabled_in_mode=true`, `wire_send=0`, and `live_landed=0`. Direct Jito auth remains absent and should be treated as an optional later latency/route improvement, but `RunLS12` is not approved from this artifact because the later `RunLS11E` freshness gate blocks it.
- current target freshness / bundle-eligibility gate (`2026-05-24`): `RunLS11E` is now implemented as `g1-host-pack/scripts/21_runls11e_target_freshness_bundle_eligibility.sh` and must pass before any live private-bundle proof. The script writes `runls11e_target_freshness_bundle_eligibility.json` and `runls11e_target_freshness_bundle_eligibility_rows.jsonl`, and exits nonzero unless the selected target is unlanded, blockhash-valid, first in the bundle, followed by our backrun tx, tipped, and simulation-passing. The current `.15` artifact fails correctly with `selection_result=blocked_target_already_landed`; it still has `target_tx_bytes_available=true`, `target_tx_decoded_len_bytes=743`, `bundle_tx_count=2`, `target_tx_first=true`, `our_backrun_tx_second=true`, and `tip_present=true`, but RPC reports the target as `confirmationStatus=finalized`, `target_already_landed=true`, `target_recent_blockhash_valid=false`, and `simulateBundle_pass=false`. `sendBundle_called=false`, `wire_send=0`, and `live_landed=0`. Operational rule: LS12 remains blocked until the target source is upgraded to LaserStream preprocessed transactions or AF_XDP target trigger plus LaserStream enrichment, then LS11E must pass on a fresh selected candidate.
- current preprocessed target proof (`2026-05-24`): `RunLS13` now exists as `g1-host-pack/scripts/21_runls13_laserstream_preprocessed_target_freshness.sh` plus the standalone `runls13-preprocessed-probe` binary. It is no-submit by construction and writes `runls13_preprocessed_target_freshness_summary.json` plus `runls13_preprocessed_target_freshness_rows.jsonl` under `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls13_laserstream_preprocessed_target_freshness/window1/`. The endpoint/key posture is now corrected and verified: LaserStream gRPC uses the dashboard URL exactly (`https://laserstream-mainnet-ewr.helius-rpc.com`) and sends the API key as gRPC metadata `x-token`, not as a URL query string. The new direct LS13 diagnostic reaches the endpoint and fails at the initial `SubscribePreprocessed` RPC with `error_code=PermissionDenied`, `error_message="Unsupported plan type"`, and `error_stage=subscribe_preprocessed_initial_request`; that same direct error appears for selected-pool, no-filter, and JUP-style filters. Treat this as a Helius plan/entitlement blocker for Shred Delivery / preprocessed transactions, not as a malformed URL, market evidence, or Shadow strategy failure. `sendBundle_called=false`, `wire_send=0`, and `live_landed=0` stayed intact. Next step is to enable/upgrade Helius preprocessed transaction access for this key, then rerun LS13; only return to LS11E/LS12 after LS13 captures an unlanded, blockhash-valid target. If that product access is not available, use the fallback branch: AF_XDP target trigger plus LaserStream enrichment.
- current preprocessed target proof after Helius Professional upgrade (`2026-05-29`): the entitlement blocker is now cleared on `.15`. Fresh `RunLS13` artifacts live under `/home/ubuntu/Guerrila/g1-host-pack/results/21_runls13_laserstream_preprocessed_target_freshness_prof/`. The selected-context reruns (`window1` at `30s`, `selected120_diag` at `120s`) both show `direct_subscribe_probe.connect_ok=true`, `initial_request_ok=true`, `error_code=null`, and `preprocessed_subscription_connect_failed=false`, which means `SubscribePreprocessed` now succeeds for this key. Those selected windows still fail closed with `selection_result=blocked_no_preprocessed_target_seen`, `preprocessed_target_seen=false`, and `stream_error=stream_window_elapsed`, so the old selected Backrun account filter simply did not see a matching target in the bounded windows. The unfiltered diagnostic (`nofilter_diag`, `60s`) proves the feed is live and fresh-capable: `preprocessed_target_seen=true`, `target_tx_bytes_available=true`, `target_blockhash_valid=true`, `target_already_landed=false`, `target_tx_first=true`, `signature_status_rpc_reachable=true`, and `simulate_bundle_rpc_method_available=true`, while `sendBundle_called=false`, `wire_send=0`, and `live_landed=0` remain intact. Treat the next Backrun blocker as selection/join work on a live preprocessed feed, not plan entitlement, malformed URL, or provider auth.
- current preprocessed-native selector / ALT / projection chain (`2026-05-29`): the first `RunLS13A` / `RunLS13B` unfiltered pass on `.15` is now historical context, not the freshest blocker read. That first `90s` nofilter window produced `preprocessed_raw_tx_seen_count=6`, `target_not_landed_at_first_check_count=6`, `target_entity_ready_count=0`, `selector_decision_counts={"UNSUPPORTED_PROGRAM":6}`, and `status=PARK_SELECTOR_TOO_NARROW`. Follow-on `RunLS13U` unsupported-program census plus `RunLS13ALT` lookup-table hydration repaired that blocker on a fresh `120s` nofilter host run: `preprocessed_raw_tx_seen_count=8`, `target_not_landed_at_first_check_count=7`, `supported_program_candidate_count=4`, `rows_with_alt_lookups=3`, `lookup_table_fetch_ok_rows=3`, `alt_resolution_ready_rows=3`, `alt_resolution_blocked_count=0`, and `RunLS13B` now reports `target_entity_ready_count=3`, `selector_decision_counts={"TARGET_ENTITY_READY":3,"ALREADY_LANDED":1,"UNSUPPORTED_PROGRAM":2,"TX_DECODE_FAILED":2}`, `ready_venue_counts={"pumpfun_amm":3}`. `PREBR0` then moved the chain to the next gate with `projected_post_target_state_count=1`, `projected_expected_out_positive_count=1`, `projected_min_out_positive_count=1`, and `projected_ev_after_tip_positive_count=1`; the inventory-aware executable-shape filter correctly parked that row at `status=PARK_EXECUTABLE_INPUT_NOT_HELD`, with `projected_ev_positive_held_input_rows=0`, `projected_ev_positive_core_input_rows=0`, and `projected_ev_positive_unheld_token_rows=1`. Treat ingest, selector, ALT, and first projection as green. The current Backrun blocker is executable input shape / held inventory, not source timing, provider auth, or bundle-shape plumbing.
- current Pump.fun R1 branch and exit-size lesson (`2026-05-29`): `PREBRPF` proved direct Pump.fun route support and `21_lsprebrinv_r1_one_mint_proposal.sh` produced an honest `R1_PROPOSAL_READY_ATA_PRECREATE_ONLY` package for mint `HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w`, pool `HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp`, with ATA rent explicitly priced. One ATA was precreated, the no-send proposal upgraded to `R1_PROPOSAL_READY_ONE_MINT_LIVE`, and the live executor then correctly aborted pre-send because the hypothetical exit quote was zero. The subsequent size-ladder / autopsy proved all tested R1 sizes `0.005,0.01,0.025,0.05,0.1` ended `SIZE_EXIT_ZERO` with `reason_expected_out_zero=integer_rounding_dust_under_zero_fee_model`. The empty HXn ATA was then closed and rent recovered. Operational rule from here: do not precreate an ATA or promote an inventory-band proposal unless the exact intended size band has `exit_expected_out_raw > 0`, `exit_min_out_raw > 0`, and `exit_non_dust=true`.
- current Backrun factory status (`2026-05-29`): the first long R1 factory rooted at `/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_20260529T114758Z` is invalid as a market-density verdict. It launched child stages outside its run root and later exited in the direct-route probe with `IsADirectoryError: '.'`. `21_lsprebrinv_r1_candidate_factory.sh` now pins all child stage outputs under one run root, and `21_lsprebrpf_pumpfun_amm_direct_route_probe.sh` now hard-fails missing upstream paths instead of falling through to `Path(".")`. The fixed `.15` smoke at `/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_fix_smoke_20260529T_host` completed end-to-end with `proposal_ready_rows=1`, `ata_precreate_only_ready_rows=1`, `one_mint_live_ready_rows=0`, `projected_ev_positive_unheld_token_rows=1`, `wire_send=0`, and `live_landed=0`. A fresh fixed `6h` no-send rerun was then launched at `/home/ubuntu/Guerrila/g1-host-pack/results/run_lsprebrinv_factory_6h_fix_20260529T124709Z`. Treat the current Backrun question as surface density / conversion rate, not an ingest-plumbing failure.
- current Launch Continuation lane (`2026-05-29`, historical no-send baseline): the new launch-follow / new-mint momentum R1 lane now exists as `22_launch0_new_mint_pool_census.sh` through `22_launch5_one_mint_live_execute.sh`, with exact compatibility entrypoints `22_launch0_preprocessed_birth_census.sh`, `22_launch1_buy_prefix_score.sh`, and `22_launch2_bonding_curve_entry_exit_ladder.sh`, plus the no-send factory wrapper `22_launchf_launch_continuation_candidate_factory.sh`. The lane is explicitly a tail-selection strategy, not a blind-buy strategy: it requires buy-prefix evidence, policy/creator-risk screening, exact-size nonzero exit ladders, PM reserve, ordered-entry bundle proof, and auto-flatten readiness before proposal. The rewired `.15` `120s` smoke at `/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_smoke_20260529T_rewire` completed the full `22_launch0 -> 22_launch4_entry_bundle_nosubmit -> 22_launch4_one_mint_live_proposal` funnel under one run root. That short window saw `launches_seen=1`, `buy_prefix_ready_rows=0`, `exit_nonzero_count=1`, `bundle_rows=1`, `entry_bundle_ready_no_submit_rows=0`, `proposal_ready_rows=0`, `one_mint_live_ready_rows=0`, and `first_blocker=buy_count_2s_below_threshold`, with `wire_send=0` and `live_landed=0`. The short threshold sweep on that smoke confirmed the same conclusion under all tested profiles (`r1_current`, `r1_5s3`, `r1_looser`): `buy_count_1s_ge_1_rows=1`, `buy_count_2s_ge_2_rows=0`, `buy_count_5s_ge_3_rows=0`, `unique_buyers_2s_ge_1_rows=1`, `unique_buyers_5s_ge_2_rows=0`, and `ready_rows=0`. Interpretation at that point: downstream ordered-entry bundle/proposal plumbing was real, and the short-window blocker was upstream buy-prefix density rather than broken later-stage wiring. `22_launch5_one_mint_live_execute.sh` is launch-native: live entry can be built as an ordered bundle `[target buy, our buy, tip]` through the Helius/Jito bundle endpoint, and flatten remains Sender-based. The launch live executor also enforces a per-day risk ledger (`SHADOW_R1_DAILY_*`): default `$60` profit-stop, `$25` loss-stop, `12` executed trades max, no new entry while any prior open exposure remains unresolved, and **live abort if `SHADOW_R1_DAILY_SOL_USD` is unset or nonpositive**. Historical long no-send launch factory: `/home/ubuntu/Guerrila/g1-host-pack/results/run_launchf_factory_6h_v2_20260529T164626Z`; this is superseded by the 2026-06-01 R2 live classification below.
- current Launch R2 live classification (`2026-06-01`): the no-send-only Launch read is superseded by `LAUNCH_R2_LIVE_ACQUIRED_RESCUE_FLATTENED` from `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_live_20260601T061754Z`. This proved signer correctness, landed live acquisition, inventory refresh, rescue flatten, and ledger return-to-flat. It did not prove clean primary flatten, and it was not a strategy-duration test: classify it as `technical_status=LIVE_ENTRY_AND_RESCUE_EXIT_PROVEN`, `strategy_status=NOT_A_STRATEGY_DURATION_TEST`, and `pnl_status=SHORT_VALIDATION_COST_DOMINATED`. Next proof is `R3_MONITORED_POSITION_PRIMARY_FLATTEN`: hold after entry, monitor adverse-move metrics, trigger exit from configured deterioration/TTL/kill logic, and land primary flatten without rescue.
- current Launch0Q coverage milestone (`2026-06-02`): record `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z` as `LAUNCH0Q_QUOTE_STATE_HYDRATION_NO_WIRE_PASS`, not live-ready. The realtime Launch coverage path now hydrates executable quote/liquidity state and clears `ENTRY_QUALITY_MISSING_QUOTE_STATE`: `unique_launch_states=566`, `trigger_b_rows=343`, `quote_state_ready_rows=166`, `exit_liquidity_guard_ready_rows=166`, `entry_quality_ready_rows=7`, `entry_edge_ready_rows=6`, `fragility_absorption_ready_rows=165`, `wire_send=0`, `live_landed=0`. Current blockers are `QUOTE_ROUTE_UNSUPPORTED` and `entry_quality_warmup`; next no-wire gate is `22_launch0r_route_support_and_warmup_reconcile.sh`.
- current Launch0R coverage milestone (`2026-06-02`): record the same 600s coverage root as `LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE_PASS`. This is still no-wire: `wire_send=0`, `live_landed=0`, `acceptance_pass=true`. It decomposes `QUOTE_ROUTE_UNSUPPORTED=333` entirely into `PUMP_BONDING_CURVE`, split as `PUMP_BONDING_CURVE_BUY=212` and `PUMP_BONDING_CURVE_SELL=121`; warmup blockers split as `PUMP_BONDING_CURVE=230` and `PUMPSWAP_AMM=19`. Current primary blocker is `PUMP_BONDING_CURVE_QUOTE_SUPPORT`; secondary blocker is `WARMUP_CALIBRATION`. Next no-wire gate is `22_launch0s_pump_bonding_curve_quote_support.sh`. Do not resume R3 live or finalize warmup calibration until Pump bonding-curve buy/sell quote hydration exists and entry quality remains blocked without exit quote support.
- current Launch0S coverage milestone (`2026-06-02`): record `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T105145Z` as `LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE_PASS`. Safety stayed closed: `wire_send=0`, `live_landed=0`, `acceptance_pass=true`. Pump bonding-curve quote hydration now reports `pump_bonding_curve_buy_quote_rows=53`, `pump_bonding_curve_sell_quote_rows=38`, `pump_bonding_curve_ready_rows=44`, and `quote_route_unsupported_pump_bonding_curve_after=0`; full downstream counts are `quote_state_ready_rows=205`, `entry_quality_ready_rows=19`, `entry_edge_ready_rows=11`, and `fragility_absorption_ready_rows=202`. Remaining Pump bonding-curve blockers are route-specific (`BONDING_CURVE_COMPLETE_OR_MIGRATED`, `BONDING_CURVE_STATE_MISSING`, `BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR`, `BONDING_CURVE_RESERVES_MISSING`, `BONDING_CURVE_PRICE_IMPACT_TOO_HIGH`). Next no-wire gate is Launch0T warmup calibration replay; live remains paused.
- current Launch0T warmup milestone (`2026-06-02`): record the same 600s coverage root as `LAUNCH0T_WARMUP_CALIBRATION_REPLAY_PASS`. Safety stayed closed: `wire_send=0`, `live_landed=0`, `acceptance_pass=true`. Warmup replay reports `best_entry_bucket=10-30s`, `normal_warmup_min_age_ms=10000`, `entry_quality_ready_rows=32`, `entry_edge_ready_rows=18`, and `exit_liquidity_guard_ready_rows=205`. This is a Launch R3-HOLD warmup rule, not sub-second scalp validation. Early override requires quote state, exit liquidity, entry edge, nonzero exit quote, and price-impact limits all green. Top non-ready blocker is `BONDING_CURVE_COMPLETE_OR_MIGRATED=171`; next no-wire gate is `LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN`.
- current Launch0M / Launch0M3 migration milestone (`2026-06-02`): record `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_600s_20260602T135525Z` as `LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN_PASS`, still no-wire: `wire_send=0`, `live_landed=0`, `acceptance_pass=true`. Launch0M2 proved `completed curve != migrated route`: `CURVE_COMPLETE_NOT_MIGRATED=394`, `PARK_MIGRATION_POOL_NOT_FOUND=1`, and no PumpSwap pool joined from the completed-only bucket. Launch0M3 then watched actual migration activity and passed with `migrate_instruction_seen_count=182`, `pumpswap_pool_joined_count=8`, and `pumpswap_quote_state_ready_rows=8`. The patched Launch0M consumed those migration-watcher rows and produced `MIGRATION_PUMPSWAP_ROUTE_READY=5`, `PARK_PUMPSWAP_BASE_RESERVE_BELOW_FLOOR=3`, `pumpswap_quote_state_ready_rows=5`, and `pumpswap_exit_liquidity_guard_ready_rows=5`. Operational rule: active bonding-curve launches use the bonding-curve route; completed-not-migrated curves are parked; PumpSwap-after-migration is valid only after a migration event/instruction/PDA/account-scan pool join proves the pool.
- current Launch0U active-curve gate (`2026-06-02`): `22_launch0u_active_bonding_curve_r3_candidate_gate.sh` is implemented and no-wire. On the fresh 600s root it reports `status=PARK_ACTIVE_CURVE_WARMUP_NOT_READY`, `active_bonding_curve_rows=1137`, `exit_liquidity_guard_ready_rows=107`, `entry_edge_ready_rows=2`, `active_curve_r3_ready_rows=0`, `wire_send=0`, and `live_landed=0`. This is a correct park, not a route failure: the only edge-ready active-curve rows were outside the Launch0T `10-30s` warmup rule. Resume R3 only after a no-wire probe produces either an active bonding-curve candidate inside the warmup/early-override rules or a migration-proven PumpSwap candidate with entry quality, exit liquidity, and fragility gates all green.
- current Launch0V / Launch0U2 candidate conversion gate (`2026-06-02`): `22_launch0v_migrated_pumpswap_r3_candidate_gate.sh` and `22_launch0u2_active_curve_warmup_blocker_audit.sh` are implemented and no-wire. On the fresh 600s root, Launch0V sees `migrated_pumpswap_rows_seen=5`, `quote_state_ready_rows=5`, `exit_liquidity_guard_ready_rows=5`, `true_loss_floor_lamports=12410000`, PM/account/cash rows green, but `entry_quality_ready_rows=0`, `entry_edge_ready_rows=0`, `r3_ready_rows=0`, first blocker `entry_quality_last_trade_not_buy`; status is `PARK_MIGRATED_PUMPSWAP_ENTRY_EDGE`. Launch0U2 explains the active curve park: `active_bonding_curve_rows=1137`, `quote_not_ready_rows=1030`, `entry_edge_weak_rows=105`, `warmup_age_not_ready_rows=2`, `pm_not_ready_rows=0`, `cost_floor_not_ready_rows=0`, and `active_curve_r3_ready_rows=0`. Continue no-wire. Live R3 resumes only when Launch0V or Launch0U emits R3-ready rows.
- current Launch0U3 / Launch0V2 blocker refinement (`2026-06-02`): `22_launch0u3_active_curve_quote_blocker_audit.sh` and `22_launch0v2_migrated_pumpswap_momentum_recheck.sh` are implemented and no-wire. Active curve quote-not-ready is now split: `quote_not_ready_rows=1030`, `top_quote_blocker=BONDING_CURVE_STATE_MISSING`, `BONDING_CURVE_STATE_MISSING=882`, `BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR=141`, `BONDING_CURVE_PRICE_IMPACT_TOO_HIGH=6`, and `BONDING_CURVE_RESERVES_MISSING=1`; `QUOTE_ROUTE_UNSUPPORTED=0`, `MINT_OR_CURVE_MISSING=0`. Migrated PumpSwap route-ready rows still do not resume live because momentum context is missing: `route_ready_rows=5`, `momentum_ready_rows=0`, `momentum_state_missing_rows=5`, `toxic_sell_rows=0`, `wire_send=0`, and `live_landed=0`. Next active-curve repair is bonding-curve state lookup/hydration. Next migrated PumpSwap repair is carrying live flow fields into the migration join: `last_trade_side`, buy/sell ages, net pressure, sell quality, absorption score, and price-change-since-migration.
- current Launch0U4 / Launch0V3 hydration repair (`2026-06-02`): `22_launch0u4_active_curve_state_hydration_repair.sh` and `22_launch0v3_migrated_pumpswap_flow_state_hydrator.sh` are implemented and no-wire. U4 tried active-curve state repair with canonical Pump bonding-curve PDA fallback and preserved existing non-state-missing classifications; it did not improve the leak: `BONDING_CURVE_STATE_MISSING` stayed `882 -> 882`, `quote_ready_rows` stayed `107`, and downstream active-curve R3 remained `active_curve_r3_ready_rows=0`. Treat this as upstream active-curve account identity/source-decode failure, not quote math. V3 consumed `flow_rows_seen=424081` but matched `0` of the `5` route-ready migrated PumpSwap rows, leaving `momentum_state_missing_rows=5`; treat this as missing flow-source join for migration-discovered PumpSwap pools. Continue no-wire; no Batch008/live R3 until one branch emits R3-ready rows.
- current Launch0U5 / Launch0V4 identity-join repair (`2026-06-02`): `22_launch0u5_active_curve_account_identity_autopsy.sh` and `22_launch0v4_migrated_pumpswap_flow_join_repair.sh` are implemented and no-wire. U5 added transaction-aware Pump instruction account-set inference. It reduced the false fixed-index mismatch bucket from `PARK_PDA_DERIVE_MISMATCH=877` to `96`, but did not convert the branch: `identity_ready_rows=107`, `BONDING_CURVE_STATE_MISSING` moved only `882 -> 881`, `quote_ready_rows` stayed `107`, and downstream active R3 stayed `active_curve_r3_ready_rows=0`. Dominant active blocker is now `PARK_ACCOUNT_NOT_FOUND=785`, so treat active curve as stale/unavailable account-state or source-row age/identity drift. V4 repaired the migrated PumpSwap flow-join taxonomy: `route_ready_rows=5`, `flow_join_decision_counts={"PARK_MOMENTUM_STATE_MISSING": 5}`, `first_flow_join_blocker_counts={"momentum_not_buy_led": 5}`, and migrated R3 stayed `r3_ready_rows=0`. Continue no-wire; no live R3 until active curve or migrated PumpSwap emits R3-ready rows.
- current Launch0U6 / Launch0U7 / Launch0V5 identity-momentum gate (`2026-06-02`): record the no-wire follow-up as `LAUNCH0U6_U7_V5_IDENTITY_MOMENTUM_NO_WIRE_PARKED`. Safety stayed closed: `wire_send=0`, `live_landed=0`. U6 split the active-curve account-state/source truth: `account_not_found_rows=785`, `ACCOUNT_FOUND_AFTER_IX_ACCOUNT_JOIN=106`, `ACCOUNT_NOT_FOUND_WRONG_PDA=97`, and stale/non-live state remains dominant (`ACCOUNT_NOT_FOUND_STALE_STATE=934` across the classified active set). U7 showed the `107` identity-ready active rows are blocked by signal quality, not execution budget: `active_curve_r3_ready_rows=0`, `ENTRY_EDGE_WEAK=105`, `WARMUP_NOT_READY=2`, with `EXIT_LIQUIDITY_BLOCKED=0`, `COST_FLOOR_BLOCKED=0`, `PM_BLOCKED=0`, and `FRAGILITY_BLOCKED=0`. V5 watched migrated PumpSwap for 900s at `/home/ubuntu/Guerrila/g1-host-pack/results/run_launch0rt_realtime_birth_rate_probe_900s_20260602T165046Z`; it preserved `route_ready_rows=5` but found `momentum_ready_rows=0`, `migrated_pumpswap_r3_ready_rows=0`, and `flow_join_decision_counts={"PARK_MOMENTUM_STATE_MISSING": 5}` with status `PARK_MIGRATED_PUMPSWAP_MOMENTUM_NOT_BUY_LED`. Live remains paused; next active work is fresh-state filtering/enhanced-create join plus entry-edge calibration, while the migrated route set remains watchable until buy-led momentum appears.
- current hybrid Backrun fallback (`2026-05-24`): that fallback is now implemented as `RunLS14`, `g1-host-pack/scripts/21_runls14_hybrid_backrun_join_proof.sh`. The code path preserves AF_XDP-decoded target raw transaction bytes in `shadow_core::target_tx_cache`, while standard processed LaserStream remains the enrichment/discovery side. The wrapper runs no live send and emits `runls14_hybrid_backrun_join_rows.jsonl` plus `runls14_hybrid_backrun_join_summary.json`; a green `selection_result=hybrid_backrun_join_ready_for_ls11e` only authorizes rerunning `RunLS11E` against the AF_XDP-captured fresh target artifact. `RunLS12` remains blocked unless LS11E proves the target is unlanded, blockhash-valid, first in bundle, tipped, and PM-reconciled.
- historical AF_XDP-only Backrun projection blocker (`2026-05-24`, superseded): `RunLS15` and early `RunAFBR1` proved processed LaserStream enrichment was too late and exposed the first AF_XDP local projection blocker: target relationship / target entity / projected quote was missing. That was an architecture blocker then, not an egress/auth/PM gap. This has since been repaired through the AFBR1 target-entity/projection chain and AFBR-H now reports `BACKRUN_STRUCTURAL_HUNT_READY`.
- current AFBR1H Backrun freshness blocker (`2026-05-26`): `21_afbr1h_immediate_bundle_ready_freshness_batch.sh` is now the authoritative Backrun live-track distribution proof. It runs native AF_XDP zero-copy, emits tiny `/dev/shm` hot bundle-ready rows before heavy artifact build, and probes target status immediately in no-submit mode. Latest batch: `status=SOURCE_LATE_DOMINANT`, `valid_afbr1h_sample_count=10`, `freshness_class_counts={"SOURCE_LATE":10}`, `fresh_at_bundle_ready_count=0`, `target_seen_to_bundle_ready_us_p50=148`, p90 `16017`, p99 `30562`, `bundle_ready_to_status_start_us_p50=997`, p90 `1377`, p99 `1587`, `sendBundle_called=false`, `wire_send=0`, and `live_landed=0`. Treat Backrun as structurally ready but `.15` AF_XDP fresh-target/source-timing gated.
- Sender status (`2026-05-23`): `helius_sender_dual` is now a host-proven no-send egress lane for plain single-transaction private flow, while bundle-only / ordered flow stays on `jito_bundle`; Sender is parked until route-specific EV or first live Sender proof is intentionally resumed

- Launch B002 frozen money lane (`2026-06-03`): Batch002 is now a separate golden Launch lane, not part of the R4 lab stack. The freeze manifest is `/home/ubuntu/Guerrila/g1-host-pack/results/22_launch_b002_freeze_manifest/manifest.json`; the replay is `/home/ubuntu/Guerrila/g1-host-pack/results/22_launch_b002_replay_frozen/summary.json`; the first green regression matrix is `/home/ubuntu/Guerrila/g1-host-pack/results/22_launch_b002_regression_matrix_20260603T070540Z/summary.json`. Golden result: `attempts_landed=3`, `attempts_flattened=3`, `attempts_rent_recovered=3`, all landed exits `EXIT_COST_PROTECTOR_TRAILING`, `total_pnl_after_rent_recovery=4527956`, `no_exit_tail_count=0`, `manual_intervention_required_count=0`, `open_exposure_count=0`, `pm_pending_after=0`. Use `22_launch_b002_live_clone_runner.sh` for any future frozen live clone; use `22_launch53_r3_batch002_frozen_runner.sh` as the exact Batch002-style runner. Do not route money through `22_launch46_r4_b002_clone_runner.sh` or generic R4 unless the lab lane first beats B002 and passes `22_launch_b002_regression_matrix.sh`.

Latency artifact note:

- `shadow-tier0` now also writes `g12_latency_by_source.json` next to the existing gate files when rollout gate export is enabled;
- it exports `public_geyser`, `external_geyser`, `local_validator_geyser`, `afxdp`, `dpdk`, and aggregate `triton_grpc` latency snapshots;
- comparable stages are `source_to_feed`, `source_to_worker`, `source_to_process_intent`, `source_to_nosend`, and `source_to_final_shadow_event`, plus the older stage-local buckets;
- gRPC source-to-worker / source-to-no-send is measurable immediately, but DPDK source-to-* will remain empty until DPDK emits real `IngressRecord`s.

If `.221` is being retired in favor of a newly prepared host, preserve before shutdown:

- `~/shadow-host-audit/`
- `/home/ubuntu/Guerrila/run/g12_gate_input.json`
- `/home/ubuntu/Guerrila/run/g12_gate_report.json`
- `/home/ubuntu/Guerrila/run/g12_latency_by_source.json` (if present)
- any temporary DPDK JSON/kernel-log artifacts captured under the host-audit tree
