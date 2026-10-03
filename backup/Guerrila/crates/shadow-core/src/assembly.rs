use crate::inventory::InventoryFencer;
use crate::opso::OpsoObserveOnlyEconomics;
use crate::policy::{DominanceDecision, PreparedExecution};
use crate::routing::{
    route_family_telemetry_code, route_uses_jito_lane, IntentKind, RouteDecision,
};
use serde::Serialize;
use shadow_ipc::ingress_record::{SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK};
use shadow_ipc::rollout_shadow::{G12DropReason, RolloutShadowEvent};
use std::time::Instant;

// ── Well-known Solana program IDs (base58-decoded, compile-time constants) ──
// ComputeBudget111111111111111111111111111111
pub const COMPUTE_BUDGET_PROGRAM_ID: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];
// System Program is all zeroes on Solana
pub const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];
// ShadowGuard program id is supplied by the caller at runtime.
// Do not hardcode placeholder bytes here.

/// Audited CU pad for the Harmonic template. Boot-validated against
/// config.submit.harmonic.cu_pad_units by boot_exec.rs.
pub const HARMONIC_CU_PAD: u32 = 150;
const VENUE_RAYDIUM_CLMM: u8 = 3;
const RAYDIUM_CLMM_PROGRAM_ID: [u8; 32] = [
    0xa5, 0xd5, 0xca, 0x9e, 0x04, 0xcf, 0x5d, 0xb5, 0x90, 0xb7, 0x14, 0xba, 0x2f, 0xe3, 0x2c, 0xb1,
    0x59, 0x13, 0x3f, 0xc1, 0xc1, 0x92, 0xb7, 0x22, 0x57, 0xfd, 0x07, 0xd3, 0x9c, 0xb0, 0x40, 0x1e,
];
pub const MAX_SWAP_ACCOUNTS: usize = 32;

/// ShadowGuard instruction data length (4 × u64 = 32 bytes, no discriminator).
pub const SHADOW_GUARD_IX_DATA_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateId {
    JitoPlain,
    HarmonicPlain,
    JitoAtba,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardBounds {
    pub min_reserve_in: u64,
    pub max_reserve_in: u64,
    pub min_reserve_out: u64,
    pub max_reserve_out: u64,
}
/// Guerrila templates currently support exactly one signer: the fee payer / worker signer.
/// all swap_accounts.is_signer[..num_accounts] must be false
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapAccountSource {
    FeePayerSigner,
    ExplicitPubkey([u8; 32]),
}

#[derive(Debug, Clone, Copy)]
pub struct FixedSwapAccountMeta {
    pub source: SwapAccountSource,
    pub is_writable: bool,
}

pub const EMPTY_SWAP_ACCOUNT_META: FixedSwapAccountMeta = FixedSwapAccountMeta {
    source: SwapAccountSource::ExplicitPubkey([0u8; 32]),
    is_writable: false,
};

#[derive(Debug, Clone)]
pub struct FixedSwapAccounts {
    pub program_id: [u8; 32],
    pub metas: [FixedSwapAccountMeta; MAX_SWAP_ACCOUNTS],
    pub num_accounts: u8,
}
#[derive(Debug, Clone)]
pub struct FixedSwapIxData {
    pub data: [u8; 64],
    pub len: u8,
    pub amount_in_offset: u8,
    pub min_out_offset: u8,
}
#[derive(Debug, Clone)]
pub struct OverlayDelta {
    pub pool_index: u32,
    pub reserve_in_delta: i64,
    pub reserve_out_delta: i64,
}
#[derive(Debug, Clone, Copy)]
pub struct JitoTipRotation {
    pub tip_account: [u8; 32],
}
pub struct ExecutionAssemblyInput<'a> {
    pub opso_economics: OpsoObserveOnlyEconomics,
    pub prepared: PreparedExecution,
    pub recent_blockhash: [u8; 32],
    pub ingress_slot: u64,
    pub deadline_unix_ns: u64,
    pub ingress_unix_ns: u64,
    pub signal_source: u8,
    pub template_id: TemplateId,
    pub quote_request_id: u32,
    pub route_base_cu_limit: u32,
    pub configured_route_base_cu_limit: u32,
    pub micro_market_observed: bool,
    pub micro_market_flat_retained_ev_lamports: i64,
    pub micro_market_delta_lamports: i64,
    pub trade_size_lamports: u64,
    pub slippage_bps: u16,
    pub quote_target_out_lamports: u64,
    pub quote_expected_out_lamports: u64,
    pub quote_baseline_reserve_in: u64,
    pub quote_baseline_reserve_out: u64,
    pub quote_projected_reserve_in: u64,
    pub quote_projected_reserve_out: u64,
    pub quote_trade_fee_num: u64,
    pub quote_trade_fee_denom: u64,
    pub quote_mint_in_transfer_fee_bps: u16,
    pub quote_mint_in_max_fee_lamports: u64,
    pub swap_accounts: FixedSwapAccounts,
    pub swap_ix_data: FixedSwapIxData,
    pub guard_bounds: GuardBounds,
    pub pool_index: u32,
    pub mint_out_index: u32,
    pub pool_pubkey: [u8; 32],
    pub mint_in_pubkey: [u8; 32],
    pub mint_out_pubkey: [u8; 32],
    pub overlay_delta: OverlayDelta,
    pub jito_tip_rotation: JitoTipRotation,
    pub inventory_fencer: &'a InventoryFencer<'a>,
    /// Real fee-payer pubkey written into account[0] of the message.
    /// Copied from `ThreadLocalSigner::pubkey` at the pipeline call site.
    pub fee_payer_pubkey: [u8; 32],
    /// Real ShadowGuard program id. Must not be a placeholder.
    pub shadow_guard_program_id: [u8; 32],
    /// Read-only vault accounts required by ShadowGuard.
    pub guard_vault_in_pubkey: [u8; 32],
    pub guard_vault_out_pubkey: [u8; 32],
    /// Metadata for PM truth threading (G8-QC #3/#6).
    pub send_timestamp_ns: u64,
    pub tactic_class: u16,
    pub worker_id: u8,
    /// Venue / DEX identifier from IngressRecord (0=RaydiumV4, etc.).
    pub venue_id: u8,
    /// Whether this Jito-shaped route is explicitly configured to use public RPC scout egress.
    pub public_rpc_scout_explicit: bool,
    pub episode_id_mod: u32,
    pub opso_owner_mode: u8,
    pub opso_fallback_mode: u8,
    pub opso_reason_code: u8,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    TacticNotAllowedOnRoute,
    InvalidTemplateForRoute,
    OversizedPayload,
    UnsupportedAdditionalSigner,
    InvalidGuardPubkey,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    RingEmpty,
    InternalError,
    NonCanonicalNonceScalar,
    FeePayerPubkeyMismatch,
}
// SignedTxEnvelope lives in the IPC layer so that both worker and proxy
// crates share one definition over /dev/shm rings.
pub use shadow_ipc::egress_ring::SignedTxEnvelope;

/// Minimal push interface for egress rings.
///
/// Production code uses `MappedSpscRing<SignedTxEnvelope>` (file-backed
/// shared-memory SPSC); test code uses a local in-memory stub.
pub trait EgressRingPush {
    /// Try to enqueue `item`. Returns `true` on success, `false` if full.
    fn try_push_envelope(&self, item: &SignedTxEnvelope) -> bool;
}

impl EgressRingPush for shadow_ipc::ring::MappedSpscRing<SignedTxEnvelope> {
    #[inline(always)]
    fn try_push_envelope(&self, item: &SignedTxEnvelope) -> bool {
        self.push(item)
    }
}

pub trait RolloutShadowPush {
    fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool;
}

pub struct ThreadLocalSigner {
    pub pubkey: [u8; 32],
    pub secret: [u8; 32],
}
#[derive(Debug, Clone, Copy)]
pub struct NonceTuple {
    pub secret_scalar: [u8; 32],
    pub nonce_pubkey: [u8; 32],
}
pub struct NonceRing {
    pub tuples: [NonceTuple; 128],
    pub head: usize,
    pub tail: usize,
}

#[inline(always)]
fn zeroize_bytes(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
}
impl NonceRing {
    /// Number of tuples available for `pop()`.
    #[inline(always)]
    pub fn available(&self) -> usize {
        self.head.wrapping_sub(self.tail) % 128
    }

    /// Push a nonce tuple into the ring.  Returns `false` (ring full) if
    /// `available() == 127` (one slot is always wasted as sentinel).
    #[inline(always)]
    pub fn push(&mut self, t: NonceTuple) -> bool {
        let next = (self.head + 1) % 128;
        if next == self.tail {
            return false; // full
        }
        self.tuples[self.head] = t;
        self.head = next;
        true
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Option<NonceTuple> {
        if self.head == self.tail {
            return None;
        }
        let mut t = NonceTuple {
            secret_scalar: [0; 32],
            nonce_pubkey: [0; 32],
        };
        t.secret_scalar
            .copy_from_slice(&self.tuples[self.tail].secret_scalar);
        t.nonce_pubkey
            .copy_from_slice(&self.tuples[self.tail].nonce_pubkey);

        zeroize_bytes(&mut self.tuples[self.tail].secret_scalar);
        zeroize_bytes(&mut self.tuples[self.tail].nonce_pubkey);

        self.tail = (self.tail + 1) % 128;
        Some(t)
    }
}
pub fn sign_from_nonce_ring(
    unsigned: &UnsignedEnvelope,
    signer: &ThreadLocalSigner,
    nonce_ring: &mut NonceRing,
) -> Result<SignedTxEnvelope, SignError> {
    let mut tuple = nonce_ring.pop().ok_or(SignError::RingEmpty)?;

    let mut signed = SignedTxEnvelope {
        raw_bytes: [0; 1232],
        len: 0,
        signature: [0; 64],
        pool_index: 0,
        mint_out_index: 0,
        reserved_notional_lamports: 0,
        send_timestamp_ns: 0,
        tactic_class: 0,
        worker_id: 0,
        venue_id: 0,
        opso_owner_mode: 0,
        opso_fallback_mode: 7,
        opso_reason_code: 0,
        episode_id_mod: 0,
        actual_bid_lamports: 0,
        baseline_bid_lamports: 0,
        opso_bid_lamports: 0,
        fallback_realized_value_lamports: 0,
        missed_backrun_value_lamports: 0,
        tip_saved_lamports: 0,
        opso_ev_lamports: 0,
        shading_gain_lamports: 0,
        _envelope_pad: [0; 4],
    };

    if unsigned.len + 65 > 1232 {
        zeroize_bytes(&mut tuple.secret_scalar);
        zeroize_bytes(&mut tuple.nonce_pubkey);
        return Err(SignError::InternalError);
    }
    // Format Solana array Header (Compact-u16 signatures len = 1)
    signed.raw_bytes[0] = 1;

    use curve25519_dalek::scalar::Scalar;
    use sha2::{Digest, Sha512};

    // Hash the true Message bytes
    let mut hasher = Sha512::new();
    hasher.update(&tuple.nonce_pubkey);
    hasher.update(&signer.pubkey);
    hasher.update(&unsigned.raw_bytes[..unsigned.len]);
    let k_bytes = hasher.finalize();
    let k = Scalar::from_bytes_mod_order_wide(&k_bytes.into());

    let mut a_hasher = Sha512::new();
    a_hasher.update(&signer.secret);
    let a_hash = a_hasher.finalize();
    let mut a_bytes = [0u8; 32];
    a_bytes.copy_from_slice(&a_hash[..32]);
    a_bytes[0] &= 248;
    a_bytes[31] &= 127;
    a_bytes[31] |= 64;
    let a = Scalar::from_bytes_mod_order(a_bytes);

    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&tuple.secret_scalar);
    let r_opt: Option<Scalar> = Scalar::from_canonical_bytes(r_bytes).into();
    let r = match r_opt {
        Some(s) => s,
        None => {
            zeroize_bytes(&mut tuple.secret_scalar);
            zeroize_bytes(&mut tuple.nonce_pubkey);
            zeroize_bytes(&mut a_bytes);
            zeroize_bytes(&mut r_bytes);
            return Err(SignError::NonCanonicalNonceScalar);
        }
    };

    let s = r + k * a;
    let s_bytes = s.to_bytes();

    let mut sig_bytes = [0u8; 64];
    sig_bytes[..32].copy_from_slice(&tuple.nonce_pubkey);
    sig_bytes[32..].copy_from_slice(&s_bytes);

    // Array Layout: [1 byte for signature count] || [64 byte sig] || [Message layout...]
    signed.raw_bytes[1..65].copy_from_slice(&sig_bytes);
    signed.raw_bytes[65..65 + unsigned.len].copy_from_slice(&unsigned.raw_bytes[..unsigned.len]);
    signed.len = 65 + unsigned.len;
    signed.signature = sig_bytes;

    zeroize_bytes(&mut tuple.secret_scalar);
    zeroize_bytes(&mut tuple.nonce_pubkey);
    zeroize_bytes(&mut a_bytes);
    zeroize_bytes(&mut r_bytes);

    Ok(signed)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    BuildFailed(BuildError),
    SignFailed(SignError),
    EnqueueFailedRingFull,
    NoSendSinkFull,
    DeadlineExceeded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitReceipt {
    pub signature: [u8; 64],
    pub pool_index: u32,
    pub mint_out_index: u32,
    pub reserved_notional_lamports: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct NoSendAssemblyProbe {
    pub build_template_ns: u64,
    pub sign_ns: u64,
    pub emit_nosend_ns: u64,
}
#[cfg(test)]
pub struct EgressRing {
    pub items: std::cell::RefCell<Vec<SignedTxEnvelope>>,
    pub capacity: usize,
}
#[cfg(test)]
impl EgressRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: std::cell::RefCell::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }
    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }
}
#[cfg(test)]
impl EgressRingPush for EgressRing {
    fn try_push_envelope(&self, item: &SignedTxEnvelope) -> bool {
        let mut items = self.items.borrow_mut();
        if items.len() >= self.capacity {
            return false;
        }
        items.push(*item);
        true
    }
}

#[cfg(test)]
pub struct RolloutShadowRing {
    pub items: std::cell::RefCell<Vec<RolloutShadowEvent>>,
    pub capacity: usize,
}

#[cfg(test)]
impl RolloutShadowRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: std::cell::RefCell::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }
}

#[cfg(test)]
impl RolloutShadowPush for RolloutShadowRing {
    fn try_push_shadow(&self, item: &RolloutShadowEvent) -> bool {
        let mut items = self.items.borrow_mut();
        if items.len() >= self.capacity {
            return false;
        }
        items.push(*item);
        true
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolOverlaySlot {
    pub pool_index: u32,
    pub reserve_in: i64,
    pub reserve_out: i64,
}
/// Per-worker pre-confirmation shadow reserve state.
/// Fixed-size (16 slots max), zero-allocation.
pub struct PoolOverlay {
    pub slots: [PoolOverlaySlot; 16],
    pub count: usize,
}
impl PoolOverlay {
    pub const fn new() -> Self {
        Self {
            slots: [PoolOverlaySlot {
                pool_index: 0,
                reserve_in: 0,
                reserve_out: 0,
            }; 16],
            count: 0,
        }
    }
    /// Accumulate a reserve delta for `delta.pool_index`.
    /// If the pool is already tracked, its reserves are updated in-place.
    /// If the overlay is full (16 pools), the delta is silently dropped to
    /// preserve zero-allocation — the caller must flush before that point.
    pub fn apply(&mut self, delta: &OverlayDelta) {
        for i in 0..self.count {
            if self.slots[i].pool_index == delta.pool_index {
                self.slots[i].reserve_in = self.slots[i]
                    .reserve_in
                    .saturating_add(delta.reserve_in_delta);
                self.slots[i].reserve_out = self.slots[i]
                    .reserve_out
                    .saturating_add(delta.reserve_out_delta);
                return;
            }
        }
        if self.count < 16 {
            self.slots[self.count] = PoolOverlaySlot {
                pool_index: delta.pool_index,
                reserve_in: delta.reserve_in_delta,
                reserve_out: delta.reserve_out_delta,
            };
            self.count += 1;
        }
    }
    /// Read the current shadow reserve for pool_index, if tracked.
    pub fn get(&self, pool_index: u32) -> Option<(i64, i64)> {
        for i in 0..self.count {
            if self.slots[i].pool_index == pool_index {
                return Some((self.slots[i].reserve_in, self.slots[i].reserve_out));
            }
        }
        None
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedEnvelope {
    pub raw_bytes: [u8; 1232],
    pub len: usize,
}

const MAX_TEMPLATE_UNSIGNED_ACCOUNTS: usize = MAX_SWAP_ACCOUNTS + 3;
const CANONICAL_FEE_PAYER_REF: u8 = u8::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CanonicalUnsignedAccount {
    pub pubkey: [u8; 32],
    pub is_writable: bool,
}

const EMPTY_CANONICAL_UNSIGNED_ACCOUNT: CanonicalUnsignedAccount = CanonicalUnsignedAccount {
    pubkey: [0u8; 32],
    is_writable: false,
};

#[derive(Debug, Clone, Copy)]
struct CanonicalAccountLayout {
    pub unsigned_accounts: [CanonicalUnsignedAccount; MAX_TEMPLATE_UNSIGNED_ACCOUNTS],
    pub unsigned_count: u8,
    pub writable_count: u8,
    pub readonly_count: u8,
    pub final_unsigned_idx: [u8; MAX_TEMPLATE_UNSIGNED_ACCOUNTS],
    pub swap_refs: [u8; MAX_SWAP_ACCOUNTS],
    pub guard_in_ref: u8,
    pub guard_out_ref: u8,
    pub tip_ref: u8,
}

#[inline(always)]
fn reserve_canonical_unsigned_account(
    layout: &mut [CanonicalUnsignedAccount; MAX_TEMPLATE_UNSIGNED_ACCOUNTS],
    count: &mut u8,
    fee_payer_pubkey: [u8; 32],
    pubkey: [u8; 32],
    is_writable: bool,
) -> Result<u8, BuildError> {
    if pubkey == fee_payer_pubkey {
        return Ok(CANONICAL_FEE_PAYER_REF);
    }
    for i in 0..*count as usize {
        if layout[i].pubkey == pubkey {
            if is_writable {
                layout[i].is_writable = true;
            }
            return Ok(i as u8);
        }
    }
    if (*count as usize) >= layout.len() {
        return Err(BuildError::OversizedPayload);
    }
    let idx = *count;
    layout[idx as usize] = CanonicalUnsignedAccount {
        pubkey,
        is_writable,
    };
    *count = count.saturating_add(1);
    Ok(idx)
}

#[inline(always)]
fn build_canonical_account_layout(
    input: &ExecutionAssemblyInput,
    include_tip_account: bool,
) -> Result<CanonicalAccountLayout, BuildError> {
    let mut layout = CanonicalAccountLayout {
        unsigned_accounts: [EMPTY_CANONICAL_UNSIGNED_ACCOUNT; MAX_TEMPLATE_UNSIGNED_ACCOUNTS],
        unsigned_count: 0,
        writable_count: 0,
        readonly_count: 0,
        final_unsigned_idx: [0u8; MAX_TEMPLATE_UNSIGNED_ACCOUNTS],
        swap_refs: [CANONICAL_FEE_PAYER_REF; MAX_SWAP_ACCOUNTS],
        guard_in_ref: CANONICAL_FEE_PAYER_REF,
        guard_out_ref: CANONICAL_FEE_PAYER_REF,
        tip_ref: CANONICAL_FEE_PAYER_REF,
    };

    if include_tip_account {
        layout.tip_ref = reserve_canonical_unsigned_account(
            &mut layout.unsigned_accounts,
            &mut layout.unsigned_count,
            input.fee_payer_pubkey,
            input.jito_tip_rotation.tip_account,
            true,
        )?;
    }

    let n = input.swap_accounts.num_accounts as usize;
    for i in 0..n {
        let meta = &input.swap_accounts.metas[i];
        layout.swap_refs[i] = match meta.source {
            SwapAccountSource::FeePayerSigner => CANONICAL_FEE_PAYER_REF,
            SwapAccountSource::ExplicitPubkey(pubkey) => reserve_canonical_unsigned_account(
                &mut layout.unsigned_accounts,
                &mut layout.unsigned_count,
                input.fee_payer_pubkey,
                pubkey,
                meta.is_writable,
            )?,
        };
    }

    layout.guard_in_ref = reserve_canonical_unsigned_account(
        &mut layout.unsigned_accounts,
        &mut layout.unsigned_count,
        input.fee_payer_pubkey,
        input.guard_vault_in_pubkey,
        false,
    )?;
    layout.guard_out_ref = reserve_canonical_unsigned_account(
        &mut layout.unsigned_accounts,
        &mut layout.unsigned_count,
        input.fee_payer_pubkey,
        input.guard_vault_out_pubkey,
        false,
    )?;

    for i in 0..layout.unsigned_count as usize {
        if layout.unsigned_accounts[i].is_writable {
            layout.final_unsigned_idx[i] = layout.writable_count;
            layout.writable_count = layout.writable_count.saturating_add(1);
        }
    }
    for i in 0..layout.unsigned_count as usize {
        if !layout.unsigned_accounts[i].is_writable {
            layout.final_unsigned_idx[i] = layout.writable_count + layout.readonly_count;
            layout.readonly_count = layout.readonly_count.saturating_add(1);
        }
    }

    Ok(layout)
}

#[inline(always)]
fn canonical_final_message_index(layout: &CanonicalAccountLayout, canonical_ref: u8) -> u8 {
    if canonical_ref == CANONICAL_FEE_PAYER_REF {
        0
    } else {
        1 + layout.final_unsigned_idx[canonical_ref as usize]
    }
}

#[inline(always)]
fn push_canonical_unsigned_accounts(
    envelope: &mut UnsignedEnvelope,
    layout: &CanonicalAccountLayout,
) -> Result<(), BuildError> {
    for i in 0..layout.unsigned_count as usize {
        if layout.unsigned_accounts[i].is_writable {
            push_into_envelope(envelope, &layout.unsigned_accounts[i].pubkey)?;
        }
    }
    for i in 0..layout.unsigned_count as usize {
        if !layout.unsigned_accounts[i].is_writable {
            push_into_envelope(envelope, &layout.unsigned_accounts[i].pubkey)?;
        }
    }
    Ok(())
}

#[inline(always)]
fn validate_swap_shape(input: &ExecutionAssemblyInput) -> Result<(), BuildError> {
    if input.venue_id == VENUE_RAYDIUM_CLMM {
        if input.swap_accounts.program_id != RAYDIUM_CLMM_PROGRAM_ID {
            return Err(BuildError::InvalidTemplateForRoute);
        }
        if input.swap_accounts.num_accounts < 10 {
            return Err(BuildError::InvalidTemplateForRoute);
        }
        if input.swap_ix_data.len != 41
            || input.swap_ix_data.amount_in_offset != 8
            || input.swap_ix_data.min_out_offset != 16
        {
            return Err(BuildError::InvalidTemplateForRoute);
        }
    }
    if (input.swap_accounts.num_accounts as usize) > input.swap_accounts.metas.len() {
        return Err(BuildError::OversizedPayload);
    }
    if (input.swap_ix_data.len as usize) > input.swap_ix_data.data.len() {
        return Err(BuildError::OversizedPayload);
    }
    if input.shadow_guard_program_id == [0u8; 32]
        || input.guard_vault_in_pubkey == [0u8; 32]
        || input.guard_vault_out_pubkey == [0u8; 32]
    {
        return Err(BuildError::InvalidGuardPubkey);
    }
    Ok(())
}
#[inline(always)]
fn push_into_envelope(envelope: &mut UnsignedEnvelope, bytes: &[u8]) -> Result<(), BuildError> {
    let n = bytes.len();
    if envelope.len + n > 1232 {
        return Err(BuildError::OversizedPayload);
    }
    envelope.raw_bytes[envelope.len..envelope.len + n].copy_from_slice(bytes);
    envelope.len += n;
    Ok(())
}

#[inline(always)]
fn patch_swap_ix_amounts(input: &ExecutionAssemblyInput, swapped_data: &mut [u8; 64]) {
    let amount_in_offset = input.swap_ix_data.amount_in_offset as usize;
    let min_out_offset = input.swap_ix_data.min_out_offset as usize;

    if amount_in_offset != u8::MAX as usize
        && amount_in_offset + 8 <= input.swap_ix_data.len as usize
    {
        swapped_data[amount_in_offset..amount_in_offset + 8]
            .copy_from_slice(&input.prepared.trade_size_lamports.to_le_bytes());
    }
    if min_out_offset != u8::MAX as usize && min_out_offset + 8 <= input.swap_ix_data.len as usize {
        swapped_data[min_out_offset..min_out_offset + 8]
            .copy_from_slice(&input.prepared.min_out.to_le_bytes());
    }
}

fn route_family_code(route: RouteDecision, public_rpc_scout_explicit: bool) -> u8 {
    route_family_telemetry_code(route, public_rpc_scout_explicit)
}

fn template_id_code(template_id: TemplateId) -> u8 {
    match template_id {
        TemplateId::JitoPlain => 1,
        TemplateId::HarmonicPlain => 2,
        TemplateId::JitoAtba => 3,
    }
}

fn tactic_family_code(kind: IntentKind) -> u8 {
    match kind {
        IntentKind::Atba => 0,
        IntentKind::ApexPartial => 1,
        _ => 2,
    }
}

fn rollback_reserved(input: &ExecutionAssemblyInput) {
    if input.prepared.reserved_notional_lamports > 0 {
        input
            .inventory_fencer
            .rollback_intent(input.prepared.reserved_notional_lamports);
    }
}

fn build_and_sign_prepared(
    input: &ExecutionAssemblyInput,
    signer: &ThreadLocalSigner,
    nonce_ring: &mut NonceRing,
) -> Result<SignedTxEnvelope, EmitError> {
    if input.ingress_unix_ns >= input.deadline_unix_ns {
        rollback_reserved(input);
        return Err(EmitError::DeadlineExceeded);
    }
    if input.fee_payer_pubkey != signer.pubkey {
        rollback_reserved(input);
        return Err(EmitError::SignFailed(SignError::FeePayerPubkeyMismatch));
    }
    let template_res = match input.prepared.route_decision {
        RouteDecision::JitoBundle
        | RouteDecision::PublicRpcScout
        | RouteDecision::TpuQuicPublic => build_jito_template(input),
        RouteDecision::HarmonicBundle => build_harmonic_template(input),
        RouteDecision::Drop => Err(BuildError::InvalidTemplateForRoute),
    };

    let unsigned = match template_res {
        Ok(t) => t,
        Err(e) => {
            rollback_reserved(input);
            return Err(EmitError::BuildFailed(e));
        }
    };

    let mut signed = match sign_from_nonce_ring(&unsigned, signer, nonce_ring) {
        Ok(s) => s,
        Err(e) => {
            rollback_reserved(input);
            return Err(EmitError::SignFailed(e));
        }
    };

    signed.pool_index = input.pool_index;
    signed.mint_out_index = input.mint_out_index;
    signed.reserved_notional_lamports = input.prepared.reserved_notional_lamports;
    signed.send_timestamp_ns = input.send_timestamp_ns;
    signed.tactic_class = input.tactic_class;
    signed.worker_id = input.worker_id;
    signed.venue_id = input.venue_id;
    signed.opso_owner_mode = input.opso_owner_mode;
    signed.opso_fallback_mode = input.opso_fallback_mode;
    signed.opso_reason_code = input.opso_reason_code;
    signed.episode_id_mod = input.episode_id_mod;
    signed.actual_bid_lamports = input.opso_economics.actual_bid_lamports;
    signed.baseline_bid_lamports = input.opso_economics.baseline_bid_lamports;
    signed.opso_bid_lamports = input.opso_economics.opso_bid_lamports;
    signed.fallback_realized_value_lamports = input.opso_economics.fallback_realized_value_lamports;
    signed.missed_backrun_value_lamports = input.opso_economics.missed_backrun_value_lamports;
    signed.tip_saved_lamports = input.opso_economics.tip_saved_lamports;
    signed.opso_ev_lamports = input.opso_economics.opso_ev_lamports;
    signed.shading_gain_lamports = input.opso_economics.shading_gain_lamports;

    Ok(signed)
}

fn build_and_sign_prepared_with_probe(
    input: &ExecutionAssemblyInput,
    signer: &ThreadLocalSigner,
    nonce_ring: &mut NonceRing,
) -> Result<(SignedTxEnvelope, NoSendAssemblyProbe), EmitError> {
    let mut probe = NoSendAssemblyProbe::default();

    if input.ingress_unix_ns >= input.deadline_unix_ns {
        rollback_reserved(input);
        return Err(EmitError::DeadlineExceeded);
    }
    if input.fee_payer_pubkey != signer.pubkey {
        rollback_reserved(input);
        return Err(EmitError::SignFailed(SignError::FeePayerPubkeyMismatch));
    }

    let build_started = Instant::now();
    let template_res = match input.prepared.route_decision {
        RouteDecision::JitoBundle
        | RouteDecision::PublicRpcScout
        | RouteDecision::TpuQuicPublic => build_jito_template(input),
        RouteDecision::HarmonicBundle => build_harmonic_template(input),
        RouteDecision::Drop => Err(BuildError::InvalidTemplateForRoute),
    };
    probe.build_template_ns = build_started.elapsed().as_nanos() as u64;

    let unsigned = match template_res {
        Ok(t) => t,
        Err(e) => {
            rollback_reserved(input);
            return Err(EmitError::BuildFailed(e));
        }
    };

    let sign_started = Instant::now();
    let mut signed = match sign_from_nonce_ring(&unsigned, signer, nonce_ring) {
        Ok(s) => s,
        Err(e) => {
            rollback_reserved(input);
            return Err(EmitError::SignFailed(e));
        }
    };
    probe.sign_ns = sign_started.elapsed().as_nanos() as u64;

    signed.pool_index = input.pool_index;
    signed.mint_out_index = input.mint_out_index;
    signed.reserved_notional_lamports = input.prepared.reserved_notional_lamports;
    signed.send_timestamp_ns = input.send_timestamp_ns;
    signed.tactic_class = input.tactic_class;
    signed.worker_id = input.worker_id;
    signed.venue_id = input.venue_id;
    signed.opso_owner_mode = input.opso_owner_mode;
    signed.opso_fallback_mode = input.opso_fallback_mode;
    signed.opso_reason_code = input.opso_reason_code;
    signed.episode_id_mod = input.episode_id_mod;
    signed.actual_bid_lamports = input.opso_economics.actual_bid_lamports;
    signed.baseline_bid_lamports = input.opso_economics.baseline_bid_lamports;
    signed.opso_bid_lamports = input.opso_economics.opso_bid_lamports;
    signed.fallback_realized_value_lamports = input.opso_economics.fallback_realized_value_lamports;
    signed.missed_backrun_value_lamports = input.opso_economics.missed_backrun_value_lamports;
    signed.tip_saved_lamports = input.opso_economics.tip_saved_lamports;
    signed.opso_ev_lamports = input.opso_economics.opso_ev_lamports;
    signed.shading_gain_lamports = input.opso_economics.shading_gain_lamports;

    Ok((signed, probe))
}
pub fn build_jito_template(input: &ExecutionAssemblyInput) -> Result<UnsignedEnvelope, BuildError> {
    if input.template_id != TemplateId::JitoPlain && input.template_id != TemplateId::JitoAtba {
        return Err(BuildError::InvalidTemplateForRoute);
    }

    validate_swap_shape(input)?;

    let mut envelope = UnsignedEnvelope {
        raw_bytes: [0; 1232],
        len: 0,
    };
    let n = input.swap_accounts.num_accounts as usize;
    let layout = build_canonical_account_layout(input, true)?;

    // fee payer + tip writable + dynamic swap accounts + two SG vault accounts + four program ids
    let num_readonly_unsigned: u8 = layout.readonly_count.saturating_add(4);
    let total_accounts: u8 = 1u8.saturating_add(layout.unsigned_count).saturating_add(4);

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;

    if total_accounts < 128 {
        push_into_envelope(&mut envelope, &[total_accounts])?;
    } else {
        push_into_envelope(
            &mut envelope,
            &[(total_accounts & 0x7f) | 0x80, (total_accounts >> 7) & 0x7f],
        )?;
    }

    push_into_envelope(&mut envelope, &input.fee_payer_pubkey)?;
    push_canonical_unsigned_accounts(&mut envelope, &layout)?;

    let guard_in_idx = canonical_final_message_index(&layout, layout.guard_in_ref);
    let guard_out_idx = canonical_final_message_index(&layout, layout.guard_out_ref);
    let cu_idx = 1u8 + layout.unsigned_count;
    let sg_idx = cu_idx + 1;
    let swap_pg_idx = cu_idx + 2;
    let sys_idx = cu_idx + 3;

    push_into_envelope(&mut envelope, &COMPUTE_BUDGET_PROGRAM_ID)?;
    push_into_envelope(&mut envelope, &input.shadow_guard_program_id)?;
    push_into_envelope(&mut envelope, &input.swap_accounts.program_id)?;
    push_into_envelope(&mut envelope, &SYSTEM_PROGRAM_ID)?;

    push_into_envelope(&mut envelope, &input.recent_blockhash)?;
    push_into_envelope(&mut envelope, &[4])?;

    // ComputeBudget::set_compute_unit_limit
    // Note: input.route_base_cu_limit is already density-optimized for Jito templates
    // (see pipeline.rs density_optimized_jito_cu_limit) — do not add a pad here.
    push_into_envelope(&mut envelope, &[cu_idx, 0, 5])?;
    push_into_envelope(&mut envelope, &[0x02])?;
    push_into_envelope(&mut envelope, &input.route_base_cu_limit.to_le_bytes())?;

    // ShadowGuard::AssertReservesInRange with the two required vault accounts
    push_into_envelope(&mut envelope, &[sg_idx, 2, guard_in_idx, guard_out_idx, 32])?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.min_reserve_in.to_le_bytes(),
    )?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.max_reserve_in.to_le_bytes(),
    )?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.min_reserve_out.to_le_bytes(),
    )?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.max_reserve_out.to_le_bytes(),
    )?;

    // Swap instruction
    push_into_envelope(&mut envelope, &[swap_pg_idx, n as u8])?;

    // The instruction requires passing exactly n accounts.
    for i in 0..n {
        push_into_envelope(
            &mut envelope,
            &[canonical_final_message_index(&layout, layout.swap_refs[i])],
        )?;
    }
    push_into_envelope(&mut envelope, &[input.swap_ix_data.len])?;
    if input.swap_ix_data.len > 0 {
        let mut swapped_data = input.swap_ix_data.data;
        patch_swap_ix_amounts(input, &mut swapped_data);
        push_into_envelope(
            &mut envelope,
            &swapped_data[..input.swap_ix_data.len as usize],
        )?;
    }

    // SystemProgram::transfer(from=fee payer, to=rotated tip account)
    let tip_idx = canonical_final_message_index(&layout, layout.tip_ref);
    push_into_envelope(&mut envelope, &[sys_idx, 2, 0, tip_idx, 12])?;
    push_into_envelope(&mut envelope, &[2u8, 0, 0, 0])?;
    push_into_envelope(&mut envelope, &input.prepared.tip_lamports.to_le_bytes())?;

    Ok(envelope)
}
pub fn build_harmonic_template(
    input: &ExecutionAssemblyInput,
) -> Result<UnsignedEnvelope, BuildError> {
    if input.template_id != TemplateId::HarmonicPlain {
        return Err(BuildError::InvalidTemplateForRoute);
    }

    use crate::routing::IntentKind;
    if input.prepared.intent_kind == IntentKind::Atba
        || input.prepared.intent_kind == IntentKind::ApexPartial
    {
        return Err(BuildError::TacticNotAllowedOnRoute);
    }

    validate_swap_shape(input)?;

    let mut envelope = UnsignedEnvelope {
        raw_bytes: [0; 1232],
        len: 0,
    };
    let n = input.swap_accounts.num_accounts as usize;
    let layout = build_canonical_account_layout(input, false)?;

    // fee payer + dynamic swap accounts + two SG vault accounts + three program ids
    let num_readonly_unsigned: u8 = layout.readonly_count.saturating_add(3);
    let total_accounts: u8 = 1u8.saturating_add(layout.unsigned_count).saturating_add(3);

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;

    if total_accounts < 128 {
        push_into_envelope(&mut envelope, &[total_accounts])?;
    } else {
        push_into_envelope(
            &mut envelope,
            &[(total_accounts & 0x7f) | 0x80, (total_accounts >> 7) & 0x7f],
        )?;
    }

    push_into_envelope(&mut envelope, &input.fee_payer_pubkey)?;
    push_canonical_unsigned_accounts(&mut envelope, &layout)?;

    let guard_in_idx = canonical_final_message_index(&layout, layout.guard_in_ref);
    let guard_out_idx = canonical_final_message_index(&layout, layout.guard_out_ref);
    let cu_idx = 1u8 + layout.unsigned_count;
    let sg_idx = cu_idx + 1;
    let swap_pg_idx = cu_idx + 2;

    push_into_envelope(&mut envelope, &COMPUTE_BUDGET_PROGRAM_ID)?;
    push_into_envelope(&mut envelope, &input.shadow_guard_program_id)?;
    push_into_envelope(&mut envelope, &input.swap_accounts.program_id)?;

    push_into_envelope(&mut envelope, &input.recent_blockhash)?;
    push_into_envelope(&mut envelope, &[4])?;

    push_into_envelope(&mut envelope, &[cu_idx, 0, 5])?;
    push_into_envelope(&mut envelope, &[0x02])?;
    push_into_envelope(
        &mut envelope,
        &input
            .route_base_cu_limit
            .saturating_add(HARMONIC_CU_PAD)
            .to_le_bytes(),
    )?;

    push_into_envelope(&mut envelope, &[cu_idx, 0, 9])?;
    push_into_envelope(&mut envelope, &[0x03])?;
    push_into_envelope(
        &mut envelope,
        &input.prepared.cu_price_micro_lamports.to_le_bytes(),
    )?;

    push_into_envelope(&mut envelope, &[sg_idx, 2, guard_in_idx, guard_out_idx, 32])?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.min_reserve_in.to_le_bytes(),
    )?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.max_reserve_in.to_le_bytes(),
    )?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.min_reserve_out.to_le_bytes(),
    )?;
    push_into_envelope(
        &mut envelope,
        &input.guard_bounds.max_reserve_out.to_le_bytes(),
    )?;
    // Swap instruction
    push_into_envelope(&mut envelope, &[swap_pg_idx, n as u8])?;

    for i in 0..n {
        push_into_envelope(
            &mut envelope,
            &[canonical_final_message_index(&layout, layout.swap_refs[i])],
        )?;
    }
    push_into_envelope(&mut envelope, &[input.swap_ix_data.len])?;
    if input.swap_ix_data.len > 0 {
        let mut swapped_data = input.swap_ix_data.data;
        patch_swap_ix_amounts(input, &mut swapped_data);
        push_into_envelope(
            &mut envelope,
            &swapped_data[..input.swap_ix_data.len as usize],
        )?;
    }

    Ok(envelope)
}
pub fn emit_prepared(
    input: &ExecutionAssemblyInput,
    signer: &ThreadLocalSigner,
    nonce_ring: &mut NonceRing,
    jito_ring: &dyn EgressRingPush,
    tpu_ring: &dyn EgressRingPush,
    harmonic_ring: &dyn EgressRingPush,
    overlay: &mut PoolOverlay,
) -> Result<EmitReceipt, EmitError> {
    let signed = build_and_sign_prepared(input, signer, nonce_ring)?;

    let push_ok = match input.prepared.route_decision {
        RouteDecision::JitoBundle | RouteDecision::PublicRpcScout => {
            jito_ring.try_push_envelope(&signed)
        }
        RouteDecision::TpuQuicPublic => tpu_ring.try_push_envelope(&signed),
        RouteDecision::HarmonicBundle => harmonic_ring.try_push_envelope(&signed),
        _ => unreachable!(),
    };

    if !push_ok {
        rollback_reserved(input);
        return Err(EmitError::EnqueueFailedRingFull);
    }
    overlay.apply(&input.overlay_delta);

    Ok(EmitReceipt {
        signature: signed.signature,
        pool_index: signed.pool_index,
        mint_out_index: signed.mint_out_index,
        reserved_notional_lamports: signed.reserved_notional_lamports,
    })
}

pub const ROLLOUT_MODE_OBSERVE_ONLY: u8 = 1;
pub const ROLLOUT_CAP_R0_OBSERVE_ONLY: u8 = 0;
pub const NO_SEND_REASON_ROLLOUT_OBSERVE_ONLY: u16 = 1;

pub fn emit_prepared_no_send(
    input: &ExecutionAssemblyInput,
    signer: &ThreadLocalSigner,
    nonce_ring: &mut NonceRing,
    sink: &dyn RolloutShadowPush,
) -> Result<EmitReceipt, EmitError> {
    let signed = build_and_sign_prepared(input, signer, nonce_ring)?;
    let dominance_would_drop =
        input.prepared.dominance_decision == DominanceDecision::WouldDropObserveOnly;
    let exhaust_win = u8::from(
        input.tactic_class == crate::quant_tactics::TacticReasonCode::ExhaustScavenging as u16,
    );
    let event = RolloutShadowEvent {
        unix_ns: input.send_timestamp_ns,
        slot: input.ingress_slot,
        pool_index: input.pool_index,
        mint_index: input.mint_out_index,
        venue_id: input.venue_id,
        signal_source: input.signal_source,
        route_family: route_family_code(
            input.prepared.route_decision,
            input.public_rpc_scout_explicit,
        ),
        tactic_family: tactic_family_code(input.prepared.intent_kind),
        rollout_mode: ROLLOUT_MODE_OBSERVE_ONLY,
        cap_rung: ROLLOUT_CAP_R0_OBSERVE_ONLY,
        would_build: true,
        would_sign: true,
        would_send: true,
        live_send_suppressed: true,
        no_send_reason: NO_SEND_REASON_ROLLOUT_OBSERVE_ONLY,
        drop_reason: if dominance_would_drop {
            G12DropReason::WouldDropDominanceGate.as_u16()
        } else {
            G12DropReason::DroppedRolloutNoSend.as_u16()
        },
        kill_reason: 0,
        _flags_pad: 0,
        would_reserve_lamports: input.prepared.would_reserve_notional_lamports,
        bid_lamports: input.opso_economics.actual_bid_lamports,
        tip_lamports: input.prepared.tip_lamports,
        cu_limit: input.route_base_cu_limit,
        route_base_cu_limit: input.configured_route_base_cu_limit,
        retained_ev_lamports_lcb: input.prepared.retained_lcb_lamports as i64,
        shadow_pnl_lamports: input.opso_economics.opso_ev_lamports,
        guard_min_reserve_in: input.guard_bounds.min_reserve_in,
        guard_max_reserve_in: input.guard_bounds.max_reserve_in,
        guard_min_reserve_out: input.guard_bounds.min_reserve_out,
        guard_max_reserve_out: input.guard_bounds.max_reserve_out,
        private_lane_admissible: u8::from(
            input.signal_source == SIGNAL_SOURCE_AFXDP || input.signal_source == SIGNAL_SOURCE_DPDK,
        ),
        stale_state_reroute: 0,
        egress_gate_reroute: 0,
        egress_gate_drop: 0,
        taylor_prefilter_drop: 0,
        dominance_would_drop: u8::from(dominance_would_drop),
        dominance_live_drop: 0,
        clamped_apex_candidate: 0,
        atba_missing_dist_candidate: 0,
        cu_density_win: u8::from(
            route_uses_jito_lane(input.prepared.route_decision)
                && input.route_base_cu_limit != input.configured_route_base_cu_limit,
        ),
        exhaust_candidate: exhaust_win,
        exhaust_win,
        micro_market_observed: u8::from(input.micro_market_observed),
        _telemetry_pad: [0; 2],
        opso_shadow_bid_delta_lamports: input.opso_economics.actual_bid_lamports as i64
            - input.opso_economics.opso_bid_lamports as i64,
        competition_adjusted_delta_lamports: 0,
        micro_market_flat_retained_ev_lamports: input.micro_market_flat_retained_ev_lamports,
        micro_market_delta_lamports: input.micro_market_delta_lamports,
        trade_size_lamports: input.trade_size_lamports,
        min_out_lamports: input.prepared.min_out,
        quote_target_out_lamports: input.quote_target_out_lamports,
        quote_expected_out_lamports: input.quote_expected_out_lamports,
        quote_baseline_reserve_in: input.quote_baseline_reserve_in,
        quote_baseline_reserve_out: input.quote_baseline_reserve_out,
        quote_projected_reserve_in: input.quote_projected_reserve_in,
        quote_projected_reserve_out: input.quote_projected_reserve_out,
        quote_slippage_bps: input.slippage_bps,
        quote_mint_in_transfer_fee_bps: input.quote_mint_in_transfer_fee_bps,
        _quote_flags_pad: 0,
        quote_trade_fee_num: input.quote_trade_fee_num,
        quote_trade_fee_denom: input.quote_trade_fee_denom,
        quote_mint_in_max_fee_lamports: input.quote_mint_in_max_fee_lamports,
        signature: signed.signature,
        primary_signer: input.fee_payer_pubkey,
        pool_pubkey: input.pool_pubkey,
        mint_in: input.mint_in_pubkey,
        mint_out: input.mint_out_pubkey,
        episode_id_mod: input.episode_id_mod,
        opso_owner_mode: input.opso_owner_mode,
        opso_fallback_mode: input.opso_fallback_mode,
        opso_reason_code: input.opso_reason_code,
        template_id: template_id_code(input.template_id),
        ingress_quote_request_id: input.quote_request_id,
        ingress_unix_ns: input.ingress_unix_ns,
        ..Default::default()
    };
    if !sink.try_push_shadow(&event) {
        return Err(EmitError::NoSendSinkFull);
    }
    Ok(EmitReceipt {
        signature: signed.signature,
        pool_index: input.pool_index,
        mint_out_index: input.mint_out_index,
        reserved_notional_lamports: 0,
    })
}

pub fn emit_prepared_no_send_with_probe(
    input: &ExecutionAssemblyInput,
    signer: &ThreadLocalSigner,
    nonce_ring: &mut NonceRing,
    sink: &dyn RolloutShadowPush,
) -> Result<(EmitReceipt, NoSendAssemblyProbe), EmitError> {
    let (signed, mut probe) = build_and_sign_prepared_with_probe(input, signer, nonce_ring)?;
    let emit_started = Instant::now();
    let dominance_would_drop =
        input.prepared.dominance_decision == DominanceDecision::WouldDropObserveOnly;
    let exhaust_win = u8::from(
        input.tactic_class == crate::quant_tactics::TacticReasonCode::ExhaustScavenging as u16,
    );
    let event = RolloutShadowEvent {
        unix_ns: input.send_timestamp_ns,
        slot: input.ingress_slot,
        pool_index: input.pool_index,
        mint_index: input.mint_out_index,
        venue_id: input.venue_id,
        signal_source: input.signal_source,
        route_family: route_family_code(
            input.prepared.route_decision,
            input.public_rpc_scout_explicit,
        ),
        tactic_family: tactic_family_code(input.prepared.intent_kind),
        rollout_mode: ROLLOUT_MODE_OBSERVE_ONLY,
        cap_rung: ROLLOUT_CAP_R0_OBSERVE_ONLY,
        would_build: true,
        would_sign: true,
        would_send: true,
        live_send_suppressed: true,
        no_send_reason: NO_SEND_REASON_ROLLOUT_OBSERVE_ONLY,
        drop_reason: if dominance_would_drop {
            G12DropReason::WouldDropDominanceGate.as_u16()
        } else {
            G12DropReason::DroppedRolloutNoSend.as_u16()
        },
        kill_reason: 0,
        _flags_pad: 0,
        would_reserve_lamports: input.prepared.would_reserve_notional_lamports,
        bid_lamports: input.opso_economics.actual_bid_lamports,
        tip_lamports: input.prepared.tip_lamports,
        cu_limit: input.route_base_cu_limit,
        route_base_cu_limit: input.configured_route_base_cu_limit,
        retained_ev_lamports_lcb: input.prepared.retained_lcb_lamports as i64,
        shadow_pnl_lamports: input.opso_economics.opso_ev_lamports,
        guard_min_reserve_in: input.guard_bounds.min_reserve_in,
        guard_max_reserve_in: input.guard_bounds.max_reserve_in,
        guard_min_reserve_out: input.guard_bounds.min_reserve_out,
        guard_max_reserve_out: input.guard_bounds.max_reserve_out,
        private_lane_admissible: u8::from(
            input.signal_source == SIGNAL_SOURCE_AFXDP || input.signal_source == SIGNAL_SOURCE_DPDK,
        ),
        stale_state_reroute: 0,
        egress_gate_reroute: 0,
        egress_gate_drop: 0,
        taylor_prefilter_drop: 0,
        dominance_would_drop: u8::from(dominance_would_drop),
        dominance_live_drop: 0,
        clamped_apex_candidate: 0,
        atba_missing_dist_candidate: 0,
        cu_density_win: u8::from(
            route_uses_jito_lane(input.prepared.route_decision)
                && input.route_base_cu_limit != input.configured_route_base_cu_limit,
        ),
        exhaust_candidate: exhaust_win,
        exhaust_win,
        micro_market_observed: u8::from(input.micro_market_observed),
        _telemetry_pad: [0; 2],
        opso_shadow_bid_delta_lamports: input.opso_economics.actual_bid_lamports as i64
            - input.opso_economics.opso_bid_lamports as i64,
        competition_adjusted_delta_lamports: 0,
        micro_market_flat_retained_ev_lamports: input.micro_market_flat_retained_ev_lamports,
        micro_market_delta_lamports: input.micro_market_delta_lamports,
        trade_size_lamports: input.trade_size_lamports,
        min_out_lamports: input.prepared.min_out,
        quote_target_out_lamports: input.quote_target_out_lamports,
        quote_expected_out_lamports: input.quote_expected_out_lamports,
        quote_baseline_reserve_in: input.quote_baseline_reserve_in,
        quote_baseline_reserve_out: input.quote_baseline_reserve_out,
        quote_projected_reserve_in: input.quote_projected_reserve_in,
        quote_projected_reserve_out: input.quote_projected_reserve_out,
        quote_slippage_bps: input.slippage_bps,
        quote_mint_in_transfer_fee_bps: input.quote_mint_in_transfer_fee_bps,
        _quote_flags_pad: 0,
        quote_trade_fee_num: input.quote_trade_fee_num,
        quote_trade_fee_denom: input.quote_trade_fee_denom,
        quote_mint_in_max_fee_lamports: input.quote_mint_in_max_fee_lamports,
        signature: signed.signature,
        primary_signer: input.fee_payer_pubkey,
        pool_pubkey: input.pool_pubkey,
        mint_in: input.mint_in_pubkey,
        mint_out: input.mint_out_pubkey,
        episode_id_mod: input.episode_id_mod,
        opso_owner_mode: input.opso_owner_mode,
        opso_fallback_mode: input.opso_fallback_mode,
        opso_reason_code: input.opso_reason_code,
        template_id: template_id_code(input.template_id),
        ingress_quote_request_id: input.quote_request_id,
        ingress_unix_ns: input.ingress_unix_ns,
        ..Default::default()
    };
    if !sink.try_push_shadow(&event) {
        return Err(EmitError::NoSendSinkFull);
    }
    probe.emit_nosend_ns = emit_started.elapsed().as_nanos() as u64;
    Ok((
        EmitReceipt {
            signature: signed.signature,
            pool_index: input.pool_index,
            mint_out_index: input.mint_out_index,
            reserved_notional_lamports: 0,
        },
        probe,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{RouteDecision, RouteReason};
    use std::sync::atomic::AtomicU64;

    fn dummy_prepared(
        decision: RouteDecision,
        kind: crate::routing::IntentKind,
    ) -> PreparedExecution {
        PreparedExecution {
            route_decision: decision,
            route_reason: RouteReason::JitoDirect,
            intent_kind: kind,
            trade_size_lamports: 1_000_000,
            tip_lamports: 100,
            cu_price_micro_lamports: 333,
            min_out: 990_000,
            leader_class_id: 1,
            alpha_bucket: 5,
            final_lut_index: 0,
            dominance_decision: crate::policy::DominanceDecision::Pass,
            reserved_notional_lamports: 1_000_000,
            would_reserve_notional_lamports: 1_000_000,
            retained_lcb_lamports: 0,
            base_state_unix_ns: 100,
        }
    }
    #[test]
    fn test_jito_template_layout() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::JitoBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 111,
                reserve_out_delta: -222,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [9u8; 32],
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        let tpl = build_jito_template(&input).unwrap();
        // Assert the byte length mapping adds up.
        assert_eq!(tpl.len, 358);
    }
    #[test]
    fn test_harmonic_template_layout() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::HarmonicBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: 0,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::HarmonicPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [9u8; 32],
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        let tpl = build_harmonic_template(&input).unwrap();
        // Assert the byte length mapping adds up.
        assert_eq!(tpl.len, 289);

        // Assert hard rejects on Guerrila forbidden tactics over Harmonic
        input.prepared.intent_kind = crate::routing::IntentKind::Atba;
        assert_eq!(
            build_harmonic_template(&input).unwrap_err(),
            BuildError::TacticNotAllowedOnRoute
        );

        input.prepared.intent_kind = crate::routing::IntentKind::ApexPartial;
        assert_eq!(
            build_harmonic_template(&input).unwrap_err(),
            BuildError::TacticNotAllowedOnRoute
        );
    }
    #[test]
    fn test_emit_prepared_rollback_on_empty_nonce_ring() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::JitoBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0u8; 32],
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        // Ring is explicitly empty
        let mut nonce_ring = NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [0; 32],
                nonce_pubkey: [0; 32],
            }; 128],
            head: 0,
            tail: 0,
        };
        let jito_ring = EgressRing::new(10);
        let tpu_ring = EgressRing::new(10);
        let harmonic_ring = EgressRing::new(10);
        let mut overlay = PoolOverlay::new();

        let mut signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };
        let res = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_ring,
            &mut overlay,
        );

        // Assert failure due to empty nonce ring
        assert_eq!(res, Err(EmitError::SignFailed(SignError::RingEmpty)));
        // Assert reservation rollback succeeded (allocation reverted 1M notional)
        assert_eq!(alloc.load(std::sync::atomic::Ordering::SeqCst), 0);
        // Overlay must not be applied
        assert_eq!(overlay.get(0), None);
    }
    #[test]
    fn test_emit_prepared_rollback_on_ring_full() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::HarmonicBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::HarmonicPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0u8; 32],
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        let mut nonce_ring = NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [1; 32],
                nonce_pubkey: [2; 32],
            }; 128],
            head: 1,
            tail: 0,
        };
        let jito_ring = EgressRing::new(10);
        let tpu_ring = EgressRing::new(10);
        // Ring is explicitly full (capacity = 0)
        let harmonic_ring = EgressRing::new(0);
        let mut overlay = PoolOverlay::new();

        let mut signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };
        let res = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_ring,
            &mut overlay,
        );

        assert_eq!(res, Err(EmitError::EnqueueFailedRingFull));
        assert_eq!(alloc.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(overlay.get(0), None);
    }
    #[test]
    fn test_emit_prepared_success_overlay_updates() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::HarmonicBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::HarmonicPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 111,
                reserve_out_delta: -222,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0u8; 32],
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        let mut nonce_ring = NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [1; 32],
                nonce_pubkey: [2; 32],
            }; 128],
            head: 1,
            tail: 0,
        };
        let jito_ring = EgressRing::new(10);
        let tpu_ring = EgressRing::new(10);
        let harmonic_ring = EgressRing::new(10);
        let mut overlay = PoolOverlay::new();

        let mut signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };
        let res = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_ring,
            &mut overlay,
        );

        assert!(res.is_ok());
        // Allocation remains held until NACK'd
        assert_eq!(alloc.load(std::sync::atomic::Ordering::SeqCst), 1_000_000);
        assert_eq!(overlay.get(0).unwrap(), (111, -222));
        assert_eq!(harmonic_ring.len(), 1);

        // Assert zeroize on the secret ring occurred natively
        let t = nonce_ring.tuples[0];
        assert_eq!(t.secret_scalar, [0; 32]);
        assert_eq!(t.nonce_pubkey, [0; 32]);
    }

    #[test]
    fn test_emit_prepared_no_send_publishes_rollout_shadow_event() {
        let alloc = AtomicU64::new(0);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics {
                actual_bid_lamports: 123,
                opso_ev_lamports: 456,
                ..Default::default()
            },
            prepared: PreparedExecution {
                reserved_notional_lamports: 0,
                would_reserve_notional_lamports: 1_000_000,
                retained_lcb_lamports: 777,
                ..dummy_prepared(
                    RouteDecision::JitoBundle,
                    crate::routing::IntentKind::PlainWithinCap,
                )
            },
            recent_blockhash: [0; 32],
            ingress_slot: 88,
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 123_456_789,
            signal_source: 0,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 17,
            route_base_cu_limit: 310_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: true,
            micro_market_flat_retained_ev_lamports: 700,
            micro_market_delta_lamports: 77,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 10,
                max_reserve_in: 11,
                min_reserve_out: 12,
                max_reserve_out: 13,
            },
            pool_index: 55,
            mint_out_index: 66,
            pool_pubkey: [11; 32],
            mint_in_pubkey: [12; 32],
            mint_out_pubkey: [13; 32],
            overlay_delta: OverlayDelta {
                pool_index: 55,
                reserve_in_delta: 1,
                reserve_out_delta: -1,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0u8; 32],
            shadow_guard_program_id: [7u8; 32],
            guard_vault_in_pubkey: [8u8; 32],
            guard_vault_out_pubkey: [10u8; 32],
            send_timestamp_ns: 999,
            tactic_class: 2,
            worker_id: 3,
            venue_id: 4,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };
        let mut nonce_ring = NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [1; 32],
                nonce_pubkey: [2; 32],
            }; 128],
            head: 1,
            tail: 0,
        };
        let signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };
        let sink = RolloutShadowRing::new(4);

        let receipt = emit_prepared_no_send(&input, &signer, &mut nonce_ring, &sink).unwrap();

        assert_eq!(receipt.reserved_notional_lamports, 0);
        assert_eq!(alloc.load(std::sync::atomic::Ordering::SeqCst), 0);
        let events = sink.items.borrow();
        assert_eq!(events.len(), 1);
        let event = events[0];
        assert_eq!(event.slot, 88);
        assert!(event.live_send_suppressed);
        assert_eq!(event.would_reserve_lamports, 1_000_000);
        assert_eq!(event.bid_lamports, 123);
        assert_eq!(event.shadow_pnl_lamports, 456);
        assert_eq!(event.route_base_cu_limit, 300_000);
        assert_eq!(event.cu_limit, 310_000);
        assert_eq!(
            event.drop_reason,
            G12DropReason::DroppedRolloutNoSend.as_u16()
        );
        assert_eq!(event.cu_density_win, 1);
        assert_eq!(event.opso_shadow_bid_delta_lamports, 123);
        assert_eq!(event.trade_size_lamports, 1_000_000);
        assert_eq!(event.min_out_lamports, 990_000);
        assert_eq!(event.quote_expected_out_lamports, 990_000);
        assert_eq!(event.quote_target_out_lamports, 500_000);
        assert_eq!(event.quote_baseline_reserve_in, 10_000_000);
        assert_eq!(event.quote_baseline_reserve_out, 9_000_000);
        assert_eq!(event.quote_projected_reserve_in, 10_500_000);
        assert_eq!(event.quote_projected_reserve_out, 8_500_000);
        assert_eq!(event.quote_trade_fee_num, 25);
        assert_eq!(event.quote_trade_fee_denom, 10_000);
        assert_eq!(event.quote_slippage_bps, 100);
        assert_eq!(event.pool_pubkey, [11; 32]);
        assert_eq!(event.mint_in, [12; 32]);
        assert_eq!(event.mint_out, [13; 32]);
        assert_eq!(event.template_id, template_id_code(TemplateId::JitoPlain));
        assert_eq!(event.ingress_quote_request_id, 17);
        assert_eq!(event.ingress_unix_ns, 123_456_789);
    }

    #[test]
    fn test_harmonic_builder_emits_fee_payer_exactly_once_and_indexes_zero() {
        let alloc = AtomicU64::new(100);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::HarmonicBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: 0,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::HarmonicPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 1,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [99; 32],
            shadow_guard_program_id: [77; 32],
            guard_vault_in_pubkey: [88; 32],
            guard_vault_out_pubkey: [100; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };
        // Explicitly inject the singular fee payer into swap accounts
        input.swap_accounts.metas[0].source = SwapAccountSource::FeePayerSigner;
        input.swap_accounts.metas[0].is_writable = true;

        let tpl = build_harmonic_template(&input).unwrap();
        // The header must announce exactly `[1, 0, N]` signaling ONE signer total.
        assert_eq!(tpl.raw_bytes[0], 1); // 1 signer
        assert_eq!(tpl.raw_bytes[1], 0); // 0 readonly signers
    }

    #[test]
    fn test_jito_builder_emits_fee_payer_exactly_once_and_indexes_zero() {
        let alloc = AtomicU64::new(100);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::JitoBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: 0,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 1,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [99; 32],
            shadow_guard_program_id: [77; 32],
            guard_vault_in_pubkey: [88; 32],
            guard_vault_out_pubkey: [100; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };
        // Explicitly inject the singular fee payer into swap accounts
        input.swap_accounts.metas[0].source = SwapAccountSource::FeePayerSigner;
        input.swap_accounts.metas[0].is_writable = true;

        let tpl = build_jito_template(&input).unwrap();
        // The header must announce exactly `[1, 0, N]` signaling ONE signer total.
        assert_eq!(tpl.raw_bytes[0], 1); // 1 signer
        assert_eq!(tpl.raw_bytes[1], 0); // 0 readonly signers
    }

    #[test]
    fn test_sign_from_nonce_ring_returns_non_canonical_on_bad_nonce() {
        let mut ring = NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [255; 32],
                nonce_pubkey: [0; 32],
            }; 128],
            head: 1,
            tail: 0,
        };
        let signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };
        let unsigned = UnsignedEnvelope {
            raw_bytes: [0; 1232],
            len: 10,
        };
        let res = sign_from_nonce_ring(&unsigned, &signer, &mut ring);
        assert_eq!(res.unwrap_err(), SignError::NonCanonicalNonceScalar);

        // Verify zeroization on the tuple
        let t = ring.tuples[0];
        assert_eq!(t.secret_scalar, [0; 32]);
        assert_eq!(t.nonce_pubkey, [0; 32]);
    }

    #[test]
    fn test_emit_prepared_rolls_back_on_fee_payer_mismatch() {
        let alloc = AtomicU64::new(100);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let prepared = dummy_prepared(
            RouteDecision::JitoBundle,
            crate::routing::IntentKind::PlainWithinCap,
        );
        fencer
            .reserve_intent(prepared.reserved_notional_lamports)
            .unwrap();

        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared,
            recent_blockhash: [0; 32],
            ingress_slot: 0,
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 0,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 0,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 0,
                max_reserve_in: 0,
                min_reserve_out: 0,
                max_reserve_out: 0,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [1; 32], // Mismatch!
            shadow_guard_program_id: [0; 32],
            guard_vault_in_pubkey: [0; 32],
            guard_vault_out_pubkey: [0; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        let signer = ThreadLocalSigner {
            pubkey: [2; 32],
            secret: [0; 32],
        }; // Mismatch!
        let mut nonce_ring = NonceRing {
            tuples: [crate::assembly::NonceTuple {
                secret_scalar: [0; 32],
                nonce_pubkey: [0; 32],
            }; 128],
            head: 1,
            tail: 0,
        };
        let jito_ring = EgressRing::new(10);
        let tpu_ring = EgressRing::new(10);
        let harmonic_ring = EgressRing::new(10);
        let mut overlay = PoolOverlay::new();

        let res = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_ring,
            &mut overlay,
        );
        assert_eq!(
            res,
            Err(EmitError::SignFailed(SignError::FeePayerPubkeyMismatch))
        );

        // Assert rollback occurred exactly
        assert_eq!(alloc.load(std::sync::atomic::Ordering::SeqCst), 100);
    }

    // REGRESSION GUARD: worker hot path must never call wall-clock or monotonic APIs.
    // Any time-fetching call in this module is a latency hazard (unconditional syscall)
    // and violates the Tier-0 no-syscall discipline.
    // If this test fails, locate and remove the time call; pass deadline state through
    // ExecutionAssemblyInput fields (deadline_unix_ns / ingress_unix_ns) instead.
    #[test]
    fn no_time_syscalls_in_worker_modules() {
        const ASSEMBLY_SRC: &str = include_str!("assembly.rs");
        // Patterns are split with concat!() to prevent the scanner from matching
        // these test literals against themselves.
        let banned: &[&str] = &[
            concat!("System", "Time::now"),
            concat!("Instant", "::now"),
            concat!("clock_get", "time"),
            concat!("libc::clock_get", "time"),
            concat!("std::time::Inst", "ant"),
            concat!("std::time::System", "Time"),
        ];
        for pattern in banned {
            assert!(
                !ASSEMBLY_SRC.contains(pattern),
                "REGRESSION: assembly.rs contains banned time call: `{}`\n\
                 Worker deadline state must come from ExecutionAssemblyInput fields only.",
                pattern,
            );
        }
    }

    // -----------------------------------------------------------------------
    // G7 – Harmonic template contract tests
    //
    // These tests prove every checklist line item for the Harmonic template
    // without modifying the template builder.  If any test fails, the template
    // is out of contract and must be fixed — not the tests.
    // -----------------------------------------------------------------------

    /// Macro: build a Harmonic template from a minimal 2-account swap fixture.
    /// Expands in the caller's scope so lifetimes are satisfied.
    macro_rules! g7_harmonic_fixture {
        ($input:ident, $tpl:ident) => {
            let alloc = AtomicU64::new(10_000_000);
            let fencer = InventoryFencer {
                current_allocation: &alloc,
                absolute_ceiling: u64::MAX,
            };
            let mut $input = ExecutionAssemblyInput {
                opso_economics: OpsoObserveOnlyEconomics::default(),
                prepared: dummy_prepared(
                    RouteDecision::HarmonicBundle,
                    crate::routing::IntentKind::PlainWithinCap,
                ),
                recent_blockhash: [0xBB; 32],
                ingress_slot: 0,
                deadline_unix_ns: 0,
                ingress_unix_ns: 0,
                signal_source: 0,
                template_id: TemplateId::HarmonicPlain,
                quote_request_id: 0,
                route_base_cu_limit: 300_000,
                configured_route_base_cu_limit: 300_000,
                micro_market_observed: false,
                micro_market_flat_retained_ev_lamports: 0,
                micro_market_delta_lamports: 0,
                trade_size_lamports: 1_000_000,
                slippage_bps: 100,
                quote_target_out_lamports: 500_000,
                quote_expected_out_lamports: 990_000,
                quote_baseline_reserve_in: 10_000_000,
                quote_baseline_reserve_out: 9_000_000,
                quote_projected_reserve_in: 10_500_000,
                quote_projected_reserve_out: 8_500_000,
                quote_trade_fee_num: 25,
                quote_trade_fee_denom: 10_000,
                quote_mint_in_transfer_fee_bps: 0,
                quote_mint_in_max_fee_lamports: 0,
                swap_accounts: FixedSwapAccounts {
                    program_id: [0x55; 32],
                    metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                    num_accounts: 2,
                },
                swap_ix_data: FixedSwapIxData {
                    data: [0; 64],
                    len: 17,
                    amount_in_offset: 1,
                    min_out_offset: 9,
                },
                guard_bounds: GuardBounds {
                    min_reserve_in: 1000,
                    max_reserve_in: 2000,
                    min_reserve_out: 3000,
                    max_reserve_out: 4000,
                },
                pool_index: 42,
                mint_out_index: 7,
                pool_pubkey: [42; 32],
                mint_in_pubkey: [43; 32],
                mint_out_pubkey: [44; 32],
                overlay_delta: OverlayDelta {
                    pool_index: 42,
                    reserve_in_delta: 500,
                    reserve_out_delta: -500,
                },
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0; 32],
                },
                inventory_fencer: &fencer,
                fee_payer_pubkey: [0x09; 32],
                shadow_guard_program_id: [0x07; 32],
                guard_vault_in_pubkey: [0x08; 32],
                guard_vault_out_pubkey: [0x0A; 32],
                send_timestamp_ns: 0,
                tactic_class: 0,
                worker_id: 0,
                venue_id: 0,
                public_rpc_scout_explicit: false,
                opso_owner_mode: 0,
                opso_fallback_mode: 7,
                opso_reason_code: 0,
                episode_id_mod: 0,
            };
            $input.swap_accounts.metas[0] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x11; 32]),
                is_writable: true,
            };
            $input.swap_accounts.metas[1] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x22; 32]),
                is_writable: false,
            };
            let $tpl = build_harmonic_template(&$input).unwrap();
        };
    }

    /// Helper: parse the unsigned message bytes to extract instruction program IDs
    /// and instruction data.  Returns Vec<(program_id, data)>.
    fn parse_message_instructions(msg: &[u8]) -> Vec<([u8; 32], Vec<u8>)> {
        // msg[0..3] = header, msg[3..] starts with compact-u16 account count
        let (acct_count, acct_start) = decode_compact_u16(msg, 3);
        let keys_end = acct_start + (acct_count as usize) * 32;
        let blockhash_end = keys_end + 32;
        let (ix_count, mut cursor) = decode_compact_u16(msg, blockhash_end);

        let mut instructions = Vec::new();
        for _ in 0..ix_count {
            let prog_idx = msg[cursor] as usize;
            cursor += 1;
            let (num_accts, next) = decode_compact_u16(msg, cursor);
            cursor = next + num_accts as usize;
            let (data_len, data_start) = decode_compact_u16(msg, cursor);
            let data = msg[data_start..data_start + data_len as usize].to_vec();
            cursor = data_start + data_len as usize;

            // Resolve program ID from account keys table.
            let key_offset = acct_start + prog_idx * 32;
            let mut key = [0u8; 32];
            key.copy_from_slice(&msg[key_offset..key_offset + 32]);
            instructions.push((key, data));
        }
        instructions
    }

    fn decode_compact_u16(buf: &[u8], offset: usize) -> (u16, usize) {
        let b0 = buf[offset] as u16;
        if b0 < 0x80 {
            return (b0, offset + 1);
        }
        let b1 = buf[offset + 1] as u16;
        if b1 < 0x80 {
            return ((b0 & 0x7f) | (b1 << 7), offset + 2);
        }
        let b2 = buf[offset + 2] as u16;
        ((b0 & 0x7f) | ((b1 & 0x7f) << 7) | (b2 << 14), offset + 3)
    }

    #[allow(dead_code)]
    struct ParsedIxFull {
        program_id: [u8; 32],
        account_indices: Vec<u8>,
        data: Vec<u8>,
    }

    /// Enhanced message parser: returns full account table + per-instruction
    /// program ID, account indices, and data (for account-ordering audits).
    fn parse_message_full(msg: &[u8]) -> (Vec<[u8; 32]>, Vec<ParsedIxFull>) {
        let (acct_count, acct_start) = decode_compact_u16(msg, 3);
        let mut accounts = Vec::new();
        for i in 0..acct_count as usize {
            let off = acct_start + i * 32;
            let mut key = [0u8; 32];
            key.copy_from_slice(&msg[off..off + 32]);
            accounts.push(key);
        }
        let keys_end = acct_start + (acct_count as usize) * 32;
        let blockhash_end = keys_end + 32;
        let (ix_count, mut cursor) = decode_compact_u16(msg, blockhash_end);

        let mut instructions = Vec::new();
        for _ in 0..ix_count {
            let prog_idx = msg[cursor];
            cursor += 1;
            let (num_accts, next) = decode_compact_u16(msg, cursor);
            cursor = next;
            let mut acct_indices = Vec::new();
            for _ in 0..num_accts {
                acct_indices.push(msg[cursor]);
                cursor += 1;
            }
            let (data_len, data_start) = decode_compact_u16(msg, cursor);
            let data = msg[data_start..data_start + data_len as usize].to_vec();
            cursor = data_start + data_len as usize;

            instructions.push(ParsedIxFull {
                program_id: accounts[prog_idx as usize],
                account_indices: acct_indices,
                data,
            });
        }
        (accounts, instructions)
    }

    fn count_unique_accounts(accounts: &[[u8; 32]]) -> usize {
        let mut keys = accounts.to_vec();
        keys.sort();
        keys.dedup();
        keys.len()
    }

    /// G7-HT1: Harmonic template contains exactly 4 instructions.
    /// SetComputeUnitLimit, SetComputeUnitPrice, ShadowGuard, Swap.
    #[test]
    fn g7_ht1_four_instructions() {
        g7_harmonic_fixture!(_input, tpl);
        let msg = &tpl.raw_bytes[..tpl.len];
        let ixs = parse_message_instructions(msg);
        assert_eq!(
            ixs.len(),
            4,
            "Harmonic template must have exactly 4 instructions"
        );
    }

    /// G7-HT2: Instruction 0 is SetComputeUnitLimit (disc=0x02, 5 bytes).
    #[test]
    fn g7_ht2_cu_limit_instruction() {
        g7_harmonic_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, data) = &ixs[0];
        assert_eq!(
            prog, &COMPUTE_BUDGET_PROGRAM_ID,
            "ix0 must be ComputeBudget"
        );
        assert_eq!(data.len(), 5, "SetComputeUnitLimit data must be 5 bytes");
        assert_eq!(
            data[0], 0x02,
            "SetComputeUnitLimit discriminator must be 0x02"
        );
        let cu_limit = u32::from_le_bytes(data[1..5].try_into().unwrap());
        assert_eq!(
            cu_limit,
            input.route_base_cu_limit.saturating_add(150),
            "CU limit must be route_base + 150"
        );
    }

    /// G7-HT3: Instruction 1 is SetComputeUnitPrice (disc=0x03, 9 bytes).
    #[test]
    fn g7_ht3_cu_price_instruction() {
        g7_harmonic_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, data) = &ixs[1];
        assert_eq!(
            prog, &COMPUTE_BUDGET_PROGRAM_ID,
            "ix1 must be ComputeBudget"
        );
        assert_eq!(data.len(), 9, "SetComputeUnitPrice data must be 9 bytes");
        assert_eq!(
            data[0], 0x03,
            "SetComputeUnitPrice discriminator must be 0x03"
        );
        let price = u64::from_le_bytes(data[1..9].try_into().unwrap());
        assert_eq!(
            price, input.prepared.cu_price_micro_lamports,
            "CU price must match prepared.cu_price_micro_lamports"
        );
    }

    /// G7-HT4: Instruction 2 is ShadowGuard::AssertReservesInRange.
    /// First non-budget instruction must be the guard, not the swap.
    #[test]
    fn g7_ht4_shadow_guard_first_non_budget() {
        g7_harmonic_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, data) = &ixs[2];
        assert_eq!(
            prog, &input.shadow_guard_program_id,
            "ix2 must be ShadowGuard"
        );
        assert_eq!(
            data.len(),
            32,
            "ShadowGuard data must be 32 bytes (4 × u64)"
        );
        let bounds = [
            u64::from_le_bytes(data[0..8].try_into().unwrap()),
            u64::from_le_bytes(data[8..16].try_into().unwrap()),
            u64::from_le_bytes(data[16..24].try_into().unwrap()),
            u64::from_le_bytes(data[24..32].try_into().unwrap()),
        ];
        assert_eq!(
            bounds,
            [
                input.guard_bounds.min_reserve_in,
                input.guard_bounds.max_reserve_in,
                input.guard_bounds.min_reserve_out,
                input.guard_bounds.max_reserve_out,
            ],
            "ShadowGuard bounds must match input"
        );
    }

    /// G7-HT5: Instruction 3 is the swap instruction (swap program ID).
    #[test]
    fn g7_ht5_swap_instruction_last() {
        g7_harmonic_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, _data) = &ixs[3];
        assert_eq!(
            prog, &input.swap_accounts.program_id,
            "ix3 must be the swap program"
        );
    }

    /// G7-HT6: No SystemProgram::Transfer (Jito tip) in the Harmonic template.
    #[test]
    fn g7_ht6_no_jito_tip_transfer() {
        g7_harmonic_fixture!(_input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        for (i, (prog, data)) in ixs.iter().enumerate() {
            if prog == &SYSTEM_PROGRAM_ID && data.len() == 12 {
                let disc = &data[0..4];
                assert_ne!(
                    disc,
                    &[0x02, 0x00, 0x00, 0x00],
                    "Harmonic template must not contain SystemProgram::Transfer (ix {i})"
                );
            }
        }
    }

    /// G7-HT7: +150 CU microscopic pad is exactly HARMONIC_CU_PAD, not configurable.
    #[test]
    fn g7_ht7_cu_pad_exactly_150() {
        const SRC: &str = include_str!("assembly.rs");
        // Find the pad in build_harmonic_template — the constant used with saturating_add.
        let fn_start = SRC.find("fn build_harmonic_template(").unwrap();
        let fn_rest = &SRC[fn_start..];
        let fn_end = fn_rest.find("\npub fn ").unwrap_or(fn_rest.len());
        let fn_body = &fn_rest[..fn_end];
        assert!(
            fn_body.contains(".saturating_add(HARMONIC_CU_PAD)"),
            "Harmonic CU pad must be exactly HARMONIC_CU_PAD via saturating_add"
        );
        assert_eq!(HARMONIC_CU_PAD, 150, "HARMONIC_CU_PAD must be 150");
    }

    /// G7-HT8: Harmonic template passes proxy-side lane-semantic validation.
    ///
    /// Integration: build a Harmonic template, wrap in sign envelope layout
    /// ([1, sig(64), message...]), and feed through validate_lane_semantics.
    #[test]
    fn g7_ht8_harmonic_template_passes_lane_validation() {
        g7_harmonic_fixture!(_input, tpl);
        // Build a pseudo-signed envelope: [sig_count=1][sig=64 bytes][message bytes]
        let msg = &tpl.raw_bytes[..tpl.len];
        let mut wire = vec![0u8; 1 + 64 + msg.len()];
        wire[0] = 1; // sig_count = 1
                     // wire[1..65] = dummy signature (zeroes)
        wire[65..].copy_from_slice(msg);

        let result = shadow_harmonic::validate_lane_semantics(&wire, wire.len());
        assert!(
            result.is_ok(),
            "Harmonic template must pass lane-semantic validation, got: {:?}",
            result.err()
        );
    }

    /// G7-HT9: Jito template FAILS proxy-side Harmonic lane-semantic validation.
    ///
    /// Proves the validator differentiates the two templates.
    #[test]
    fn g7_ht9_jito_template_fails_lane_validation() {
        // Build a Jito template.
        let alloc = AtomicU64::new(10_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::JitoBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0xBB; 32],
            ingress_slot: 0,
            deadline_unix_ns: 0,
            ingress_unix_ns: 0,
            signal_source: 0,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: [0x55; 32],
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 2,
            },
            swap_ix_data: FixedSwapIxData {
                data: [0; 64],
                len: 17,
                amount_in_offset: 1,
                min_out_offset: 9,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 1000,
                max_reserve_in: 2000,
                min_reserve_out: 3000,
                max_reserve_out: 4000,
            },
            pool_index: 0,
            mint_out_index: 0,
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            overlay_delta: OverlayDelta {
                pool_index: 0,
                reserve_in_delta: 0,
                reserve_out_delta: 0,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0x09; 32],
            shadow_guard_program_id: [0x07; 32],
            guard_vault_in_pubkey: [0x08; 32],
            guard_vault_out_pubkey: [0x0A; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };

        let tpl = build_jito_template(&input).unwrap();
        let msg = &tpl.raw_bytes[..tpl.len];
        let mut wire = vec![0u8; 1 + 64 + msg.len()];
        wire[0] = 1;
        wire[65..].copy_from_slice(msg);

        let result = shadow_harmonic::validate_lane_semantics(&wire, wire.len());
        assert!(
            result.is_err(),
            "Jito template must FAIL Harmonic lane validation"
        );
    }

    /// G7-HT10: emit_prepared routes HarmonicBundle to harmonic_ring (not jito_ring).
    #[test]
    fn g7_ht10_emit_routes_to_harmonic_ring() {
        g7_harmonic_fixture!(input, _tpl);
        input.deadline_unix_ns = u64::MAX;
        input.fee_payer_pubkey = [0; 32]; // match signer

        let mut nonce_ring = NonceRing {
            tuples: [NonceTuple {
                secret_scalar: [1; 32],
                nonce_pubkey: [2; 32],
            }; 128],
            head: 1,
            tail: 0,
        };
        let jito_ring = EgressRing::new(10);
        let tpu_ring = EgressRing::new(10);
        let harmonic_ring = EgressRing::new(10);
        let mut overlay = PoolOverlay::new();
        let signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };

        let res = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_ring,
            &mut overlay,
        );
        assert!(res.is_ok(), "emit_prepared must succeed for Harmonic");
        assert_eq!(
            jito_ring.len(),
            0,
            "Jito ring must be empty — routed to Harmonic"
        );
        assert_eq!(harmonic_ring.len(), 1, "Harmonic ring must have 1 envelope");
    }

    /// G7-HT11: atba and apex_partial are rejected by harmonic template.
    #[test]
    fn g7_ht11_forbidden_tactics_rejected() {
        g7_harmonic_fixture!(input, _unused_tpl);

        input.prepared.intent_kind = crate::routing::IntentKind::Atba;
        assert_eq!(
            build_harmonic_template(&input).unwrap_err(),
            BuildError::TacticNotAllowedOnRoute,
            "atba must be rejected on Harmonic"
        );

        input.prepared.intent_kind = crate::routing::IntentKind::ApexPartial;
        assert_eq!(
            build_harmonic_template(&input).unwrap_err(),
            BuildError::TacticNotAllowedOnRoute,
            "apex_partial must be rejected on Harmonic"
        );
    }

    /// G7-HT12: Overlay applied only on successful enqueue, not on ring-full.
    #[test]
    fn g7_ht12_overlay_only_on_success() {
        g7_harmonic_fixture!(input, _unused_tpl);
        input.deadline_unix_ns = u64::MAX;
        input.fee_payer_pubkey = [0; 32];

        let mut nonce_ring = NonceRing {
            tuples: [NonceTuple {
                secret_scalar: [1; 32],
                nonce_pubkey: [2; 32],
            }; 128],
            head: 2,
            tail: 0,
        };
        let signer = ThreadLocalSigner {
            pubkey: [0; 32],
            secret: [0; 32],
        };

        // Case 1: ring full → overlay NOT applied
        let jito_ring = EgressRing::new(10);
        let tpu_ring = EgressRing::new(10);
        let harmonic_full = EgressRing::new(0);
        let mut overlay1 = PoolOverlay::new();
        let res1 = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_full,
            &mut overlay1,
        );
        assert_eq!(res1, Err(EmitError::EnqueueFailedRingFull));
        assert_eq!(
            overlay1.get(42),
            None,
            "overlay must not be applied on ring-full"
        );

        // Refill nonce ring for second attempt
        nonce_ring.head = 2;
        nonce_ring.tail = 0;
        nonce_ring.tuples[0] = NonceTuple {
            secret_scalar: [3; 32],
            nonce_pubkey: [4; 32],
        };
        nonce_ring.tuples[1] = NonceTuple {
            secret_scalar: [5; 32],
            nonce_pubkey: [6; 32],
        };

        // Case 2: ring has space → overlay applied
        let harmonic_open = EgressRing::new(10);
        let mut overlay2 = PoolOverlay::new();
        let res2 = emit_prepared(
            &input,
            &signer,
            &mut nonce_ring,
            &jito_ring,
            &tpu_ring,
            &harmonic_open,
            &mut overlay2,
        );
        assert!(res2.is_ok());
        assert_eq!(
            overlay2.get(42),
            Some((500, -500)),
            "overlay must be applied on success"
        );
    }

    // -----------------------------------------------------------------------
    // G10 – Jito template contract tests + Harmonic constant-lock
    //
    // Mirrors the G7-HT series for the Jito template.  Every test locks one
    // property of the template shape; if it fails, the template is out of
    // contract — fix the template, not the test.
    // -----------------------------------------------------------------------

    /// Macro: build a Jito template from a minimal 2-account swap fixture.
    macro_rules! g10_jito_fixture {
        ($input:ident, $tpl:ident) => {
            let alloc = AtomicU64::new(10_000_000);
            let fencer = InventoryFencer {
                current_allocation: &alloc,
                absolute_ceiling: u64::MAX,
            };
            let mut $input = ExecutionAssemblyInput {
                opso_economics: OpsoObserveOnlyEconomics::default(),
                prepared: dummy_prepared(
                    RouteDecision::JitoBundle,
                    crate::routing::IntentKind::PlainWithinCap,
                ),
                recent_blockhash: [0xBB; 32],
                ingress_slot: 0,
                deadline_unix_ns: 0,
                ingress_unix_ns: 0,
                signal_source: 0,
                template_id: TemplateId::JitoPlain,
                quote_request_id: 0,
                route_base_cu_limit: 300_000,
                configured_route_base_cu_limit: 300_000,
                micro_market_observed: false,
                micro_market_flat_retained_ev_lamports: 0,
                micro_market_delta_lamports: 0,
                trade_size_lamports: 1_000_000,
                slippage_bps: 100,
                quote_target_out_lamports: 500_000,
                quote_expected_out_lamports: 990_000,
                quote_baseline_reserve_in: 10_000_000,
                quote_baseline_reserve_out: 9_000_000,
                quote_projected_reserve_in: 10_500_000,
                quote_projected_reserve_out: 8_500_000,
                quote_trade_fee_num: 25,
                quote_trade_fee_denom: 10_000,
                quote_mint_in_transfer_fee_bps: 0,
                quote_mint_in_max_fee_lamports: 0,
                swap_accounts: FixedSwapAccounts {
                    program_id: [0x55; 32],
                    metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                    num_accounts: 2,
                },
                swap_ix_data: FixedSwapIxData {
                    data: [0; 64],
                    len: 17,
                    amount_in_offset: 1,
                    min_out_offset: 9,
                },
                guard_bounds: GuardBounds {
                    min_reserve_in: 1000,
                    max_reserve_in: 2000,
                    min_reserve_out: 3000,
                    max_reserve_out: 4000,
                },
                pool_index: 42,
                mint_out_index: 7,
                pool_pubkey: [42; 32],
                mint_in_pubkey: [43; 32],
                mint_out_pubkey: [44; 32],
                overlay_delta: OverlayDelta {
                    pool_index: 42,
                    reserve_in_delta: 500,
                    reserve_out_delta: -500,
                },
                jito_tip_rotation: JitoTipRotation {
                    tip_account: [0xCC; 32],
                },
                inventory_fencer: &fencer,
                fee_payer_pubkey: [0x09; 32],
                shadow_guard_program_id: [0x07; 32],
                guard_vault_in_pubkey: [0x08; 32],
                guard_vault_out_pubkey: [0x0A; 32],
                send_timestamp_ns: 0,
                tactic_class: 0,
                worker_id: 0,
                venue_id: 0,
                public_rpc_scout_explicit: false,
                opso_owner_mode: 0,
                opso_fallback_mode: 7,
                opso_reason_code: 0,
                episode_id_mod: 0,
            };
            $input.swap_accounts.metas[0] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x11; 32]),
                is_writable: true,
            };
            $input.swap_accounts.metas[1] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x22; 32]),
                is_writable: false,
            };
            let $tpl = build_jito_template(&$input).unwrap();
        };
    }

    /// G10-JT1: Jito template has exactly 4 instructions.
    /// [SetComputeUnitLimit, ShadowGuard, Swap, SystemProgram::Transfer]
    #[test]
    fn g10_jt1_four_instructions() {
        g10_jito_fixture!(_input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        assert_eq!(
            ixs.len(),
            4,
            "Jito template must have exactly 4 instructions"
        );
    }

    #[test]
    fn g10_jito_accepts_raydium_clmm_supported_template() {
        let alloc = AtomicU64::new(10_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::PublicRpcScout,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0xBB; 32],
            ingress_slot: 0,
            deadline_unix_ns: 1,
            ingress_unix_ns: 0,
            signal_source: SIGNAL_SOURCE_AFXDP,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: RAYDIUM_CLMM_PROGRAM_ID,
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 13,
            },
            swap_ix_data: FixedSwapIxData {
                data: {
                    let mut data = [0; 64];
                    data[40] = 1;
                    data
                },
                len: 41,
                amount_in_offset: 8,
                min_out_offset: 16,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 1000,
                max_reserve_in: 2000,
                min_reserve_out: 3000,
                max_reserve_out: 4000,
            },
            pool_index: 42,
            mint_out_index: 7,
            pool_pubkey: [42; 32],
            mint_in_pubkey: [43; 32],
            mint_out_pubkey: [44; 32],
            overlay_delta: OverlayDelta {
                pool_index: 42,
                reserve_in_delta: 500,
                reserve_out_delta: -500,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0x09; 32],
            shadow_guard_program_id: [0x07; 32],
            guard_vault_in_pubkey: [0x08; 32],
            guard_vault_out_pubkey: [0x0A; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: VENUE_RAYDIUM_CLMM,
            public_rpc_scout_explicit: true,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };
        input.swap_accounts.metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::FeePayerSigner,
            is_writable: true,
        };
        for i in 1..13 {
            input.swap_accounts.metas[i] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([i as u8; 32]),
                is_writable: i <= 6,
            };
        }

        let tpl = build_jito_template(&input).expect("supported CLMM template must build");
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        assert_eq!(ixs[2].0, RAYDIUM_CLMM_PROGRAM_ID);
    }

    #[test]
    fn g10_jito_dedupes_shared_clmm_guard_vault_accounts() {
        let alloc = AtomicU64::new(10_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::PublicRpcScout,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0xBB; 32],
            ingress_slot: 0,
            deadline_unix_ns: 1,
            ingress_unix_ns: 0,
            signal_source: SIGNAL_SOURCE_AFXDP,
            template_id: TemplateId::JitoPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: RAYDIUM_CLMM_PROGRAM_ID,
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 10,
            },
            swap_ix_data: FixedSwapIxData {
                data: {
                    let mut data = [0; 64];
                    data[40] = 1;
                    data
                },
                len: 41,
                amount_in_offset: 8,
                min_out_offset: 16,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 1000,
                max_reserve_in: 2000,
                min_reserve_out: 3000,
                max_reserve_out: 4000,
            },
            pool_index: 42,
            mint_out_index: 7,
            pool_pubkey: [42; 32],
            mint_in_pubkey: [43; 32],
            mint_out_pubkey: [44; 32],
            overlay_delta: OverlayDelta {
                pool_index: 42,
                reserve_in_delta: 500,
                reserve_out_delta: -500,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0x09; 32],
            shadow_guard_program_id: [0x07; 32],
            guard_vault_in_pubkey: [0x31; 32],
            guard_vault_out_pubkey: [0x32; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: VENUE_RAYDIUM_CLMM,
            public_rpc_scout_explicit: true,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };
        input.swap_accounts.metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::FeePayerSigner,
            is_writable: true,
        };
        input.swap_accounts.metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey(input.guard_vault_in_pubkey),
            is_writable: true,
        };
        input.swap_accounts.metas[2] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey(input.guard_vault_out_pubkey),
            is_writable: true,
        };
        input.swap_accounts.metas[3] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x44; 32]),
            is_writable: false,
        };
        for i in 4..10 {
            input.swap_accounts.metas[i] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x44 + i as u8; 32]),
                is_writable: false,
            };
        }

        let tpl = build_jito_template(&input).expect("shared-vault CLMM Jito template must build");
        let (accts, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        assert_eq!(count_unique_accounts(&accts), accts.len());
        assert_eq!(
            accts.len(),
            15,
            "shared guard vaults must not duplicate account keys"
        );
        assert_eq!(ixs[2].program_id, RAYDIUM_CLMM_PROGRAM_ID);
        assert_eq!(ixs[1].account_indices[0], ixs[2].account_indices[1]);
        assert_eq!(ixs[1].account_indices[1], ixs[2].account_indices[2]);
    }

    #[test]
    fn g10_harmonic_dedupes_shared_clmm_guard_vault_accounts() {
        let alloc = AtomicU64::new(10_000_000);
        let fencer = InventoryFencer {
            current_allocation: &alloc,
            absolute_ceiling: u64::MAX,
        };
        let mut input = ExecutionAssemblyInput {
            opso_economics: OpsoObserveOnlyEconomics::default(),
            prepared: dummy_prepared(
                RouteDecision::HarmonicBundle,
                crate::routing::IntentKind::PlainWithinCap,
            ),
            recent_blockhash: [0xBB; 32],
            ingress_slot: 0,
            deadline_unix_ns: 1,
            ingress_unix_ns: 0,
            signal_source: SIGNAL_SOURCE_AFXDP,
            template_id: TemplateId::HarmonicPlain,
            quote_request_id: 0,
            route_base_cu_limit: 300_000,
            configured_route_base_cu_limit: 300_000,
            micro_market_observed: false,
            micro_market_flat_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            trade_size_lamports: 1_000_000,
            slippage_bps: 100,
            quote_target_out_lamports: 500_000,
            quote_expected_out_lamports: 990_000,
            quote_baseline_reserve_in: 10_000_000,
            quote_baseline_reserve_out: 9_000_000,
            quote_projected_reserve_in: 10_500_000,
            quote_projected_reserve_out: 8_500_000,
            quote_trade_fee_num: 25,
            quote_trade_fee_denom: 10_000,
            quote_mint_in_transfer_fee_bps: 0,
            quote_mint_in_max_fee_lamports: 0,
            swap_accounts: FixedSwapAccounts {
                program_id: RAYDIUM_CLMM_PROGRAM_ID,
                metas: [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS],
                num_accounts: 10,
            },
            swap_ix_data: FixedSwapIxData {
                data: {
                    let mut data = [0; 64];
                    data[40] = 1;
                    data
                },
                len: 41,
                amount_in_offset: 8,
                min_out_offset: 16,
            },
            guard_bounds: GuardBounds {
                min_reserve_in: 1000,
                max_reserve_in: 2000,
                min_reserve_out: 3000,
                max_reserve_out: 4000,
            },
            pool_index: 42,
            mint_out_index: 7,
            pool_pubkey: [42; 32],
            mint_in_pubkey: [43; 32],
            mint_out_pubkey: [44; 32],
            overlay_delta: OverlayDelta {
                pool_index: 42,
                reserve_in_delta: 500,
                reserve_out_delta: -500,
            },
            jito_tip_rotation: JitoTipRotation {
                tip_account: [0xCC; 32],
            },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [0x09; 32],
            shadow_guard_program_id: [0x07; 32],
            guard_vault_in_pubkey: [0x31; 32],
            guard_vault_out_pubkey: [0x32; 32],
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: VENUE_RAYDIUM_CLMM,
            public_rpc_scout_explicit: false,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
        };
        input.swap_accounts.metas[0] = FixedSwapAccountMeta {
            source: SwapAccountSource::FeePayerSigner,
            is_writable: true,
        };
        input.swap_accounts.metas[1] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey(input.guard_vault_in_pubkey),
            is_writable: true,
        };
        input.swap_accounts.metas[2] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey(input.guard_vault_out_pubkey),
            is_writable: true,
        };
        input.swap_accounts.metas[3] = FixedSwapAccountMeta {
            source: SwapAccountSource::ExplicitPubkey([0x44; 32]),
            is_writable: false,
        };
        for i in 4..10 {
            input.swap_accounts.metas[i] = FixedSwapAccountMeta {
                source: SwapAccountSource::ExplicitPubkey([0x44 + i as u8; 32]),
                is_writable: false,
            };
        }

        let tpl = build_harmonic_template(&input)
            .expect("shared-vault CLMM Harmonic template must build");
        let (accts, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        assert_eq!(count_unique_accounts(&accts), accts.len());
        assert_eq!(
            accts.len(),
            13,
            "shared guard vaults must not duplicate account keys"
        );
        assert_eq!(ixs[3].program_id, RAYDIUM_CLMM_PROGRAM_ID);
        assert_eq!(ixs[2].account_indices[0], ixs[3].account_indices[1]);
        assert_eq!(ixs[2].account_indices[1], ixs[3].account_indices[2]);
    }

    /// G10-JT2: Instruction 0 is SetComputeUnitLimit (disc=0x02, 5 bytes).
    /// CU limit equals `route_base_cu_limit` exactly — NO pad.
    #[test]
    fn g10_jt2_cu_limit_is_ix0() {
        g10_jito_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, data) = &ixs[0];
        assert_eq!(
            prog, &COMPUTE_BUDGET_PROGRAM_ID,
            "ix0 must be ComputeBudget"
        );
        assert_eq!(data.len(), 5, "SetComputeUnitLimit data must be 5 bytes");
        assert_eq!(
            data[0], 0x02,
            "SetComputeUnitLimit discriminator must be 0x02"
        );
        let cu_limit = u32::from_le_bytes(data[1..5].try_into().unwrap());
        assert_eq!(
            cu_limit, input.route_base_cu_limit,
            "Jito CU limit must be route_base_cu_limit exactly (no pad)"
        );
    }

    /// G10-JT3: Jito template contains NO SetComputeUnitPrice instruction.
    /// Jito bundles do not need priority fees — they pay via the tip transfer.
    #[test]
    fn g10_jt3_no_cu_price() {
        g10_jito_fixture!(_input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        for (i, (prog, data)) in ixs.iter().enumerate() {
            if prog == &COMPUTE_BUDGET_PROGRAM_ID && !data.is_empty() {
                assert_ne!(
                    data[0], 0x03,
                    "Jito template must not contain SetComputeUnitPrice (found at ix {i})"
                );
            }
        }
    }

    /// G10-JT4: Instruction 1 is ShadowGuard::AssertReservesInRange.
    /// 32 bytes data, bounds match input.
    #[test]
    fn g10_jt4_guard_is_ix1() {
        g10_jito_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, data) = &ixs[1];
        assert_eq!(
            prog, &input.shadow_guard_program_id,
            "ix1 must be ShadowGuard"
        );
        assert_eq!(
            data.len(),
            SHADOW_GUARD_IX_DATA_LEN,
            "ShadowGuard data must be 32 bytes"
        );
        let bounds = [
            u64::from_le_bytes(data[0..8].try_into().unwrap()),
            u64::from_le_bytes(data[8..16].try_into().unwrap()),
            u64::from_le_bytes(data[16..24].try_into().unwrap()),
            u64::from_le_bytes(data[24..32].try_into().unwrap()),
        ];
        assert_eq!(
            bounds,
            [
                input.guard_bounds.min_reserve_in,
                input.guard_bounds.max_reserve_in,
                input.guard_bounds.min_reserve_out,
                input.guard_bounds.max_reserve_out,
            ],
            "ShadowGuard bounds must match input"
        );
    }

    /// G10-JT5: Instruction 2 is the swap instruction.
    #[test]
    fn g10_jt5_swap_is_ix2() {
        g10_jito_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, _data) = &ixs[2];
        assert_eq!(
            prog, &input.swap_accounts.program_id,
            "ix2 must be the swap program"
        );
    }

    /// G10-JT6: Instruction 3 is SystemProgram::Transfer (Jito tip), and it is the last ix.
    /// Transfer discriminator is [0x02, 0x00, 0x00, 0x00], total data is 12 bytes.
    #[test]
    fn g10_jt6_tip_is_ix3_and_last() {
        g10_jito_fixture!(_input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (prog, data) = &ixs[3];
        assert_eq!(prog, &SYSTEM_PROGRAM_ID, "ix3 must be SystemProgram");
        assert_eq!(
            data.len(),
            12,
            "SystemProgram::Transfer data must be 12 bytes"
        );
        assert_eq!(
            &data[0..4],
            &[0x02, 0x00, 0x00, 0x00],
            "Transfer discriminator must be 0x02000000"
        );
        assert_eq!(ixs.len(), 4, "tip must be the final instruction");
    }

    /// G10-JT7: Jito CU limit uses route_base_cu_limit verbatim (no CU pad).
    #[test]
    fn g10_jt7_cu_limit_no_pad() {
        const SRC: &str = include_str!("assembly.rs");
        let fn_start = SRC.find("fn build_jito_template(").unwrap();
        let fn_rest = &SRC[fn_start..];
        let fn_end = fn_rest.find("\npub fn ").unwrap_or(fn_rest.len());
        let fn_body = &fn_rest[..fn_end];
        assert!(
            fn_body.contains("route_base_cu_limit.to_le_bytes()"),
            "Jito CU limit must use route_base_cu_limit directly"
        );
        // The Jito template must NOT pad CU limit (no HARMONIC_CU_PAD reference).
        assert!(
            !fn_body.contains("HARMONIC_CU_PAD"),
            "Jito template must NOT reference HARMONIC_CU_PAD"
        );
        assert!(
            !fn_body.contains("route_base_cu_limit.saturating_add"),
            "Jito CU limit must NOT use saturating_add (that would be a pad)"
        );
    }

    /// G10-HT1: HARMONIC_CU_PAD constant matches the value baked into the template.
    #[test]
    fn g10_ht1_cu_pad_constant_matches() {
        assert_eq!(HARMONIC_CU_PAD, 150, "HARMONIC_CU_PAD must be 150");
        g7_harmonic_fixture!(input, tpl);
        let ixs = parse_message_instructions(&tpl.raw_bytes[..tpl.len]);
        let (_prog, data) = &ixs[0];
        let cu_limit = u32::from_le_bytes(data[1..5].try_into().unwrap());
        assert_eq!(
            cu_limit,
            input.route_base_cu_limit + HARMONIC_CU_PAD,
            "Harmonic CU limit must be route_base + HARMONIC_CU_PAD"
        );
    }

    // -----------------------------------------------------------------------
    // G10 – Account ordering, program set, fee safety, ATBA dormancy
    // -----------------------------------------------------------------------

    /// G10-JT8: Jito account table exact order.
    /// [fee_payer, tip, writable_swap.., readonly_swap.., vault_in, vault_out,
    ///  ComputeBudget, ShadowGuard, SwapProgram, SystemProgram]
    #[test]
    fn g10_jt8_account_table_order() {
        g10_jito_fixture!(input, tpl);
        let (accts, _) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        assert_eq!(accts[0], input.fee_payer_pubkey, "acct[0] = fee_payer");
        assert_eq!(
            accts[1], input.jito_tip_rotation.tip_account,
            "acct[1] = tip_account"
        );
        assert_eq!(accts[2], [0x11; 32], "acct[2] = writable swap");
        assert_eq!(accts[3], [0x22; 32], "acct[3] = readonly swap");
        assert_eq!(accts[4], input.guard_vault_in_pubkey, "acct[4] = vault_in");
        assert_eq!(
            accts[5], input.guard_vault_out_pubkey,
            "acct[5] = vault_out"
        );
        assert_eq!(
            accts[6], COMPUTE_BUDGET_PROGRAM_ID,
            "acct[6] = ComputeBudget"
        );
        assert_eq!(
            accts[7], input.shadow_guard_program_id,
            "acct[7] = ShadowGuard"
        );
        assert_eq!(
            accts[8], input.swap_accounts.program_id,
            "acct[8] = SwapProgram"
        );
        assert_eq!(accts[9], SYSTEM_PROGRAM_ID, "acct[9] = SystemProgram");
        assert_eq!(
            accts.len(),
            10,
            "Jito 2-acct fixture must have 10 accounts total"
        );
    }

    /// G10-JT9: Guard instruction references vault_in and vault_out in order.
    #[test]
    fn g10_jt9_guard_ix_accounts() {
        g10_jito_fixture!(input, tpl);
        let (accts, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        let g = &ixs[1];
        assert_eq!(
            g.account_indices.len(),
            2,
            "Guard must reference exactly 2 accounts"
        );
        assert_eq!(
            accts[g.account_indices[0] as usize],
            input.guard_vault_in_pubkey
        );
        assert_eq!(
            accts[g.account_indices[1] as usize],
            input.guard_vault_out_pubkey
        );
    }

    /// G10-JT10: Tip transfer references fee_payer (from) and tip_account (to).
    #[test]
    fn g10_jt10_tip_ix_accounts() {
        g10_jito_fixture!(input, tpl);
        let (accts, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        let t = &ixs[3];
        assert_eq!(
            t.account_indices.len(),
            2,
            "Tip must reference exactly 2 accounts"
        );
        assert_eq!(
            accts[t.account_indices[0] as usize], input.fee_payer_pubkey,
            "tip from"
        );
        assert_eq!(
            accts[t.account_indices[1] as usize], input.jito_tip_rotation.tip_account,
            "tip to"
        );
    }

    /// G10-JT11: Jito template uses exactly 4 unique programs.
    #[test]
    fn g10_jt11_four_programs() {
        g10_jito_fixture!(input, tpl);
        let (_, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        let mut progs: Vec<[u8; 32]> = ixs.iter().map(|ix| ix.program_id).collect();
        progs.sort();
        progs.dedup();
        assert_eq!(progs.len(), 4, "Jito must use exactly 4 programs");
        assert!(progs.contains(&COMPUTE_BUDGET_PROGRAM_ID));
        assert!(progs.contains(&input.shadow_guard_program_id));
        assert!(progs.contains(&input.swap_accounts.program_id));
        assert!(progs.contains(&SYSTEM_PROGRAM_ID));
    }

    /// G10-HT2: Harmonic account table exact order.
    /// [fee_payer, writable_swap.., readonly_swap.., vault_in, vault_out,
    ///  ComputeBudget, ShadowGuard, SwapProgram]
    #[test]
    fn g10_ht2_account_table_order() {
        g7_harmonic_fixture!(input, tpl);
        let (accts, _) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        assert_eq!(accts[0], input.fee_payer_pubkey, "acct[0] = fee_payer");
        assert_eq!(accts[1], [0x11; 32], "acct[1] = writable swap");
        assert_eq!(accts[2], [0x22; 32], "acct[2] = readonly swap");
        assert_eq!(accts[3], input.guard_vault_in_pubkey, "acct[3] = vault_in");
        assert_eq!(
            accts[4], input.guard_vault_out_pubkey,
            "acct[4] = vault_out"
        );
        assert_eq!(
            accts[5], COMPUTE_BUDGET_PROGRAM_ID,
            "acct[5] = ComputeBudget"
        );
        assert_eq!(
            accts[6], input.shadow_guard_program_id,
            "acct[6] = ShadowGuard"
        );
        assert_eq!(
            accts[7], input.swap_accounts.program_id,
            "acct[7] = SwapProgram"
        );
        assert_eq!(
            accts.len(),
            8,
            "Harmonic 2-acct fixture must have 8 accounts total"
        );
    }

    /// G10-HT3: Harmonic guard references vault_in and vault_out.
    #[test]
    fn g10_ht3_guard_ix_accounts() {
        g7_harmonic_fixture!(input, tpl);
        let (accts, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        let g = &ixs[2]; // Harmonic: ix2 = guard (after SetCULimit, SetCUPrice)
        assert_eq!(
            g.account_indices.len(),
            2,
            "Guard must reference exactly 2 accounts"
        );
        assert_eq!(
            accts[g.account_indices[0] as usize],
            input.guard_vault_in_pubkey
        );
        assert_eq!(
            accts[g.account_indices[1] as usize],
            input.guard_vault_out_pubkey
        );
    }

    /// G10-HT4: Harmonic template uses exactly 3 programs (no SystemProgram).
    #[test]
    fn g10_ht4_three_programs() {
        g7_harmonic_fixture!(input, tpl);
        let (_, ixs) = parse_message_full(&tpl.raw_bytes[..tpl.len]);
        let mut progs: Vec<[u8; 32]> = ixs.iter().map(|ix| ix.program_id).collect();
        progs.sort();
        progs.dedup();
        assert_eq!(progs.len(), 3, "Harmonic must use exactly 3 programs");
        assert!(progs.contains(&COMPUTE_BUDGET_PROGRAM_ID));
        assert!(progs.contains(&input.shadow_guard_program_id));
        assert!(progs.contains(&input.swap_accounts.program_id));
        assert!(
            !progs.contains(&SYSTEM_PROGRAM_ID),
            "Harmonic must NOT have SystemProgram"
        );
    }

    /// G10-HT5: Fee overhead from Harmonic +150 CU pad is negligible.
    /// Extra fee = cu_price_micro_lamports × HARMONIC_CU_PAD / 1_000_000.
    /// At 100k µlam/CU (aggressive), extra = 15 lamports ≈ 0.000000015 SOL.
    #[test]
    fn g10_ht5_cu_pad_fee_negligible() {
        let worst_price: u64 = 100_000;
        let extra = worst_price * (HARMONIC_CU_PAD as u64) / 1_000_000;
        assert!(
            extra < 1000,
            "+{} CU pad fee must be <1000 lam; got {extra}",
            HARMONIC_CU_PAD
        );
    }

    /// G10-ATBA: ATBA unconditionally dormant on Harmonic lane.
    /// Gate is structural in build_harmonic_template (no config bypass).
    #[test]
    fn g10_atba_dormant() {
        g7_harmonic_fixture!(input, _tpl);
        input.prepared.intent_kind = crate::routing::IntentKind::Atba;
        assert_eq!(
            build_harmonic_template(&input).unwrap_err(),
            BuildError::TacticNotAllowedOnRoute,
        );
        // Structural: gate is unconditional.
        const SRC: &str = include_str!("assembly.rs");
        let start = SRC.find("fn build_harmonic_template(").unwrap();
        let rest = &SRC[start..];
        let end = rest.find("\npub fn ").unwrap_or(rest.len());
        let body = &rest[..end];
        assert!(
            body.contains("IntentKind::Atba"),
            "ATBA gate must exist in source"
        );
        assert!(
            body.contains("TacticNotAllowedOnRoute"),
            "ATBA must produce TacticNotAllowedOnRoute"
        );
        assert!(
            !body.contains("enable_atba"),
            "ATBA gate must be unconditional"
        );
    }
}
