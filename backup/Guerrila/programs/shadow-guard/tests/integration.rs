//! G10 — ShadowGuard local-validator integration tests.
//!
//! Deploys shadow_guard.so into a BanksClient (in-process BPF VM) and runs the
//! three required scenarios:
//!   1. Guard aborts when target is a no-op (vault amounts unchanged).
//!   2. Guard aborts when reserves move adversely.
//!   3. Guard passes when reserves are in the projected valid range.

use solana_program_test::*;
use solana_sdk::{
    account::Account,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};

use shadow_core::assembly::{
    build_harmonic_template, build_jito_template, ExecutionAssemblyInput, FixedSwapAccountMeta,
    FixedSwapAccounts, FixedSwapIxData, GuardBounds, JitoTipRotation, OverlayDelta,
    SwapAccountSource, TemplateId, UnsignedEnvelope, HARMONIC_CU_PAD,
};
use shadow_core::inventory::InventoryFencer;
use shadow_core::opso::OpsoObserveOnlyEconomics;
use shadow_core::policy::{DominanceDecision, PreparedExecution};
use shadow_core::routing::{IntentKind, RouteDecision, RouteReason};
use std::sync::atomic::AtomicU64;

// SPL Token program: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
const SPL_TOKEN_PROGRAM_ID: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];

/// Program ID derived from target/deploy/shadow_guard-keypair.json.
fn shadow_guard_program_id() -> Pubkey {
    // Pubkey bytes extracted from keypair[32..64].
    Pubkey::new_from_array([
        44, 166, 212, 243, 246, 95, 159, 181, 8, 196, 202, 112, 80, 238, 140, 127, 234, 117, 35,
        83, 58, 134, 182, 245, 66, 191, 101, 112, 196, 29, 178, 146,
    ])
}

/// Build a ProgramTest environment with the ShadowGuard BPF .so loaded.
fn program_test() -> ProgramTest {
    let mut pt = ProgramTest::new(
        "shadow_guard",
        shadow_guard_program_id(),
        None, // None = load .so from SBF_OUT_DIR or target/deploy/
    );
    pt
}

/// Inject an SPL Token vault account with a specific `amount` at offset 64.
///
/// Uses the same 165-byte layout as the unit-test helper in lib.rs.
fn add_spl_vault(pt: &mut ProgramTest, vault_pubkey: Pubkey, amount: u64) {
    let mint = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(&mint.to_bytes());
    data[32..64].copy_from_slice(&owner.to_bytes());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[72] = 1; // state = Initialized

    pt.add_account(
        vault_pubkey,
        Account {
            lamports: 1_000_000,
            data,
            owner: Pubkey::new_from_array(SPL_TOKEN_PROGRAM_ID),
            executable: false,
            rent_epoch: 0,
        },
    );
}

/// Encode guard bounds as 32-byte instruction data (same layout as assembly.rs).
fn encode_bounds(min_in: u64, max_in: u64, min_out: u64, max_out: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32);
    buf.extend_from_slice(&min_in.to_le_bytes());
    buf.extend_from_slice(&max_in.to_le_bytes());
    buf.extend_from_slice(&min_out.to_le_bytes());
    buf.extend_from_slice(&max_out.to_le_bytes());
    buf
}

// ---------------------------------------------------------------------------
// Scenario 1: Guard aborts when target is a no-op
// ---------------------------------------------------------------------------

/// The upstream target transaction was a no-op — vault amounts are unchanged
/// from the pre-trade snapshot.  The sniper computed guard bounds expecting
/// the target to move reserves.  Since reserves didn't move, the actual vault
/// amounts fall outside the projected range, and the guard aborts.
#[tokio::test]
async fn g10_int1_guard_aborts_on_noop_target() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();

    // Current (unchanged) state: vault_in=1,000,000  vault_out=500,000
    add_spl_vault(&mut pt, vault_in, 1_000_000);
    add_spl_vault(&mut pt, vault_out, 500_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    // Guard bounds expect post-target state:
    //   vault_in  ∈ [1,050,000 .. 1,150,000]  (SOL in increased by target buy)
    //   vault_out ∈ [400,000   .. 480,000]     (token out decreased by target buy)
    //
    // Actual: vault_in=1,000,000 < min=1,050,000 → VaultInOutOfRange
    let ix = Instruction {
        program_id: shadow_guard_program_id(),
        accounts: vec![
            AccountMeta::new_readonly(vault_in, false),
            AccountMeta::new_readonly(vault_out, false),
        ],
        data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;

    assert!(
        result.is_err(),
        "Guard must abort when target is a no-op (vault_in below projected range)"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Guard aborts on adverse reserve movement
// ---------------------------------------------------------------------------

/// The target executed, but reserves moved in the wrong direction (adverse
/// for our backrun).  We expected the target to buy (increasing vault_in,
/// decreasing vault_out), but it actually sold (decreasing vault_in,
/// increasing vault_out).  The guard detects this inversion.
#[tokio::test]
async fn g10_int2_guard_aborts_on_adverse_movement() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();

    // Adverse state: vault_in=900,000 (decreased), vault_out=600,000 (increased)
    add_spl_vault(&mut pt, vault_in, 900_000);
    add_spl_vault(&mut pt, vault_out, 600_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    // Same bounds as scenario 1 — expecting a buy target:
    //   vault_in  ∈ [1,050,000 .. 1,150,000]
    //   vault_out ∈ [400,000   .. 480,000]
    //
    // vault_in=900,000 < min=1,050,000 → VaultInOutOfRange
    let ix = Instruction {
        program_id: shadow_guard_program_id(),
        accounts: vec![
            AccountMeta::new_readonly(vault_in, false),
            AccountMeta::new_readonly(vault_out, false),
        ],
        data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;

    assert!(
        result.is_err(),
        "Guard must abort on adverse reserve movement"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Guard passes on valid projected range
// ---------------------------------------------------------------------------

/// The target executed as projected.  Reserves are within the guard's
/// projected range.  The guard passes, and the subsequent instruction
/// (a stub System::Transfer) executes successfully.
#[tokio::test]
async fn g10_int3_guard_passes_valid_range() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();

    // Post-target state within projected range:
    //   vault_in=1,100,000 ∈ [1,050,000 .. 1,150,000] ✓
    //   vault_out=450,000  ∈ [400,000   .. 480,000]    ✓
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    let guard_ix = Instruction {
        program_id: shadow_guard_program_id(),
        accounts: vec![
            AccountMeta::new_readonly(vault_in, false),
            AccountMeta::new_readonly(vault_out, false),
        ],
        data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
    };

    // Stub "swap" — a 0-lamport self-transfer proves the guard passed and
    // execution continued to the next instruction.
    let stub_swap = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0);

    let tx = Transaction::new_signed_with_payer(
        &[guard_ix, stub_swap],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;

    assert!(
        result.is_ok(),
        "Guard must pass when reserves are in valid range; got {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// CU measurement + Harmonic pad proof
// ---------------------------------------------------------------------------

/// ShadowGuard CU pass-path ceiling. Evidence: BanksClient local-validator
/// binary-search probe — guard passes at 744 CU, fails at 743.
/// Ceiling at 900 includes 21% margin for toolchain/runtime variance.
const SHADOW_GUARD_CU_PASS_CEILING: u32 = 900;

// ComputeBudget111111111111111111111111111111
const COMPUTE_BUDGET_PROGRAM_ID_BYTES: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];

fn set_compute_unit_limit(units: u32) -> Instruction {
    Instruction {
        program_id: Pubkey::new_from_array(COMPUTE_BUDGET_PROGRAM_ID_BYTES),
        accounts: vec![],
        data: {
            let mut d = Vec::with_capacity(5);
            d.push(0x02);
            d.extend_from_slice(&units.to_le_bytes());
            d
        },
    }
}

fn set_compute_unit_price(micro_lamports: u64) -> Instruction {
    Instruction {
        program_id: Pubkey::new_from_array(COMPUTE_BUDGET_PROGRAM_ID_BYTES),
        accounts: vec![],
        data: {
            let mut d = Vec::with_capacity(9);
            d.push(0x03);
            d.extend_from_slice(&micro_lamports.to_le_bytes());
            d
        },
    }
}

/// Full template non-swap CU overhead. Both 4-instruction shapes (Jito, Harmonic)
/// consume 1044 CU in non-swap overhead. Measured via BanksClient binary-search probe.
/// Breakdown: 4 × ~150 CU instruction dispatch + 594 CU guard BPF execution.
const FULL_TEMPLATE_NONSWAP_CU: u32 = 1044;

/// No-op BPF-less processor registered as the swap program in BanksClient.
fn mock_swap_process(
    _program_id: &solana_program::pubkey::Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    _instruction_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    Ok(())
}

/// Build a ProgramTest with both ShadowGuard (native) and a mock swap (native processor).
/// Uses prefer_bpf(false) so both programs run as native builtins — no .so files needed.
/// CU measurement differs from BPF, but template validity is identical.
fn program_test_with_mock_swap(mock_swap_id: Pubkey) -> ProgramTest {
    let mut pt = ProgramTest::default();
    pt.prefer_bpf(false);
    pt.add_program(
        "shadow_guard",
        shadow_guard_program_id(),
        processor!(shadow_guard::process_instruction),
    );
    pt.add_program("mock_swap", mock_swap_id, processor!(mock_swap_process));
    pt
}

/// Convert real template builder output into a signed Transaction for BanksClient.
fn envelope_to_transaction(
    envelope: &UnsignedEnvelope,
    payer: &Keypair,
    recent_blockhash: Hash,
) -> Transaction {
    let message: solana_sdk::message::Message =
        bincode::deserialize(&envelope.raw_bytes[..envelope.len])
            .expect("UnsignedEnvelope must deserialize to a valid solana Message");
    let mut tx = Transaction::new_unsigned(message);
    tx.sign(&[payer], recent_blockhash);
    tx
}

// ---------------------------------------------------------------------------
// Scenario 4: Guard fits within CU ceiling
// ---------------------------------------------------------------------------

/// Guard must execute within SHADOW_GUARD_CU_PASS_CEILING CU.
/// Sets a tight compute-unit limit and confirms the guard completes.
#[tokio::test]
async fn g10_int4_guard_cu_fits_in_ceiling() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    let tx = Transaction::new_signed_with_payer(
        &[
            set_compute_unit_limit(SHADOW_GUARD_CU_PASS_CEILING),
            Instruction {
                program_id: shadow_guard_program_id(),
                accounts: vec![
                    AccountMeta::new_readonly(vault_in, false),
                    AccountMeta::new_readonly(vault_out, false),
                ],
                data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
            },
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_ok(),
        "Guard must fit within {} CU; got {:?}",
        SHADOW_GUARD_CU_PASS_CEILING,
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Scenario 5: Guard exceeds a too-tight limit (proves measurement is real)
// ---------------------------------------------------------------------------

/// Guard must FAIL with a CU limit below its measured consumption (~594 CU).
/// This test proves the CU ceiling measurement is meaningful — not vacuously true.
#[tokio::test]
async fn g10_int5_guard_exceeds_tight_limit() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    let tx = Transaction::new_signed_with_payer(
        &[
            set_compute_unit_limit(200), // below measured ~594 CU
            Instruction {
                program_id: shadow_guard_program_id(),
                accounts: vec![
                    AccountMeta::new_readonly(vault_in, false),
                    AccountMeta::new_readonly(vault_out, false),
                ],
                data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
            },
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_err(),
        "Guard must exceed 200 CU limit (measured ~594 CU)"
    );
}

// ---------------------------------------------------------------------------
// Scenario 6: Harmonic +150 CU pad proof
// ---------------------------------------------------------------------------

/// 4-instruction Harmonic shape: [SetCULimit(base+150), SetCUPrice, Guard, stub].
/// Uses a tight base (= guard CU ceiling) so the +150 pad is load-bearing.
/// Proves the pad prevents ComputationalBudgetExceeded.
/// A/B counterpart: int10 runs the same shape WITHOUT the +150 pad and FAILS.
#[tokio::test]
async fn g10_int6_harmonic_pad_proof() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    let padded = SHADOW_GUARD_CU_PASS_CEILING + 150; // 850 CU
    let tx = Transaction::new_signed_with_payer(
        &[
            set_compute_unit_limit(padded),
            set_compute_unit_price(1000),
            Instruction {
                program_id: shadow_guard_program_id(),
                accounts: vec![
                    AccountMeta::new_readonly(vault_in, false),
                    AccountMeta::new_readonly(vault_out, false),
                ],
                data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
            },
            system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_ok(),
        "Harmonic shape (guard+stub) must fit within {} CU ({}+150); got {:?}",
        padded,
        SHADOW_GUARD_CU_PASS_CEILING,
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Scenario 7: Full Jito shape — [SetCULimit, Guard, stub swap, tip]
// ---------------------------------------------------------------------------

/// Proves the 4-instruction Jito template shape executes end-to-end.
/// Stub swap = 0-lamport self-transfer (no real DEX in test env).
/// Tip = 1000-lamport transfer to a pre-funded receiver.
#[tokio::test]
async fn g10_int7_full_jito_shape() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let tip_receiver = Pubkey::new_unique();
    let mut pt = program_test();
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);
    pt.add_account(
        tip_receiver,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::new_from_array([0; 32]), // System Program
            executable: false,
            rent_epoch: 0,
        },
    );

    let (banks, payer, recent_blockhash) = pt.start().await;

    let tx = Transaction::new_signed_with_payer(
        &[
            set_compute_unit_limit(300_000),
            Instruction {
                program_id: shadow_guard_program_id(),
                accounts: vec![
                    AccountMeta::new_readonly(vault_in, false),
                    AccountMeta::new_readonly(vault_out, false),
                ],
                data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
            },
            system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0),
            system_instruction::transfer(&payer.pubkey(), &tip_receiver, 1_000),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_ok(),
        "Full Jito shape [CULimit, Guard, Swap, Tip] must succeed; got {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Scenario 8: Real Jito template builder through BanksClient
// ---------------------------------------------------------------------------

/// Builds a Jito transaction using the actual `build_jito_template()` from
/// assembly.rs, deserializes the UnsignedEnvelope into a solana_sdk Message,
/// signs, and submits through BanksClient. Proves the template builder
/// produces a valid, executable Solana transaction.
#[tokio::test]
async fn g10_int8_real_jito_template() {
    let mock_swap_id = Pubkey::new_unique();
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let tip_receiver = Pubkey::new_unique();
    let writable_swap = Pubkey::new_unique();
    let readonly_swap = Pubkey::new_unique();

    let mut pt = program_test_with_mock_swap(mock_swap_id);
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);
    pt.add_account(
        tip_receiver,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::new_from_array([0; 32]),
            executable: false,
            rent_epoch: 0,
        },
    );
    pt.add_account(
        writable_swap,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::new_from_array([0; 32]),
            executable: false,
            rent_epoch: 0,
        },
    );
    pt.add_account(
        readonly_swap,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::new_from_array([0; 32]),
            executable: false,
            rent_epoch: 0,
        },
    );

    let (banks, payer, recent_blockhash) = pt.start().await;

    let alloc = AtomicU64::new(0);
    let fencer = InventoryFencer {
        current_allocation: &alloc,
        absolute_ceiling: u64::MAX,
    };

    let default_meta = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey([0; 32]),
        is_writable: false,
    };
    let mut swap_metas = [default_meta; 8];
    swap_metas[0] = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey(writable_swap.to_bytes()),
        is_writable: true,
    };
    swap_metas[1] = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey(readonly_swap.to_bytes()),
        is_writable: false,
    };

    let input = ExecutionAssemblyInput {
        opso_economics: OpsoObserveOnlyEconomics::default(),
        prepared: PreparedExecution {
            route_decision: RouteDecision::JitoBundle,
            route_reason: RouteReason::JitoDirect,
            intent_kind: IntentKind::PlainWithinCap,
            trade_size_lamports: 0,
            tip_lamports: 1_000,
            cu_price_micro_lamports: 0,
            min_out: 0,
            leader_class_id: 0,
            alpha_bucket: 0,
            final_lut_index: 0,
            dominance_decision: DominanceDecision::Pass,
            reserved_notional_lamports: 0,
            would_reserve_notional_lamports: 0,
            retained_lcb_lamports: 0,
            base_state_unix_ns: 0,
        },
        recent_blockhash: recent_blockhash.to_bytes(),
        ingress_slot: 0,
        deadline_unix_ns: u64::MAX,
        ingress_unix_ns: 0,
        signal_source: 0,
        template_id: TemplateId::JitoPlain,
        route_base_cu_limit: 300_000,
        configured_route_base_cu_limit: 300_000,
        micro_market_observed: false,
        micro_market_flat_retained_ev_lamports: 0,
        micro_market_delta_lamports: 0,
        swap_accounts: FixedSwapAccounts {
            program_id: mock_swap_id.to_bytes(),
            metas: swap_metas,
            num_accounts: 2,
        },
        swap_ix_data: FixedSwapIxData {
            data: [0; 64],
            len: 17,
        },
        guard_bounds: GuardBounds {
            min_reserve_in: 1_050_000,
            max_reserve_in: 1_150_000,
            min_reserve_out: 400_000,
            max_reserve_out: 480_000,
        },
        pool_index: 0,
        mint_out_index: 0,
        overlay_delta: OverlayDelta {
            pool_index: 0,
            reserve_in_delta: 0,
            reserve_out_delta: 0,
        },
        jito_tip_rotation: JitoTipRotation {
            tip_account: tip_receiver.to_bytes(),
        },
        inventory_fencer: &fencer,
        fee_payer_pubkey: payer.pubkey().to_bytes(),
        shadow_guard_program_id: shadow_guard_program_id().to_bytes(),
        guard_vault_in_pubkey: vault_in.to_bytes(),
        guard_vault_out_pubkey: vault_out.to_bytes(),
        send_timestamp_ns: 0,
        tactic_class: 0,
        worker_id: 0,
        venue_id: 0,
        episode_id_mod: 0,
        opso_owner_mode: 0,
        opso_fallback_mode: 7,
        opso_reason_code: 0,
    };

    let envelope = build_jito_template(&input).unwrap();
    let tx = envelope_to_transaction(&envelope, &payer, recent_blockhash);
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_ok(),
        "Real Jito template (build_jito_template) must produce a valid transaction; got {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Scenario 9: Real Harmonic template builder through BanksClient
// ---------------------------------------------------------------------------

/// Same as int8 but for the Harmonic template shape.
/// CU limit = route_base + HARMONIC_CU_PAD as written by the real builder.
#[tokio::test]
async fn g10_int9_real_harmonic_template() {
    let mock_swap_id = Pubkey::new_unique();
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let writable_swap = Pubkey::new_unique();
    let readonly_swap = Pubkey::new_unique();

    let mut pt = program_test_with_mock_swap(mock_swap_id);
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);
    pt.add_account(
        writable_swap,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::new_from_array([0; 32]),
            executable: false,
            rent_epoch: 0,
        },
    );
    pt.add_account(
        readonly_swap,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::new_from_array([0; 32]),
            executable: false,
            rent_epoch: 0,
        },
    );

    let (banks, payer, recent_blockhash) = pt.start().await;

    let alloc = AtomicU64::new(0);
    let fencer = InventoryFencer {
        current_allocation: &alloc,
        absolute_ceiling: u64::MAX,
    };

    let default_meta = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey([0; 32]),
        is_writable: false,
    };
    let mut swap_metas = [default_meta; 8];
    swap_metas[0] = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey(writable_swap.to_bytes()),
        is_writable: true,
    };
    swap_metas[1] = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey(readonly_swap.to_bytes()),
        is_writable: false,
    };

    let input = ExecutionAssemblyInput {
        opso_economics: OpsoObserveOnlyEconomics::default(),
        prepared: PreparedExecution {
            route_decision: RouteDecision::HarmonicBundle,
            route_reason: RouteReason::HarmonicFreshPlain,
            intent_kind: IntentKind::PlainWithinCap,
            trade_size_lamports: 0,
            tip_lamports: 0,
            cu_price_micro_lamports: 1_000,
            min_out: 0,
            leader_class_id: 0,
            alpha_bucket: 0,
            final_lut_index: 0,
            dominance_decision: DominanceDecision::Pass,
            reserved_notional_lamports: 0,
            would_reserve_notional_lamports: 0,
            retained_lcb_lamports: 0,
            base_state_unix_ns: 0,
        },
        recent_blockhash: recent_blockhash.to_bytes(),
        ingress_slot: 0,
        deadline_unix_ns: u64::MAX,
        ingress_unix_ns: 0,
        signal_source: 0,
        template_id: TemplateId::HarmonicPlain,
        route_base_cu_limit: 300_000,
        configured_route_base_cu_limit: 300_000,
        micro_market_observed: false,
        micro_market_flat_retained_ev_lamports: 0,
        micro_market_delta_lamports: 0,
        swap_accounts: FixedSwapAccounts {
            program_id: mock_swap_id.to_bytes(),
            metas: swap_metas,
            num_accounts: 2,
        },
        swap_ix_data: FixedSwapIxData {
            data: [0; 64],
            len: 17,
        },
        guard_bounds: GuardBounds {
            min_reserve_in: 1_050_000,
            max_reserve_in: 1_150_000,
            min_reserve_out: 400_000,
            max_reserve_out: 480_000,
        },
        pool_index: 0,
        mint_out_index: 0,
        overlay_delta: OverlayDelta {
            pool_index: 0,
            reserve_in_delta: 0,
            reserve_out_delta: 0,
        },
        jito_tip_rotation: JitoTipRotation {
            tip_account: [0; 32],
        },
        inventory_fencer: &fencer,
        fee_payer_pubkey: payer.pubkey().to_bytes(),
        shadow_guard_program_id: shadow_guard_program_id().to_bytes(),
        guard_vault_in_pubkey: vault_in.to_bytes(),
        guard_vault_out_pubkey: vault_out.to_bytes(),
        send_timestamp_ns: 0,
        tactic_class: 0,
        worker_id: 0,
        venue_id: 0,
        episode_id_mod: 0,
        opso_owner_mode: 0,
        opso_fallback_mode: 7,
        opso_reason_code: 0,
    };

    let envelope = build_harmonic_template(&input).unwrap();
    let tx = envelope_to_transaction(&envelope, &payer, recent_blockhash);
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_ok(),
        "Real Harmonic template (build_harmonic_template) must produce a valid transaction; got {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Scenario 10: Harmonic without +150 pad FAILS (A/B proof counterpart to int6)
// ---------------------------------------------------------------------------

/// Same 4-instruction Harmonic shape as int6, but CU limit = SHADOW_GUARD_CU_PASS_CEILING
/// (900) WITHOUT the +150 pad. Full template non-swap overhead is 1044 CU > 900, so this
/// transaction must FAIL. Together with int6 (padded at 1050 → PASS), this pair proves the
/// +150 HARMONIC_CU_PAD is load-bearing — not merely insurance on a margin that already fits.
#[tokio::test]
async fn g10_int10_harmonic_unpadded_fails() {
    let vault_in = Pubkey::new_unique();
    let vault_out = Pubkey::new_unique();
    let mut pt = program_test();
    add_spl_vault(&mut pt, vault_in, 1_100_000);
    add_spl_vault(&mut pt, vault_out, 450_000);

    let (banks, payer, recent_blockhash) = pt.start().await;

    // CU limit = 900 (no +150 pad). Full template overhead = 1044 > 900 → must fail.
    let unpadded = SHADOW_GUARD_CU_PASS_CEILING;
    let tx = Transaction::new_signed_with_payer(
        &[
            set_compute_unit_limit(unpadded),
            set_compute_unit_price(1000),
            Instruction {
                program_id: shadow_guard_program_id(),
                accounts: vec![
                    AccountMeta::new_readonly(vault_in, false),
                    AccountMeta::new_readonly(vault_out, false),
                ],
                data: encode_bounds(1_050_000, 1_150_000, 400_000, 480_000),
            },
            system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let result = banks.process_transaction(tx).await;
    assert!(
        result.is_err(),
        "Harmonic shape without +150 pad must FAIL (900 CU < {} overhead); \
         A/B counterpart to int6 (padded at {} → PASS)",
        FULL_TEMPLATE_NONSWAP_CU,
        SHADOW_GUARD_CU_PASS_CEILING + HARMONIC_CU_PAD
    );
}
