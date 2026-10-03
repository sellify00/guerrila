import re

with open("crates/shadow-core/src/assembly.rs", "r") as f:
    text = f.read()

# Replace swap ix logic
old_swap_jito = """    push_into_envelope(&mut envelope, &[input.swap_ix_data.len])?;
    if input.swap_ix_data.len > 0 {
        push_into_envelope(
            &mut envelope,
            &input.swap_ix_data.data[..input.swap_ix_data.len as usize],
        )?;
    }"""

new_swap_jito = """    push_into_envelope(&mut envelope, &[input.swap_ix_data.len])?;
    if input.swap_ix_data.len > 0 {
        let mut swapped_data = input.swap_ix_data.data;
        
        // Dynamically patch clamped amount_in (u64 at offset 1 for Raydium/Orca args)
        if swapped_data.len() >= 9 {
            swapped_data[1..9].copy_from_slice(&input.prepared.trade_size_lamports.to_le_bytes());
        }
        // Dynamically patch minimum_out (u64 at offset 9 for Raydium/Orca args)
        if swapped_data.len() >= 17 {
            swapped_data[9..17].copy_from_slice(&input.prepared.min_out.to_le_bytes());
        }
        
        push_into_envelope(
            &mut envelope,
            &swapped_data[..input.swap_ix_data.len as usize],
        )?;
    }"""
text = text.replace(old_swap_jito, new_swap_jito)

with open("crates/shadow-core/src/assembly.rs", "w") as f:
    f.write(text)
