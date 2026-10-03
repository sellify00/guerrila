use std::sync::OnceLock;

fn parse_pubkey(raw: &str) -> Option<[u8; 32]> {
    let decoded = bs58::decode(raw.trim()).into_vec().ok()?;
    if decoded.len() != 32 {
        return None;
    }
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&decoded);
    Some(pubkey)
}

fn parse_traced_pool_pubkey() -> Option<[u8; 32]> {
    parse_pubkey(&std::env::var("SHADOW_STATE_TRACE_POOL_PUBKEY").ok()?)
}

fn parse_traced_mint_pubkeys() -> Vec<[u8; 32]> {
    std::env::var("SHADOW_STATE_TRACE_MINT_PUBKEYS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(parse_pubkey)
                .collect::<Vec<[u8; 32]>>()
        })
        .unwrap_or_default()
}

fn parse_trace_quote_probe_enabled() -> bool {
    std::env::var("SHADOW_STATE_TRACE_CLMM_QUOTE_PROBE")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn traced_pool_pubkey() -> Option<[u8; 32]> {
    static TRACED_POOL: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    *TRACED_POOL.get_or_init(parse_traced_pool_pubkey)
}

pub fn traced_mint_pubkeys() -> &'static Vec<[u8; 32]> {
    static TRACED_MINTS: OnceLock<Vec<[u8; 32]>> = OnceLock::new();
    TRACED_MINTS.get_or_init(parse_traced_mint_pubkeys)
}

pub fn is_traced_pool(pubkey: &[u8; 32]) -> bool {
    traced_pool_pubkey().as_ref() == Some(pubkey)
}

pub fn is_traced_mint(pubkey: &[u8; 32]) -> bool {
    traced_mint_pubkeys()
        .iter()
        .any(|candidate| candidate == pubkey)
}

pub fn trace_quote_probe_enabled_for(
    pool_pubkey: &[u8; 32],
    mint_in_pubkey: &[u8; 32],
    mint_out_pubkey: &[u8; 32],
) -> bool {
    static TRACE_QUOTE_PROBE: OnceLock<bool> = OnceLock::new();
    *TRACE_QUOTE_PROBE.get_or_init(parse_trace_quote_probe_enabled)
        || is_traced_pool(pool_pubkey)
        || is_traced_mint(mint_in_pubkey)
        || is_traced_mint(mint_out_pubkey)
}

pub fn encode_pubkey(pubkey: &[u8; 32]) -> String {
    bs58::encode(pubkey).into_string()
}
