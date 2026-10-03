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
- `SHADOW_GEYSER_ENDPOINT` — Triton gRPC URL for live state/account streaming only
- `SHADOW_GEYSER_TOKEN` — Triton auth token
- `SHADOW_GRPC_FALLBACK_WORKERS` — tier0 gRPC fallback ingest workers (`0` disables tier0 gRPC ingest while preserving `shadow-state` Triton state)
- `SHADOW_RPC_URL` — HTTP RPC for `shadow-state` bootstrap / hydration / targeted reads (default this to Helius, not Triton)
- `SHADOW_BOOTSTRAP_RPC_URL` — optional high-throughput HTTP override for bootstrap / hot-pool hydration (defaults to `SHADOW_RPC_URL`)
- `SHADOW_SOLANA_RPC_URL` — HTTP RPC for PM blockhash / signature-status polling (default this to Helius)
- `SHADOW_PUBLIC_RPC_URL` — HTTP RPC for explicit `public_rpc` `sendTransaction` (prefer Helius Gatekeeper when available)
- `SHADOW_NIC_IFACE` — NIC interface name (run `ip link` to find)
- `SHADOW_MONEY_CORES` — isolated cores for money workers (check `lscpu` for NUMA)
- Credential paths — copy keypairs to `/etc/shadow-engine/`

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
