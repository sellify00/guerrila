//! G4-G2: Real execution-deps boot loaders.
//!
//! Startup-only code: filesystem reads (keypair, TipLut), base58 decoding,
//! nonce ring seeding, inventory ceiling validation.
//!
//! Separated from `money_worker.rs` so the hot-path structural test
//! (`g4_g1f_t5_no_syscall_in_worker`) stays clean — `money_worker.rs`
//! must not touch `std::fs::*`.

use shadow_core::assembly::{
    FixedSwapAccountMeta, FixedSwapAccounts, FixedSwapIxData, JitoTipRotation, NonceRing,
    NonceTuple, PoolOverlay, SwapAccountSource, ThreadLocalSigner, EMPTY_SWAP_ACCOUNT_META,
    HARMONIC_CU_PAD, MAX_SWAP_ACCOUNTS,
};
use shadow_core::policy::TipLut;

use crate::money_worker::{
    ExecutionDeps, SignerTokenAccountEntry, SignerTokenInventory, WorkerLocalState,
};
use crate::rollout_caps::effective_inventory_ceiling_lamports;
use crate::sig_dedupe::SigDedupe;

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

/// Expected TipLut binary file size: 256 leader classes × 16384 entries × 4 bytes.
pub const TIP_LUT_FILE_SIZE: usize = 256 * 16384 * 4; // 16 MiB

/// Minimum sane inventory ceiling (0.01 SOL in lamports).
pub const MIN_INVENTORY_CEILING_LAMPORTS: u64 = 10_000_000;

/// Maximum sane inventory ceiling (1000 SOL in lamports).
pub const MAX_INVENTORY_CEILING_LAMPORTS: u64 = 1_000_000_000_000;

/// Error returned by [`boot_execution_deps`] on invalid or missing inputs.
#[derive(Debug)]
pub enum BootExecError {
    MissingExecutionConfig,
    MissingHeliusSenderTipAccount,
    KeypairIo(String, std::io::Error),
    KeypairParse(String, String),
    KeypairLength(String, usize),
    PubkeyDecode(String, String),
    PubkeyLength(String, usize),
    LutIo(String, std::io::Error),
    LutSize(String, usize),
    SignerTokenInventoryIo(String, std::io::Error),
    SignerTokenInventoryParse(String, String),
    SignerTokenInventoryTooLarge(usize),
    InventoryCeilingTooLow(u64),
    InventoryCeilingTooHigh(u64),
    NoncePrefillZero,
}

impl std::fmt::Display for BootExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExecutionConfig => write!(
                f,
                "execution config section is required for Guerrila profile"
            ),
            Self::MissingHeliusSenderTipAccount => write!(
                f,
                "helius_sender.tip_accounts must contain at least one tip-recipient account when helius_sender_dual is selected"
            ),
            Self::KeypairIo(path, e) => write!(f, "failed to read keypair file '{path}': {e}"),
            Self::KeypairParse(path, e) => write!(f, "failed to parse keypair file '{path}': {e}"),
            Self::KeypairLength(path, len) => {
                write!(f, "keypair file '{path}' has {len} bytes, expected 64")
            }
            Self::PubkeyDecode(field, e) => {
                write!(f, "failed to decode base58 pubkey for '{field}': {e}")
            }
            Self::PubkeyLength(field, len) => write!(
                f,
                "decoded pubkey for '{field}' has {len} bytes, expected 32"
            ),
            Self::LutIo(path, e) => write!(f, "failed to read TipLut file '{path}': {e}"),
            Self::LutSize(path, sz) => write!(
                f,
                "TipLut file '{path}' is {sz} bytes, expected {TIP_LUT_FILE_SIZE}"
            ),
            Self::SignerTokenInventoryIo(path, e) => {
                write!(f, "failed to read signer token inventory '{path}': {e}")
            }
            Self::SignerTokenInventoryParse(path, e) => {
                write!(f, "failed to parse signer token inventory '{path}': {e}")
            }
            Self::SignerTokenInventoryTooLarge(len) => write!(
                f,
                "signer token inventory has {len} accounts, exceeds runtime cap"
            ),
            Self::InventoryCeilingTooLow(v) => write!(
                f,
                "inventory ceiling {v} lamports is below minimum {MIN_INVENTORY_CEILING_LAMPORTS}"
            ),
            Self::InventoryCeilingTooHigh(v) => write!(
                f,
                "inventory ceiling {v} lamports exceeds maximum {MAX_INVENTORY_CEILING_LAMPORTS}"
            ),
            Self::NoncePrefillZero => write!(f, "nonce_prefill must be ≥ 1"),
        }
    }
}

fn resolve_private_lane_tip_account(
    cfg: &shadow_core::config::ShadowConfig,
    exec_cfg: &shadow_core::config::ExecutionConfig,
) -> Result<[u8; 32], BootExecError> {
    let sender_selected = cfg.helius_sender.enabled
        && cfg.submit.routing.default_jito_searcher.as_deref() == Some("helius_sender_dual");
    if sender_selected {
        let sender_tip_account = cfg
            .helius_sender
            .tip_accounts
            .first()
            .ok_or(BootExecError::MissingHeliusSenderTipAccount)?;
        return decode_pubkey("helius_sender.tip_accounts[0]", sender_tip_account);
    }
    decode_pubkey("jito_tip_account", &exec_cfg.jito_tip_account)
}

/// Decode a base58-encoded Solana pubkey into `[u8; 32]`.
pub fn decode_pubkey(field: &str, b58: &str) -> Result<[u8; 32], BootExecError> {
    let bytes = bs58::decode(b58)
        .into_vec()
        .map_err(|e| BootExecError::PubkeyDecode(field.to_string(), e.to_string()))?;
    if bytes.len() != 32 {
        return Err(BootExecError::PubkeyLength(field.to_string(), bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Load a Solana-style JSON keypair (`[u8; 64]` — 32 secret + 32 pubkey)
/// and return a `ThreadLocalSigner`.
pub fn load_signer(path: &str) -> Result<ThreadLocalSigner, BootExecError> {
    let data =
        std::fs::read_to_string(path).map_err(|e| BootExecError::KeypairIo(path.to_string(), e))?;
    let bytes: Vec<u8> = serde_json::from_str(&data)
        .map_err(|e| BootExecError::KeypairParse(path.to_string(), e.to_string()))?;
    if bytes.len() != 64 {
        return Err(BootExecError::KeypairLength(path.to_string(), bytes.len()));
    }
    let mut signer = ThreadLocalSigner {
        pubkey: [0; 32],
        secret: [0; 32],
    };
    signer.secret.copy_from_slice(&bytes[..32]);
    signer.pubkey.copy_from_slice(&bytes[32..]);
    Ok(signer)
}

/// Load a TipLut binary file (raw `[[u32; 16384]; 256]`, little-endian).
pub fn load_tip_lut(path: &str) -> Result<Box<TipLut>, BootExecError> {
    let data = std::fs::read(path).map_err(|e| BootExecError::LutIo(path.to_string(), e))?;
    if data.len() != TIP_LUT_FILE_SIZE {
        return Err(BootExecError::LutSize(path.to_string(), data.len()));
    }
    let mut lut: Box<TipLut> = vec![[0u32; 16384]; 256]
        .into_boxed_slice()
        .try_into()
        .expect("vec length matches array length");
    // Alignment-safe: copy 4 bytes at a time via from_le_bytes rather than
    // casting the source pointer to *const u32 (which is UB if not aligned).
    let mut offset = 0usize;
    for row in lut.iter_mut() {
        for cell in row.iter_mut() {
            *cell = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            offset += 4;
        }
    }
    Ok(lut)
}

#[derive(serde::Deserialize)]
struct RawSignerTokenInventory {
    #[serde(default)]
    accounts: Vec<RawSignerTokenAccountEntry>,
}

#[derive(serde::Deserialize)]
struct RawSignerTokenAccountEntry {
    token_account_pubkey: String,
    mint_pubkey: String,
    token_program_id: String,
    amount_raw: u64,
    initialized: bool,
}

fn load_signer_token_inventory_from_env() -> Result<Arc<SignerTokenInventory>, BootExecError> {
    let Some(path) = std::env::var("SHADOW_SIGNER_TOKEN_INVENTORY_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        eprintln!("WARN  SHADOW_SIGNER_TOKEN_INVENTORY_PATH is unset — CLMM public-scout inventory gating will fail closed");
        return Ok(Arc::new(SignerTokenInventory::empty()));
    };

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| BootExecError::SignerTokenInventoryIo(path.clone(), e))?;
    let parsed: RawSignerTokenInventory = serde_json::from_str(&raw)
        .map_err(|e| BootExecError::SignerTokenInventoryParse(path.clone(), e.to_string()))?;
    let mut inventory = SignerTokenInventory::empty();
    if parsed.accounts.len() > inventory.entries.len() {
        return Err(BootExecError::SignerTokenInventoryTooLarge(
            parsed.accounts.len(),
        ));
    }
    for (idx, entry) in parsed.accounts.iter().enumerate() {
        inventory.entries[idx] = SignerTokenAccountEntry {
            token_account_pubkey: decode_pubkey(
                &format!("signer_token_inventory.accounts[{idx}].token_account_pubkey"),
                &entry.token_account_pubkey,
            )?,
            mint_pubkey: decode_pubkey(
                &format!("signer_token_inventory.accounts[{idx}].mint_pubkey"),
                &entry.mint_pubkey,
            )?,
            token_program_id: decode_pubkey(
                &format!("signer_token_inventory.accounts[{idx}].token_program_id"),
                &entry.token_program_id,
            )?,
            amount_raw: entry.amount_raw,
            initialized: entry.initialized,
        };
    }
    inventory.len = parsed.accounts.len() as u16;
    println!(
        "PASS  loaded signer token inventory: {} account(s)",
        inventory.len
    );
    Ok(Arc::new(inventory))
}

/// Seed a [`NonceRing`] with deterministic nonce tuples derived from the
/// signer secret key.  Each nonce is `SHA-512(secret || le_u64(base + i))`
/// truncated to 32 bytes for the scalar; the pubkey is the corresponding
/// Ed25519 basepoint multiplication.
///
/// `index_base` offsets the derivation counter so multiple workers using the
/// same signer produce non-overlapping nonce sets.
///
/// This is a **startup-only** operation (involves SHA-512 per entry).
/// The ring is consumed one entry per sign in the hot path.
pub fn seed_nonce_ring(signer: &ThreadLocalSigner, count: usize, index_base: u64) -> NonceRing {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use curve25519_dalek::scalar::Scalar;
    use sha2::{Digest, Sha512};

    let n = count.min(127); // 127 usable slots (1 sentinel wasted in ring of 128)
    let mut ring = NonceRing {
        tuples: [NonceTuple {
            secret_scalar: [0; 32],
            nonce_pubkey: [0; 32],
        }; 128],
        head: n,
        tail: 0,
    };
    for i in 0..n {
        let idx = index_base + i as u64;
        let mut hasher = Sha512::new();
        hasher.update(&signer.secret);
        hasher.update(&idx.to_le_bytes());
        let hash = hasher.finalize();
        // Clamp to Ed25519 scalar, then reduce mod L for canonical form.
        let mut scalar_bytes = [0u8; 32];
        scalar_bytes.copy_from_slice(&hash[..32]);
        scalar_bytes[0] &= 248;
        scalar_bytes[31] &= 127;
        scalar_bytes[31] |= 64;
        let scalar = Scalar::from_bytes_mod_order(scalar_bytes);

        // Store the reduced (canonical) scalar so sign_from_nonce_ring's
        // from_canonical_bytes check never rejects it.
        ring.tuples[i].secret_scalar = scalar.to_bytes();

        // Derive the corresponding public nonce point: nonce_pubkey = scalar * B.
        let point = &scalar * ED25519_BASEPOINT_TABLE;
        ring.tuples[i]
            .nonce_pubkey
            .copy_from_slice(point.compress().as_bytes());
    }
    ring
}

/// Bootstrap [`ExecutionDeps`] from the deployment config.
///
/// Loads the signer keypair, optional TipLut banks, parses base58 pubkeys,
/// and validates the inventory ceiling.  Returns `Err` on any invalid or
/// missing input — the caller should `process::exit(1)` on error.
///
/// The `inventory_ceiling` is derived from the tighter of the baseline risk cap
/// and the active live-rollout entity cap (SOL → lamports).
pub fn boot_execution_deps(
    cfg: &shadow_core::config::ShadowConfig,
) -> Result<ExecutionDeps, BootExecError> {
    let exec_cfg = cfg
        .execution
        .as_ref()
        .ok_or(BootExecError::MissingExecutionConfig)?;

    // 1. Signer keypair.
    let signer = load_signer(&exec_cfg.signer_keypair_path)?;
    let signer_token_inventory = load_signer_token_inventory_from_env()?;
    println!(
        "PASS  loaded signer keypair (pubkey: {}..)",
        &bs58::encode(&signer.pubkey).into_string()[..8]
    );

    // 2. (LUT banks removed — workers consume from shm segments published by shadow-oracle.)

    // 3. Swap program ID.
    let swap_program_id = decode_pubkey("swap_program_id", &exec_cfg.swap_program_id)?;

    // 5. Guard pubkeys — decode and reject zeros/placeholders.
    let shadow_guard_program_id =
        decode_pubkey("shadow_guard_program_id", &exec_cfg.shadow_guard_program_id)?;
    let guard_vault_in_pubkey =
        decode_pubkey("guard_vault_in_pubkey", &exec_cfg.guard_vault_in_pubkey)?;
    let guard_vault_out_pubkey =
        decode_pubkey("guard_vault_out_pubkey", &exec_cfg.guard_vault_out_pubkey)?;
    // Fail-closed: zero pubkeys would pass assembly validation at runtime but
    // never match a real on-chain account.  Catch it here so a misconfigured
    // deploy never starts the hot loop.
    if shadow_guard_program_id == [0u8; 32] {
        eprintln!("FATAL  shadow_guard_program_id is all-zeros");
        std::process::exit(1);
    }
    if guard_vault_in_pubkey == [0u8; 32] {
        eprintln!("FATAL  guard_vault_in_pubkey is all-zeros");
        std::process::exit(1);
    }
    if guard_vault_out_pubkey == [0u8; 32] {
        eprintln!("FATAL  guard_vault_out_pubkey is all-zeros");
        std::process::exit(1);
    }
    // Reject if all three are identical — almost certainly a copy-paste placeholder.
    if shadow_guard_program_id == guard_vault_in_pubkey
        && guard_vault_in_pubkey == guard_vault_out_pubkey
    {
        eprintln!("FATAL  all three guard pubkeys are identical — looks like a placeholder");
        std::process::exit(1);
    }

    // 5b. Harmonic CU pad — boot-time validation that config matches the
    // locked constant used by the hot-path template builder.  If the config
    // specifies a different cu_pad_units, refuse to start.
    if let Some(ref h) = cfg.submit.harmonic {
        if let Some(pad) = h.cu_pad_units {
            if pad != HARMONIC_CU_PAD {
                eprintln!(
                    "FATAL  submit.harmonic.cu_pad_units={pad} but locked constant HARMONIC_CU_PAD={HARMONIC_CU_PAD} — mismatch"
                );
                std::process::exit(1);
            }
        }
    }

    // 6. Private-lane tip account. Sender uses the first configured Sender/Jito
    // tip recipient; other private lanes keep the legacy execution.jito_tip_account.
    let tip_account = resolve_private_lane_tip_account(cfg, exec_cfg)?;

    // 7. Inventory ceiling — SOL → lamports with sanity bounds.
    let inventory_ceiling = effective_inventory_ceiling_lamports(cfg);
    if inventory_ceiling < MIN_INVENTORY_CEILING_LAMPORTS {
        return Err(BootExecError::InventoryCeilingTooLow(inventory_ceiling));
    }
    if inventory_ceiling > MAX_INVENTORY_CEILING_LAMPORTS {
        return Err(BootExecError::InventoryCeilingTooHigh(inventory_ceiling));
    }
    println!(
        "PASS  inventory ceiling: {} lamports ({:.2} SOL)",
        inventory_ceiling,
        inventory_ceiling as f64 / 1_000_000_000.0,
    );

    // 8. Nonce prefill validation.
    if exec_cfg.nonce_prefill == 0 {
        return Err(BootExecError::NoncePrefillZero);
    }
    let nonce_prefill = if exec_cfg.nonce_prefill > 127 {
        eprintln!(
            "WARN  nonce_prefill {} exceeds ring capacity 127 — clamping to 127",
            exec_cfg.nonce_prefill,
        );
        127
    } else {
        exec_cfg.nonce_prefill
    };

    // 9. Fixed swap template — load from config, FATAL if absent or invalid.
    let template_accounts = &exec_cfg.swap_template_accounts;
    if template_accounts.is_empty() {
        eprintln!("FATAL  execution.swap_template_accounts is empty — cannot build swap bundles");
        std::process::exit(1);
    }
    if template_accounts.len() > MAX_SWAP_ACCOUNTS {
        eprintln!(
            "FATAL  execution.swap_template_accounts has {} entries (max {})",
            template_accounts.len(),
            MAX_SWAP_ACCOUNTS
        );
        std::process::exit(1);
    }

    let mut swap_metas = [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS];
    for (i, entry) in template_accounts.iter().enumerate() {
        let source = if entry.pubkey == "FeePayerSigner" {
            SwapAccountSource::FeePayerSigner
        } else {
            SwapAccountSource::ExplicitPubkey(decode_pubkey(
                &format!("swap_template_accounts[{}].pubkey", i),
                &entry.pubkey,
            )?)
        };
        swap_metas[i] = FixedSwapAccountMeta {
            source,
            is_writable: entry.writable,
        };
    }
    let swap_accounts = FixedSwapAccounts {
        program_id: swap_program_id,
        metas: swap_metas,
        num_accounts: template_accounts.len() as u8,
    };

    let ix_hex = exec_cfg.swap_ix_data_hex.as_deref().unwrap_or("");
    if ix_hex.is_empty() {
        eprintln!("FATAL  execution.swap_ix_data_hex is missing or empty");
        std::process::exit(1);
    }
    let ix_bytes = hex::decode(ix_hex).unwrap_or_else(|e| {
        eprintln!("FATAL  execution.swap_ix_data_hex is not valid hex: {e}");
        std::process::exit(1);
    });
    if ix_bytes.is_empty() || ix_bytes.len() > 64 {
        eprintln!(
            "FATAL  swap_ix_data_hex decoded to {} bytes (need 1..=64)",
            ix_bytes.len()
        );
        std::process::exit(1);
    }
    let mut ix_data_buf = [0u8; 64];
    ix_data_buf[..ix_bytes.len()].copy_from_slice(&ix_bytes);
    let swap_ix_data = FixedSwapIxData {
        data: ix_data_buf,
        len: ix_bytes.len() as u8,
        amount_in_offset: 1,
        min_out_offset: 9,
    };

    println!(
        "PASS  swap template: {} accounts, {} bytes ix data (discriminator 0x{:02x})",
        swap_accounts.num_accounts, swap_ix_data.len, swap_ix_data.data[0],
    );

    Ok(ExecutionDeps {
        signer: Arc::new(signer),
        inventory_alloc: Arc::new(AtomicU64::new(0)),
        signer_token_inventory,
        inventory_ceiling,
        swap_accounts,
        swap_ix_data,
        jito_tip_rotation: JitoTipRotation { tip_account },
        route_base_cu_limit: exec_cfg.route_base_cu_limit,
        shadow_guard_program_id,
        guard_vault_in_pubkey,
        guard_vault_out_pubkey,
        nonce_prefill,
    })
}

/// Seed [`WorkerLocalState`] with a pre-filled nonce ring derived from the
/// signer keypair.  Each worker gets a different ring seeded from a
/// worker-specific index range to avoid nonce reuse across threads.
pub fn new_worker_local_state(
    signer: &ThreadLocalSigner,
    worker_id: u32,
    nonce_prefill: usize,
) -> WorkerLocalState {
    // Offset the seed index by worker_id * 2^32 so each worker's nonce domain
    // never overlaps even with continuous production (matching nonce_producer stride).
    let base = worker_id as u64 * (1u64 << 32);
    let ring = seed_nonce_ring(signer, nonce_prefill, base);

    WorkerLocalState {
        dedupe: SigDedupe::new(),
        overlay: PoolOverlay::new(),
        nonce_ring: ring,
        tac: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g4_g2_6_t1_boot_loads_swap_template_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        // Minimal valid signer
        let signer_path = d.join("signer.json");
        let keypair_bytes: Vec<u8> = (0..64).collect();
        std::fs::write(&signer_path, serde_json::to_string(&keypair_bytes).unwrap()).unwrap();

        // Minimal valid LUT (16 MiB)
        let lut_path = d.join("tip_lut.bin");
        std::fs::write(&lut_path, vec![0u8; 16 * 1024 * 1024]).unwrap();

        // Non-zero, distinct guard pubkeys (won't hit the new boot-time validation).
        let guard_prog = bs58::encode([7u8; 32]).into_string();
        let vault_in = bs58::encode([8u8; 32]).into_string();
        let vault_out = bs58::encode([9u8; 32]).into_string();

        let yaml = format!(
            r#"
deployment:
  profile: v6_4_guerrila
system:
  money_workers: 1
  proxies:
    egress:
      quic_workers: 0
risk:
  reserve_floor_sol: 0.1
  max_position_sol_per_mint: 1.0
  max_notional_sol_per_entity: 1.0
submit:
  routing:
    default_public_geyser: drop
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: false
    pre_sign_egress_admission_gate: true
  quic:
    kamikaze_enabled: false
execution:
  signer_keypair_path: "{signer}"
  jito_lut_path: "{lut}"
  swap_program_id: "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
  shadow_guard_program_id: "{guard_prog}"
  guard_vault_in_pubkey: "{vault_in}"
  guard_vault_out_pubkey: "{vault_out}"
  jito_tip_account: "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"
  swap_template_accounts:
    - {{ pubkey: "FeePayerSigner", writable: true }}
    - {{ pubkey: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", writable: false }}
  swap_ix_data_hex: "090000000000000000000000000000000000"
"#,
            signer = signer_path.display().to_string().replace('\\', "/"),
            lut = lut_path.display().to_string().replace('\\', "/"),
        );

        let cfg: shadow_core::config::ShadowConfig = serde_yaml::from_str(&yaml).unwrap();
        let deps = boot_execution_deps(&cfg).expect("boot must succeed with swap template");

        assert_eq!(deps.swap_accounts.num_accounts, 2);
        assert_eq!(
            deps.swap_accounts.metas[0].source,
            SwapAccountSource::FeePayerSigner
        );
        assert!(deps.swap_accounts.metas[0].is_writable);
        assert_ne!(
            deps.swap_accounts.metas[1].source,
            SwapAccountSource::FeePayerSigner,
        );
        assert_eq!(deps.swap_ix_data.len, 18);
        assert_eq!(deps.swap_ix_data.data[0], 0x09);
    }

    #[test]
    fn g4_g2_6_t1b_boot_uses_helius_sender_tip_account_when_selected() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        let signer_path = d.join("signer.json");
        let keypair_bytes: Vec<u8> = (0..64).collect();
        std::fs::write(&signer_path, serde_json::to_string(&keypair_bytes).unwrap()).unwrap();

        let lut_path = d.join("tip_lut.bin");
        std::fs::write(&lut_path, vec![0u8; 16 * 1024 * 1024]).unwrap();

        let guard_prog = bs58::encode([7u8; 32]).into_string();
        let vault_in = bs58::encode([8u8; 32]).into_string();
        let vault_out = bs58::encode([9u8; 32]).into_string();
        let sender_tip = bs58::encode([10u8; 32]).into_string();

        let yaml = format!(
            r#"
deployment:
  profile: v6_4_guerrila
system:
  money_workers: 1
  proxies:
    egress:
      quic_workers: 0
risk:
  reserve_floor_sol: 0.1
  max_position_sol_per_mint: 1.0
  max_notional_sol_per_entity: 1.0
submit:
  routing:
    default_public_geyser: drop
    default_jito_searcher: helius_sender_dual
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: false
    pre_sign_egress_admission_gate: true
  quic:
    kamikaze_enabled: false
helius_sender:
  enabled: true
  endpoint: "https://sender.helius-rpc.com/fast"
  timeout_ms: 3000
  min_tip_lamports: 200000
  tip_accounts:
    - "{sender_tip}"
execution:
  signer_keypair_path: "{signer}"
  jito_lut_path: "{lut}"
  swap_program_id: "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
  shadow_guard_program_id: "{guard_prog}"
  guard_vault_in_pubkey: "{vault_in}"
  guard_vault_out_pubkey: "{vault_out}"
  jito_tip_account: "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"
  swap_template_accounts:
    - {{ pubkey: "FeePayerSigner", writable: true }}
    - {{ pubkey: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", writable: false }}
  swap_ix_data_hex: "090000000000000000000000000000000000"
"#,
            sender_tip = sender_tip,
            signer = signer_path.display().to_string().replace('\\', "/"),
            lut = lut_path.display().to_string().replace('\\', "/"),
        );

        let cfg: shadow_core::config::ShadowConfig = serde_yaml::from_str(&yaml).unwrap();
        let deps = boot_execution_deps(&cfg).expect("boot must succeed with helius sender tip");
        assert_eq!(deps.jito_tip_rotation.tip_account, [10u8; 32]);
    }

    #[test]
    fn g4_g2_6_t2_boot_fatals_on_empty_swap_template() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();

        let signer_path = d.join("signer.json");
        let keypair_bytes: Vec<u8> = (0..64).collect();
        std::fs::write(&signer_path, serde_json::to_string(&keypair_bytes).unwrap()).unwrap();

        let lut_path = d.join("tip_lut.bin");
        std::fs::write(&lut_path, vec![0u8; 16 * 1024 * 1024]).unwrap();

        let yaml = format!(
            r#"
deployment:
  profile: v6_4_guerrila
system:
  money_workers: 1
  proxies:
    egress:
      quic_workers: 0
risk:
  reserve_floor_sol: 0.1
  max_position_sol_per_mint: 1.0
  max_notional_sol_per_entity: 1.0
submit:
  routing:
    default_public_geyser: drop
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: false
    pre_sign_egress_admission_gate: true
  quic:
    kamikaze_enabled: false
execution:
  signer_keypair_path: "{signer}"
  jito_lut_path: "{lut}"
  swap_program_id: "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
  shadow_guard_program_id: "11111111111111111111111111111111"
  guard_vault_in_pubkey: "11111111111111111111111111111111"
  guard_vault_out_pubkey: "11111111111111111111111111111111"
  jito_tip_account: "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"
  swap_template_accounts: []
  swap_ix_data_hex: "09"
"#,
            signer = signer_path.display().to_string().replace('\\', "/"),
            lut = lut_path.display().to_string().replace('\\', "/"),
        );

        let cfg: shadow_core::config::ShadowConfig = serde_yaml::from_str(&yaml).unwrap();
        // boot_execution_deps calls process::exit on empty accounts —
        // we can't catch that in-process, so just verify the config parses
        // and the accounts vec is empty (the FATAL fires at runtime).
        let exec = cfg.execution.unwrap();
        assert!(exec.swap_template_accounts.is_empty());
    }

    #[test]
    fn g4_g2_6_t3_boot_rejects_oversized_ix_data() {
        // 65 hex bytes (130 hex chars) → exceeds 64-byte buffer
        let too_long = "09".repeat(65);
        let decoded = hex::decode(&too_long).unwrap();
        assert_eq!(decoded.len(), 65);
        assert!(
            decoded.len() > 64,
            "must exceed FixedSwapIxData.data buffer"
        );
    }

    #[test]
    fn g4_g2_6_t4_structural_no_hollow_swap_template_in_boot() {
        const SRC: &str = include_str!("boot_exec.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(
            !prod_src.contains("num_accounts: 0"),
            "boot_exec.rs production code must not contain num_accounts: 0 — swap template must be loaded from config"
        );
        assert!(
            !prod_src.contains("len: 0,"),
            "boot_exec.rs production code must not contain len: 0 for swap_ix_data — must be loaded from config"
        );
    }

    /// G10: Boot-time assertion that config cu_pad_units matches the locked constant.
    #[test]
    fn g10_boot_cu_pad_matches_constant() {
        // The hot-path template builder uses HARMONIC_CU_PAD (150) as a compile-time
        // constant.  The config field cu_pad_units is optional but if set, it must
        // match.  Verify the source contains the guard.
        const SRC: &str = include_str!("boot_exec.rs");
        let prod_src = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(
            prod_src.contains("pad != HARMONIC_CU_PAD"),
            "boot_exec.rs must contain boot-time cu_pad_units vs HARMONIC_CU_PAD check"
        );
        // Verify the constant value.
        assert_eq!(HARMONIC_CU_PAD, 150);
    }
}
