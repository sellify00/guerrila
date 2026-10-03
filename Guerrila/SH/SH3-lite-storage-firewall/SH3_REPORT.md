# SH3 Report — NVMe Storage Correction + Solana-Aware Hardening

## Scope

This report records the first **SH3-lite** execution on the current combined host:

- host: `m4-metal-large-tyo-1`
- ssh: first constrained host
- primary NIC: `eno1`

The goal was to correct the validator storage layout and apply Solana-aware hardening without adding Yellowstone, Shadow, AF_XDP, or any live promotion behavior.

## Why SH3-lite was required

The first no-Geyser run proved the box could bootstrap, but it also showed that Agave was still writing to the OS mirror:

```text
--ledger   /home/ubuntu/SH/SH1-local-agave-yellowstone/lab/ledger
--accounts /home/ubuntu/SH/SH1-local-agave-yellowstone/lab/accounts
```

Those paths lived on `/dev/md0`, the mirrored `447 GiB` OS volume. The snapshot was intentionally stopped during bootstrap so the validator could be moved onto the two `3.5 TiB` NVMe devices before the rehearsal continued.

## Work performed

### 1. Data NVMe preparation

The two empty `3.5 TiB` devices were identified as:

```text
/dev/nvme2n1
/dev/nvme3n1
```

They were formatted as XFS and mounted persistently via `/etc/fstab`:

```text
LABEL=sol-ledger    -> /mnt/solana-ledger
LABEL=sol-accounts  -> /mnt/solana-accounts
```

The active SH3-lite layout is now:

```text
/mnt/solana-ledger
/mnt/solana-ledger/snapshots
/mnt/solana-accounts
/var/log/solana
```

### 2. SH1 pack correction

The SH1 lab pack was updated to support an explicit snapshots path:

```text
SH1_SNAPSHOTS_DIR
```

That lets the validator pass:

```text
--ledger /mnt/solana-ledger
--snapshots /mnt/solana-ledger/snapshots
--accounts /mnt/solana-accounts
```

### 3. Service correction

The transient launch was replaced by a persistent unit:

```text
/etc/systemd/system/sh1-agave-no-geyser-2.service
```

Current validator shape:

```text
--identity /home/ubuntu/SH/SH1-local-agave-yellowstone/lab/validator-keypair.json
--no-voting
--full-rpc-api
--private-rpc
--ledger /mnt/solana-ledger
--snapshots /mnt/solana-ledger/snapshots
--accounts /mnt/solana-accounts
--log /var/log/solana/agave-devnet-no-geyser.log
--rpc-port 8899
--rpc-bind-address 127.0.0.1
--dynamic-port-range 8000-8025
--wal-recovery-mode skip_any_corrupted_record
--limit-ledger-size
```

### 4. Host-specific firewall hardening

The generic SH2 baseline script was **not** used on the active validator host. Instead, the host now uses a Solana-aware `ufw` policy:

```text
allow OpenSSH
allow 8000-8025/tcp
allow 8000-8025/udp
deny 8899/tcp
deny 8900/tcp
```

This preserves the validator’s actual dynamic range while keeping RPC non-public.

## Current live evidence

At the time this report was created:

- `sh1-agave-no-geyser-2.service` was active
- the restarted validator was writing to `/mnt/solana-ledger`
- `tmp-genesis`, `contact-info.bin`, and the restarted snapshot archive were appearing under the mounted NVMe path
- the validator was again binding the expected `8000-8025` socket range
- RPC remained private via `--rpc-bind-address 127.0.0.1`
- `ufw` was active with the host-specific rules above
- the runtime monitor was already running at:
  - `~/shadow-host-audit/sh1-runtime-rehearsal/live_watch.log`
  - `~/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt`

## Notable operational findings

### RAM position

This host remains a **partial accept** only:

```text
usable for SH2/SH3-lite lab work
not accepted as the final production validator/Geyser/Shadow/AF_XDP combined host
```

Reason: the listing claimed `384 GB`, but the machine exposes only about `125 GiB` RAM.

### Migration gotchas discovered

Two practical issues surfaced during SH3-lite:

1. XFS labels must be `<= 12` characters, so `solana-ledger` / `solana-accounts` had to be shortened to:
   - `sol-ledger`
   - `sol-accounts`
2. Windows-origin shell edits must be normalized back to LF before restarting the Linux unit, otherwise bash will fail on CRLF line endings.

## Final runtime result

The 2-hour runtime evidence completed successfully.

Final evidence file:

```text
~/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt
```

The finished artifact showed:

- `sh1-agave-no-geyser-2.service` active for more than `2` hours
- no swap in use
- no kernel `oom`, `out of memory`, or `killed process` lines
- validator still running from:
  - `/mnt/solana-ledger`
  - `/mnt/solana-ledger/snapshots`
  - `/mnt/solana-accounts`
- RPC and PubSub listening only on `127.0.0.1:8899` and `127.0.0.1:8900`
- dynamic Solana ports still bound on the `8000-8025` range
- replay/root advancement visible via `bank frozen` and `new root` events

Observed end-of-window resource position:

- host memory available: about `97 GiB`
- root filesystem free: about `366 GiB`
- `/mnt/solana-ledger` used: about `122 GiB`
- `/mnt/solana-accounts` used: about `416 GiB`

## Current interpretation

SH3-lite is now **green as a constrained runtime rehearsal**:

- storage correction is real
- Solana-aware firewall hardening is real
- the validator stayed up long enough to satisfy the no-Geyser runtime gate

This does **not** upgrade the host into final production acceptance. The RAM mismatch still keeps it in the lab / constrained-host category.

## Next step

Do **not** widen scope on this host beyond the constrained proof already captured here.

Next session plan:

```text
shut down the first constrained host
provision replacement host in intended 256/384 GB class
run SH2 audit pack on replacement host
replay SH3-lite storage/firewall/no-geyser proof on replacement host
only then begin SH4-lite Yellowstone smoke
```

The important point is that the replacement host does **not** restart from zero:

- SH2 procedure already exists in `SH\SH2-latitude-host-audit`
- SH1 build/runtime pack already exists in `SH\SH1-local-agave-yellowstone`
- SH3-lite storage/firewall corrections are captured in this report

When SH4-lite starts on the replacement host:

- keep the source label as `local_validator_geyser`
- smoke slot + transaction streams
- do not treat local Geyser as private-grade
- do not add Shadow or AF_XDP yet
