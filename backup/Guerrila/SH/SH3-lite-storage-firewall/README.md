# SH3-lite Storage + Solana-Aware Hardening

This folder captures the **SH3-lite** phase for the current combined SH2/SH1 host: move Agave state off the OS mirror, replace the skipped generic SH2 firewall baseline with a Solana-aware policy, and restart the no-Geyser validator from the mounted NVMe paths.

This is still a **constrained bring-up** package:

- no Yellowstone yet
- no Shadow attachment yet
- no AF_XDP yet
- no `r1_tiny`
- no live promotion changes

## Purpose

SH3-lite exists to prove that the first constrained host can run the non-voting validator from the correct storage layout without polluting the OS mirror and without exposing RPC publicly.

The acceptance target for this stage is:

- ledger, snapshots, and accounts live on the `3.5 TiB` NVMe devices
- validator runs from a persistent `systemd` unit
- RPC remains localhost/private only
- `ufw` allows only OpenSSH plus the exact Solana dynamic range
- no-Geyser bootstrap continues from the corrected paths
- runtime evidence is collected long enough to judge stability on this `125 GiB` host

## File map

- `README.md` - reusable SH3-lite procedure summary and scope.
- `SH3_REPORT.md` - first live execution report for the first constrained host.

## Current host layout

The current SH3-lite layout on the first constrained host is:

```text
/mnt/solana-ledger             ledger
/mnt/solana-ledger/snapshots   snapshots
/mnt/solana-accounts           accounts
/var/log/solana                validator logs
```

## Current validator posture

The restarted validator currently uses:

```text
--no-voting
--private-rpc
--rpc-bind-address 127.0.0.1
--dynamic-port-range 8000-8025
--limit-ledger-size
--ledger /mnt/solana-ledger
--snapshots /mnt/solana-ledger/snapshots
--accounts /mnt/solana-accounts
```

## Current firewall posture

`ufw` is configured as:

- allow `OpenSSH`
- allow `8000-8025/tcp`
- allow `8000-8025/udp`
- deny `8899/tcp`
- deny `8900/tcp`

This is intentionally different from the generic SH2 baseline because the host is already carrying active Solana traffic.

## Evidence paths

Runtime evidence for this stage lives on-host under:

```text
~/shadow-host-audit/sh1-runtime-rehearsal/live_watch.log
~/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt
```

## Stop condition for this folder

SH3-lite is closed when the no-Geyser validator has enough runtime evidence from the NVMe-backed layout to show:

- service remains active without restart loops
- snapshot/bootstrap progresses
- no swap use
- no OOM evidence
- dynamic Solana ports stay bound
- RPC remains private

Only after that should the next step begin:

```text
SH4-lite = Yellowstone build/load smoke on the same constrained host
```
