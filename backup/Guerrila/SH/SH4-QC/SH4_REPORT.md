# SH4 Report - Yellowstone Smoke on Replacement Constrained Host

## Scope

This report records the current **SH4-lite** execution on the replayed constrained host:

- host: `m4-metal-large-tyo-1`
- ssh: current constrained replacement host
- primary NIC: `eno1`

Goal:

```text
current Agave non-voting devnet validator
-> add Yellowstone Geyser plugin
-> expose gRPC only on localhost
-> prove slots + transactions stream locally
-> collect memory/stability evidence
-> no Shadow, no AF_XDP, no r1_tiny, no live path
```

## Why SH4-lite is separate

SH3-lite proved the validator storage layout, firewall posture, private RPC, and constrained-host no-Geyser runtime on the earlier host. The replacement `.55` host had to be replayed back into that shape before Yellowstone smoke could begin.

SH4-lite is the first stage where the host must prove:

- a compatible Yellowstone plugin build
- validator startup with `--geyser-plugin-config`
- localhost-only listener posture for gRPC and metrics
- local slot and transaction client proof
- bounded resource overhead on a `125 GiB` host

## Work performed on `.55`

### 1. Host replay back to SH3-lite substrate

The fresh `.55` replacement host was brought back into the required pre-SH4 shape:

- SH2 audit rerun
- Rust/build dependencies installed
- Agave validator sysctls restored
- `/dev/nvme2n1` and `/dev/nvme3n1` formatted as XFS
- mounted as:
  - `/mnt/solana-ledger`
  - `/mnt/solana-ledger/snapshots`
  - `/mnt/solana-accounts`
- Solana-aware `ufw` policy applied

### 2. Matched validator/plugin build

Pinned versions:

```text
Agave       v3.1.14
Yellowstone v12.3.0+solana.3.1.14
```

The build completed successfully after adding `libudev-dev` for the Agave `hidapi` dependency.

### 3. Yellowstone configuration and service shape

Yellowstone `config-check` passed.

The current SH4-lite service is:

```text
/etc/systemd/system/sh4-agave-yellowstone-smoke.service
```

It is a cloned rollback-safe unit derived from:

```text
/etc/systemd/system/sh1-agave-no-geyser-2.service
```

Current SH4 validator shape:

```text
--no-voting
--private-rpc
--rpc-bind-address 127.0.0.1
--dynamic-port-range 8000-8025
--ledger /mnt/solana-ledger
--snapshots /mnt/solana-ledger/snapshots
--accounts /mnt/solana-accounts
--geyser-plugin-config /home/ubuntu/SH/SH1-local-agave-yellowstone/lab/yellowstone-config.json
```

### 4. Evidence capture correction

The first SH4 evidence jobs used fixed sleep windows and could have produced a misleading "final" artifact before the localhost Yellowstone listeners ever appeared.

That was replaced with a listener-driven gate:

```text
~/yellowstone/sh4_wait_until_green.sh
```

The repo copy lives in:

```text
SH\SH4-QC\sh4_wait_until_green.sh
```

This gate now:

- waits for `127.0.0.1:10000` and `127.0.0.1:8999`
- fails closed if the SH4 service dies or swap appears
- runs the Yellowstone client smoke only after listener bind
- runs a post-listener stability watch
- writes the final artifact only after the real SH4 checks complete

## Current live status

SH4-lite is **closed / green** on `.55`.

Final accepted result:

- `sh4-agave-yellowstone-smoke.service` stayed active
- Yellowstone bound `127.0.0.1:10000` and `127.0.0.1:8999`
- private RPC and PubSub remained on `127.0.0.1:8899` and `127.0.0.1:8900`
- the local Yellowstone client smoke completed successfully
- the post-listener watch completed and the gate wrote the final artifact
- swap stayed at `0`
- no kernel OOM / killed-process evidence appeared
- dynamic `8000-8025` Solana ports remained bound

## On-host evidence paths

Current SH4-lite evidence is being written under:

```text
~/shadow-host-audit/sh4-yellowstone-smoke/
```

Key artifacts:

- `listener_wait.log`
- `client_smoke.log`
- `post_listener_watch.log`
- `yellowstone_smoke_final.txt`
- `sh4_wait_until_green.stdout`

Archived repo copy:

```text
SH\SH4-QC\evidence\2026-04-28-sh4-green\
```

## Green criteria

The completed SH4-lite proof satisfied all of the following:

- `127.0.0.1:10000` is listening
- `127.0.0.1:8999` is listening
- local client smoke proves slot updates
- local client smoke proves transaction updates
- the post-listener watch completes
- no swap
- no kernel OOM / killed-process evidence
- RPC/PubSub remain localhost/private
- dynamic `8000-8025` Solana ports remain bound

## Rollback

If the SH4 service dies, swap appears, or memory climbs into failure territory:

```text
stop sh4-agave-yellowstone-smoke.service
start sh1-agave-no-geyser-2.service
```

That rollback preserves the warmed no-Geyser validator path while keeping Shadow, AF_XDP, and any live behavior out of scope.

## Next step

SH4-lite is closed. The next stage is:

```text
SH5-lite
-> point Shadow at http://127.0.0.1:10000
-> keep grpc_source_grade = local_validator_geyser
-> current SH5-A state: green / closed
-> current SH5-B state: not started / still zero
-> next gate: prove observe_candidates_local_validator_geyser > 0 without contaminating private-grade or live counters
-> keep AF_XDP/private-grade counters at 0
-> keep live send disabled
```
