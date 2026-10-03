# SH4-QC - Yellowstone Smoke Pack

This folder captures the current **SH4-lite** phase on the constrained replacement host:

- host: `m4-metal-large-tyo-1`
- ssh: current constrained replacement host
- scope: localhost-only Yellowstone smoke on top of the non-voting devnet validator

This remains an **observe-only infrastructure proof**:

- no Shadow attachment
- no AF_XDP
- no `r1_tiny`
- no live send path
- no private-grade relabeling

## Purpose

SH4-lite exists to prove that the replayed SH3-lite validator on `.55` can load the Yellowstone Geyser plugin, keep RPC private, expose gRPC and metrics only on localhost, stream slots and transactions locally, and survive a constrained-host stability watch.

## File map

- `README.md` - reusable SH4-lite scope, acceptance target, and evidence map.
- `SH4_REPORT.md` - live execution report for the current `.55` run.
- `sh4_wait_until_green.sh` - listener-driven gate script copied from the live host workflow.

## Current pinned stack

The current SH4-lite run is pinned to:

```text
Agave       v3.1.14
Yellowstone v12.3.0+solana.3.1.14
```

Current live services:

```text
sh1-agave-no-geyser-2.service      rollback-safe baseline
sh4-agave-yellowstone-smoke.service active SH4-lite service
```

## Current network posture

The validator host is intentionally constrained to:

- allow `OpenSSH`
- allow `8000-8025/tcp`
- allow `8000-8025/udp`
- deny `8899/tcp`
- deny `8900/tcp`

SH4-lite must additionally prove:

```text
127.0.0.1:10000  Yellowstone gRPC
127.0.0.1:8999   Yellowstone metrics
127.0.0.1:8899   private RPC
127.0.0.1:8900   private PubSub if enabled
```

## On-host evidence paths

The current `.55` evidence bundle lives under:

```text
~/shadow-host-audit/sh4-yellowstone-smoke/
```

Key files:

- `listener_wait.log`
- `client_smoke.log`
- `post_listener_watch.log`
- `yellowstone_smoke_final.txt`
- `sh4_wait_until_green.stdout`

Archived repo copy:

```text
SH\SH4-QC\evidence\2026-04-28-sh4-green\
```

## Acceptance target

SH4-lite is green only when all of the following are true:

- Yellowstone binds `127.0.0.1:10000`
- Yellowstone metrics bind `127.0.0.1:8999`
- slot smoke succeeds
- transaction smoke succeeds
- the post-listener watch completes without swap or OOM evidence
- RPC/PubSub remain localhost/private
- dynamic `8000-8025` Solana ports remain bound

## Current status

SH4-lite is **closed / green** on the constrained `.55` host.

The finished SH4-lite evidence proved:

- `127.0.0.1:10000` bound for Yellowstone gRPC
- `127.0.0.1:8999` bound for Yellowstone metrics
- `127.0.0.1:8899` and `127.0.0.1:8900` stayed private
- the Yellowstone client smoke completed
- the post-listener watch completed
- no swap was used
- no OOM / killed-process evidence appeared
- the dynamic `8000-8025` Solana ports remained bound

Next step:

```text
SH5-lite = Shadow ingest from local validator Geyser
current SH5-A state: green / closed
current SH5-B state: not started / still zero
next gate: prove observe_candidates_local_validator_geyser > 0 without contaminating private-grade or live counters
```
