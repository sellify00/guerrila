import re

with open("crates/shadow-core/src/assembly.rs", "r") as f:
    text = f.read()

# Replace build_jito_template header handling
old_jito_header = """    let total_accounts: u8 = 6u8.saturating_add(input.swap_accounts.num_accounts);
    let mut num_readonly_unsigned: u8 = 4; // compute-budget, ShadowGuard, swap program, system program
    for i in 0..input.swap_accounts.num_accounts as usize {
        if !input.swap_accounts.is_writable[i] {
            num_readonly_unsigned = num_readonly_unsigned.saturating_add(1);
        }
    }

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;
    push_into_envelope(&mut envelope, &[total_accounts])?;"""

new_jito_header = """    let total_accounts: u8 = 6u8.saturating_add(input.swap_accounts.num_accounts);
    
    // Solana Header
    // num_required_signatures = 1 (signer)
    // num_readonly_signed_accounts = 0
    let mut num_readonly_unsigned: u8 = 4; // compute-budget, ShadowGuard, swap program, system program
    for i in 0..input.swap_accounts.num_accounts as usize {
        if !input.swap_accounts.is_writable[i] {
            num_readonly_unsigned = num_readonly_unsigned.saturating_add(1);
        }
    }

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;
    
    // Compact-u16 len
    if total_accounts < 128 {
        push_into_envelope(&mut envelope, &[total_accounts])?;
    } else {
        push_into_envelope(&mut envelope, &[
            (total_accounts & 0x7f) | 0x80,
            (total_accounts >> 7) & 0x7f,
        ])?;
    }"""
text = text.replace(old_jito_header, new_jito_header)

# Replace build_harmonic_template header handling
old_harmonic_header = """    let total_accounts: u8 = 4u8.saturating_add(input.swap_accounts.num_accounts);
    let mut num_readonly_unsigned: u8 = 3; // compute-budget, ShadowGuard, swap program
    for i in 0..input.swap_accounts.num_accounts as usize {
        if !input.swap_accounts.is_writable[i] {
            num_readonly_unsigned = num_readonly_unsigned.saturating_add(1);
        }
    }

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;
    push_into_envelope(&mut envelope, &[total_accounts])?;"""

new_harmonic_header = """    let total_accounts: u8 = 4u8.saturating_add(input.swap_accounts.num_accounts);
    let mut num_readonly_unsigned: u8 = 3; // compute-budget, ShadowGuard, swap program
    for i in 0..input.swap_accounts.num_accounts as usize {
        if !input.swap_accounts.is_writable[i] {
            num_readonly_unsigned = num_readonly_unsigned.saturating_add(1);
        }
    }

    push_into_envelope(&mut envelope, &[1, 0, num_readonly_unsigned])?;
    if total_accounts < 128 {
        push_into_envelope(&mut envelope, &[total_accounts])?;
    } else {
        push_into_envelope(&mut envelope, &[
            (total_accounts & 0x7f) | 0x80,
            (total_accounts >> 7) & 0x7f,
        ])?;
    }"""
text = text.replace(old_harmonic_header, new_harmonic_header)

# Replace Instruction Compact-u16 handling in Jito
old_jito_ix = """    // Instructions: CU limit, ShadowGuard, swap, Jito tip transfer.
    push_into_envelope(&mut envelope, &[4])?;"""

new_jito_ix = """    // Compact-u16 Instruction count
    // Instructions: CU limit, ShadowGuard, swap, Jito tip transfer.
    push_into_envelope(&mut envelope, &[4])?;"""
text = text.replace(old_jito_ix, new_jito_ix)


# Rewrite sign_from_nonce_ring to format actual Solana Message wrapping
old_sign = """    signed.raw_bytes[0] = 1;

    use curve25519_dalek::scalar::Scalar;
    use sha2::{Digest, Sha512};

    let mut hasher = Sha512::new();"""

new_sign = """    // Format Solana array Header (Compact-u16 signatures len = 1)
    signed.raw_bytes[0] = 1;

    use curve25519_dalek::scalar::Scalar;
    use sha2::{Digest, Sha512};

    // Hash the true Message bytes
    let mut hasher = Sha512::new();"""
text = text.replace(old_sign, new_sign)

old_sign_assembly = """    signed.raw_bytes[1..65].copy_from_slice(&sig_bytes);
    signed.raw_bytes[65..65 + unsigned.len]
        .copy_from_slice(&unsigned.raw_bytes[..unsigned.len]);
    signed.len = 65 + unsigned.len;
    signed.signature = sig_bytes;"""

new_sign_assembly = """    // Array Layout: [1 byte for signature count] || [64 byte sig] || [Message layout...]
    signed.raw_bytes[1..65].copy_from_slice(&sig_bytes);
    signed.raw_bytes[65..65 + unsigned.len]
        .copy_from_slice(&unsigned.raw_bytes[..unsigned.len]);
    signed.len = 65 + unsigned.len;
    signed.signature = sig_bytes;"""
text = text.replace(old_sign_assembly, new_sign_assembly)


with open("crates/shadow-core/src/assembly.rs", "w") as f:
    f.write(text)
