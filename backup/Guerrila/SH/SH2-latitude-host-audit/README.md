# SH2 Latitude Host Audit Pack

This folder is the reusable **SH2 acceptance pack** for a freshly provisioned Latitude Tokyo host. Its job is to qualify the host before any Agave install or validator bring-up.

## Provisioning target

Provision the host as:

- OS: Ubuntu 24.04
- Billing: shortest practical commitment / hourly if available
- Access: SSH key only
- Public IP: yes
- NAT: no, if selectable
- Disk mode: raw local NVMe / no managed abstraction if selectable
- Name/tag: `shadow-solana-tokyo-sh2`

Avoid managed firewall or DDoS modes that silently filter high-volume UDP unless they are optional and can be disabled.

## Purpose

SH2 is an **acceptance host**, not yet the production validator. It exists to prove the real hardware and network surface:

- NIC model and driver
- NUMA layout
- RX/TX queue count
- XDP / AF_XDP feature surface
- hardware timestamping support
- NVMe layout
- UDP behavior
- Solana gossip / TVU reachability

## Stop condition

Stop after the host audit and baseline security posture. Do **not** install Agave yet.

If the audit is clean, the next milestone is SH3:

- mount NVMe layout
- create `sol` user
- apply kernel/sysctl/limits tuning
- install Agave CLI / validator
- generate identity
- start a non-voting testnet/devnet validator first
- then add Yellowstone

## File map

- `env.sh.example` - optional environment defaults for host tag, audit directory, and interface.
- `lib.sh` - shared helper functions for the SH2 scripts.
- `00_install_audit_utils.sh` - installs `ethtool`, `numactl`, `pciutils`, `jq`, `curl`, and `chrony`.
- `10_collect_base_audit.sh` - writes `latitude_tokyo_audit_base.txt`.
- `20_collect_nic_audit.sh` - writes `latitude_tokyo_audit_nic.txt` for a selected interface.
- `30_secure_host_baseline.sh` - installs `ufw`, `fail2ban`, and `chrony`, then applies the initial deny-by-default firewall posture without opening Solana or RPC ports.

## Expected outputs

Collect and keep:

- `~/shadow-host-audit/latitude_tokyo_audit_base.txt`
- `~/shadow-host-audit/latitude_tokyo_audit_nic.txt`

Also record:

- Latitude SKU
- hourly/monthly price
- public IP
- NIC model
- NIC driver
- RAM
- disk layout
- primary NIC name

## Execution order

### 1. Install audit utilities

```bash
cd ~/SH/SH2-latitude-host-audit
bash ./00_install_audit_utils.sh
```

### 2. Collect the base audit

```bash
bash ./10_collect_base_audit.sh
```

### 3. Identify the primary public NIC

```bash
ip -br link
```

Then either export the interface:

```bash
export SH2_IFACE=<primary_public_nic>
bash ./20_collect_nic_audit.sh
```

or pass it directly:

```bash
bash ./20_collect_nic_audit.sh <primary_public_nic>
```

### 4. Apply the first security baseline

```bash
bash ./30_secure_host_baseline.sh
```

This intentionally:

- keeps incoming traffic denied by default
- allows only OpenSSH
- enables chrony
- does **not** open Solana dynamic ports yet
- does **not** expose RPC publicly

## OOB / serial console notes

If the provider OOB console is active, the session typically looks like:

```text
ssh <server_id>@<server_ip> -p 2222
[SOL Session operational. Use ~? for help]
```

Useful OOB behaviors:

- before boot: inspect hardware setup, change boot order
- during reboot: enter BIOS with `DEL` or `ESC`, watch POST and GRUB
- after boot: press Enter to reach the Linux login prompt on the serial console
- the shell is equivalent in function to SSH, but on the serial console (`ttyS0`)

To end the OOB session:

```text
~.
```

This is useful for first-boot inspection, NIC/boot troubleshooting, and recovery if network access breaks.

## Rules during SH2

- Do not install Agave yet.
- Do not open RPC publicly.
- Do not start Yellowstone yet.
- Do not start AF_XDP / SH8 yet.
- Do not start `r1_tiny` or any live promotion path.

## First live audit record

The SH2 pack was executed on `2026-04-27` against the current combined SH1/SH2 host:

- SSH target: first constrained host
- hostname: `m4-metal-large-tyo-1`
- primary NIC: `eno1`
- audit artifacts:
  - `~/shadow-host-audit/latitude_tokyo_audit_base.txt`
  - `~/shadow-host-audit/latitude_tokyo_audit_nic.txt`

Executed scripts:

- `00_install_audit_utils.sh`
- `10_collect_base_audit.sh`
- `20_collect_nic_audit.sh eno1`

Deliberately not executed on this host:

- `30_secure_host_baseline.sh`

Reason: this box was already running the SH1 non-voting validator. The deny-by-default `ufw` policy in the baseline script would have disrupted the active Solana P2P sockets unless we first converted it to an explicit Solana-port-aware firewall policy.

Observed hardware and network summary:

- CPU: `AMD EPYC 9254 24-Core Processor` (`48` logical CPUs, `1` socket, `1` NUMA node)
- RAM: `125 GiB` total
- NIC: `Intel Ethernet Controller X550` on `ixgbe`
- queue count: `48` combined channels on `eno1`
- timestamping: hardware TX/RX timestamping available
- public route: direct `/31` on `eno1` with provider-assigned default gateway
- disks:
  - `2 x 447 GiB` Micron 7450 mirrored into `md0` for the OS/root filesystem
  - `2 x 3.5 TiB` additional NVMe devices present for later role assignment

Observed live Solana reachability on the same host:

- `sh1-agave-no-geyser-2.service` was active during the audit
- validator gossip metrics were flowing
- genesis unpack completed
- snapshot download had started
- Solana sockets were bound on the expected dynamic range beginning at `8000`

Current interpretation:

- This host is good enough for practical combined-host SH2/SH1 work and proves real NIC, NUMA, queue, timestamping, route, and live gossip/bootstrap behavior.
- This host does **not** satisfy the original `256 GiB RAM minimum` target from the checklist.
- The generic SH2 firewall baseline was intentionally skipped on the live validator host, but SH3-lite has now replaced it with a Solana-aware `ufw` policy that preserves the active `8000-8025` validator range while keeping public RPC closed.
- The current acceptance result should be treated as **partial accept**: keep the box for constrained SH1/SH3-lite bring-up, but do not bless it as the final production Agave RPC/Geyser plus Shadow plus AF_XDP combined host.
