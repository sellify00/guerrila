# G1 Bare-Metal Runbook and Handoff

**Project:** Shadow Engine V6.4-Guerrila  
**Scope:** G1 host procurement, host validation, real-host seal procedure for G4 and G5  
**Date:** 2026-04-17  
**Audience:** Any engineer or chatbot picking up host work later and needing full context fast

---

## 1. Purpose of this document

This file is the full G1 runbook and handoff for future bare-metal rentals.

It has two jobs:

1. preserve everything learned from the successful G1 closeout host session so nobody has to rediscover it under hourly billing;
2. provide a step-by-step procedure for provisioning, validating, running, sealing, archiving, and shutting down the next host.

This document is intentionally practical. It includes:

- what G1 was for;
- what was already complete before any host was rented;
- what must be prepared before renting a server;
- how to provision the server;
- how to set up SSH keys;
- what packages to install;
- what to verify immediately;
- how the host was tuned;
- what exact tests were run for G4 and G5;
- the final results from the successful host session;
- pitfalls and bugs found during the live session;
- what “done” looks like before shutting the host off.

---

## 2. What G1 was actually for

G1 was never about inventing more core engine logic on the live host. The software body for G4 and G5 was already complete enough that the only missing work was **real-host proof**:

- **G4**: the remaining open item was the **real-host ingress performance seal**;
- **G5**: the remaining open items were the **real Linux `strace` smoke** and the **real dequeue→egress microbench on a pinned core**.

That means the right G1 posture is:

- do all coding and harness prep off-host first;
- rent the box only when the host-facing scripts and binaries are ready;
- use paid time only for host validation, measurement, seal runs, artifact collection, and shutdown.

---

## 3. High-level outcome of the successful G1 session

The successful host session established the following working project state:

- **G1**: DONE
- **G4**: DONE
- **G5**: DONE

The host session proved:

- the machine was suitable for Shadow Engine’s real-host requirements;
- the worker hot path easily satisfied the 150 µs p99 target;
- the G5 runtime `strace` seal passed;
- the G4 ingress seal passed all four scenarios on the rented bare-metal host session.

### 3.1 Successful host identity

The successful host snapshot was:

| Property | Value |
|---|---|
| Hostname | `g1-host-a` |
| Public IP | historical G1 host address |
| Region | Tokyo 4 |
| CPU | AMD EPYC 9254, 24C/48T, single socket |
| RAM | 386 GB |
| NUMA | 1 node (node 0) |
| Kernel | `6.8.0-106-generic` |
| OS | Ubuntu LTS (`22.04+` reported on-host) |
| NICs | dual Intel X710 / `i40e` |
| Management NIC | `eno1` |
| Test NIC | `eno2` |
| RX timestamp capability | present |
| PTP raw clock | present |

### 3.2 Final on-host seal results

#### G5

- Linux `strace` runtime proof: **PASS**
- dequeue→egress microbench: **PASS**
  - best: **2130 ns**
  - p50: **2420 ns**
  - p95: **3280 ns**
  - p99: **8979 ns**
  - worst: **8979 ns**
  - target: **p99 ≤ 150,000 ns (150 µs)**

#### G4

All four scenarios passed:

| Scenario | Frames | Parse | Data | Coding | FEC Recovered | Archive OK | Archive Drop | Recycle | Verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Baseline | 128 | 100% | 64 | 64 | 64 | 128 | 0 | 128 flush | PASS |
| Reorder | 128 | 100% | 64 | 64 | — | 128 | 0 | 128 flush | PASS |
| FEC-degraded | 124 | 100% | 60 | 64 | 48 | 124 | 0 | 124 flush | PASS |
| Archive pressure | 6022 | 100% | 3014 | 3008 | — | 32 | 5990 | 6022 flush | PASS |

Archive pressure ran at roughly **401 PPS** with ring cap `32`, proving non-blocking ingress under backpressure in the seal harness.

### 3.3 Final project test counts recorded at the end of the G1 closeout session

| Crate | Tests | Status |
|---|---:|---|
| shadow-core | 385 | PASS |
| shadow-ipc | 245 (7 ignored) | PASS |
| shadow-ingest | 152 | PASS |
| shadow-state | 152 | PASS |
| shadow-tier0 | 575 | PASS |
| shadow-harmonic | 177 | PASS |
| shadow-egress-jito | 104 (1 ignored) | PASS |
| shadow-guard | 13 | PASS |
| **Total** | **1,803** | **0 failures** |

---

## 4. What must already be prepared before renting a host

Do **not** rent a host until all of the following exist locally and are ready to copy over.

### 4.1 Required off-host preparation

1. **Release artifacts** already built, preferably off-host.
   - Use the `release-guerrila` profile.
   - Prefer per-crate builds only.
   - Do **not** rely on workspace-wide builds on the rented server.

2. **Host pack** ready to upload.
   - configs
   - scripts
   - runbook
   - seal harnesses
   - results directory skeleton

3. **G5 harnesses ready**
   - `strace` wrapper
   - dequeue→egress microbench with raw nanosecond output

4. **G4 harnesses ready**
   - AF_XDP / XDP metadata BPF object if needed
   - ingress replay harness
   - degraded-flow / FEC scenario harness
   - archive-pressure harness
   - result summarizer

5. **No missing code work**
   - If a harness is not ready, do not rent the host yet.
   - The host is for validation, not invention.

### 4.2 Suggested local directory layout before upload

```text
c:\Shadow Engine\Guerrila\
├── g1-host-pack/
│   ├── configs/
│   ├── scripts/
│   ├── results/
│   ├── g4_seal_results/
│   ├── G1_RUNBOOK.md
│   └── env.g1.sh
└── target/
    └── release-guerrila/
```

### 4.3 Build discipline

Use per-crate commands only. Do **not** use workspace-wide cargo commands or full `cargo clean`.

Example build pattern:

```bash
cargo build -p shadow-tier0 --profile release-guerrila
cargo build -p shadow-ingest --profile release-guerrila
cargo build -p shadow-state --profile release-guerrila
```

If helper binaries live elsewhere, build those explicitly the same way.

---

## 5. SSH key setup for future host rentals

### 5.1 Generate a dedicated keypair

#### PowerShell / Windows

```powershell
ssh-keygen -t ed25519 -f $HOME\.ssh\shadow_g1_baremetal -C "shadow-g1-baremetal"
Get-Content $HOME\.ssh\shadow_g1_baremetal.pub
```

#### Linux / macOS

```bash
ssh-keygen -t ed25519 -f ~/.ssh/shadow_g1_baremetal -C "shadow-g1-baremetal"
cat ~/.ssh/shadow_g1_baremetal.pub
```

### 5.2 What to paste into the provider UI

Paste the **public key** only.

Never paste the private key.

### 5.3 Provider UI choices that worked well

Use these choices unless there is a specific reason not to:

- **SSH key scope**: Project
- **Billing**: Hourly
- **Quantity**: 1
- **RAID**: No RAID
- **User data**: blank unless cloud-init is already tested and minimal
- **Hostname**: simple, for example `g1-host-a`
- **OS**: Ubuntu LTS (successful run used Ubuntu LTS on `6.8.0-106-generic`)

Reasoning:

- No RAID avoids unnecessary setup complexity.
- User-data failures are expensive to debug under hourly billing.
- The host should come up as simply and predictably as possible.

---

## 6. First login and immediate package installation

### 6.1 SSH in

The successful host used the `ubuntu` user.

```bash
ssh ubuntu@<SERVER_IP>
```

From Windows, if needed:

```powershell
ssh -i "$HOME\.ssh\shadow_g1_baremetal" ubuntu@<SERVER_IP>
```

### 6.2 Create working directories

```bash
mkdir -p ~/g1/{artifacts,configs,scripts,results}
```

### 6.3 Install packages used in the successful session

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential clang llvm pkg-config libelf-dev libbpf-dev libssl-dev \
  ethtool numactl pciutils jq curl wget git strace stress-ng bc \
  iproute2 net-tools bpftool linux-tools-$(uname -r)
```

If a package name differs on the current Ubuntu image, adjust minimally, but keep the overall toolset intact.

### 6.4 Optional quick sanity checks

```bash
which ethtool
which strace
which numactl
which bpftool
rustc --version || true
uname -a
```

---

## 7. Copy the prepared host pack and binaries to the server

### 7.1 Upload host pack

From the local machine:

#### PowerShell

```powershell
scp -i "$HOME\.ssh\shadow_g1_baremetal" -r .\g1-host-pack ubuntu@<SERVER_IP>:~/
```

#### Linux / macOS

```bash
scp -i ~/.ssh/shadow_g1_baremetal -r ./g1-host-pack ubuntu@<SERVER_IP>:~/
```

### 7.2 Upload release artifacts if not building on-host

```bash
scp -i ~/.ssh/shadow_g1_baremetal -r ./target/release-guerrila ubuntu@<SERVER_IP>:~/Guerrila/target/
```

Keep binaries where the runbook expects them. The successful host used:

```text
~/Guerrila/target/release-guerrila/
```

---

## 8. First 10-minute host quick verify (must happen before any heavy tuning)

Do **not** immediately mutate the machine. Capture baseline facts first.

### 8.1 Baseline capture

```bash
mkdir -p ~/g1/results/00_host_facts
cd ~/g1/results/00_host_facts

date -Is | tee date.txt
hostname | tee hostname.txt
uname -a | tee uname.txt
cat /etc/os-release | tee os-release.txt
lscpu | tee lscpu.txt
numactl --hardware | tee numactl-hardware.txt
lspci -nn | tee lspci-nn.txt
ip -br link | tee ip-br-link.txt
ip -br addr | tee ip-br-addr.txt
cat /proc/interrupts > interrupts.txt
```

### 8.2 Find the management NIC and the test NIC

Run:

```bash
ip route
ip route get 1.1.1.1
ip -4 addr show eno1
ip -4 addr show eno2
```

Successful host interpretation:

- `eno1` carried the default route and public IPv4
- `eno2` had no IPv4 address and was used as the dedicated test NIC

Rule for future runs:

- keep the default-route NIC as **management only**;
- use the other port as the **AF_XDP / ingress test NIC**.

### 8.3 Verify NIC identity and timestamp capabilities

Replace `<IFACE>` with the management NIC first, then the test NIC.

```bash
ethtool -i <IFACE>
ethtool -T <IFACE>
ethtool -k <IFACE>
ethtool -l <IFACE>
ethtool -g <IFACE>
cat /sys/class/net/<IFACE>/device/numa_node
```

### 8.4 What the successful host looked like

The successful host quick-verify showed:

- NIC driver: `i40e`
- NIC model: Intel X710 (`8086:15ff`)
- RX hardware timestamps: present
- raw PTP clock: present
- NUMA node: `0`
- queue headroom: good
- one NUMA node for all cores and NIC locality

That is the known-good baseline.

### 8.5 Abort criteria

Terminate the host early if any of these fail:

- wrong NIC family or unknown driver stack;
- no hardware RX timestamp capability;
- no workable AF_XDP / XDP path;
- broken NIC locality;
- provider image is hostile to the required tooling;
- management/test NIC split is awkward enough to waste time.

---

## 9. Endpoint path probe before deep tuning

Measure real path quality to the actual private-lane endpoints before doing more work.

### 9.1 Minimal Jito connect-time probe

```bash
mkdir -p ~/g1/results/03_endpoint_probe
cd ~/g1/results/03_endpoint_probe

for host in \
  tokyo.mainnet.block-engine.jito.wtf \
  mainnet.block-engine.jito.wtf \
  ny.mainnet.block-engine.jito.wtf \
  amsterdam.mainnet.block-engine.jito.wtf \
  frankfurt.mainnet.block-engine.jito.wtf
  do
    echo "=== $host ===" | tee -a jito-connect.txt
    for i in $(seq 1 10); do
      /usr/bin/time -f "%e" timeout 5 bash -lc "echo > /dev/tcp/$host/443" 2>> jito-connect.txt
    done
  done
```

### 9.2 Successful host result

The successful host was clearly closest to Tokyo:

| Endpoint | Approximate TCP connect |
|---|---:|
| `tokyo.mainnet.block-engine.jito.wtf` | ~7 ms |
| `mainnet.block-engine.jito.wtf` | ~50 ms |
| `ny.mainnet.block-engine.jito.wtf` | ~146 ms |
| `amsterdam.mainnet.block-engine.jito.wtf` | ~215 ms |
| `frankfurt.mainnet.block-engine.jito.wtf` | ~228 ms |

Use actual measurements on the current host. Do not assume Tokyo is best unless the probe says so.

---

## 10. Non-reboot shaping that was safe and useful

Do the non-reboot tuning first. Do **not** rush into a GRUB reboot unless there is a specific reason.

The successful G1 closeout was achieved **without** requiring reboot-based isolation tuning.

### 10.1 Governor to performance

```bash
sudo cpupower frequency-set -g performance || true

for cpu in /sys/devices/system/cpu/cpu[0-9]*; do
  [ -f "$cpu/cpufreq/scaling_governor" ] && echo performance | sudo tee "$cpu/cpufreq/scaling_governor" >/dev/null
done
```

Verify:

```bash
grep -H . /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | head
```

### 10.2 Reserve hugepages

Successful run used 4096 hugepages at 2 MB each (8 GB).

```bash
echo 4096 | sudo tee /proc/sys/vm/nr_hugepages
grep -i huge /proc/meminfo
```

### 10.3 Reduce test NIC to one combined queue if the AF_XDP harness requires it

Successful run set the test NIC to one combined queue.

```bash
sudo ethtool -L <TEST_IFACE> combined 1
sudo ethtool -l <TEST_IFACE>
```

### 10.4 What was intentionally not required to close G1

The successful session did **not** require final GRUB isolation to close G1. These were intentionally deferred:

- `isolcpus`
- `nohz_full`
- `rcu_nocbs`
- full IRQ steering plan
- SMT-off via reboot/firmware

Those are useful production-polish steps later, but they were not needed to close the host seals.

---

## 11. AF_XDP / XDP bringup checks

### 11.1 Pre-XDP inspection on the test NIC

```bash
mkdir -p ~/g1/results/02_nic_pre_tune
cd ~/g1/results/02_nic_pre_tune

sudo ethtool -g <TEST_IFACE> | tee ring-pre.txt
sudo ethtool -l <TEST_IFACE> | tee channels-pre.txt
sudo ethtool -k <TEST_IFACE> | tee features-pre.txt
sudo ethtool -S <TEST_IFACE> | tee stats-pre.txt
cat /proc/interrupts | tee interrupts-pre.txt
```

### 11.2 Successful host notes

On the successful host, the test NIC (`eno2`) had:

- ring buffers: `512 / 512` current, `8160` max;
- combined channels: `1` current for AF_XDP testing;
- `i40e` driver;
- XDP features supported.

### 11.3 Known-good bringup outcome

The successful run established:

- XDP program load succeeded;
- AF_XDP socket creation succeeded;
- UMEM allocation succeeded;
- XDP statistics were readable;
- zero corruption was observed in the AF_XDP functional proof.

---

## 12. G5 host seal runbook

G5 had two host-blocked proofs:

1. Linux `strace` smoke
2. dequeue→egress microbench on a pinned core

### 12.1 G5 `strace` seal

Run the dedicated wrapper, not an improvised command.

The successful proof used the runtime categories:

- `%network`
- `%clock`

**Important:** on the host used in the successful session, old `strace -e trace=clock` syntax had to be corrected to `trace=%clock` on the installed `strace` version.

#### Pass condition

- zero banned runtime network syscalls;
- zero banned runtime clock syscalls;
- structural scan above `#[cfg(test)]` also clean.

#### Successful result

- **PASS**

### 12.2 Dequeue→egress microbench

Use the corrected raw-nanosecond harness only.

**Do not** use any whole-test-process wall-clock harness.

#### Correct timing boundary

The correct scope was:

```text
SigDedupe + early_drop_gates + hydrate_route + build_intent + process_intent(→egress push)
```

#### Successful result

```text
Best:  2130 ns
p50:   2420 ns
p95:   3280 ns
p99:   8979 ns
Worst: 8979 ns
```

#### Official interpretation

- target = `p99 ≤ 150,000 ns`
- actual = `8,979 ns`
- result = **PASS**

### 12.3 Harness mistakes that must never be repeated

Two invalid measurements happened before the correct one:

1. **~145 ms** result
   - cause: timing around the entire cargo-test process
   - wrong because it included dynamic linker, mmap, tempdirs, test harness setup, cleanup

2. **~1.74 ms** result
   - cause: timer scope still included `seed_nonce_ring` crypto/setup work
   - wrong because it was not the intended dequeue→egress boundary

Future engineers must ignore those old numbers and trust only the raw-nanosecond dequeue→egress harness.

---

## 13. G4 real-host ingress seal runbook

### 13.1 What G4 had to prove

The real-host ingress seal had to prove all of the following:

- AF_XDP live path works in the real host session;
- canonical shred parser path is exercised;
- reorder/reassembly path works;
- degraded FEC path recovers as expected;
- timestamps are tracked correctly;
- archive backpressure does not block ingress progress.

### 13.2 Harness structure used in the successful session

The successful G4 closeout used a dedicated seal harness with saved results under:

```text
g1-host-pack/g4_seal_results/
```

The final server state recorded for that run included:

- veth pair: `seal_rx` ↔ `seal_tx` in namespace `seal_ns`
- XDP program loaded on `seal_rx`
- XSKMAP pinned at `/sys/fs/bpf/shadow_xsks_map`
- binaries under `~/Guerrila/target/release-guerrila/`

### 13.3 The four required scenarios

Run all four and archive each scenario separately:

1. **Baseline**
2. **Reorder**
3. **FEC-degraded**
4. **Archive pressure**

### 13.4 Successful final results

| Scenario | Frames | Parse | Data | Coding | FEC Recovered | Archive OK | Archive Drop | Recycle | Verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Baseline | 128 | 100% | 64 | 64 | 64 | 128 | 0 | 128 flush | PASS |
| Reorder | 128 | 100% | 64 | 64 | — | 128 | 0 | 128 flush | PASS |
| FEC-degraded | 124 | 100% | 60 | 64 | 48 | 124 | 0 | 124 flush | PASS |
| Archive pressure | 6022 | 100% | 3014 | 3008 | — | 32 | 5990 | 6022 flush | PASS |

### 13.5 Archive pressure interpretation

The successful archive-pressure run reached about **401 PPS** with ring capacity `32` and still demonstrated non-blocking ingress behavior in the seal harness.

### 13.6 Bugs found and fixed during the successful G4 host run

These are worth preserving because they will save time later:

1. **`socat` silent failure inside netns**
   - fix: switched to bash `/dev/udp` delivery for reliable cross-namespace UDP send

2. **Debug dump slice panic**
   - symptom: `range start 496 out of range for slice len 48`
   - fix: use `frame_size=2048` with proper bounds in metadata dump
   - commit: `590bca7`

3. **Replay sender off-by-one**
   - symptom: 127/128 packets sent
   - cause: `break` occurred before `send`
   - fix: move send before index check
   - commit: `5036ad0`

4. **Ethernet/IP/UDP headers were not stripped before shred parse**
   - symptom: 0% parse rate
   - cause: AF_XDP delivered full Ethernet frames
   - fix: add `strip_udp_headers()` before `parse_shred()`
   - impact: real production bug fixed
   - commit: `0591f2f`

5. **`observe_rx_timestamp` never fired**
   - symptom: timestamp min/max remained sentinel values
   - cause: call was incorrectly gated behind parse success
   - fix: move it to fire unconditionally for all frames
   - commit: `0591f2f`

6. **Archive pressure showed zero recycles**
   - symptom: false FAIL
   - cause: `DelayFifo` depth too large to prime in the short run
   - fix: use `survival_flush_count` as valid recycle evidence in pressure verdict logic
   - commit: `508bbe1`

7. **Git author identity missing on server**
   - fix: configured `user.email` and `user.name` on host

### 13.7 Session commits recorded during the host G4 seal run

```text
f7f546c  temp debug tracing to poll_and_drain
590bca7  fix debug dump slice panic in metadata read
c4fce1e  revert debug tracing (clean tree)
5036ad0  fix replay sender off-by-one (last packet skipped)
0591f2f  strip ETH+IP+UDP headers before shred parse + move rx_timestamp
508bbe1  count survival flushes as recycle evidence in pressure verdict
dcd680a  archive G4 seal results (server commit)
```

---

## 14. What the future operator should check before declaring G1 closed

Before marking G1 done on any future host, require all of the following:

### 14.1 Host acceptance pack

- host identity and region
- kernel and OS
- CPU, RAM, NUMA
- NIC driver/model
- RX timestamp capability
- management/test NIC split
- queue/ring state
- governor and hugepage state

### 14.2 G5 seal pack

- runtime `strace` PASS
- dequeue→egress microbench PASS with raw-ns numbers
- no invalid millisecond harness reused

### 14.3 G4 seal pack

- Baseline PASS
- Reorder PASS
- FEC-degraded PASS
- Archive pressure PASS
- summary artifacts saved

### 14.4 Final close conditions

If all of the above exist and are archived, then:

- **G1 = DONE**
- **G4 = DONE**
- **G5 = DONE**

---

## 15. Practical minute-by-minute run order for future host rentals

This is the condensed live run sequence.

### 15.1 Before renting

- confirm host pack is complete;
- confirm release artifacts are built;
- confirm G4 and G5 harnesses are ready;
- confirm there is no open coding dependency.

### 15.2 After host comes up

1. SSH in
2. install packages
3. capture baseline facts
4. determine management NIC vs test NIC
5. verify NIC driver, timestamping, PTP, NUMA
6. measure endpoint connect times
7. switch governor to `performance`
8. reserve hugepages
9. set test NIC queueing as required
10. run AF_XDP/XDP bringup checks
11. run G5 `strace`
12. run G5 dequeue→egress microbench
13. run G4 baseline/reorder/FEC/archive-pressure scenarios
14. archive all results
15. shut the host down

### 15.3 What not to waste time on

Do **not** spend hourly time on any of these unless truly required:

- writing new code from scratch;
- rebuilding half the repo because a local pack was missing;
- improvising cloud-init debug;
- rebooting for GRUB isolation before host proof requires it;
- rerunning invalid old harnesses.

---

## 16. Recommended quick-verify command block

Use this when a future chatbot/user wants the shortest reliable host sanity pass.

```bash
mkdir -p ~/g1/results/00_host_facts
cd ~/g1/results/00_host_facts

date -Is | tee date.txt
hostname | tee hostname.txt
uname -a | tee uname.txt
cat /etc/os-release | tee os-release.txt
lscpu | tee lscpu.txt
numactl --hardware | tee numactl-hardware.txt
lspci -nn | tee lspci-nn.txt
ip -br link | tee ip-br-link.txt
ip -br addr | tee ip-br-addr.txt
cat /proc/interrupts > interrupts.txt

ip route | tee ip-route.txt
ip route get 1.1.1.1 | tee ip-route-get.txt

ethtool -i eno1 | tee ethtool-eno1-i.txt
ethtool -T eno1 | tee ethtool-eno1-T.txt
ethtool -i eno2 | tee ethtool-eno2-i.txt
ethtool -T eno2 | tee ethtool-eno2-T.txt
cat /sys/class/net/eno1/device/numa_node | tee eno1-numa.txt
cat /sys/class/net/eno2/device/numa_node | tee eno2-numa.txt
```

If the host does not look basically like the known-good X710 / `i40e` / RX-timestamp / NUMA-local profile, consider stopping early.

---

## 17. Recommended artifact archive before shutting down the host

Create a single compressed archive before terminating the server.

```bash
tar -C ~/g1 -czf ~/g1-host-artifacts-$(date +%F-%H%M).tar.gz results
```

Copy it back locally:

```bash
scp -i ~/.ssh/shadow_g1_baremetal ubuntu@<SERVER_IP>:~/g1-host-artifacts-*.tar.gz .
```

Also archive or record:

- current git SHA(s)
- seal-run commit list
- active kernel version
- NIC driver/firmware versions
- queue/ring state
- any on-host hotfix commits made during the session

After artifacts are safely copied off, shut the host down:

```bash
sudo poweroff
```

---

## 18. Current project position after G1 closeout

After the successful host closeout described in this document, the working milestone posture is:

- **G1**: DONE
- **G4**: DONE
- **G5**: DONE
- **G6**: DONE
- **G7**: implemented / QC-passed in the working conversation
- **G8**: advanced heavily / effectively completed in the working conversation
- **Next software milestone after host work**: **G9**

The key practical meaning is:

- the host session is over;
- do not keep an hourly box alive while continuing G9 software development;
- rent a new box later only when there is a fresh host-only reason.

---

## 19. Final reminders for future operators and chatbots

1. **Keep `eno1` as management and `eno2` as the test NIC unless the routing facts are different on the new server.**
2. **Do not trust any old millisecond G5 latency result.** The correct G5 seal is the raw-nanosecond dequeue→egress harness.
3. **Do not use workspace-wide cargo commands.**
4. **Do not leave the host running while coding missing harnesses.**
5. **If the host quick verify fails, stop early and save money.**
6. **If the harnesses are ready and the host matches the known-good profile, the live sequence should be fast and mechanical.**

---

## 20. One-page condensed checklist

### Before renting

- [ ] release-guerrila artifacts built
- [ ] host pack ready
- [ ] G5 `strace` harness ready
- [ ] G5 dequeue→egress raw-ns harness ready
- [ ] G4 baseline/reorder/FEC/archive-pressure harness ready
- [ ] no open coding tasks needed for host proof

### Provisioning

- [ ] hourly host
- [ ] project-scoped SSH key
- [ ] no RAID
- [ ] Ubuntu LTS
- [ ] user-data blank unless tested

### First 15 minutes on host

- [ ] install packages
- [ ] capture baseline facts
- [ ] identify management NIC and test NIC
- [ ] verify NIC model/driver/timestamp/NUMA
- [ ] endpoint probe
- [ ] governor → performance
- [ ] hugepages set
- [ ] test NIC queueing adjusted

### Seal work

- [ ] AF_XDP/XDP bringup verified
- [ ] G5 `strace` PASS
- [ ] G5 dequeue→egress PASS
- [ ] G4 baseline PASS
- [ ] G4 reorder PASS
- [ ] G4 FEC-degraded PASS
- [ ] G4 archive-pressure PASS

### Closeout

- [ ] archive results
- [ ] copy artifacts off host
- [ ] update milestone docs
- [ ] power off host

---

**End of G1 runbook**
