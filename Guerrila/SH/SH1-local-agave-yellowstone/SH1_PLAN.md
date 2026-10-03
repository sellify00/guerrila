# SH1 Local Agave / Yellowstone Lab

Goal:
- Build or install Agave tooling.
- Build Yellowstone gRPC plugin.
- Validate a non-voting RPC/Geyser node config on testnet/devnet first.
- Prove local Yellowstone emits transaction/slot streams.
- Do not label local Geyser as AF_XDP/private-grade.

Acceptance:
- agave-validator --version works.
- solana --version works.
- Yellowstone plugin .so builds.
- agave-validator starts with --no-voting and --private-rpc.
- Validator reaches gossip/catchup on testnet/devnet.
- Geyser plugin loads cleanly with --geyser-plugin-config.
- A local Yellowstone client receives slots and transactions.

Lab pack files:
- `env.sh.example` - local lab environment defaults
- `lib.sh` - shared cluster defaults and helper functions
- `00_install_build_deps.sh` - Ubuntu/WSL package bootstrap
- `10_build_agave.sh` - clone/build Agave CLI + validator binaries
- `20_build_yellowstone.sh` - clone/build Yellowstone and run config-check
- `30_prepare_identity_and_dirs.sh` - create lab directories and validator identity
- `40_start_agave_no_geyser.sh` - first no-plugin validator bring-up
- `50_write_yellowstone_config.sh` - write the localhost-bound plugin config
- `60_start_agave_with_yellowstone.sh` - attach Yellowstone after the no-plugin pass
- `70_smoke_yellowstone_client.sh` - local health/slot/transaction smoke checks

Node shape:
- Use non-voting RPC node semantics, not voting-validator semantics.
- Keep the node private and localhost-bound for RPC during the first lab pass.
- Validate sync/gossip/catchup/log stability before attaching Yellowstone.
- After the no-plugin pass is stable, attach Yellowstone and prove the local gRPC stream.
- Reuse the same runbook on Latitude Tokyo only after the local lab is clean.

Reference startup shape:

```bash
exec agave-validator \
  --identity /home/sol/validator-keypair.json \
  --no-voting \
  --full-rpc-api \
  --private-rpc \
  --ledger /mnt/ledger \
  --accounts /mnt/accounts \
  --log /home/sol/agave-rpc.log \
  --rpc-port 8899 \
  --rpc-bind-address 127.0.0.1 \
  --dynamic-port-range 8000-8025 \
  --entrypoint <cluster-entrypoint> \
  --expected-genesis-hash <cluster-genesis-hash> \
  --wal-recovery-mode skip_any_corrupted_record \
  --limit-ledger-size
```

When Yellowstone is added, add only:

```bash
  --geyser-plugin-config /home/sol/yellowstone/config.json
```

Execution order:
1. Install or build Agave/Solana tooling locally and confirm versions.
2. Start a non-voting testnet/devnet Agave RPC node without Yellowstone attached.
3. Prove gossip, catchup, and log stability.
4. Build the Yellowstone gRPC plugin and point `libpath` at the built `.so`.
5. Restart with `--geyser-plugin-config` and confirm the plugin loads cleanly.
6. Run a local Yellowstone client and prove slot + transaction streams.
7. Only then promote the same runbook to Latitude Tokyo / mainnet.

Guardrails:
- Do not start SH8 AF_XDP TVU/shred capture yet.
- Do not open RPC to the public internet.
- Do not run a voting validator.
- Do not create a vote account.
- Do not stake anything.
- Do not label local Geyser as private-grade.
- Do not change G12 promotion gates.
- Do not start `r1_tiny`.

Notes:
- Anza RPC-node docs describe the intended non-voting pattern: `--no-voting`, `--private-rpc`, `--full-rpc-api`, `--ledger`, `--accounts`, `--rpc-port`, and `--dynamic-port-range`.
- Geyser is attached with `--geyser-plugin-config`, and the plugin config must point `libpath` at the built plugin `.so`.
- Keep plugin processing lightweight; if the plugin stalls, the validator can fall behind.
- Keep the SH1 gRPC bind localhost-only; Shadow can flip to `grpc_source_grade: local_validator_geyser` later without relabeling the source as AF_XDP/private-grade.
- First-host findings so far: Ubuntu 24.04 needed `libclang-18-dev` for native Rust builds, Agave rejected `8000-8020` and required at least `8000-8025`, and a reliable launch needed systemd resource limits rather than a plain SSH shell.
