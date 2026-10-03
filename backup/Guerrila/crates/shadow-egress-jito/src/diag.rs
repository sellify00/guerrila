use serde_json::Value;
use shadow_ipc::blockhash::BlockhashSnapshot;
use shadow_ipc::egress_ring::SignedTxEnvelope;
use shadow_ipc::segment::MappedSegment;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

const COMPUTE_BUDGET_PROGRAM_ID: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];
const CB_SET_COMPUTE_UNIT_LIMIT_DISC: u8 = 0x02;
const CB_SET_COMPUTE_UNIT_PRICE_DISC: u8 = 0x03;
const MAX_REJECTION_TEXT_CHARS: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JitoProxyDiagnosticsSnapshot {
    pub public_rpc_transport_attempt: u64,
    pub public_rpc_transport_failure: u64,
    pub public_rpc_transport_rejected: u64,
    pub public_rpc_nack_recorded: u64,
    pub first_public_rpc_attempt_signature: Option<[u8; 64]>,
    pub last_public_rpc_rejection_code: i64,
    pub last_public_rpc_rejection_message: String,
    pub last_public_rpc_rejection_body: String,
    pub last_public_rpc_rejection_data_err: String,
    pub last_public_rpc_rejection_simulation_logs: String,
    pub last_public_rpc_rejection_signature: Option<[u8; 64]>,
    pub last_public_rpc_rejection_tx_size: u64,
    pub last_public_rpc_rejection_blockhash: Option<[u8; 32]>,
    pub last_public_rpc_rejection_blockhash_matches_current: bool,
    pub last_public_rpc_rejection_blockhash_age_ns: u64,
    pub last_public_rpc_rejection_cu_limit: u64,
    pub last_public_rpc_rejection_cu_price_micro_lamports: u64,
    pub last_public_rpc_rejection_reason: String,
    pub last_public_rpc_rejection_ix1_program_id: Option<[u8; 32]>,
    pub last_public_rpc_rejection_ix1_account_0: Option<[u8; 32]>,
    pub last_public_rpc_rejection_ix1_account_1: Option<[u8; 32]>,
    pub last_public_rpc_rejection_ix1_account_count: u64,
    pub last_public_rpc_rejection_ix2_program_id: Option<[u8; 32]>,
    pub last_public_rpc_rejection_ix2_amount_in: u64,
    pub last_public_rpc_rejection_ix2_min_out: u64,
    pub last_public_rpc_rejection_ix2_account_count: u64,
    pub last_public_rpc_rejection_actual_output_lamports: u64,
    pub last_public_rpc_rejection_required_output_lamports: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ParsedTransactionDiagnostics {
    blockhash: Option<[u8; 32]>,
    cu_limit: u64,
    cu_price_micro_lamports: u64,
    ix1_program_id: Option<[u8; 32]>,
    ix1_account_0: Option<[u8; 32]>,
    ix1_account_1: Option<[u8; 32]>,
    ix1_account_count: u64,
    ix2_program_id: Option<[u8; 32]>,
    ix2_amount_in: u64,
    ix2_min_out: u64,
    ix2_account_count: u64,
}

static PUBLIC_RPC_TRANSPORT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static PUBLIC_RPC_TRANSPORT_FAILURE: AtomicU64 = AtomicU64::new(0);
static PUBLIC_RPC_TRANSPORT_REJECTED: AtomicU64 = AtomicU64::new(0);
static PUBLIC_RPC_NACK_RECORDED: AtomicU64 = AtomicU64::new(0);
static PROXY_DIAGNOSTICS_TEST_LOCK: Mutex<()> = Mutex::new(());
static LAST_PUBLIC_RPC_REJECTION: LazyLock<Mutex<JitoProxyDiagnosticsSnapshot>> =
    LazyLock::new(|| Mutex::new(JitoProxyDiagnosticsSnapshot::default()));
static FIRST_PUBLIC_RPC_ATTEMPT_SIGNATURE: LazyLock<Mutex<Option<[u8; 64]>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn snapshot_proxy_diagnostics() -> JitoProxyDiagnosticsSnapshot {
    let mut snapshot = LAST_PUBLIC_RPC_REJECTION
        .lock()
        .expect("proxy rejection diagnostics lock poisoned")
        .clone();
    snapshot.public_rpc_transport_attempt = PUBLIC_RPC_TRANSPORT_ATTEMPT.load(Ordering::Relaxed);
    snapshot.public_rpc_transport_failure = PUBLIC_RPC_TRANSPORT_FAILURE.load(Ordering::Relaxed);
    snapshot.public_rpc_transport_rejected = PUBLIC_RPC_TRANSPORT_REJECTED.load(Ordering::Relaxed);
    snapshot.public_rpc_nack_recorded = PUBLIC_RPC_NACK_RECORDED.load(Ordering::Relaxed);
    snapshot.first_public_rpc_attempt_signature = *FIRST_PUBLIC_RPC_ATTEMPT_SIGNATURE
        .lock()
        .expect("proxy first-attempt diagnostics lock poisoned");
    snapshot
}

pub(crate) fn record_public_rpc_transport_attempt(envelope: &SignedTxEnvelope) {
    if PUBLIC_RPC_TRANSPORT_ATTEMPT.fetch_add(1, Ordering::Relaxed) == 0 {
        *FIRST_PUBLIC_RPC_ATTEMPT_SIGNATURE
            .lock()
            .expect("proxy first-attempt diagnostics lock poisoned") = Some(envelope.signature);
    }
}

pub(crate) fn record_public_rpc_transport_failure() {
    PUBLIC_RPC_TRANSPORT_FAILURE.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_public_rpc_transport_rejected(
    envelope: &SignedTxEnvelope,
    rejection_body: &str,
) {
    PUBLIC_RPC_TRANSPORT_REJECTED.fetch_add(1, Ordering::Relaxed);
    let tx_meta = parse_transaction_diagnostics(&envelope.raw_bytes[..envelope.len]);
    let (code, message, data_err, simulation_logs, raw_body) = parse_rejection_body(rejection_body);
    let (actual_output_lamports, required_output_lamports) =
        parse_too_little_output_received_logs(&simulation_logs);
    let (blockhash_matches_current, blockhash_age_ns) =
        current_blockhash_diagnostics(tx_meta.blockhash);
    let classification = classify_rejection(&message, &data_err, &simulation_logs, &raw_body, code);
    let mut snapshot = LAST_PUBLIC_RPC_REJECTION
        .lock()
        .expect("proxy rejection diagnostics lock poisoned");
    snapshot.last_public_rpc_rejection_code = code;
    snapshot.last_public_rpc_rejection_message = message;
    snapshot.last_public_rpc_rejection_body = raw_body;
    snapshot.last_public_rpc_rejection_data_err = data_err;
    snapshot.last_public_rpc_rejection_simulation_logs = simulation_logs;
    snapshot.last_public_rpc_rejection_signature = Some(envelope.signature);
    snapshot.last_public_rpc_rejection_tx_size = envelope.len as u64;
    snapshot.last_public_rpc_rejection_blockhash = tx_meta.blockhash;
    snapshot.last_public_rpc_rejection_blockhash_matches_current = blockhash_matches_current;
    snapshot.last_public_rpc_rejection_blockhash_age_ns = blockhash_age_ns;
    snapshot.last_public_rpc_rejection_cu_limit = tx_meta.cu_limit;
    snapshot.last_public_rpc_rejection_cu_price_micro_lamports = tx_meta.cu_price_micro_lamports;
    snapshot.last_public_rpc_rejection_reason = classification;
    snapshot.last_public_rpc_rejection_ix1_program_id = tx_meta.ix1_program_id;
    snapshot.last_public_rpc_rejection_ix1_account_0 = tx_meta.ix1_account_0;
    snapshot.last_public_rpc_rejection_ix1_account_1 = tx_meta.ix1_account_1;
    snapshot.last_public_rpc_rejection_ix1_account_count = tx_meta.ix1_account_count;
    snapshot.last_public_rpc_rejection_ix2_program_id = tx_meta.ix2_program_id;
    snapshot.last_public_rpc_rejection_ix2_amount_in = tx_meta.ix2_amount_in;
    snapshot.last_public_rpc_rejection_ix2_min_out = tx_meta.ix2_min_out;
    snapshot.last_public_rpc_rejection_ix2_account_count = tx_meta.ix2_account_count;
    snapshot.last_public_rpc_rejection_actual_output_lamports = actual_output_lamports;
    snapshot.last_public_rpc_rejection_required_output_lamports = required_output_lamports;
}

pub(crate) fn record_public_rpc_nack_recorded() {
    PUBLIC_RPC_NACK_RECORDED.fetch_add(1, Ordering::Relaxed);
}

fn parse_rejection_body(rejection_body: &str) -> (i64, String, String, String, String) {
    let raw_body = truncate_text(rejection_body);
    let Ok(value) = serde_json::from_str::<Value>(rejection_body) else {
        return (
            0,
            truncate_text(rejection_body),
            String::new(),
            String::new(),
            raw_body,
        );
    };
    let code = value
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .map(truncate_text)
        .unwrap_or_else(|| raw_body.clone());
    let data_err = value
        .get("data")
        .and_then(|data| data.get("err"))
        .map(json_value_to_text)
        .unwrap_or_default();
    let simulation_logs = value
        .get("data")
        .and_then(|data| data.get("logs"))
        .and_then(Value::as_array)
        .map(|logs| {
            truncate_text(
                &logs
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })
        .unwrap_or_default();
    (code, message, data_err, simulation_logs, raw_body)
}

fn json_value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => truncate_text(text),
        _ => truncate_text(&value.to_string()),
    }
}

fn truncate_text(text: &str) -> String {
    text.chars().take(MAX_REJECTION_TEXT_CHARS).collect()
}

fn parse_too_little_output_received_logs(simulation_logs: &str) -> (u64, u64) {
    let mut left = None;
    let mut right = None;
    for line in simulation_logs.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed
            .strip_prefix("Program log: Left:")
            .or_else(|| trimmed.strip_prefix("Left:"))
        {
            left = value.trim().parse::<u64>().ok();
        } else if let Some(value) = trimmed
            .strip_prefix("Program log: Right:")
            .or_else(|| trimmed.strip_prefix("Right:"))
        {
            right = value.trim().parse::<u64>().ok();
        }
    }
    match (left, right) {
        (Some(actual), Some(required)) => (actual, required),
        _ => (0, 0),
    }
}

fn classify_rejection(
    message: &str,
    data_err: &str,
    simulation_logs: &str,
    raw_body: &str,
    code: i64,
) -> String {
    let haystack = format!(
        "{}\n{}\n{}\n{}",
        message.to_ascii_lowercase(),
        data_err.to_ascii_lowercase(),
        simulation_logs.to_ascii_lowercase(),
        raw_body.to_ascii_lowercase()
    );
    if haystack.contains("blockhash not found")
        || haystack.contains("block height exceeded")
        || haystack.contains("stale blockhash")
    {
        "blockhash_stale".into()
    } else if haystack.contains("insufficient funds")
        || haystack.contains("insufficient lamports")
        || haystack.contains("insufficient balance")
    {
        "insufficient_funds".into()
    } else if haystack.contains("address lookup table")
        || haystack.contains("lookup table")
        || haystack.contains("account not found")
        || haystack.contains("account missing")
    {
        "account_or_alt_missing".into()
    } else if haystack.contains("shadowguard")
        || haystack.contains("shadow guard")
        || haystack.contains("ndqkwygz7liuaqk8fxvrxa9ucce")
    {
        "shadowguard_or_template".into()
    } else if haystack.contains("min out")
        || haystack.contains("slippage")
        || haystack.contains("too little received")
        || haystack.contains("too little output received")
        || haystack.contains("toolittleoutputreceived")
        || haystack.contains("exceeds desired slippage limit")
    {
        "slippage_min_out".into()
    } else if haystack.contains("transaction too large")
        || haystack.contains("too large")
        || haystack.contains("exceeds max packet size")
        || haystack.contains("1232")
    {
        "tx_too_large".into()
    } else if haystack.contains("signature verification")
        || haystack.contains("missing required signature")
        || haystack.contains("fee payer")
    {
        "signature_or_fee_payer".into()
    } else if haystack.contains("compute unit")
        || haystack.contains("computational budget exceeded")
        || haystack.contains("priority fee")
        || haystack.contains("compute budget")
    {
        "priority_fee_or_cu".into()
    } else if haystack.contains("simulation failed")
        || haystack.contains("preflight")
        || code == -32002
    {
        "rpc_preflight_simulation".into()
    } else {
        "rejected_unknown".into()
    }
}

fn parse_transaction_diagnostics(raw_tx: &[u8]) -> ParsedTransactionDiagnostics {
    let (sig_count, mut p) = match read_compact_u16_wire(raw_tx, 0) {
        Some(v) if v.0 > 0 && v.0 <= 8 => v,
        _ => return ParsedTransactionDiagnostics::default(),
    };
    let sig_bytes = match sig_count.checked_mul(64) {
        Some(v) => v,
        None => return ParsedTransactionDiagnostics::default(),
    };
    if p.checked_add(sig_bytes)
        .map_or(true, |end| end > raw_tx.len())
    {
        return ParsedTransactionDiagnostics::default();
    }
    p += sig_bytes;

    let versioned = raw_tx.get(p).is_some_and(|byte| byte & 0x80 != 0);
    if versioned {
        if raw_tx.get(p).copied() != Some(0x80) {
            return ParsedTransactionDiagnostics::default();
        }
        p += 1;
    }

    if p.checked_add(3).map_or(true, |end| end > raw_tx.len()) {
        return ParsedTransactionDiagnostics::default();
    }
    p += 3;

    let (key_count, next) = match read_compact_u16_wire(raw_tx, p) {
        Some(v) if v.0 > 0 && v.0 <= 64 => v,
        _ => return ParsedTransactionDiagnostics::default(),
    };
    p = next;
    let keys_start = p;
    let key_bytes = match key_count.checked_mul(32) {
        Some(v) => v,
        None => return ParsedTransactionDiagnostics::default(),
    };
    if p.checked_add(key_bytes)
        .map_or(true, |end| end > raw_tx.len())
    {
        return ParsedTransactionDiagnostics::default();
    }
    p += key_bytes;

    let blockhash = raw_tx.get(p..p + 32).map(|slice| {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(slice);
        bytes
    });
    p += 32;

    let (instruction_count, next) = match read_compact_u16_wire(raw_tx, p) {
        Some(v) if v.0 <= 64 => v,
        _ => {
            return ParsedTransactionDiagnostics {
                blockhash,
                ..ParsedTransactionDiagnostics::default()
            };
        }
    };
    p = next;

    let mut cu_limit = 0u64;
    let mut cu_price_micro_lamports = 0u64;
    let mut ix1_program_id = None;
    let mut ix1_account_0 = None;
    let mut ix1_account_1 = None;
    let mut ix1_account_count = 0u64;
    let mut ix2_program_id = None;
    let mut ix2_amount_in = 0u64;
    let mut ix2_min_out = 0u64;
    let mut ix2_account_count = 0u64;
    for instruction_idx in 0..instruction_count {
        let Some(program_id_index) = raw_tx.get(p).copied() else {
            break;
        };
        p += 1;
        let Some((account_count, next)) = read_compact_u16_wire(raw_tx, p) else {
            break;
        };
        p = next;
        if p.checked_add(account_count)
            .map_or(true, |end| end > raw_tx.len())
        {
            break;
        }
        let account_indices = &raw_tx[p..p + account_count];
        p += account_count;
        let Some((data_len, next)) = read_compact_u16_wire(raw_tx, p) else {
            break;
        };
        p = next;
        let Some(data) = raw_tx.get(p..p + data_len) else {
            break;
        };
        p += data_len;

        let Some(program_id) =
            read_message_account_pubkey(raw_tx, keys_start, key_count, program_id_index)
        else {
            continue;
        };
        if instruction_idx == 1 {
            ix1_program_id = Some(program_id);
            ix1_account_count = account_count as u64;
            ix1_account_0 = account_indices
                .first()
                .and_then(|idx| read_message_account_pubkey(raw_tx, keys_start, key_count, *idx));
            ix1_account_1 = account_indices
                .get(1)
                .and_then(|idx| read_message_account_pubkey(raw_tx, keys_start, key_count, *idx));
        } else if instruction_idx == 2 {
            ix2_program_id = Some(program_id);
            ix2_account_count = account_count as u64;
            if data.len() >= 24 {
                let mut amount_in_bytes = [0u8; 8];
                amount_in_bytes.copy_from_slice(&data[8..16]);
                ix2_amount_in = u64::from_le_bytes(amount_in_bytes);
                let mut min_out_bytes = [0u8; 8];
                min_out_bytes.copy_from_slice(&data[16..24]);
                ix2_min_out = u64::from_le_bytes(min_out_bytes);
            }
        }
        if program_id != COMPUTE_BUDGET_PROGRAM_ID {
            continue;
        }
        match data.first().copied() {
            Some(CB_SET_COMPUTE_UNIT_LIMIT_DISC) if data.len() >= 5 => {
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(&data[1..5]);
                cu_limit = u32::from_le_bytes(bytes) as u64;
            }
            Some(CB_SET_COMPUTE_UNIT_PRICE_DISC) if data.len() >= 9 => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[1..9]);
                cu_price_micro_lamports = u64::from_le_bytes(bytes);
            }
            _ => {}
        }
    }

    ParsedTransactionDiagnostics {
        blockhash,
        cu_limit,
        cu_price_micro_lamports,
        ix1_program_id,
        ix1_account_0,
        ix1_account_1,
        ix1_account_count,
        ix2_program_id,
        ix2_amount_in,
        ix2_min_out,
        ix2_account_count,
    }
}

fn read_message_account_pubkey(
    raw_tx: &[u8],
    keys_start: usize,
    key_count: usize,
    key_index: u8,
) -> Option<[u8; 32]> {
    let key_index = usize::from(key_index);
    if key_index >= key_count {
        return None;
    }
    let key_offset = keys_start + key_index * 32;
    let slice = raw_tx.get(key_offset..key_offset + 32)?;
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(slice);
    Some(pubkey)
}

fn read_compact_u16_wire(buf: &[u8], pos: usize) -> Option<(usize, usize)> {
    let mut value = 0usize;
    let mut shift = 0usize;
    for i in 0..3 {
        let byte = *buf.get(pos + i)?;
        value |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some((value, pos + i + 1));
        }
        shift += 7;
    }
    None
}

fn current_blockhash_diagnostics(tx_blockhash: Option<[u8; 32]>) -> (bool, u64) {
    let Some(tx_blockhash) = tx_blockhash else {
        return (false, 0);
    };
    let segment =
        match MappedSegment::<BlockhashSnapshot>::open_ro(Path::new("/dev/shm/shadow_blockhash")) {
            Ok(segment) => segment,
            Err(_) => return (false, 0),
        };
    let Some(current) = segment.payload().read_active() else {
        return (false, 0);
    };
    let matches_current = current.blockhash == tx_blockhash;
    if !matches_current {
        return (false, 0);
    }
    let now_unix_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    (
        true,
        now_unix_ns.saturating_sub(current.last_update_unix_ns),
    )
}

#[doc(hidden)]
pub fn lock_proxy_diagnostics_for_test() -> MutexGuard<'static, ()> {
    PROXY_DIAGNOSTICS_TEST_LOCK
        .lock()
        .expect("proxy diagnostics test lock poisoned")
}

#[doc(hidden)]
pub fn reset_proxy_diagnostics_for_test() {
    set_proxy_diagnostics_for_test(JitoProxyDiagnosticsSnapshot::default());
}

#[doc(hidden)]
pub fn set_proxy_diagnostics_for_test(snapshot: JitoProxyDiagnosticsSnapshot) {
    PUBLIC_RPC_TRANSPORT_ATTEMPT.store(snapshot.public_rpc_transport_attempt, Ordering::Relaxed);
    PUBLIC_RPC_TRANSPORT_FAILURE.store(snapshot.public_rpc_transport_failure, Ordering::Relaxed);
    PUBLIC_RPC_TRANSPORT_REJECTED.store(snapshot.public_rpc_transport_rejected, Ordering::Relaxed);
    PUBLIC_RPC_NACK_RECORDED.store(snapshot.public_rpc_nack_recorded, Ordering::Relaxed);
    *FIRST_PUBLIC_RPC_ATTEMPT_SIGNATURE
        .lock()
        .expect("proxy first-attempt diagnostics lock poisoned") =
        snapshot.first_public_rpc_attempt_signature;
    let mut rejection = LAST_PUBLIC_RPC_REJECTION
        .lock()
        .expect("proxy rejection diagnostics lock poisoned");
    *rejection = JitoProxyDiagnosticsSnapshot {
        public_rpc_transport_attempt: 0,
        public_rpc_transport_failure: 0,
        public_rpc_transport_rejected: 0,
        public_rpc_nack_recorded: 0,
        first_public_rpc_attempt_signature: None,
        ..snapshot
    };
}
