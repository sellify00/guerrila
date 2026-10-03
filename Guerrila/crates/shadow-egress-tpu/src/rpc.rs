use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum RpcProbeError {
    #[error("failed to build blocking RPC client: {0}")]
    ClientBuild(reqwest::Error),
    #[error("RPC transport error for {method}: {source}")]
    Transport {
        method: &'static str,
        source: reqwest::Error,
    },
    #[error("RPC HTTP {status} for {method}")]
    Http {
        method: &'static str,
        status: StatusCode,
    },
    #[error("RPC parse error for {method}: {source}")]
    Parse {
        method: &'static str,
        source: reqwest::Error,
    },
    #[error("RPC response missing result for {0}")]
    MissingResult(&'static str),
    #[error("RPC error for {method}: {message}")]
    Rpc {
        method: &'static str,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct LocalRpcClient {
    endpoint: String,
    client: Client,
}

impl LocalRpcClient {
    pub fn new(endpoint: String, timeout_ms: u64) -> Result<Self, RpcProbeError> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms.max(1)))
            .build()
            .map_err(RpcProbeError::ClientBuild)?;
        Ok(Self { endpoint, client })
    }

    pub fn get_slot(&self) -> Result<u64, RpcProbeError> {
        self.call("getSlot", json!([{ "commitment": "processed" }]))
    }

    pub fn get_slot_leaders(
        &self,
        start_slot: u64,
        limit: usize,
    ) -> Result<Vec<String>, RpcProbeError> {
        self.call("getSlotLeaders", json!([start_slot, limit]))
    }

    pub fn get_cluster_nodes(&self) -> Result<Vec<ClusterNode>, RpcProbeError> {
        self.call("getClusterNodes", json!([]))
    }

    pub fn get_latest_blockhash(&self) -> Result<String, RpcProbeError> {
        let response: LatestBlockhashResult =
            self.call("getLatestBlockhash", json!([{ "commitment": "processed" }]))?;
        Ok(response.value.blockhash)
    }

    pub fn get_signature_statuses(
        &self,
        signatures: &[String],
    ) -> Result<Vec<Option<SignatureStatus>>, RpcProbeError> {
        let response: SignatureStatusesResult = self.call(
            "getSignatureStatuses",
            json!([signatures, { "searchTransactionHistory": true }]),
        )?;
        Ok(response.value)
    }

    fn call<T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<T, RpcProbeError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let response = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .map_err(|source| RpcProbeError::Transport { method, source })?;
        let status = response.status();
        if !status.is_success() {
            return Err(RpcProbeError::Http { method, status });
        }
        let envelope: RpcEnvelope<T> = response
            .json()
            .map_err(|source| RpcProbeError::Parse { method, source })?;
        if let Some(error) = envelope.error {
            return Err(RpcProbeError::Rpc {
                method,
                message: error.message,
            });
        }
        envelope.result.ok_or(RpcProbeError::MissingResult(method))
    }
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope<T> {
    result: Option<T>,
    error: Option<RpcErrorObject>,
}

#[derive(Debug, Deserialize)]
struct RpcErrorObject {
    message: String,
}

#[derive(Debug, Deserialize)]
struct LatestBlockhashResult {
    value: LatestBlockhashValue,
}

#[derive(Debug, Deserialize)]
struct LatestBlockhashValue {
    blockhash: String,
}

#[derive(Debug, Deserialize)]
struct SignatureStatusesResult {
    value: Vec<Option<SignatureStatus>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureStatus {
    pub slot: u64,
    #[serde(default)]
    pub err: Option<serde_json::Value>,
    #[serde(default)]
    pub confirmation_status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClusterNode {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub tpu: Option<String>,
    #[serde(default, alias = "tpuQuic", alias = "tpu_quic")]
    pub tpu_quic: Option<String>,
}

impl ClusterNode {
    pub fn has_tpu_contact(&self) -> bool {
        self.tpu
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
            || self
                .tpu_quic
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
    }

    pub fn quic_tpu_contact(&self) -> Option<&str> {
        self.tpu_quic
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn preferred_tpu_contact(&self) -> Option<&str> {
        self.quic_tpu_contact().or_else(|| {
            self.tpu
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
    }
}
