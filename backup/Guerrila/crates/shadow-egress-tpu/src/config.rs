use shadow_core::config::{ConfigLoadError, ShadowConfig};
use std::env;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunMode {
    Disabled,
    #[default]
    DryRun,
    NoSend,
    TinyLive,
}

impl RunMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            RunMode::Disabled => "disabled",
            RunMode::DryRun => "dry_run",
            RunMode::NoSend => "no_send",
            RunMode::TinyLive => "tiny_live",
        }
    }
}

impl fmt::Display for RunMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RunMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "disabled" => Ok(RunMode::Disabled),
            "dry_run" => Ok(RunMode::DryRun),
            "no_send" => Ok(RunMode::NoSend),
            "tiny_live" => Ok(RunMode::TinyLive),
            other => Err(format!("unsupported Run40 mode '{other}'")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TpuEgressConfig {
    pub money_workers: u32,
    pub rpc_url: String,
    pub rpc_timeout_ms: u64,
    pub mode: RunMode,
    pub no_send_poll_ms: u64,
    pub no_send_idle_polls: u32,
    pub max_attempts: u32,
    pub max_inflight: u32,
    pub enable_live_send: bool,
    pub allow_public_rpc_fallback: bool,
    pub allow_jito_fallback: bool,
    pub allow_harmonic_fallback: bool,
    pub tiny_live_status_poll_ms: u64,
    pub tiny_live_status_poll_attempts: u32,
    pub tiny_live_expiry_slots: u64,
    pub tiny_live_sender_mode: String,
    pub rollout_mode_tiny_live: bool,
    pub rollout_no_send_backend: bool,
    pub selected_signature_filter: Option<String>,
    pub artifact_dir: Option<PathBuf>,
}

impl TpuEgressConfig {
    pub fn from_file(
        config_path: &Path,
        mode: RunMode,
        rpc_url_override: Option<String>,
        summary_file: Option<PathBuf>,
    ) -> Result<Self, ConfigLoadError> {
        let cfg = ShadowConfig::from_file(config_path)?;
        Ok(Self::from_shadow_config(
            &cfg,
            mode,
            rpc_url_override,
            summary_file,
        ))
    }

    pub fn from_shadow_config(
        cfg: &ShadowConfig,
        mode: RunMode,
        rpc_url_override: Option<String>,
        summary_file: Option<PathBuf>,
    ) -> Self {
        Self {
            money_workers: cfg.system.money_workers,
            rpc_url: rpc_url_override.unwrap_or_else(|| "http://127.0.0.1:8899".to_owned()),
            rpc_timeout_ms: cfg.public_rpc.timeout_ms.max(1),
            mode,
            no_send_poll_ms: env_u64("SHADOW_RUN40_NO_SEND_POLL_MS", 50),
            no_send_idle_polls: env_u32("SHADOW_RUN40_NO_SEND_IDLE_POLLS", 200),
            max_attempts: env_u32("SHADOW_RUN40_MAX_ATTEMPTS", 1),
            max_inflight: env_u32("SHADOW_RUN40_MAX_INFLIGHT", 1),
            enable_live_send: env_bool("SHADOW_RUN40_ENABLE_LIVE_SEND", false),
            allow_public_rpc_fallback: env_bool("SHADOW_RUN40_ALLOW_PUBLIC_RPC_FALLBACK", false),
            allow_jito_fallback: env_bool("SHADOW_RUN40_ALLOW_JITO_FALLBACK", false),
            allow_harmonic_fallback: env_bool("SHADOW_RUN40_ALLOW_HARMONIC_FALLBACK", false),
            tiny_live_status_poll_ms: env_u64("SHADOW_RUN40_TINY_LIVE_STATUS_POLL_MS", 250),
            tiny_live_status_poll_attempts: env_u32(
                "SHADOW_RUN40_TINY_LIVE_STATUS_POLL_ATTEMPTS",
                20,
            ),
            tiny_live_expiry_slots: env_u64("SHADOW_RUN40_TINY_LIVE_EXPIRY_SLOTS", 150),
            tiny_live_sender_mode: env::var("SHADOW_RUN40_TINY_LIVE_SENDER_MODE")
                .unwrap_or_else(|_| "udp".to_owned()),
            rollout_mode_tiny_live: cfg
                .rollout
                .as_ref()
                .map(|rollout| rollout.mode == shadow_core::config::RolloutMode::TinyLive)
                .unwrap_or(false),
            rollout_no_send_backend: cfg
                .rollout
                .as_ref()
                .map(|rollout| rollout.no_send_backend)
                .unwrap_or(false),
            selected_signature_filter: env_string("SHADOW_RUN40_SELECTED_SIGNATURE"),
            artifact_dir: summary_file
                .as_ref()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
        }
    }
}

fn env_u32(key: &str, default: u32) -> u32 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
        .max(1)
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
        .max(1)
}

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn env_string(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
