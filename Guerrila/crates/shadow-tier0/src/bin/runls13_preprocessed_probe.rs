#![recursion_limit = "256"]

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use clap::Parser;
use futures::StreamExt;
use helius_laserstream::grpc::{
    subscribe_preprocessed_update, SubscribePreprocessedRequest,
    SubscribePreprocessedRequestFilterTransactions, SubscribePreprocessedTransaction,
    SubscribePreprocessedUpdate,
};
use helius_laserstream::solana::storage::confirmed_block::{Message, Transaction};
use helius_laserstream::{subscribe_preprocessed, LaserstreamConfig};
use laserstream_core_client::{ClientTlsConfig, Interceptor};
use laserstream_core_proto::geyser::geyser_client::GeyserClient;
use laserstream_core_proto::tonic::{
    metadata::MetadataValue, transport::Endpoint, Request, Status,
};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Parser)]
#[command(
    name = "runls13-preprocessed-probe",
    about = "RunLS13 LaserStream preprocessed target freshness proof without live submit"
)]
struct Args {
    #[arg(long)]
    endpoint: String,
    #[arg(long)]
    api_key: String,
    #[arg(long)]
    rpc_url: String,
    #[arg(long)]
    out_dir: PathBuf,
    #[arg(long, default_value_t = 30)]
    window_seconds: u64,
    #[arg(long, action = clap::ArgAction::Append)]
    account_include: Vec<String>,
    #[arg(long)]
    selected_bundle_json: Option<PathBuf>,
    #[arg(long)]
    provider_json: Option<PathBuf>,
    #[arg(long)]
    direct_probe: bool,
    #[arg(long)]
    stream_jsonl: bool,
    #[arg(long, default_value_t = 0)]
    max_candidates: u64,
}

#[derive(Debug, Default, Clone)]
struct SelectedContext {
    pool: Option<String>,
    mint_in: Option<String>,
    mint_out: Option<String>,
    target_direction: Option<String>,
    backrun_direction: Option<String>,
    expected_out_raw: Option<u64>,
    min_out_raw: Option<u64>,
    net_ev_after_tip: Option<i64>,
    pm_reserve_created: bool,
    pm_reserve_released: bool,
    pm_reserve_id: Option<String>,
    pm_release_reason: Option<String>,
    pm_pending_after: Option<u64>,
}

#[derive(Debug, Clone)]
struct PreprocessedCandidate {
    target_signature: String,
    target_signature_hex: String,
    target_slot: u64,
    target_receive_ns: u64,
    provider_created_at_ns: Option<u64>,
    target_tx_base64: String,
    target_tx_decoded_len_bytes: usize,
    target_recent_blockhash: Option<String>,
    target_tx_decode_error: Option<String>,
    target_message_hash: Option<String>,
    target_account_keys: Vec<String>,
    target_program_ids: Vec<String>,
}

#[derive(Debug, Default)]
struct RpcProbe {
    reachable: bool,
    method_available: bool,
    auth_failed: bool,
    probe: String,
    error_code: Option<i64>,
    error_message: Option<String>,
    result: Option<Value>,
}

#[derive(Debug)]
struct RunOutput {
    summary: Value,
    row: Value,
    pass: bool,
}

#[derive(Debug, Default, Clone)]
struct SubscribeProbe {
    mode: &'static str,
    connect_ok: bool,
    initial_request_ok: bool,
    first_message_ok: bool,
    first_message_timeout: bool,
    update_kind: Option<String>,
    error_stage: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
}

#[derive(Clone)]
struct XTokenInterceptor {
    x_token: Option<laserstream_core_proto::tonic::metadata::AsciiMetadataValue>,
}

impl XTokenInterceptor {
    fn new(api_key: &str) -> Result<Self, Status> {
        let x_token = if api_key.is_empty() {
            None
        } else {
            Some(
                api_key
                    .parse()
                    .map_err(|err| Status::invalid_argument(format!("invalid x-token: {err}")))?,
            )
        };
        Ok(Self { x_token })
    }
}

impl Interceptor for XTokenInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(x_token) = self.x_token.clone() {
            request.metadata_mut().insert("x-token", x_token);
        }
        request
            .metadata_mut()
            .insert("x-sdk-name", MetadataValue::from_static("shadow-runls13"));
        request.metadata_mut().insert(
            "x-sdk-version",
            MetadataValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        Ok(request)
    }
}

fn main() {
    let args = Args::parse();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("failed to build tokio runtime: {err}"))
        .and_then(|runtime| runtime.block_on(run(args)));

    match result {
        Ok(output) => {
            if output.pass {
                std::process::exit(0);
            }
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

async fn run(args: Args) -> Result<RunOutput, String> {
    fs::create_dir_all(&args.out_dir).map_err(|err| {
        format!(
            "failed to create output dir {}: {err}",
            args.out_dir.display()
        )
    })?;

    let selected = load_selected_context(args.selected_bundle_json.as_deref())?;
    let account_include = account_filters(&args, &selected);
    let mut request = SubscribePreprocessedRequest::default();
    request.transactions.insert(
        "runls13_preprocessed".to_string(),
        SubscribePreprocessedRequestFilterTransactions {
            vote: Some(false),
            signature: None,
            account_include: account_include.clone(),
            account_exclude: Vec::new(),
            account_required: Vec::new(),
        },
    );
    let direct_probe = if args.direct_probe {
        Some(direct_subscribe_probe(&args.endpoint, &args.api_key, request.clone()).await)
    } else {
        None
    };

    if args.stream_jsonl {
        return run_stream(&args, account_include, request, direct_probe).await;
    }

    let config = LaserstreamConfig {
        endpoint: args.endpoint.clone(),
        api_key: args.api_key.clone(),
        max_reconnect_attempts: Some(3),
        replay: false,
        ..Default::default()
    };

    let (stream, _handle) = subscribe_preprocessed(config, request);
    tokio::pin!(stream);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(args.window_seconds.max(1));
    let mut stream_error: Option<String> = None;
    let mut skipped_updates = 0u64;
    let mut candidate: Option<PreprocessedCandidate> = None;

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next = match tokio::time::timeout(remaining, stream.next()).await {
            Ok(next) => next,
            Err(_) => {
                stream_error = Some("stream_window_elapsed".to_string());
                break;
            }
        };
        let Some(next) = next else {
            stream_error = Some("preprocessed stream ended".to_string());
            break;
        };

        match next {
            Ok(update) => match capture_preprocessed_update(update) {
                Ok(captured) => {
                    candidate = Some(captured);
                    break;
                }
                Err(reason) => {
                    skipped_updates = skipped_updates.saturating_add(1);
                    if reason != "non_transaction_update" {
                        stream_error = Some(reason);
                    }
                }
            },
            Err(err) => {
                stream_error = Some(err.to_string());
                break;
            }
        }
    }

    let output = build_output(
        &args,
        selected,
        account_include,
        candidate,
        skipped_updates,
        stream_error,
        direct_probe,
    )?;
    write_outputs(&args.out_dir, &output)?;
    Ok(output)
}

async fn run_stream(
    args: &Args,
    account_include: Vec<String>,
    request: SubscribePreprocessedRequest,
    direct_probe: Option<SubscribeProbe>,
) -> Result<RunOutput, String> {
    let config = LaserstreamConfig {
        endpoint: args.endpoint.clone(),
        api_key: args.api_key.clone(),
        max_reconnect_attempts: Some(3),
        replay: false,
        ..Default::default()
    };

    let rows_path = args.out_dir.join("runls13_preprocessed_stream_rows.jsonl");
    let summary_path = args.out_dir.join("runls13_preprocessed_stream_summary.json");
    let mut rows_file = File::create(&rows_path)
        .map_err(|err| format!("failed to create {}: {err}", rows_path.display()))?;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    let (stream, _handle) = subscribe_preprocessed(config, request);
    tokio::pin!(stream);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(args.window_seconds.max(1));
    let mut stream_error: Option<String> = None;
    let mut skipped_updates = 0u64;
    let mut emitted_rows = 0u64;
    let mut last_row = Value::Null;
    let mut seen_signatures = BTreeSet::<String>::new();

    while tokio::time::Instant::now() < deadline {
        if args.max_candidates > 0 && emitted_rows >= args.max_candidates {
            break;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next = match tokio::time::timeout(remaining, stream.next()).await {
            Ok(next) => next,
            Err(_) => {
                stream_error = Some("stream_window_elapsed".to_string());
                break;
            }
        };
        let Some(next) = next else {
            stream_error = Some("preprocessed stream ended".to_string());
            break;
        };

        match next {
            Ok(update) => match capture_preprocessed_update(update) {
                Ok(candidate) => {
                    if !seen_signatures.insert(candidate.target_signature.clone()) {
                        continue;
                    }
                    let row = json!({
                        "record_type": "runls13_preprocessed_stream_row",
                        "source": "laserstream_preprocessed_live",
                        "account_include": account_include,
                        "target_signature": candidate.target_signature,
                        "target_signature_hex": candidate.target_signature_hex,
                        "target_slot": candidate.target_slot,
                        "target_receive_ns": candidate.target_receive_ns,
                        "provider_created_at_ns": candidate.provider_created_at_ns,
                        "target_tx_bytes_available": !candidate.target_tx_base64.is_empty(),
                        "target_tx_base64": candidate.target_tx_base64,
                        "target_tx_base64_len": candidate.target_tx_base64.len(),
                        "target_tx_decoded_len_bytes": candidate.target_tx_decoded_len_bytes,
                        "target_recent_blockhash": candidate.target_recent_blockhash,
                        "target_tx_decode_error": candidate.target_tx_decode_error,
                        "target_message_hash": candidate.target_message_hash,
                        "target_account_keys": candidate.target_account_keys,
                        "target_program_ids": candidate.target_program_ids,
                    });
                    let line = serde_json::to_string(&row)
                        .map_err(|err| format!("failed to serialize stream row: {err}"))?;
                    writeln!(rows_file, "{line}")
                        .map_err(|err| format!("failed to append {}: {err}", rows_path.display()))?;
                    writeln!(stdout, "{line}")
                        .map_err(|err| format!("failed to emit stream row to stdout: {err}"))?;
                    stdout.flush()
                        .map_err(|err| format!("failed to flush stdout: {err}"))?;
                    emitted_rows = emitted_rows.saturating_add(1);
                    last_row = row;
                }
                Err(reason) => {
                    skipped_updates = skipped_updates.saturating_add(1);
                    if reason != "non_transaction_update" {
                        stream_error = Some(reason);
                    }
                }
            },
            Err(err) => {
                stream_error = Some(err.to_string());
                break;
            }
        }
    }

    let summary = json!({
        "record_type": "runls13_preprocessed_stream_summary",
        "status": "runls13_preprocessed_stream_complete",
        "source": "laserstream_preprocessed_live",
        "window_seconds": args.window_seconds,
        "account_include": account_include,
        "emitted_rows": emitted_rows,
        "unique_target_signatures": seen_signatures.len(),
        "skipped_updates": skipped_updates,
        "stream_error": stream_error,
        "direct_subscribe_probe": direct_probe.as_ref().map(subscribe_probe_json),
        "rows_path": rows_path.display().to_string(),
    });
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary)
            .map_err(|err| format!("failed to serialize stream summary: {err}"))?
            + "\n",
    )
    .map_err(|err| format!("failed to write {}: {err}", summary_path.display()))?;

    Ok(RunOutput {
        summary,
        row: last_row,
        pass: true,
    })
}

async fn direct_subscribe_probe(
    endpoint: &str,
    api_key: &str,
    request: SubscribePreprocessedRequest,
) -> SubscribeProbe {
    let mut probe = SubscribeProbe {
        mode: "direct_tonic_subscribe_preprocessed",
        ..Default::default()
    };

    let interceptor = match XTokenInterceptor::new(api_key) {
        Ok(interceptor) => interceptor,
        Err(status) => {
            probe.error_stage = Some("interceptor".to_string());
            probe.error_code = Some(format!("{:?}", status.code()));
            probe.error_message = Some(status.message().to_string());
            return probe;
        }
    };

    let endpoint = match Endpoint::from_shared(endpoint.to_string()) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            probe.error_stage = Some("endpoint_parse".to_string());
            probe.error_message = Some(err.to_string());
            return probe;
        }
    };
    let endpoint = match endpoint
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .tcp_nodelay(true)
        .tls_config(ClientTlsConfig::new().with_enabled_roots())
    {
        Ok(endpoint) => endpoint,
        Err(err) => {
            probe.error_stage = Some("tls_config".to_string());
            probe.error_message = Some(err.to_string());
            return probe;
        }
    };

    let channel = match endpoint.connect().await {
        Ok(channel) => {
            probe.connect_ok = true;
            channel
        }
        Err(err) => {
            probe.error_stage = Some("connect".to_string());
            probe.error_message = Some(err.to_string());
            return probe;
        }
    };

    let mut client = GeyserClient::with_interceptor(channel, interceptor)
        .max_decoding_message_size(1_000_000_000)
        .max_encoding_message_size(32_000_000);
    let request_stream = futures::stream::once(async move { request });
    let response = match client.subscribe_preprocessed(request_stream).await {
        Ok(response) => {
            probe.initial_request_ok = true;
            response
        }
        Err(status) => {
            probe.error_stage = Some("subscribe_preprocessed_initial_request".to_string());
            probe.error_code = Some(format!("{:?}", status.code()));
            probe.error_message = Some(status.message().to_string());
            return probe;
        }
    };

    let mut stream = response.into_inner();
    match tokio::time::timeout(Duration::from_secs(5), stream.message()).await {
        Ok(Ok(Some(update))) => {
            probe.first_message_ok = true;
            probe.update_kind = Some(match update.update_oneof {
                Some(subscribe_preprocessed_update::UpdateOneof::Transaction(_)) => {
                    "transaction".to_string()
                }
                Some(subscribe_preprocessed_update::UpdateOneof::Ping(_)) => "ping".to_string(),
                Some(subscribe_preprocessed_update::UpdateOneof::Pong(_)) => "pong".to_string(),
                None => "empty".to_string(),
            });
        }
        Ok(Ok(None)) => {
            probe.error_stage = Some("first_message_stream_end".to_string());
        }
        Ok(Err(status)) => {
            probe.error_stage = Some("first_message_status".to_string());
            probe.error_code = Some(format!("{:?}", status.code()));
            probe.error_message = Some(status.message().to_string());
        }
        Err(_) => {
            probe.first_message_timeout = true;
        }
    }

    probe
}

fn load_selected_context(path: Option<&Path>) -> Result<SelectedContext, String> {
    let Some(path) = path else {
        return Ok(SelectedContext::default());
    };
    if !path.exists() {
        return Ok(SelectedContext::default());
    }
    let doc: Value = serde_json::from_slice(
        &fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?,
    )
    .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    Ok(SelectedContext {
        pool: first_string(&doc, &["selected_pool", "pool", "pool_pubkey_hex"]),
        mint_in: first_string(&doc, &["selected_mint_in", "mint_in", "mint_in_hex"]),
        mint_out: first_string(&doc, &["selected_mint_out", "mint_out", "mint_out_hex"]),
        target_direction: first_string(&doc, &["selected_target_direction", "target_direction"]),
        backrun_direction: first_string(&doc, &["selected_backrun_direction", "backrun_direction"]),
        expected_out_raw: first_u64(&doc, &["selected_expected_out_raw", "expected_out_raw"]),
        min_out_raw: first_u64(&doc, &["selected_min_out_raw", "min_out_raw"]),
        net_ev_after_tip: first_i64(&doc, &["selected_net_ev_after_tip", "net_ev_after_tip"]),
        pm_reserve_created: first_bool(
            &doc,
            &["selected_pm_reserve_created", "pm_reserve_created"],
        ),
        pm_reserve_released: first_bool(
            &doc,
            &["selected_pm_reserve_released", "pm_reserve_released"],
        ),
        pm_reserve_id: first_string(&doc, &["selected_pm_reserve_id", "pm_reserve_id"]),
        pm_release_reason: first_string(&doc, &["selected_pm_release_reason", "pm_release_reason"]),
        pm_pending_after: first_u64(&doc, &["selected_pm_pending_after", "pm_pending_after"]),
    })
}

fn account_filters(args: &Args, selected: &SelectedContext) -> Vec<String> {
    let mut filters = BTreeSet::new();
    for value in &args.account_include {
        if !value.trim().is_empty() {
            filters.insert(normalize_pubkey_filter(value));
        }
    }
    if filters.is_empty() {
        if let Some(pool) = selected.pool.as_deref() {
            let normalized = normalize_pubkey_filter(pool);
            if !normalized.is_empty() {
                filters.insert(normalized);
            }
        }
    }
    filters.into_iter().collect()
}

fn capture_preprocessed_update(
    update: SubscribePreprocessedUpdate,
) -> Result<PreprocessedCandidate, String> {
    let receive_ns = wall_clock_nanos();
    let provider_created_at_ns = update
        .created_at
        .as_ref()
        .and_then(|ts| timestamp_parts_to_nanos(ts.seconds, ts.nanos));
    match update.update_oneof {
        Some(subscribe_preprocessed_update::UpdateOneof::Transaction(tx_update)) => {
            capture_preprocessed_transaction(tx_update, receive_ns, provider_created_at_ns)
        }
        Some(_) | None => Err("non_transaction_update".to_string()),
    }
}

fn capture_preprocessed_transaction(
    tx_update: SubscribePreprocessedTransaction,
    receive_ns: u64,
    provider_created_at_ns: Option<u64>,
) -> Result<PreprocessedCandidate, String> {
    let info = tx_update
        .transaction
        .ok_or_else(|| "missing_preprocessed_transaction_info".to_string())?;
    if info.is_vote {
        return Err("vote_transaction_skipped".to_string());
    }
    let proto_tx = info
        .transaction
        .ok_or_else(|| "missing_preprocessed_transaction".to_string())?;
    let proto_msg = proto_tx
        .message
        .as_ref()
        .ok_or_else(|| "missing_preprocessed_message".to_string())?;
    let signature_bytes = if !info.signature.is_empty() {
        info.signature.as_slice()
    } else {
        proto_tx
            .signatures
            .first()
            .map(Vec::as_slice)
            .ok_or_else(|| "missing_preprocessed_signature".to_string())?
    };
    if signature_bytes.len() != 64 {
        return Err("invalid_preprocessed_signature_len".to_string());
    }

    let raw_tx = reassemble_wire_tx(&proto_tx, proto_msg)
        .ok_or_else(|| "preprocessed_wire_reconstruction_failed".to_string())?;
    let parsed = parse_wire_tx(&raw_tx).unwrap_or_else(|err| ParsedWireTx {
        recent_blockhash: None,
        message_hash: None,
        account_keys: Vec::new(),
        program_ids: Vec::new(),
        decode_error: Some(err),
    });

    Ok(PreprocessedCandidate {
        target_signature: bs58::encode(signature_bytes).into_string(),
        target_signature_hex: hex::encode(signature_bytes),
        target_slot: tx_update.slot,
        target_receive_ns: receive_ns,
        provider_created_at_ns,
        target_tx_base64: BASE64_STANDARD.encode(&raw_tx),
        target_tx_decoded_len_bytes: raw_tx.len(),
        target_recent_blockhash: parsed.recent_blockhash,
        target_tx_decode_error: parsed.decode_error,
        target_message_hash: parsed.message_hash,
        target_account_keys: parsed.account_keys,
        target_program_ids: parsed.program_ids,
    })
}

fn build_output(
    args: &Args,
    selected: SelectedContext,
    account_include: Vec<String>,
    candidate: Option<PreprocessedCandidate>,
    skipped_updates: u64,
    stream_error: Option<String>,
    direct_probe: Option<SubscribeProbe>,
) -> Result<RunOutput, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("failed to build rpc client: {err}"))?;
    let provider_doc = load_json(args.provider_json.as_deref()).unwrap_or(Value::Null);
    let provider_name = first_string(&provider_doc, &["provider", "provider_route_name"])
        .unwrap_or_else(|| "jito_via_helius".to_string());
    let provider_auth_material_present = first_bool(
        &provider_doc,
        &["auth_material_present", "provider_auth_material_present"],
    );
    let provider_status_api_reachable = first_bool(
        &provider_doc,
        &["status_api_reachable", "provider_status_api_reachable"],
    );
    let provider_simulate_available = first_bool(
        &provider_doc,
        &["simulate_bundle_available", "simulateBundle_available"],
    );

    let mut blockers = Vec::<String>::new();
    let mut signature_status: Option<Value> = None;
    let mut signature_probe = RpcProbe::default();
    let mut blockhash_probe = RpcProbe::default();
    let mut simulate_probe = RpcProbe::default();
    let mut target_blockhash_valid = false;
    let mut target_already_landed = false;
    let mut simulate_bundle_pass = false;
    let mut simulate_bundle_summary: Option<Value> = None;

    if let Some(candidate) = candidate.as_ref() {
        signature_probe = rpc_call(
            &client,
            &args.rpc_url,
            "getSignatureStatuses",
            json!([[candidate.target_signature], {"searchTransactionHistory": true}]),
        );
        signature_status = signature_probe
            .result
            .as_ref()
            .and_then(|result| result.get("value"))
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .filter(|value| !value.is_null())
            .cloned();
        target_already_landed = signature_status.is_some();

        if let Some(blockhash) = candidate.target_recent_blockhash.as_deref() {
            blockhash_probe = rpc_call(
                &client,
                &args.rpc_url,
                "isBlockhashValid",
                json!([blockhash, {"commitment": "processed"}]),
            );
            target_blockhash_valid = blockhash_probe
                .result
                .as_ref()
                .and_then(|result| result.get("value"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }

        simulate_probe = rpc_call(
            &client,
            &args.rpc_url,
            "simulateBundle",
            json!([
                {"encodedTransactions": [candidate.target_tx_base64.clone()]},
                {
                    "preExecutionAccountsConfigs": [null],
                    "postExecutionAccountsConfigs": [null],
                    "skipSigVerify": true,
                    "simulationBank": {"commitment": {"commitment": "processed"}},
                    "transactionEncoding": "base64",
                    "replaceRecentBlockhash": false
                }
            ]),
        );
        if let Some(result) = simulate_probe.result.as_ref() {
            simulate_bundle_summary = result
                .get("value")
                .and_then(|value| value.get("summary"))
                .cloned();
            simulate_bundle_pass =
                simulate_bundle_summary.as_ref().and_then(Value::as_str) == Some("succeeded");
        }
    }

    let preprocessed_target_seen = candidate.is_some();
    let target_tx_bytes_available = candidate
        .as_ref()
        .map(|candidate| !candidate.target_tx_base64.is_empty())
        .unwrap_or(false);
    let target_tx_first = preprocessed_target_seen && target_tx_bytes_available;
    let our_backrun_tx_built = false;
    let our_backrun_tx_second = false;
    let tip_present = false;
    let bundle_tx_count = if target_tx_bytes_available { 1 } else { 0 };
    let pm_reserve_created = false;
    let pm_reserve_released = false;
    let send_bundle_called = false;
    let preprocessed_subscription_connect_failed =
        candidate.is_none() && stream_error.as_deref().is_some_and(is_stream_connect_error);
    let direct_probe_value = direct_probe.as_ref().map(subscribe_probe_json);

    if preprocessed_subscription_connect_failed {
        blockers.push("blocked_preprocessed_subscription_connect_failed".to_string());
    }
    if !preprocessed_target_seen {
        blockers.push("blocked_no_preprocessed_target_seen".to_string());
    }
    if !target_tx_bytes_available {
        blockers.push("blocked_target_tx_bytes_unavailable".to_string());
    }
    if target_already_landed {
        blockers.push("blocked_preprocessed_target_already_landed".to_string());
    }
    if let Some(candidate) = candidate.as_ref() {
        if candidate.target_tx_decode_error.is_some() {
            blockers.push("blocked_target_tx_decode_failed".to_string());
        }
        if candidate.target_recent_blockhash.is_none() || !target_blockhash_valid {
            blockers.push("blocked_preprocessed_target_blockhash_invalid".to_string());
        }
    }
    if !target_tx_first {
        blockers.push("blocked_target_tx_first_false".to_string());
    }
    if !our_backrun_tx_built {
        blockers.push("blocked_backrun_builder_not_integrated".to_string());
    }
    if !our_backrun_tx_second {
        blockers.push("blocked_our_backrun_tx_second_false".to_string());
    }
    if !tip_present {
        blockers.push("blocked_tip_missing".to_string());
    }
    if !pm_reserve_created {
        blockers.push("blocked_pm_reserve_not_created".to_string());
    }
    if send_bundle_called {
        blockers.push("blocked_send_bundle_called".to_string());
    }

    let selection_result = if blockers.is_empty() {
        "target_fresh_bundle_eligible"
    } else if blockers
        .iter()
        .any(|b| b == "blocked_preprocessed_subscription_connect_failed")
    {
        "blocked_preprocessed_subscription_connect_failed"
    } else if blockers
        .iter()
        .any(|b| b == "blocked_no_preprocessed_target_seen")
    {
        "blocked_no_preprocessed_target_seen"
    } else if blockers
        .iter()
        .any(|b| b == "blocked_preprocessed_target_already_landed")
    {
        "blocked_preprocessed_target_already_landed"
    } else if blockers
        .iter()
        .any(|b| b == "blocked_preprocessed_target_blockhash_invalid")
    {
        "blocked_preprocessed_target_blockhash_invalid"
    } else if blockers
        .iter()
        .any(|b| b == "blocked_backrun_builder_not_integrated")
    {
        "blocked_backrun_builder_not_integrated"
    } else {
        blockers
            .first()
            .map(String::as_str)
            .unwrap_or("blocked_unknown")
    };

    let source = "laserstream_preprocessed";
    let row = json!({
        "record_type": "runls13_preprocessed_target_freshness_row",
        "source": source,
        "selection_result": selection_result,
        "blockers": blockers,
        "direct_subscribe_probe": direct_probe_value.clone(),
        "preprocessed_subscription_connect_failed": preprocessed_subscription_connect_failed,
        "preprocessed_target_seen": preprocessed_target_seen,
        "target_signature": candidate.as_ref().map(|c| c.target_signature.as_str()),
        "target_signature_hex": candidate.as_ref().map(|c| c.target_signature_hex.as_str()),
        "target_slot": candidate.as_ref().map(|c| c.target_slot),
        "target_receive_ns": candidate.as_ref().map(|c| c.target_receive_ns),
        "provider_created_at_ns": candidate.as_ref().and_then(|c| c.provider_created_at_ns),
        "target_tx_bytes_available": target_tx_bytes_available,
        "target_tx_encoding": if target_tx_bytes_available { Some("base64") } else { None },
        "target_tx_base64": candidate.as_ref().map(|c| c.target_tx_base64.as_str()),
        "target_tx_base64_len": candidate.as_ref().map(|c| c.target_tx_base64.len()),
        "target_tx_decoded_len_bytes": candidate.as_ref().map(|c| c.target_tx_decoded_len_bytes),
        "target_recent_blockhash": candidate.as_ref().and_then(|c| c.target_recent_blockhash.as_deref()),
        "target_blockhash_valid": target_blockhash_valid,
        "target_signature_status_before_bundle": signature_status,
        "target_already_landed": target_already_landed,
        "target_tx_decode_error": candidate.as_ref().and_then(|c| c.target_tx_decode_error.as_deref()),
        "target_message_hash": candidate.as_ref().and_then(|c| c.target_message_hash.as_deref()),
        "target_account_keys": candidate.as_ref().map(|c| c.target_account_keys.clone()).unwrap_or_default(),
        "target_program_ids": candidate.as_ref().map(|c| c.target_program_ids.clone()).unwrap_or_default(),
        "pool": selected.pool,
        "mint_in": selected.mint_in,
        "mint_out": selected.mint_out,
        "target_direction": selected.target_direction.unwrap_or_else(|| "unknown".to_string()),
        "backrun_direction": selected.backrun_direction.unwrap_or_else(|| "unknown".to_string()),
        "our_backrun_tx_built": our_backrun_tx_built,
        "bundle_tx_count": bundle_tx_count,
        "target_tx_first": target_tx_first,
        "our_backrun_tx_second": our_backrun_tx_second,
        "tip_present": tip_present,
        "expected_out_raw": selected.expected_out_raw,
        "min_out_raw": selected.min_out_raw,
        "net_ev_after_tip": selected.net_ev_after_tip,
        "pm_reserve_created": pm_reserve_created,
        "pm_reserve_released": pm_reserve_released,
        "selected_prior_pm_reserve_created": selected.pm_reserve_created,
        "selected_prior_pm_reserve_released": selected.pm_reserve_released,
        "selected_prior_pm_reserve_id": selected.pm_reserve_id,
        "selected_prior_pm_release_reason": selected.pm_release_reason,
        "selected_prior_pm_pending_after": selected.pm_pending_after,
        "simulate_bundle_available": simulate_probe.method_available || provider_simulate_available,
        "simulate_bundle_scope": if target_tx_bytes_available { "target_only_no_submit_probe" } else { "not_attempted_no_target" },
        "simulate_bundle_pass": simulate_bundle_pass,
        "simulate_bundle_error": simulate_probe.error_message,
        "simulate_bundle_summary": simulate_bundle_summary,
        "send_bundle_called": send_bundle_called,
        "sendBundle_called": send_bundle_called,
        "wire_send": 0,
        "live_landed": 0
    });

    let summary = json!({
        "record_type": "runls13_preprocessed_target_freshness_summary",
        "source": source,
        "selection_result": selection_result,
        "blockers": row.get("blockers").cloned().unwrap_or_else(|| json!([])),
        "direct_subscribe_probe": direct_probe_value,
        "window_seconds": args.window_seconds,
        "account_include": account_include,
        "preprocessed_subscription_connect_failed": preprocessed_subscription_connect_failed,
        "preprocessed_target_seen": preprocessed_target_seen,
        "skipped_updates": skipped_updates,
        "stream_error": stream_error,
        "target_signature": row.get("target_signature").cloned().unwrap_or(Value::Null),
        "target_slot": row.get("target_slot").cloned().unwrap_or(Value::Null),
        "target_receive_ns": row.get("target_receive_ns").cloned().unwrap_or(Value::Null),
        "target_tx_bytes_available": target_tx_bytes_available,
        "target_recent_blockhash": row.get("target_recent_blockhash").cloned().unwrap_or(Value::Null),
        "target_blockhash_valid": target_blockhash_valid,
        "target_signature_status_before_bundle": row.get("target_signature_status_before_bundle").cloned().unwrap_or(Value::Null),
        "target_already_landed": target_already_landed,
        "target_tx_first": target_tx_first,
        "our_backrun_tx_built": our_backrun_tx_built,
        "our_backrun_tx_second": our_backrun_tx_second,
        "tip_present": tip_present,
        "bundle_tx_count": bundle_tx_count,
        "pm_reserve_created": pm_reserve_created,
        "pm_reserve_released": pm_reserve_released,
        "simulate_bundle_available": row.get("simulate_bundle_available").cloned().unwrap_or(Value::Bool(false)),
        "simulate_bundle_scope": row.get("simulate_bundle_scope").cloned().unwrap_or(Value::Null),
        "simulate_bundle_pass": simulate_bundle_pass,
        "send_bundle_called": send_bundle_called,
        "sendBundle_called": send_bundle_called,
        "wire_send": 0,
        "live_landed": 0,
        "provider": provider_name,
        "provider_auth_material_present": provider_auth_material_present,
        "provider_status_api_reachable": provider_status_api_reachable,
        "signature_status_rpc_probe": signature_probe.probe,
        "signature_status_rpc_reachable": signature_probe.reachable,
        "blockhash_rpc_probe": blockhash_probe.probe,
        "blockhash_rpc_reachable": blockhash_probe.reachable,
        "simulate_bundle_rpc_probe": simulate_probe.probe,
        "simulate_bundle_rpc_reachable": simulate_probe.reachable,
        "simulate_bundle_rpc_method_available": simulate_probe.method_available,
        "rpc_url": redact_url(&args.rpc_url)
    });

    let pass = selection_result == "target_fresh_bundle_eligible";
    Ok(RunOutput { summary, row, pass })
}

fn write_outputs(out_dir: &Path, output: &RunOutput) -> Result<(), String> {
    let summary_path = out_dir.join("runls13_preprocessed_target_freshness_summary.json");
    let rows_path = out_dir.join("runls13_preprocessed_target_freshness_rows.jsonl");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&output.summary)
            .map_err(|err| format!("failed to serialize summary: {err}"))?
            + "\n",
    )
    .map_err(|err| format!("failed to write {}: {err}", summary_path.display()))?;

    let mut rows = File::create(&rows_path)
        .map_err(|err| format!("failed to create {}: {err}", rows_path.display()))?;
    writeln!(
        rows,
        "{}",
        serde_json::to_string(&output.row)
            .map_err(|err| format!("failed to serialize row: {err}"))?
    )
    .map_err(|err| format!("failed to write {}: {err}", rows_path.display()))?;
    Ok(())
}

fn rpc_call(client: &Client, url: &str, method: &str, params: Value) -> RpcProbe {
    if is_placeholder(url) {
        return RpcProbe {
            reachable: false,
            method_available: false,
            auth_failed: false,
            probe: "placeholder_url".to_string(),
            error_code: None,
            error_message: None,
            result: None,
        };
    }

    let response = client
        .post(url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        }))
        .send();

    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return RpcProbe {
                reachable: false,
                method_available: false,
                auth_failed: false,
                probe: err.to_string(),
                error_code: None,
                error_message: Some(err.to_string()),
                result: None,
            };
        }
    };
    let status = response.status();
    let text = response.text().unwrap_or_default();
    let payload: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text}));
    let error = payload.get("error");
    let error_code = error
        .and_then(|err| err.get("code"))
        .and_then(Value::as_i64);
    let error_message = error
        .and_then(|err| err.get("message"))
        .and_then(Value::as_str)
        .map(|msg| msg.chars().take(240).collect::<String>());
    let method_missing = error_code == Some(-32601)
        || error_message_contains(error_message.as_deref(), "method not found");
    let auth_failed = status.as_u16() == 401
        || status.as_u16() == 403
        || error_message_contains(error_message.as_deref(), "api key")
        || error_message_contains(error_message.as_deref(), "unauthorized");

    RpcProbe {
        reachable: status.as_u16() < 500 && !auth_failed,
        method_available: !method_missing && !auth_failed,
        auth_failed,
        probe: format!("http_{}", status.as_u16()),
        error_code,
        error_message,
        result: payload.get("result").cloned(),
    }
}

fn error_message_contains(message: Option<&str>, needle: &str) -> bool {
    message
        .map(|message| message.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

fn is_stream_connect_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("connection failed")
        || lower.contains("reconnection attempts")
        || lower.contains("unauthorized")
        || lower.contains("permission")
        || lower.contains("invalidargument")
        || lower.contains("unimplemented")
        || lower.contains("unavailable")
        || lower.contains("transport")
        || lower.contains("tls")
}

fn subscribe_probe_json(probe: &SubscribeProbe) -> Value {
    json!({
        "mode": probe.mode,
        "connect_ok": probe.connect_ok,
        "initial_request_ok": probe.initial_request_ok,
        "first_message_ok": probe.first_message_ok,
        "first_message_timeout": probe.first_message_timeout,
        "update_kind": probe.update_kind,
        "error_stage": probe.error_stage,
        "error_code": probe.error_code,
        "error_message": probe.error_message,
    })
}

fn reassemble_wire_tx(proto_tx: &Transaction, proto_msg: &Message) -> Option<Vec<u8>> {
    let sig_count = proto_tx.signatures.len();
    let mut buf = Vec::with_capacity(3 + sig_count * 64 + 256 + proto_msg.account_keys.len() * 32);
    encode_compact_u16(&mut buf, sig_count as u16);
    for sig in &proto_tx.signatures {
        if sig.len() != 64 {
            return None;
        }
        buf.extend_from_slice(sig);
    }
    if proto_msg.versioned {
        buf.push(0x80);
    }
    reassemble_message_bytes(&mut buf, proto_msg)?;
    Some(buf)
}

fn reassemble_message_bytes(buf: &mut Vec<u8>, msg: &Message) -> Option<()> {
    let header = msg.header.as_ref()?;
    buf.push(header.num_required_signatures as u8);
    buf.push(header.num_readonly_signed_accounts as u8);
    buf.push(header.num_readonly_unsigned_accounts as u8);

    encode_compact_u16(buf, msg.account_keys.len() as u16);
    for key in &msg.account_keys {
        if key.len() != 32 {
            return None;
        }
        buf.extend_from_slice(key);
    }
    if msg.recent_blockhash.len() != 32 {
        return None;
    }
    buf.extend_from_slice(&msg.recent_blockhash);

    encode_compact_u16(buf, msg.instructions.len() as u16);
    for ix in &msg.instructions {
        buf.push(ix.program_id_index as u8);
        encode_compact_u16(buf, ix.accounts.len() as u16);
        buf.extend_from_slice(&ix.accounts);
        encode_compact_u16(buf, ix.data.len() as u16);
        buf.extend_from_slice(&ix.data);
    }

    if msg.versioned && !msg.address_table_lookups.is_empty() {
        encode_compact_u16(buf, msg.address_table_lookups.len() as u16);
        for lookup in &msg.address_table_lookups {
            if lookup.account_key.len() != 32 {
                return None;
            }
            buf.extend_from_slice(&lookup.account_key);
            encode_compact_u16(buf, lookup.writable_indexes.len() as u16);
            buf.extend_from_slice(&lookup.writable_indexes);
            encode_compact_u16(buf, lookup.readonly_indexes.len() as u16);
            buf.extend_from_slice(&lookup.readonly_indexes);
        }
    }
    Some(())
}

fn encode_compact_u16(buf: &mut Vec<u8>, val: u16) {
    if val < 0x80 {
        buf.push(val as u8);
    } else if val < 0x4000 {
        buf.push((val & 0x7F) as u8 | 0x80);
        buf.push((val >> 7) as u8);
    } else {
        buf.push((val & 0x7F) as u8 | 0x80);
        buf.push(((val >> 7) & 0x7F) as u8 | 0x80);
        buf.push((val >> 14) as u8);
    }
}

#[derive(Debug)]
struct ParsedWireTx {
    recent_blockhash: Option<String>,
    message_hash: Option<String>,
    account_keys: Vec<String>,
    program_ids: Vec<String>,
    decode_error: Option<String>,
}

fn parse_wire_tx(data: &[u8]) -> Result<ParsedWireTx, String> {
    let (sig_count, mut offset) = decode_shortvec(data, 0)?;
    offset = offset
        .checked_add(sig_count * 64)
        .ok_or_else(|| "signature_offset_overflow".to_string())?;
    if offset >= data.len() {
        return Err("message_offset_out_of_bounds".to_string());
    }
    let message_start = offset;
    let first_message_byte = data[offset];
    if first_message_byte & 0x80 != 0 {
        offset += 1;
    }
    offset = offset
        .checked_add(3)
        .ok_or_else(|| "header_offset_overflow".to_string())?;
    let (account_count, mut offset) = decode_shortvec(data, offset)?;
    let mut account_keys = Vec::with_capacity(account_count);
    for _ in 0..account_count {
        if offset + 32 > data.len() {
            return Err("account_key_out_of_bounds".to_string());
        }
        account_keys.push(bs58::encode(&data[offset..offset + 32]).into_string());
        offset += 32;
    }
    if offset + 32 > data.len() {
        return Err("recent_blockhash_out_of_bounds".to_string());
    }
    let recent_blockhash = bs58::encode(&data[offset..offset + 32]).into_string();
    offset += 32;

    let (instruction_count, mut offset) = decode_shortvec(data, offset)?;
    let mut program_ids = BTreeSet::new();
    for _ in 0..instruction_count {
        if offset >= data.len() {
            return Err("instruction_program_id_out_of_bounds".to_string());
        }
        let program_id_index = data[offset] as usize;
        offset += 1;
        if let Some(program_id) = account_keys.get(program_id_index) {
            program_ids.insert(program_id.clone());
        }
        let (account_index_count, next_offset) = decode_shortvec(data, offset)?;
        offset = next_offset
            .checked_add(account_index_count)
            .ok_or_else(|| "instruction_account_offset_overflow".to_string())?;
        let (data_len, next_offset) = decode_shortvec(data, offset)?;
        offset = next_offset
            .checked_add(data_len)
            .ok_or_else(|| "instruction_data_offset_overflow".to_string())?;
        if offset > data.len() {
            return Err("instruction_out_of_bounds".to_string());
        }
    }

    let message_hash = if message_start < data.len() {
        Some(hex::encode(Sha256::digest(&data[message_start..])))
    } else {
        None
    };

    Ok(ParsedWireTx {
        recent_blockhash: Some(recent_blockhash),
        message_hash,
        account_keys,
        program_ids: program_ids.into_iter().collect(),
        decode_error: None,
    })
}

fn decode_shortvec(data: &[u8], mut offset: usize) -> Result<(usize, usize), String> {
    let mut size = 0usize;
    let mut shift = 0usize;
    loop {
        if offset >= data.len() {
            return Err("shortvec_out_of_bounds".to_string());
        }
        let elem = data[offset];
        offset += 1;
        size |= ((elem & 0x7F) as usize) << shift;
        if elem & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 28 {
            return Err("shortvec_too_large".to_string());
        }
    }
    Ok((size, offset))
}

fn first_string(doc: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| doc.get(*key))
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn first_u64(doc: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(value) = doc.get(*key) else {
            continue;
        };
        if let Some(value) = value.as_u64() {
            return Some(value);
        }
        if let Some(value) = value.as_i64() {
            if value >= 0 {
                return Some(value as u64);
            }
        }
        if let Some(value) = value.as_str().and_then(|value| value.parse::<u64>().ok()) {
            return Some(value);
        }
    }
    None
}

fn first_i64(doc: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        let Some(value) = doc.get(*key) else {
            continue;
        };
        if let Some(value) = value.as_i64() {
            return Some(value);
        }
        if let Some(value) = value.as_u64() {
            return i64::try_from(value).ok();
        }
        if let Some(value) = value.as_str().and_then(|value| value.parse::<i64>().ok()) {
            return Some(value);
        }
    }
    None
}

fn first_bool(doc: &Value, keys: &[&str]) -> bool {
    for key in keys {
        let Some(value) = doc.get(*key) else {
            continue;
        };
        if let Some(value) = value.as_bool() {
            return value;
        }
        if let Some(value) = value.as_str() {
            match value.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => return true,
                "0" | "false" | "no" | "off" => return false,
                _ => {}
            }
        }
    }
    false
}

fn load_json(path: Option<&Path>) -> Result<Value, String> {
    let Some(path) = path else {
        return Ok(Value::Null);
    };
    if !path.exists() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(
        &fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?,
    )
    .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn normalize_pubkey_filter(value: &str) -> String {
    let value = value.trim();
    if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        if let Ok(bytes) = hex::decode(value) {
            if bytes.len() == 32 {
                return bs58::encode(bytes).into_string();
            }
        }
    }
    value.to_string()
}

fn is_placeholder(url: &str) -> bool {
    url.trim().is_empty() || url.contains("CHANGE_ME") || url.contains("${")
}

fn redact_url(url: &str) -> String {
    let mut redacted = url.to_string();
    for key in ["api-key=", "apikey=", "key=", "token="] {
        if let Some(idx) = redacted.to_ascii_lowercase().find(key) {
            let start = idx + key.len();
            let end = redacted[start..]
                .find('&')
                .map(|offset| start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(start..end, "REDACTED");
        }
    }
    redacted
}

fn timestamp_parts_to_nanos(seconds: i64, nanos: i32) -> Option<u64> {
    if seconds <= 0 || nanos < 0 {
        return None;
    }
    let secs = seconds as u64;
    let nanos = nanos as u64;
    secs.checked_mul(1_000_000_000)?.checked_add(nanos)
}

fn wall_clock_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(1)
}
