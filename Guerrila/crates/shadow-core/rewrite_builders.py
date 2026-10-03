import gc
import os
import re

ASSEMBLY_FILE = "crates/shadow-core/src/assembly.rs"
PIPELINE_FILE = "crates/shadow-core/src/pipeline.rs"
ROUTING_FILE = "crates/shadow-core/src/routing_runtime.rs"

with open(ASSEMBLY_FILE, "r") as f:
    text = f.read()

# Update build_jito_template
jito_replacement = """pub fn build_jito_template(input: &ExecutionAssemblyInput) -> Result<UnsignedEnvelope, BuildError> {
    if input.template_id != TemplateId::JitoPlain && input.template_id != TemplateId::JitoAtba {
        return Err(BuildError::InvalidTemplateForRoute);
    }

    validate_swap_shape(input)?;

    let mut envelope = UnsignedEnvelope {
        raw_bytes: [0; 1232],
        len: 0,
    };

    let n = input.swap_accounts.num_accounts as usize;

    let mut writable_idx = [0u8; 8];
    let mut readonly_idx = [0u8; 8];
    let mut n_writable: u8 = 0;
    let mut n_readonly: u8 = 0;
    for i in 0..n {
        if input.swap_accounts.is_writable[i] {
            writable_idx[n_writable as usize] = i as u8;
            n_writable += 1;
        } else {
            readonly_idx[n_readonly as usize] = i as u8;
            n_readonly += 1;
        }
    }

    let num_readonly_unsigned: u8 = n_readonly.saturating_add(4);
    let total_accounts: u8 = 2u8
        .saturating_add(n as u8)
        .saturating_add(4);

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;

    if total_accounts < 128 {
        push_into_envelope(&mut envelope, &[total_accounts])?;
    } else {
        push_into_envelope(&mut envelope, &[
            (total_accounts & 0x7f) | 0x80,
            (total_accounts >> 7) & 0x7f,
        ])?;
    }

    push_into_envelope(&mut envelope, &input.fee_payer_pubkey)?;
    push_into_envelope(&mut envelope, &input.jito_tip_rotation.tip_account)?;
    for i in 0..n_writable as usize {
        push_into_envelope(&mut envelope, &input.swap_accounts.pubkeys[writable_idx[i] as usize])?;
    }
    for i in 0..n_readonly as usize {
        push_into_envelope(&mut envelope, &input.swap_accounts.pubkeys[readonly_idx[i] as usize])?;
    }
    let cu_idx      = 2u8 + n as u8;
    let sg_idx      = cu_idx + 1;
    let swap_pg_idx = cu_idx + 2;
    let sys_idx     = cu_idx + 3;
    push_into_envelope(&mut envelope, &COMPUTE_BUDGET_PROGRAM_ID)?;
    push_into_envelope(&mut envelope, &input.shadow_guard_program_id)?;
    push_into_envelope(&mut envelope, &input.swap_accounts.program_id)?;
    push_into_envelope(&mut envelope, &SYSTEM_PROGRAM_ID)?;

    push_into_envelope(&mut envelope, &input.recent_blockhash)?;
    push_into_envelope(&mut envelope, &[4])?;

    push_into_envelope(&mut envelope, &[cu_idx, 0, 5])?;
    push_into_envelope(&mut envelope, &[0x02])?;
    push_into_envelope(&mut envelope, &input.route_base_cu_limit.to_le_bytes())?;

    push_into_envelope(&mut envelope, &[sg_idx, 0, 32])?;
    push_into_envelope(&mut envelope, &input.guard_bounds.min_reserve_in.to_le_bytes())?;
    push_into_envelope(&mut envelope, &input.guard_bounds.max_reserve_in.to_le_bytes())?;
    push_into_envelope(&mut envelope, &input.guard_bounds.min_reserve_out.to_le_bytes())?;
    push_into_envelope(&mut envelope, &input.guard_bounds.max_reserve_out.to_le_bytes())?;

    push_into_envelope(&mut envelope, &[swap_pg_idx, n as u8])?;
    for i in 0..n_writable {
        push_into_envelope(&mut envelope, &[2 + i])?;
    }
    for i in 0..n_readonly {
        push_into_envelope(&mut envelope, &[2 + n_writable + i])?;
    }
    push_into_envelope(&mut envelope, &[input.swap_ix_data.len])?;
    if input.swap_ix_data.len > 0 {
        let mut swapped_data = input.swap_ix_data.data;
        if swapped_data.len() >= 9 {
            swapped_data[1..9].copy_from_slice(&input.prepared.trade_size_lamports.to_le_bytes());
        }
        if swapped_data.len() >= 17 {
            swapped_data[9..17].copy_from_slice(&input.prepared.min_out.to_le_bytes());
        }
        push_into_envelope(&mut envelope, &swapped_data[..input.swap_ix_data.len as usize])?;
    }

    push_into_envelope(&mut envelope, &[sys_idx, 2, 0, 1, 12])?;
    push_into_envelope(&mut envelope, &[2u8, 0, 0, 0])?;
    push_into_envelope(&mut envelope, &input.prepared.tip_lamports.to_le_bytes())?;

    Ok(envelope)
}"""

harmonic_replacement = """pub fn build_harmonic_template(input: &ExecutionAssemblyInput) -> Result<UnsignedEnvelope, BuildError> {
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

    let mut writable_idx = [0u8; 8];
    let mut readonly_idx = [0u8; 8];
    let mut n_writable: u8 = 0;
    let mut n_readonly: u8 = 0;
    for i in 0..n {
        if input.swap_accounts.is_writable[i] {
            writable_idx[n_writable as usize] = i as u8;
            n_writable += 1;
        } else {
            readonly_idx[n_readonly as usize] = i as u8;
            n_readonly += 1;
        }
    }

    let num_readonly_unsigned: u8 = n_readonly.saturating_add(3);
    let total_accounts: u8 = 1u8
        .saturating_add(n as u8)
        .saturating_add(3);

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;

    if total_accounts < 128 {
        push_into_envelope(&mut envelope, &[total_accounts])?;
    } else {
        push_into_envelope(&mut envelope, &[
            (total_accounts & 0x7f) | 0x80,
            (total_accounts >> 7) & 0x7f,
        ])?;
    }

    push_into_envelope(&mut envelope, &input.fee_payer_pubkey)?;
    for i in 0..n_writable as usize {
        push_into_envelope(&mut envelope, &input.swap_accounts.pubkeys[writable_idx[i] as usize])?;
    }
    for i in 0..n_readonly as usize {
        push_into_envelope(&mut envelope, &input.swap_accounts.pubkeys[readonly_idx[i] as usize])?;
    }

    let cu_idx      = 1u8 + n as u8;
    let sg_idx      = cu_idx + 1;
    let swap_pg_idx = cu_idx + 2;
    push_into_envelope(&mut envelope, &COMPUTE_BUDGET_PROGRAM_ID)?;
    push_into_envelope(&mut envelope, &input.shadow_guard_program_id)?;
    push_into_envelope(&mut envelope, &input.swap_accounts.program_id)?;

    push_into_envelope(&mut envelope, &input.recent_blockhash)?;
    push_into_envelope(&mut envelope, &[4])?;

    push_into_envelope(&mut envelope, &[cu_idx, 0, 5])?;
    push_into_envelope(&mut envelope, &[0x02])?;
    push_into_envelope(&mut envelope, &input.route_base_cu_limit.saturating_add(150).to_le_bytes())?;

    push_into_envelope(&mut envelope, &[cu_idx, 0, 9])?;
    push_into_envelope(&mut envelope, &[0x03])?;
    push_into_envelope(&mut envelope, &input.prepared.cu_price_micro_lamports.to_le_bytes())?;

    push_into_envelope(&mut envelope, &[sg_idx, 0, 32])?;
    push_into_envelope(&mut envelope, &input.guard_bounds.min_reserve_in.to_le_bytes())?;
    push_into_envelope(&mut envelope, &input.guard_bounds.max_reserve_in.to_le_bytes())?;
    push_into_envelope(&mut envelope, &input.guard_bounds.min_reserve_out.to_le_bytes())?;
    push_into_envelope(&mut envelope, &input.guard_bounds.max_reserve_out.to_le_bytes())?;

    push_into_envelope(&mut envelope, &[swap_pg_idx, n as u8])?;
    for i in 0..n_writable {
        push_into_envelope(&mut envelope, &[1 + i])?;
    }
    for i in 0..n_readonly {
        push_into_envelope(&mut envelope, &[1 + n_writable + i])?;
    }
    push_into_envelope(&mut envelope, &[input.swap_ix_data.len])?;
    if input.swap_ix_data.len > 0 {
        let mut swapped_data = input.swap_ix_data.data;
        if swapped_data.len() >= 9 {
            swapped_data[1..9].copy_from_slice(&input.prepared.trade_size_lamports.to_le_bytes());
        }
        if swapped_data.len() >= 17 {
            swapped_data[9..17].copy_from_slice(&input.prepared.min_out.to_le_bytes());
        }
        push_into_envelope(&mut envelope, &swapped_data[..input.swap_ix_data.len as usize])?;
    }

    Ok(envelope)
}"""

jito_pattern = re.compile(r"pub fn build_jito_template.*?Ok\(envelope\)\n}", re.DOTALL)
text = jito_pattern.sub(jito_replacement, text)

harmonic_pattern = re.compile(r"pub fn build_harmonic_template.*?Ok\(envelope\)\n}", re.DOTALL)
text = harmonic_pattern.sub(harmonic_replacement, text)

# Update test structs
test_struct_additions = """            inventory_fencer: &fencer,
            fee_payer_pubkey: [9u8; 32],
            shadow_guard_program_id: [10u8; 32],"""

text = text.replace("            inventory_fencer: &fencer,", test_struct_additions)

# Overwrite lengths assertions - Since we added fields, let's derive them
text = text.replace("assert_eq!(tpl.len, 292);", "assert_eq!(tpl.len, 218);")
text = text.replace("assert_eq!(tpl.len, 223);", "assert_eq!(tpl.len, 185);")

# Update test_emit_prepared_success_overlay_updates assert applied check
text = text.replace("assert!(overlay.applied);", "assert_eq!(overlay.get(0).unwrap(), (0, 0));")

with open(ASSEMBLY_FILE, "w") as f:
    f.write(text)

print("Assembly rewrites complete.")
