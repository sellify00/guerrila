# SH1 Local Agave / Yellowstone Lab

This folder is the reusable SH1 lab pack for bringing up a **non-voting Agave RPC node** first, then attaching **Yellowstone gRPC** only after the validator has a clean no-plugin bootstrap path. It exists to prove `local_validator_geyser` honestly, without ever relabeling local Geyser as AF_XDP/private-grade.

## Folder contents

- `SH1_PLAN.md` - scope, acceptance criteria, node shape, and guardrails for SH1.
- `env.sh.example` - default environment variables for cluster, paths, ports, and Yellowstone bindings.
- `lib.sh` - shared helpers for cluster defaults, libclang detection, validator arg assembly, and file/path validation.
- `00_install_build_deps.sh` - installs Ubuntu build prerequisites including `clang`, `libclang-18-dev`, `cmake`, `pkg-config`, and `protobuf-compiler`.
- `10_build_agave.sh` - clones `anza-xyz/agave`, builds `agave-validator`, `solana`, `solana-keygen`, and `solana-test-validator`.
- `20_build_yellowstone.sh` - clones `rpcpool/yellowstone-grpc`, builds the workspace, writes the plugin config, and runs `config-check`.
- `30_prepare_identity_and_dirs.sh` - creates the lab directory tree and validator identity.
- `40_start_agave_no_geyser.sh` - starts the first non-voting validator without a Geyser plugin.
- `50_write_yellowstone_config.sh` - writes the localhost-bound Yellowstone config JSON with the built plugin `libpath`.
- `60_start_agave_with_yellowstone.sh` - starts the validator with `--geyser-plugin-config`.
- `70_smoke_yellowstone_client.sh` - runs health, version, and slot/transaction stream checks against the local Yellowstone endpoint.

## Intended execution order

1. Run `00_install_build_deps.sh`.
2. Run `10_build_agave.sh`.
3. Run `20_build_yellowstone.sh`.
4. Run `30_prepare_identity_and_dirs.sh`.
5. Launch `40_start_agave_no_geyser.sh` and wait for gossip, genesis, snapshot, and catchup proof.
6. Generate plugin config with `50_write_yellowstone_config.sh` if it was not already created.
7. Launch `60_start_agave_with_yellowstone.sh`.
8. Run `70_smoke_yellowstone_client.sh`.

## First host execution record

Current live host: first constrained host

| Item | Current value |
| --- | --- |
| Hostname | `m4-metal-large-tyo-1` |
| Kernel | `6.8.0-110-generic` |
| Validator service | `sh1-agave-no-geyser-2.service` |
| Service style | persistent `systemd` unit |
| Service state | `active` |
| Current ledger path | `/mnt/solana-ledger` |
| Current snapshots path | `/mnt/solana-ledger/snapshots` |
| Current accounts path | `/mnt/solana-accounts` |
| Current log path | `/var/log/solana/agave-devnet-no-geyser.log` |
| Dynamic port range | `8000-8025` |
| Firewall posture | `ufw` allows OpenSSH plus `8000-8025` TCP/UDP, denies `8899` and `8900` |
| Swap | `0 B` |
| Agave version | `agave-validator 4.1.0-alpha.0 (src:5f7171cc; feat:753d4762, client:Agave)` |
| CLI version | `solana-cli 4.1.0-alpha.0 (src:5f7171cc; feat:753d4762, client:Agave)` |
| Yellowstone plugin | `/home/ubuntu/SH/SH1-local-agave-yellowstone/lab/yellowstone-grpc/target/release/libyellowstone_grpc_geyser.so` |
| Runtime watch log | `/home/ubuntu/shadow-host-audit/sh1-runtime-rehearsal/live_watch.log` |
| Runtime final log | `/home/ubuntu/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt` |

## What has already been proven

- SSH access to the host works with the existing host key workflow.
- Agave builds cleanly enough on the host to produce the validator and CLI binaries.
- Yellowstone builds cleanly enough to produce `libyellowstone_grpc_geyser.so`.
- The validator launches with the intended **non-voting**, **private RPC**, **localhost RPC bind**, **known-validator / entrypoint / expected-genesis-hash**, and **bounded retention** arguments (`--limit-ledger-size`, `--maximum-full-snapshots-to-retain`, `--maximum-incremental-snapshots-to-retain`).
- The first no-Geyser run on the OS mirror discovered devnet peers and RPC nodes, downloaded `genesis.tar.bz2`, and drove the snapshot download as far as about `74%` before the run was intentionally stopped for SH3-lite storage migration.
- The current no-Geyser run has been restarted from mounted NVMe paths and is again discovering peers, downloading `genesis.tar.bz2`, and downloading the large snapshot archive.
- The validator is binding the expected dynamic Solana sockets beginning at `8000`.
- Ledger, snapshots, and accounts are no longer on the OS mirror.
- A Solana-aware `ufw` policy is now active for the actual validator port range.

## Issues discovered during the first live SH1 run

- Ubuntu 24.04 needed `libclang-18-dev`; the runtime shared libraries alone were not enough for `clang-sys`.
- The default dynamic port range had to widen from `8000-8020` to `8000-8025`.
- A plain SSH shell started with `ulimit -n 1024`, which was too low for Agave.
- The validator launch needed transient systemd limits instead of a plain shell launch.
- `build_validator_args()` in `lib.sh` had a nameref bug; it prevented `--known-validator`, `--entrypoint`, and `--expected-genesis-hash` from being appended until fixed.
- SH3-lite migration exposed two additional operational issues: XFS labels must be `<= 12` characters, and copying shell scripts from Windows requires normalizing them back to LF before restarting the service on Linux.

## Host settings now known to matter

- Host sysctls:
  - `net.core.rmem_max = 134217728`
  - `net.core.wmem_max = 134217728`
  - `vm.max_map_count = 1000000`
  - `fs.nr_open = 1000000`
- Launch style:
  - use a persistent `systemd` unit once the host moves past the first transient bootstrap
  - set `LimitNOFILE=1000000`
  - set `LimitMEMLOCK=2000000000`
- Keep RPC bound to `127.0.0.1`.
- Keep local Yellowstone bound to localhost as well.
- Keep ledger, snapshots, and accounts on the data NVMe mounts, not on the OS mirror.
- Keep the host firewall explicit: OpenSSH plus the exact Agave dynamic range only.

## Current status

The no-Geyser runtime rehearsal is now **green for this constrained host**.

Final evidence from `~/shadow-host-audit/sh1-runtime-rehearsal/no_geyser_runtime_rehearsal_final.txt` showed:

- validator active for more than `2` hours from the corrected NVMe-backed paths
- no swap in use
- no kernel OOM / killed-process evidence
- ledger, snapshots, and accounts all on the data NVMe mounts
- RPC and PubSub bound only to `127.0.0.1`
- dynamic Solana ports still bound in the expected range
- replay/root advancement visible through `bank frozen` and `new root` events

Resource position at the end of the rehearsal:

- host memory: about `97 GiB` available
- root filesystem: about `366 GiB` free
- `/mnt/solana-ledger`: about `122 GiB` used
- `/mnt/solana-accounts`: about `416 GiB` used

The host remains a **partial accept** only because of the RAM mismatch, but the no-Geyser runtime gate itself is satisfied.

## Next procedural step

Do **not** continue to Yellowstone on this host.

This box has already done its job as a constrained SH2/SH3-lite proof host. The next session should:

1. shut down the first constrained host
2. provision a replacement host in the intended `256/384 GB` class
3. run the SH2 audit pack on the replacement host
4. replay SH3-lite storage correction and no-Geyser runtime proof on the replacement host
5. only then start **SH4-lite** Yellowstone smoke

Use this folder together with:

- `SH\SH2-latitude-host-audit`
- `SH\SH3-lite-storage-firewall`

so the new host does **not** restart from scratch.
