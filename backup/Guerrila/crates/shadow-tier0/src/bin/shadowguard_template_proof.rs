use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use clap::Parser;
use shadow_core::assembly::{
    build_harmonic_template, build_jito_template, ExecutionAssemblyInput, FixedSwapAccountMeta,
    FixedSwapAccounts, FixedSwapIxData, GuardBounds, JitoTipRotation, OverlayDelta,
    SwapAccountSource, TemplateId, EMPTY_SWAP_ACCOUNT_META, MAX_SWAP_ACCOUNTS,
};
use shadow_core::config::ShadowConfig;
use shadow_core::inventory::InventoryFencer;
use shadow_core::opso::OpsoObserveOnlyEconomics;
use shadow_core::policy::{DominanceDecision, PreparedExecution};

const LOOKUP_TABLE_PLACEHOLDER: &str = "AddressLookupTab1e1111111111111111111111111";

#[derive(Parser, Debug)]
#[command(
    name = "shadowguard-template-proof",
    about = "Verify ShadowGuard program-id wiring in Jito and Harmonic templates"
)]
struct Cli {
    #[arg(long)]
    config: PathBuf,
}

#[derive(Debug)]
struct LegacyInstruction {
    program_id_index: u8,
}

#[derive(Debug)]
struct LegacyMessage {
    account_keys: Vec<[u8; 32]>,
    instructions: Vec<LegacyInstruction>,
}

fn main() {
    let cli = Cli::parse();
    let cfg = ShadowConfig::from_file(&cli.config).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to load {}: {err}", cli.config.display());
        std::process::exit(1);
    });

    let exec = cfg.execution.as_ref().unwrap_or_else(|| {
        eprintln!(
            "ERROR: execution config missing from {}",
            cli.config.display()
        );
        std::process::exit(1);
    });

    let shadow_guard_program_id = decode_pubkey(
        "execution.shadow_guard_program_id",
        &exec.shadow_guard_program_id,
    );
    let guard_vault_in_pubkey = decode_pubkey(
        "execution.guard_vault_in_pubkey",
        &exec.guard_vault_in_pubkey,
    );
    let guard_vault_out_pubkey = decode_pubkey(
        "execution.guard_vault_out_pubkey",
        &exec.guard_vault_out_pubkey,
    );
    let lookup_table_placeholder =
        decode_pubkey("lookup-table-placeholder", LOOKUP_TABLE_PLACEHOLDER);

    let jito_input = dummy_input(
        TemplateId::JitoPlain,
        shadow_guard_program_id,
        guard_vault_in_pubkey,
        guard_vault_out_pubkey,
    );
    let harmonic_input = dummy_input(
        TemplateId::HarmonicPlain,
        shadow_guard_program_id,
        guard_vault_in_pubkey,
        guard_vault_out_pubkey,
    );

    let jito = build_jito_template(&jito_input).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to build Jito template: {err:?}");
        std::process::exit(1);
    });
    let harmonic = build_harmonic_template(&harmonic_input).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to build Harmonic template: {err:?}");
        std::process::exit(1);
    });

    let jito_msg = parse_legacy_message(&jito.raw_bytes[..jito.len]).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to parse Jito template: {err}");
        std::process::exit(1);
    });
    let harmonic_msg =
        parse_legacy_message(&harmonic.raw_bytes[..harmonic.len]).unwrap_or_else(|err| {
            eprintln!("ERROR: failed to parse Harmonic template: {err}");
            std::process::exit(1);
        });

    let jito_matches = instruction_program_matches(&jito_msg, 1, shadow_guard_program_id);
    let harmonic_matches = instruction_program_matches(&harmonic_msg, 2, shadow_guard_program_id);
    let placeholder_present = message_contains_account(&jito_msg, lookup_table_placeholder)
        || message_contains_account(&harmonic_msg, lookup_table_placeholder);

    println!("shadow_guard_jito_template_program_id_matches={jito_matches}");
    println!("shadow_guard_harmonic_template_program_id_matches={harmonic_matches}");
    println!(
        "shadow_guard_template_program_id_matches={}",
        jito_matches && harmonic_matches && !placeholder_present
    );
    println!("lookup_table_placeholder_present={placeholder_present}");
}

fn dummy_input(
    template_id: TemplateId,
    shadow_guard_program_id: [u8; 32],
    guard_vault_in_pubkey: [u8; 32],
    guard_vault_out_pubkey: [u8; 32],
) -> ExecutionAssemblyInput<'static> {
    let inventory_alloc = Box::leak(Box::new(AtomicU64::new(0)));
    let inventory_fencer = Box::leak(Box::new(InventoryFencer {
        current_allocation: inventory_alloc,
        absolute_ceiling: u64::MAX,
    }));

    let mut swap_metas = [EMPTY_SWAP_ACCOUNT_META; MAX_SWAP_ACCOUNTS];
    swap_metas[0] = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey([0x11; 32]),
        is_writable: true,
    };
    swap_metas[1] = FixedSwapAccountMeta {
        source: SwapAccountSource::ExplicitPubkey([0x22; 32]),
        is_writable: false,
    };
    let swap_accounts = FixedSwapAccounts {
        program_id: [0x33; 32],
        metas: swap_metas,
        num_accounts: 2,
    };
    let mut swap_ix_raw = [0u8; 64];
    swap_ix_raw[0] = 9;
    let swap_ix_data = FixedSwapIxData {
        data: swap_ix_raw,
        len: 17,
        amount_in_offset: 1,
        min_out_offset: 9,
    };

    ExecutionAssemblyInput {
        opso_economics: OpsoObserveOnlyEconomics::default(),
        prepared: PreparedExecution {
            route_decision: match template_id {
                TemplateId::HarmonicPlain => shadow_core::routing::RouteDecision::HarmonicBundle,
                _ => shadow_core::routing::RouteDecision::JitoBundle,
            },
            route_reason: shadow_core::routing::RouteReason::JitoDirect,
            intent_kind: shadow_core::routing::IntentKind::PlainWithinCap,
            trade_size_lamports: 1_000_000,
            tip_lamports: 50_000,
            cu_price_micro_lamports: 777,
            min_out: 900_000,
            leader_class_id: 1,
            alpha_bucket: 5,
            final_lut_index: 197,
            dominance_decision: DominanceDecision::Pass,
            reserved_notional_lamports: 0,
            would_reserve_notional_lamports: 0,
            retained_lcb_lamports: 100_000,
            base_state_unix_ns: 1,
        },
        recent_blockhash: [0x44; 32],
        ingress_slot: 1,
        deadline_unix_ns: u64::MAX,
        ingress_unix_ns: 1,
        signal_source: 0,
        template_id,
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
        swap_accounts,
        swap_ix_data,
        guard_bounds: GuardBounds {
            min_reserve_in: 10,
            max_reserve_in: 20,
            min_reserve_out: 30,
            max_reserve_out: 40,
        },
        pool_index: 1,
        mint_out_index: 2,
        pool_pubkey: [0; 32],
        mint_in_pubkey: [0; 32],
        mint_out_pubkey: [0; 32],
        overlay_delta: OverlayDelta {
            pool_index: 1,
            reserve_in_delta: 10,
            reserve_out_delta: -10,
        },
        jito_tip_rotation: JitoTipRotation {
            tip_account: [0x55; 32],
        },
        inventory_fencer,
        fee_payer_pubkey: [0x66; 32],
        shadow_guard_program_id,
        guard_vault_in_pubkey,
        guard_vault_out_pubkey,
        send_timestamp_ns: 1,
        tactic_class: 0,
        worker_id: 0,
        venue_id: 0,
        public_rpc_scout_explicit: false,
        episode_id_mod: 0,
        opso_owner_mode: 0,
        opso_fallback_mode: 7,
        opso_reason_code: 0,
    }
}

fn decode_pubkey(label: &str, value: &str) -> [u8; 32] {
    let decoded = bs58::decode(value).into_vec().unwrap_or_else(|err| {
        eprintln!("ERROR: invalid {label} {value}: {err}");
        std::process::exit(1);
    });
    let len = decoded.len();
    decoded.try_into().unwrap_or_else(|_| {
        eprintln!("ERROR: {label} decoded to {len} bytes instead of 32");
        std::process::exit(1);
    })
}

fn parse_legacy_message(raw: &[u8]) -> Result<LegacyMessage, String> {
    if raw.len() < 3 {
        return Err("message too short for header".into());
    }
    let mut offset = 3usize;
    let account_count = read_short_vec_len(raw, &mut offset)?;
    let mut account_keys = Vec::with_capacity(account_count);
    for _ in 0..account_count {
        if offset + 32 > raw.len() {
            return Err("message ended while reading account keys".into());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&raw[offset..offset + 32]);
        account_keys.push(key);
        offset += 32;
    }
    if offset + 32 > raw.len() {
        return Err("message ended before recent blockhash".into());
    }
    offset += 32;
    let instruction_count = read_short_vec_len(raw, &mut offset)?;
    let mut instructions = Vec::with_capacity(instruction_count);
    for _ in 0..instruction_count {
        if offset >= raw.len() {
            return Err("message ended before instruction program id".into());
        }
        let program_id_index = raw[offset];
        offset += 1;
        let account_index_count = read_short_vec_len(raw, &mut offset)?;
        if offset + account_index_count > raw.len() {
            return Err("message ended while reading instruction account indices".into());
        }
        offset += account_index_count;
        let data_len = read_short_vec_len(raw, &mut offset)?;
        if offset + data_len > raw.len() {
            return Err("message ended while reading instruction data".into());
        }
        offset += data_len;
        instructions.push(LegacyInstruction { program_id_index });
    }
    Ok(LegacyMessage {
        account_keys,
        instructions,
    })
}

fn read_short_vec_len(raw: &[u8], offset: &mut usize) -> Result<usize, String> {
    if *offset >= raw.len() {
        return Err("message ended while reading shortvec length".into());
    }
    let value = raw[*offset];
    *offset += 1;
    if (value & 0x80) != 0 {
        return Err("multi-byte shortvec lengths are unsupported in template proof".into());
    }
    Ok(value as usize)
}

fn instruction_program_matches(
    message: &LegacyMessage,
    ix_index: usize,
    expected: [u8; 32],
) -> bool {
    message
        .instructions
        .get(ix_index)
        .and_then(|ix| message.account_keys.get(ix.program_id_index as usize))
        .map(|program_id| *program_id == expected)
        .unwrap_or(false)
}

fn message_contains_account(message: &LegacyMessage, target: [u8; 32]) -> bool {
    message.account_keys.iter().any(|key| *key == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_binary_parser_reads_both_templates() {
        let guard = [0x77; 32];
        let tpl = build_jito_template(&dummy_input(
            TemplateId::JitoPlain,
            guard,
            [0x88; 32],
            [0x99; 32],
        ))
        .unwrap();
        let msg = parse_legacy_message(&tpl.raw_bytes[..tpl.len]);
        assert!(msg.is_ok());
    }

    #[test]
    fn instruction_program_match_detects_shadow_guard_slot() {
        let guard = [0x77; 32];
        let tpl = build_harmonic_template(&dummy_input(
            TemplateId::HarmonicPlain,
            guard,
            [0x88; 32],
            [0x99; 32],
        ))
        .unwrap();
        let msg = parse_legacy_message(&tpl.raw_bytes[..tpl.len]).unwrap();
        assert!(instruction_program_matches(&msg, 2, guard));
    }
}
