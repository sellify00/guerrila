# SH Wallet Notes

This document records the current SH wallet layout, the local backup state, and the safe restore/use procedure. It does **not** contain raw private key material.

## Current wallets

### 1. Worker signer / live trade signer

- Role: fee payer and live trade signer for Shadow execution
- Pubkey: `ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX`
- Current host path: `/etc/shadow-engine/worker-signer.json`

### 2. ShadowGuard deployer

- Role: ShadowGuard deploy/deployer wallet
- Pubkey: `4iYqgdyuU8YE6FkZRq3195SZU7m5E5MkaNeaHayfoiZJ`
- Current host path: `/etc/shadow-engine/shadow-guard-deployer.json`

### 3. ShadowGuard on-chain IDs

- Program ID: `NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW`
- Guard vault in: `HAULecjkcF2GHGSQ566yRBuwRoHxH24YGZs1n6B3QpAG`
- Guard vault out: `9mo6Dhx8RhrwNqxCBGcfqEZzmGPGr4hz1mfTdW8tpsq7`

### 4. Private-lane auth keypairs

These are authentication keypairs for private-lane providers. Having the local keypair file is not enough: the provider must also authorize the pubkey/account, and the configured endpoint must accept the connection.

- Jito auth pubkey: `3i9N1VhsmKJBXB4STP4xdinmAXYCRecmNHBbLkSzwCW4`
- Jito host path: `/etc/shadow-engine/jito-auth-keypair.json`
- Harmonic auth pubkey: `9ZaQnhU56rxrw3mkGrVHaebTFXF1FgqAdvanTPUJMPwa`
- Harmonic host path: `/etc/shadow-engine/harmonic-auth-keypair.json`

### 5. Current CLMM signer SPL accounts

These are the signer-owned token accounts needed for the current Raydium CLMM proof path on host `.15`.

Historical Run38 public-RPC target:

- Target CLMM pool: `71Dcr9vTtnDintGVj6AXAS2pazAY687XvApDEjrM9QTV`
- Input mint: `So11111111111111111111111111111111111111112`
- Output mint: `FGkmpY8zc4EkZ9EHWYyqEDhRJ8yDAFN8DUNBgLnRxZYV`

Current Run41 Jito no-send target:

- Target CLMM pool: `8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj`
- Input mint: `So11111111111111111111111111111111111111112`
- Output mint: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`
- Route: `jito_bundle`
- Conservative proof size: `0.1 WSOL` (`100000000` raw)
- Host signer inventory path: `/etc/shadow-engine/signer-token-inventory.json`

Current signer ATAs:

- WSOL ATA: `2mbhm1JXARh5f2SzVuvwnNmfz1n2K1kPjfF8R6ujKGHR`
- USDC ATA: `CbWgXU427fZRdKGUiXXsmkEkTnh26s4PfTQbftu8AjNo`
- Broad-surface output ATA for `MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF`: `HF49H7c4D7fBJEsAj8tnqztnDseRs49JLpkM3pMmFNxo`
- Broad-surface output ATA for `ioSEni9v3w2avsHAYMdpiewfnayykfWQPsE73cX4Jh7`: `XZAKuyGJQh8ktKUYw5yvuobTPEaVv5jiTcM83KgeAag`
- Historical Run38 output ATA: `8vbtpMNEFUQJRd9staLLCLMQjQQBV4Mg62k5H6ponyXX`

Current status on host `.15`:

- Latest checked native worker-signer balance: `12.500010001 SOL`
- ATA-create tx: `JE7AG8JE1P9WV86AZ2SDJbfhNZgeJtZg6hZsyBMSky662143CVo3LzyYxvcWAvqSUkRjCxa8cxPgGCWmSY9z9pw`
- WSOL sync-native tx: `2pWNckdgGYje3MsEgW29j7PGja2fkhMbwd138N68bAtR13vLfEybFbXa8hdoer8esAbvpho3c22o2ZPuGP3arr1h`
- WSOL ATA parsed token balance after sync: `0.2 WSOL` (`200000000` raw)
- USDC ATA parsed token balance after `2026-05-17` funding: `230.088909 USDC` (`230088909` raw)
- Broad-surface output ATA create tx for `MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF`: `3Lbx3Wk1i6KaUZercq3A1sicnCpHFfox3kjKRL9BfAtb5LdMzhfxZZRznJWvu3HjiiHeJXgC5kkuLvctEEpmzU2z`
- Broad-surface output ATA create tx for `ioSEni9v3w2avsHAYMdpiewfnayykfWQPsE73cX4Jh7`: `63qvVFT39nQHdMR1uy7hiNhrYu9xQxrYfqXkWj64USk9gcx6wb1RTX1LhEwd1um7ggphxWaX5m2eHvssr4F4siFQ`

Important distinction:

- The worker signer wallet holds native SOL for fees, reserves, and any future explicit wrap step.
- The current fixed CLMM proof path still uses signer-owned SPL token accounts for template truth.
- The current scorecard and proof harness read the pre-funded WSOL ATA from `/etc/shadow-engine/signer-token-inventory.json`, not the native SOL balance.
- `auto_wrap_supported=false` is the current invariant: do **not** assume native SOL will be wrapped inside the first Run41 proof transaction.
- For the current Run41 target, the WSOL ATA must already be funded and the USDC ATA only needs to exist and be initialized.
- A "funded signer" does **not** mean every possible broad-surface input mint is funded; it only means the signer has native SOL plus whatever SPL mint balances already exist in signer-owned token accounts.

How signer output token accounts work:

- For any non-SOL output mint, the signer needs a token account to receive that mint.
- In normal Solana usage this is usually the mint's ATA (Associated Token Account) for the signer pubkey.
- The output token account starts empty; it is just the destination container for future received tokens.
- This is **not** where "profitable lamports" are stored. Lamports are the smallest unit of SOL and only appear here as a PnL measurement unit.
- Creating the ATA is a one-time Solana transaction that pays small rent/fees and creates an empty token account for a specific `(owner, mint)` pair.
- For the worker signer `ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX`, the fee payer signs the ATA-create transaction; the token account owner is still the worker signer.

Example CLI from any machine with Solana CLI + SPL Token CLI:

```bash
spl-token create-account MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF \
  --owner ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX
```

That creates the worker signer's ATA for mint `MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF`; the account is expected to be empty immediately after creation.

Current fixed-template proof state:

- Run39 broad-surface discovery proved held-input candidates exist, but the current surface is `jito_bundle`, not `public_rpc_scout`.
- The best current target is pool `8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj`, `WSOL -> USDC`, route `jito_bundle`.
- The observed Run39 candidate size for that row was `1.0 WSOL`, which is larger than the currently pre-wrapped `0.2 WSOL` ATA.
- The conservative Run41 proof cap is now `0.1 WSOL`, so the current `0.2 WSOL` ATA is already sufficient and no extra wrap is needed.
- The latest Run41 smoke was blocked by local Agave preflight on `.15`, not by missing signer capital.

Current broad-surface canonical candidate state (`2026-05-17`):

- Latest canonical AF_XDP positive-EV row was:
  - venue: `meteora_damm_v2`
  - route: `jito_bundle`
  - pool: `9i1GxRjhZRyjPK5M4JTxAKB3HPNmiAQKVQLaBbGKCtRs`
  - input mint: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` (USDC)
  - output mint: `MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF`
  - required input size: `110.311978 USDC` (`110311978` raw)
  - estimated net-after-fee: `36766` lamports
- The USDC funding blocker is now cleared because the signer USDC ATA holds `230.088909 USDC`.
- The output ATA blocker is now cleared: the worker signer ATA for `MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF` is `HF49H7c4D7fBJEsAj8tnqztnDseRs49JLpkM3pMmFNxo`.
- After the output ATA creation, the next rerun can test whether this row advances from inventory-ready into template/route readiness.

Current gRPC/broad-surface follow-up candidate state (`2026-05-17`):

- The bounded Run45 gRPC fallback window surfaced another approved-held AF_XDP Meteora row:
  - venue: `meteora_damm_v2`
  - route: `jito_bundle`
  - pool: `7Jny1dhCwHuuXxiR7NwFenN8VSTNegTfaFYt8Q6GsfAj`
  - input mint: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` (USDC)
  - output mint: `ioSEni9v3w2avsHAYMdpiewfnayykfWQPsE73cX4Jh7`
  - required input size: `46.361004 USDC` (`46361004` raw)
  - estimated net-after-fee: `12553` lamports
- That output ATA blocker is now also cleared: the worker signer ATA for `ioSEni9v3w2avsHAYMdpiewfnayykfWQPsE73cX4Jh7` is `XZAKuyGJQh8ktKUYw5yvuobTPEaVv5jiTcM83KgeAag`.

Current live private-lane blocker:

- Jito Block Engine rejects the staged Jito auth pubkey as unauthorized to generate a token.
- The worker signer pubkey `ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX` was also tested as Jito auth and rejected.
- Harmonic regional endpoints currently fail at channel connect before becoming an admissible route.

Required unblock:

- Ask Jito to authorize `3i9N1VhsmKJBXB4STP4xdinmAXYCRecmNHBbLkSzwCW4`, or replace `/etc/shadow-engine/jito-auth-keypair.json` with a Jito-authorized searcher auth keypair.
- Ask Harmonic/provider for the correct reachable regional endpoints and authorization package for `9ZaQnhU56rxrw3mkGrVHaebTFXF1FgqAdvanTPUJMPwa`, or replace the staged Harmonic auth config with provider-issued credentials.
- If provider authorization is not practically available, do not keep retrying Jito gRPC/Harmonic auth as the only live path. R1-JITO-JSON no-send is green using Jito Block Engine JSON-RPC selection with `auth_uuid: ""`, Harmonic disabled, and public fallback still disabled. Public-tiny is only a later separately labeled fallback if JSON-RPC tiny-live fails, with private-lane landing evidence still marked blocked.

Provider contact/request path:

- Jito: use the official Jito docs/site/support/Discord path for Block Engine/Searcher access. Request gRPC Block Engine auth for the staged searcher auth pubkey, or ask for the supported JSON-RPC UUID auth path if gRPC keypair auth is not available for the account. Current docs entry point: `https://docs.jito.wtf/lowlatencytxnsend/`.
- Harmonic: use the direct Harmonic/provider contact that issued or documented the `fra/ams/nrt.harmonic.xyz` endpoints. Request the reachable production gRPC endpoints, TLS requirements, auth mode, and account authorization for the staged Harmonic auth pubkey. If no such vendor access exists, keep `submit.routing.enable_harmonic_lane=false`.

Safe provider request template:

```text
We are integrating a Solana private-lane searcher client. Please authorize this auth pubkey for bundle submission / token generation, or provide the correct production credentials and endpoint set.

Provider: <Jito or Harmonic>
Cluster: mainnet-beta
Auth pubkey: <pubkey only, no private key>
Current endpoint(s): <configured endpoint(s)>
Current error: <PermissionDenied / transport error>

We will not send private key material. Please confirm the expected auth mode, endpoint URL(s), TLS requirements, and any account/UUID/API-key header requirements.
```

## Local backup state

The wallet keypairs were copied off-host to the local Windows machine and are now independent of any single Solana host.

Local backup files:

- `C:\Users\alung\.shadow-engine\worker-signer-ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX-20260430_205643.json`
- `C:\Users\alung\.shadow-engine\worker-signer-live-current.json`
- `C:\Users\alung\.shadow-engine\shadow-guard-deployer-4iYqgdyuU8YE6FkZRq3195SZU7m5E5MkaNeaHayfoiZJ-20260430_205852.json`

Local manifest:

- `C:\Users\alung\.shadow-engine\wallet-access.env`

The manifest records:

- wallet pubkeys
- private-lane auth pubkeys
- local keypair backup paths
- SHA256 checksums
- RPC URL
- ShadowGuard IDs
- current CLMM signer ATA addresses
- current CLMM host inventory path
- ATA creation / sync tx signatures

The `.env` file points at the keypair backups and does **not** duplicate raw private key bytes.

Keypair parsing regression guard:

- Standard Solana JSON keypair exports are typically `64` bytes:
  - bytes `[0:32]` = secret scalar / seed material
  - bytes `[32:64]` = public key
- Do **not** derive the signer pubkey by base58-encoding the first `32` bytes.
- For `64`-byte keypair JSON, the signer pubkey must match the final `32`-byte public-key portion or a library-derived pubkey.
- Launch hot-path startup should fail closed unless:
  - resolved signer pubkey matches `/etc/shadow-engine/signer-token-inventory.json` owner
  - resolved signer has nonzero native SOL balance

## What "host independent" means

The wallet address is defined by the keypair file, not by the host.

If `.57` disappears, the wallets remain usable as long as the local backup JSON files remain available. A future host can reuse the same wallet by restoring the same keypair file there.

## Safe restore procedure on a new host

1. Stop the old host before reusing the same live signer elsewhere.
2. Copy the desired local keypair JSON to the new host over a trusted channel.
3. Install it with strict permissions.
4. Point config at the restored file.
5. Verify the pubkey matches before funding or running live.

Example on a new Linux host:

```bash
sudo install -m 600 worker-signer.json /etc/shadow-engine/worker-signer.json
sudo chown root:root /etc/shadow-engine/worker-signer.json
sudo solana-keygen pubkey /etc/shadow-engine/worker-signer.json
```

Expected worker signer pubkey:

```text
ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX
```

## Funding and withdrawal

For R1 tiny-live, fund the worker signer wallet:

```text
ANutmYD3QaoTP3YQgTACJUTX17WaB8kviJBS3FSe9ieX
```

Recommended target for the current R1 tiny-live plan:

```text
12.5 SOL
```

Latest checked worker-signer balance after the post-host-shutdown withdrawal:

```text
12.500010001 SOL
```

Current CLMM proof funding target:

```text
Send WSOL funding only to 2mbhm1JXARh5f2SzVuvwnNmfz1n2K1kPjfF8R6ujKGHR
```

Current funded/synced amount:

```text
0.2 WSOL
```

Current Run41 proof funding posture:

- No extra WSOL wrap is needed for the current `0.1 WSOL` proof cap.
- The current fixed CLMM template still assumes pre-funded WSOL in the ATA; do **not** rely on native SOL auto-wrap until that support is explicitly implemented and proven.

The worker signer remains host-independent because the local keypair backup is under `C:\Users\alung\.shadow-engine\`. The `.57` host has been powered off; do not assume `/etc/shadow-engine/worker-signer.json` is currently reachable until a new host is restored.

Breakdown:

- `10.0 SOL` reserve floor
- `2.0 SOL` tiny-live entity ceiling
- `0.5 SOL` fee / retry / tip headroom

To send funds back out later from a Linux host that holds the signer:

```bash
solana transfer <DESTINATION_WALLET> <AMOUNT> \
  --keypair /etc/shadow-engine/worker-signer.json \
  --url https://api.mainnet-beta.solana.com
```

On local WSL, copy the Windows backup to a local WSL path first and use that as `--from` / `--fee-payer`; older Solana CLI versions may require SOL-denominated amounts rather than `--lamports`.

## Safety rules

- Never commit raw keypair JSON into the repository.
- Never paste raw private key material into markdown, chat, or issue trackers.
- Do not run the same live signer on two hosts at once.
- Keep the local backup files ACL-restricted.
- Treat `wallet-access.env` as operationally sensitive because it tells you where the local wallet files live.
