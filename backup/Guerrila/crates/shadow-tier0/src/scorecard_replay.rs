use std::env;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use shadow_core::config::ShadowConfig;
use shadow_ingest::pipeline::{run_decode_pipeline, DecodeInput};
use shadow_ipc::ingress_record::{
    create_feed_ring, feed_ring_path, open_feed_ring_rw, IngressRecord, FEED_RING_DEFAULT_CAPACITY,
    SIGNAL_SOURCE_AFXDP, SIGNAL_SOURCE_DPDK, SIGNAL_SOURCE_EXTERNAL_GEYSER,
    SIGNAL_SOURCE_HARMONIC_SEARCHER_API, SIGNAL_SOURCE_JITO_SEARCHER_API,
    SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER, SIGNAL_SOURCE_PRIVATE_MEMPOOL,
    SIGNAL_SOURCE_PUBLIC_GEYSER,
};
use shadow_ipc::{
    control_flags::{open_shadow_control_flags_rw, HOT_POOL_HINT_SLOT_COUNT},
    tag_map::open_pool_index_map_ro,
};

use crate::blocking_http_client::build_blocking_json_rpc_client;
use crate::file_allowlist::PassAllAllowlist;
use crate::grpc_producer::ValidatedTopology;
use crate::PipelineDeps;

const SCORECARD_REPLAY_ENV: &str = "SHADOW_RUN40_SCORECARD_REPLAY_SPEC";
const SCORECARD_REPLAY_RPC_ENV: &str = "SHADOW_RUN40_SCORECARD_REPLAY_RPC_URL";
const SCORECARD_REPLAY_PREWARM_POLL_MS: u64 = 250;
const SCORECARD_REPLAY_PREWARM_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Deserialize)]
struct ScorecardReplaySpec {
    signature: String,
    signal_source: String,
    slot: u64,
    unix_ns: u64,
    pool: String,
    mint_in: String,
    mint_out: String,
}

pub(crate) fn scorecard_replay_requested() -> bool {
    env::var(SCORECARD_REPLAY_ENV)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

pub(crate) fn replay_producer_id(topology: &ValidatedTopology) -> u32 {
    topology.shred_capture_workers + topology.grpc_workers
}

pub(crate) fn bootstrap_scorecard_replay_feed_rings(
    topology: &ValidatedTopology,
) -> Result<(), String> {
    if !scorecard_replay_requested() {
        return Ok(());
    }

    let producer_id = replay_producer_id(topology) as usize;
    for worker_id in 0..topology.money_workers as usize {
        let path = feed_ring_path(worker_id, producer_id);
        create_feed_ring(&path, FEED_RING_DEFAULT_CAPACITY).map_err(|err| {
            format!(
                "scorecard replay feed ring bootstrap failed for w{worker_id}_p{producer_id}: {err}"
            )
        })?;
    }

    println!(
        "PASS  bootstrapped {} scorecard replay feed ring(s) (producer_id={})",
        topology.money_workers, producer_id
    );
    Ok(())
}

pub(crate) fn inject_scorecard_replay(
    cfg: &ShadowConfig,
    topology: &ValidatedTopology,
    pipeline_deps: &PipelineDeps,
    boot_salt: u64,
) -> Result<(), String> {
    if !scorecard_replay_requested() {
        return Ok(());
    }

    let spec_path =
        env::var(SCORECARD_REPLAY_ENV).map_err(|_| format!("{SCORECARD_REPLAY_ENV} is not set"))?;
    let spec: ScorecardReplaySpec = serde_json::from_str(
        &std::fs::read_to_string(&spec_path)
            .map_err(|err| format!("scorecard replay spec read failed ({spec_path}): {err}"))?,
    )
    .map_err(|err| format!("scorecard replay spec parse failed ({spec_path}): {err}"))?;

    let producer_id = replay_producer_id(topology);
    let route_salt = boot_salt ^ (producer_id as u64);
    let timeout_ms = cfg.pm.as_ref().map(|pm| pm.rpc_timeout_ms).unwrap_or(5_000);
    let raw_tx = fetch_raw_tx(&replay_rpc_urls(cfg), timeout_ms, &spec.signature)?;
    let signal_source = parse_signal_source(&spec.signal_source)?;
    let pool_pubkey = parse_pubkey(&spec.pool, "pool")?;

    let input = DecodeInput {
        raw_tx: &raw_tx,
        signal_source,
        unix_ns: spec.unix_ns,
        slot: spec.slot,
        num_workers: topology.money_workers as u16,
        route_salt,
        manifest_swap: None,
    };
    let mut output = run_decode_pipeline(
        &input,
        &PassAllAllowlist,
        pipeline_deps.alt_resolver.as_ref(),
        pipeline_deps.mint_resolver.as_ref(),
    )
    .map_err(|reason| format!("scorecard replay decode failed: {reason:?}"))?;

    validate_decoded_record(&output.record, &spec, signal_source)?;
    output.record.mark_scorecard_replay_dev_proof();
    prewarm_replay_pool(cfg, pool_pubkey, signal_source)?;

    thread::sleep(Duration::from_millis(250));

    let worker_id = output.route.primary_worker as usize;
    let ring_path = feed_ring_path(worker_id, producer_id as usize);
    let ring = open_feed_ring_rw(&ring_path).map_err(|err| {
        format!("scorecard replay feed ring open failed for w{worker_id}_p{producer_id}: {err}")
    })?;
    if !ring.push(&output.record) {
        return Err(format!(
            "scorecard replay feed ring full for w{worker_id}_p{producer_id}"
        ));
    }

    println!(
        "PASS  scorecard replay injected signature={} worker={} producer_id={} pool={} mint_in={} mint_out={} signal_source={}",
        spec.signature,
        worker_id,
        producer_id,
        spec.pool,
        spec.mint_in,
        spec.mint_out,
        spec.signal_source,
    );
    Ok(())
}

fn prewarm_replay_pool(
    cfg: &ShadowConfig,
    pool_pubkey: [u8; 32],
    signal_source: u8,
) -> Result<(), String> {
    let shm_dir = cfg
        .system
        .proxies
        .ingress
        .as_ref()
        .and_then(|ingress| ingress.shm_dir.as_deref())
        .unwrap_or("/dev/shm");
    let pool_map_path = Path::new(shm_dir).join("pool_index_map");
    let pool_tag_map = open_pool_index_map_ro(&pool_map_path)
        .map_err(|err| format!("scorecard replay pool index map open failed: {err}"))?;
    let pool_b58 = bs58::encode(pool_pubkey).into_string();
    if pool_tag_map.find_by_hash40(&pool_pubkey).is_some() {
        println!("PASS  scorecard replay pool already present pool={pool_b58}");
        return Ok(());
    }

    let control_flags = open_shadow_control_flags_rw()
        .map_err(|err| format!("scorecard replay control flags open failed: {err}"))?;
    let poll_interval = Duration::from_millis(SCORECARD_REPLAY_PREWARM_POLL_MS);
    let deadline = Instant::now() + Duration::from_millis(SCORECARD_REPLAY_PREWARM_TIMEOUT_MS);
    let mut attempts = 0u32;
    while Instant::now() < deadline {
        let slot = (attempts as usize % HOT_POOL_HINT_SLOT_COUNT) as u8;
        attempts += 1;
        let _ = control_flags
            .payload()
            .record_hot_pool_hint(slot, pool_pubkey, signal_source);
        thread::sleep(poll_interval);
        if pool_tag_map.find_by_hash40(&pool_pubkey).is_some() {
            println!(
                "PASS  scorecard replay pool prewarmed pool={} attempts={}",
                pool_b58, attempts
            );
            return Ok(());
        }
    }

    Err(format!(
        "scorecard replay pool prewarm timed out pool={pool_b58} attempts={attempts} timeout_ms={SCORECARD_REPLAY_PREWARM_TIMEOUT_MS}"
    ))
}

fn replay_rpc_urls(cfg: &ShadowConfig) -> Vec<String> {
    let mut urls = Vec::new();
    if let Ok(url) = env::var(SCORECARD_REPLAY_RPC_ENV) {
        if !url.trim().is_empty() {
            urls.push(url);
        }
    }
    if let Some(pm) = cfg.pm.as_ref() {
        if !pm.solana_rpc_url.trim().is_empty() {
            urls.push(pm.solana_rpc_url.clone());
        }
    }
    dedupe_urls(urls)
}

fn dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for url in urls {
        if !out.iter().any(|existing| existing == &url) {
            out.push(url);
        }
    }
    out
}

fn parse_pubkey(value: &str, label: &str) -> Result<[u8; 32], String> {
    let bytes = bs58::decode(value)
        .into_vec()
        .map_err(|err| format!("invalid {label} pubkey '{value}': {err}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "invalid {label} pubkey '{value}': expected 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn fetch_raw_tx(urls: &[String], timeout_ms: u64, signature: &str) -> Result<Vec<u8>, String> {
    if urls.is_empty() {
        return Err("scorecard replay has no HTTP RPC URL candidates".to_owned());
    }

    let client = build_blocking_json_rpc_client(timeout_ms);
    let mut errors = Vec::new();
    for url in urls {
        match fetch_raw_tx_once(&client, url, signature) {
            Ok(bytes) => return Ok(bytes),
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    Err(format!(
        "scorecard replay getTransaction failed for {signature}: {}",
        errors.join(" | ")
    ))
}

fn fetch_raw_tx_once(
    client: &reqwest::blocking::Client,
    url: &str,
    signature: &str,
) -> Result<Vec<u8>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [
            signature,
            {
                "encoding": "base64",
                "commitment": "confirmed",
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    let response = client
        .post(url)
        .json(&body)
        .send()
        .map_err(|err| format!("HTTP error: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }

    let payload: Value = response
        .json()
        .map_err(|err| format!("JSON parse failed: {err}"))?;
    if let Some(error) = payload.get("error") {
        return Err(format!("RPC error: {error}"));
    }
    let result = payload
        .get("result")
        .ok_or_else(|| "missing result field".to_owned())?;
    if result.is_null() {
        return Err("transaction not found".to_owned());
    }

    let encoded = extract_base64_transaction(result)?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|err| format!("base64 decode failed: {err}"))
}

fn extract_base64_transaction(result: &Value) -> Result<&str, String> {
    let transaction = result
        .get("transaction")
        .ok_or_else(|| "result.transaction missing".to_owned())?;
    if let Some(array) = transaction.as_array() {
        if let Some(encoded) = array.first().and_then(Value::as_str) {
            return Ok(encoded);
        }
    }
    if let Some(encoded) = transaction.as_str() {
        return Ok(encoded);
    }
    Err("result.transaction is not base64-encoded".to_owned())
}

fn parse_signal_source(source: &str) -> Result<u8, String> {
    match source.trim().to_ascii_lowercase().as_str() {
        "public_geyser" => Ok(SIGNAL_SOURCE_PUBLIC_GEYSER),
        "jito_searcher_api" => Ok(SIGNAL_SOURCE_JITO_SEARCHER_API),
        "private_mempool" => Ok(SIGNAL_SOURCE_PRIVATE_MEMPOOL),
        "harmonic_searcher_api" => Ok(SIGNAL_SOURCE_HARMONIC_SEARCHER_API),
        "afxdp" => Ok(SIGNAL_SOURCE_AFXDP),
        "external_geyser" => Ok(SIGNAL_SOURCE_EXTERNAL_GEYSER),
        "local_validator_geyser" => Ok(SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER),
        "dpdk" => Ok(SIGNAL_SOURCE_DPDK),
        other => Err(format!(
            "unsupported scorecard replay signal_source: {other}"
        )),
    }
}

fn validate_decoded_record(
    record: &IngressRecord,
    spec: &ScorecardReplaySpec,
    expected_signal_source: u8,
) -> Result<(), String> {
    let signature = bs58::encode(record.signature).into_string();
    let pool = bs58::encode(record.pool_pubkey).into_string();
    let mint_in = bs58::encode(record.mint_in).into_string();
    let mint_out = bs58::encode(record.mint_out).into_string();
    if record.signal_source != expected_signal_source {
        return Err(format!(
            "decoded signal_source mismatch: expected {} got {}",
            expected_signal_source, record.signal_source
        ));
    }
    if signature != spec.signature {
        return Err(format!(
            "decoded signature mismatch: expected {} got {}",
            spec.signature, signature
        ));
    }
    if pool != spec.pool {
        return Err(format!(
            "decoded pool mismatch: expected {} got {}",
            spec.pool, pool
        ));
    }
    if mint_in != spec.mint_in {
        return Err(format!(
            "decoded mint_in mismatch: expected {} got {}",
            spec.mint_in, mint_in
        ));
    }
    if mint_out != spec.mint_out {
        return Err(format!(
            "decoded mint_out mismatch: expected {} got {}",
            spec.mint_out, mint_out
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_signal_source() {
        assert_eq!(
            parse_signal_source("external_geyser").unwrap(),
            SIGNAL_SOURCE_EXTERNAL_GEYSER
        );
    }
}
