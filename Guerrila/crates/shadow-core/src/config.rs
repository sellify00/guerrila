use serde::Deserialize;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowConfig {
    pub deployment: DeploymentConfig,
    pub system: SystemConfig,
    pub risk: RiskConfig,
    pub submit: SubmitConfig,
    #[serde(default)]
    pub oracle: Option<OracleConfig>,
    #[serde(default)]
    pub state: Option<StateConfig>,
    #[serde(default)]
    pub quant_tactics: Option<QuantTacticsConfig>,
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
    #[serde(default)]
    pub entry: Option<EntryConfig>,
    #[serde(default)]
    pub jito: Option<JitoConfig>,
    #[serde(default)]
    pub harmonic: Option<HarmonicConfig>,
    #[serde(default)]
    pub public_rpc: PublicRpcConfig,
    #[serde(default)]
    pub helius_sender: HeliusSenderConfig,
    #[serde(default)]
    pub execution: Option<ExecutionConfig>,
    #[serde(default)]
    pub rollout: Option<RolloutConfig>,
    #[serde(default)]
    pub r1: Option<R1Config>,
    #[serde(default)]
    pub pm: Option<PmConfig>,
    #[serde(default)]
    pub shadow_control: Option<ShadowControlPlaneConfig>,
}

// ---------------------------------------------------------------------------
// deployment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentConfig {
    pub profile: DeploymentProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentProfile {
    V6_4Full,
    V6_4Guerrila,
}

// ---------------------------------------------------------------------------
// system
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct SystemConfig {
    #[serde(default)]
    pub mode: Option<String>,
    /// Number of money worker threads (build → sign → enqueue).
    /// One SPSC egress ring per worker per egress lane.
    pub money_workers: u32,
    pub proxies: ProxiesConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxiesConfig {
    #[serde(default)]
    pub ingress: Option<IngressConfig>,
    pub egress: EgressConfig,
    #[serde(default)]
    pub health: Option<HealthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IngressConfig {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub shred_capture_workers: Option<u32>,
    #[serde(default)]
    pub private_packet_backend: PrivatePacketIngressBackend,
    #[serde(default)]
    pub afxdp_bind_mode: AfxdpBindMode,
    #[serde(default)]
    pub dpdk: DpdkIngressConfig,
    #[serde(default)]
    pub grpc_fallback: Option<String>,
    #[serde(default)]
    pub grpc_fallback_token: Option<String>,
    #[serde(default)]
    pub grpc_workers: Option<u32>,
    #[serde(default)]
    pub grpc_source_grade: GrpcSourceGrade,
    #[serde(default)]
    pub grpc_source_probe_mode: GrpcSourceProbeMode,
    /// Path to the binary allowlist file (signer pubkey → entity_id).
    /// If absent, operates in pass-all mode (entity_id = 0 for all signers).
    #[serde(default)]
    pub allowlist_path: Option<String>,
    /// Path to the binary pool mint cache file (pool → vaults + mints).
    /// If absent, mint resolution returns None for all pools.
    #[serde(default)]
    pub pool_mint_cache_path: Option<String>,
    /// Shared-memory directory for ALT state + index map.
    /// Defaults to `/dev/shm` when absent.
    #[serde(default)]
    pub shm_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AfxdpBindMode {
    #[default]
    ZeroCopy,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrivatePacketIngressBackend {
    #[default]
    Afxdp,
    Dpdk,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DpdkIngressConfig {
    #[serde(default)]
    pub port_id: u16,
    #[serde(default = "default_dpdk_rx_desc")]
    pub rx_desc: u16,
    #[serde(default = "default_dpdk_burst_size")]
    pub burst_size: u16,
    #[serde(default = "default_dpdk_mbuf_count")]
    pub mbuf_count: u32,
    #[serde(default = "default_dpdk_mbuf_cache_size")]
    pub mbuf_cache_size: u32,
    #[serde(default = "default_dpdk_promiscuous")]
    pub promiscuous: bool,
}

impl Default for DpdkIngressConfig {
    fn default() -> Self {
        Self {
            port_id: 0,
            rx_desc: default_dpdk_rx_desc(),
            burst_size: default_dpdk_burst_size(),
            mbuf_count: default_dpdk_mbuf_count(),
            mbuf_cache_size: default_dpdk_mbuf_cache_size(),
            promiscuous: default_dpdk_promiscuous(),
        }
    }
}

fn default_dpdk_rx_desc() -> u16 {
    1024
}

fn default_dpdk_burst_size() -> u16 {
    32
}

fn default_dpdk_mbuf_count() -> u32 {
    8192
}

fn default_dpdk_mbuf_cache_size() -> u32 {
    256
}

fn default_dpdk_promiscuous() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GrpcSourceGrade {
    #[default]
    ExternalGeyser,
    LocalValidatorGeyser,
    Laserstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GrpcSourceProbeMode {
    #[default]
    Disabled,
    UnsupportedProgramNoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Sh8AfxdpSourceProbeMode {
    #[default]
    Disabled,
    ParsedShredNoop,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthConfig {
    pub publish_egress_health: bool,
    pub degraded_hysteresis_batches: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EgressConfig {
    pub quic_workers: u32,
    #[serde(default)]
    pub jito_workers: Option<u32>,
    #[serde(default)]
    pub harmonic_workers: Option<u32>,
}

// ---------------------------------------------------------------------------
// jito
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct JitoConfig {
    /// Jito block engine gRPC endpoint.
    pub block_engine_url: String,

    /// `"no_auth"` or path to an Ed25519 keypair JSON file.
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,

    /// Timeout (ms) for the initial gRPC connect handshake.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,

    /// Timeout (ms) for a single `SendBundle` RPC.
    #[serde(default = "default_send_timeout_ms")]
    pub send_timeout_ms: u64,

    /// Initial backoff (ms) before first reconnect attempt after a drop.
    #[serde(default = "default_reconnect_initial_backoff_ms")]
    pub reconnect_initial_backoff_ms: u64,

    /// Maximum backoff (ms) cap for exponential reconnect jitter.
    #[serde(default = "default_reconnect_max_backoff_ms")]
    pub reconnect_max_backoff_ms: u64,

    /// Explicitly require TLS regardless of URL scheme.
    #[serde(default = "default_tls_enabled")]
    pub tls_enabled: bool,

    /// Optional Jito Block Engine JSON-RPC submission path.
    #[serde(default)]
    pub json_rpc: JitoJsonRpcConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JitoJsonRpcConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_jito_json_rpc_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub mode: JitoJsonRpcMode,
    #[serde(default)]
    pub auth_uuid: String,
}

impl Default for JitoJsonRpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_jito_json_rpc_endpoint(),
            mode: JitoJsonRpcMode::BundleOnly,
            auth_uuid: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JitoJsonRpcMode {
    #[default]
    BundleOnly,
}

fn default_auth_mode() -> String {
    "no_auth".into()
}
fn default_connect_timeout_ms() -> u64 {
    5_000
}
fn default_send_timeout_ms() -> u64 {
    3_000
}
fn default_reconnect_initial_backoff_ms() -> u64 {
    250
}
fn default_reconnect_max_backoff_ms() -> u64 {
    10_000
}
fn default_tls_enabled() -> bool {
    true
}
fn default_jito_json_rpc_endpoint() -> String {
    "https://tokyo.mainnet.block-engine.jito.wtf".into()
}

// ---------------------------------------------------------------------------
// public_rpc
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PublicRpcConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_public_rpc_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_public_rpc_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_public_rpc_max_pending")]
    pub max_pending_tx: u32,
}

impl Default for PublicRpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_public_rpc_endpoint(),
            timeout_ms: default_public_rpc_timeout_ms(),
            max_pending_tx: default_public_rpc_max_pending(),
        }
    }
}

fn default_public_rpc_endpoint() -> String {
    "https://api.mainnet-beta.solana.com".into()
}

fn default_public_rpc_timeout_ms() -> u64 {
    3_000
}

fn default_public_rpc_max_pending() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeliusSenderConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_helius_sender_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_helius_sender_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_helius_sender_min_tip_lamports")]
    pub min_tip_lamports: u64,
    #[serde(default)]
    pub tip_accounts: Vec<String>,
    #[serde(default)]
    pub api_key: String,
}

impl Default for HeliusSenderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_helius_sender_endpoint(),
            timeout_ms: default_helius_sender_timeout_ms(),
            min_tip_lamports: default_helius_sender_min_tip_lamports(),
            tip_accounts: Vec::new(),
            api_key: String::new(),
        }
    }
}

fn default_helius_sender_endpoint() -> String {
    "https://sender.helius-rpc.com/fast".into()
}

fn default_helius_sender_timeout_ms() -> u64 {
    3_000
}

fn default_helius_sender_min_tip_lamports() -> u64 {
    200_000
}

// ---------------------------------------------------------------------------
// harmonic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct HarmonicConfig {
    /// Regional gRPC endpoints, e.g.
    /// `["https://fra.harmonic.xyz", "https://ams.harmonic.xyz"]`.
    pub region_endpoints: Vec<String>,

    /// `"no_auth"` or path to an Ed25519 keypair JSON file.
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,

    /// Timeout (ms) for the initial gRPC connect handshake per region.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,

    /// Timeout (ms) for a single `SendBundle` RPC.
    #[serde(default = "default_send_timeout_ms")]
    pub send_timeout_ms: u64,

    /// Initial backoff (ms) before first reconnect attempt after a drop.
    #[serde(default = "default_reconnect_initial_backoff_ms")]
    pub reconnect_initial_backoff_ms: u64,

    /// Maximum backoff (ms) cap for exponential reconnect jitter.
    #[serde(default = "default_reconnect_max_backoff_ms")]
    pub reconnect_max_backoff_ms: u64,

    /// Explicitly require TLS regardless of URL scheme.
    #[serde(default = "default_tls_enabled")]
    pub tls_enabled: bool,

    /// Consecutive send failures required to transition healthy → degraded.
    #[serde(default = "default_degrade_failure_threshold")]
    pub degrade_failure_threshold: u32,

    /// Consecutive send failures required to transition degraded → down.
    #[serde(default = "default_down_failure_threshold")]
    pub down_failure_threshold: u32,

    /// Consecutive success batches required to recover from degraded/down → healthy.
    #[serde(default = "default_hysteresis_recover_batches")]
    pub hysteresis_recover_batches: u32,

    /// Hysteresis batches value to publish in the egress health segment.
    #[serde(default = "default_degraded_hysteresis_batches")]
    pub degraded_hysteresis_batches: u16,
}

fn default_degrade_failure_threshold() -> u32 {
    3
}
fn default_down_failure_threshold() -> u32 {
    10
}
fn default_hysteresis_recover_batches() -> u32 {
    2
}
fn default_degraded_hysteresis_batches() -> u16 {
    2
}

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct StateConfig {
    #[serde(default)]
    pub geyser: Option<GeyserStateConfig>,
    #[serde(default)]
    pub clmm: Option<ClmmConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeyserStateConfig {
    #[serde(default)]
    pub harmonic_max_state_slot_lag: Option<u64>,
    #[serde(default)]
    pub require_live_transfer_fee_updates: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClmmConfig {
    #[serde(default)]
    pub publish_dist_to_tick_lamports: Option<bool>,
}

// ---------------------------------------------------------------------------
// risk
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    pub reserve_floor_sol: f64,
    pub max_position_sol_per_mint: f64,
    pub max_notional_sol_per_entity: f64,
}

// ---------------------------------------------------------------------------
// submit
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct SubmitConfig {
    pub routing: RoutingConfig,
    pub quic: QuicConfig,
    #[serde(default)]
    pub harmonic: Option<HarmonicSubmitConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutingConfig {
    pub default_public_geyser: PublicGeyserAction,
    #[serde(default)]
    pub default_jito_searcher: Option<String>,
    pub private_mempool_strategy: PrivateMempoolStrategy,
    #[serde(default)]
    pub block_private_bundle_routes: bool,
    pub enable_harmonic_lane: bool,
    /// Required when `enable_harmonic_lane` is true.
    /// Controls the Harmonic 1-Strike Lethal Catch quarantine breaker.
    #[serde(default)]
    pub harmonic_quarantine_enabled: Option<bool>,

    pub pre_sign_egress_admission_gate: bool,

    #[serde(default)]
    pub stale_harmonic_state_action: Option<StaleHarmonicStateAction>,
    #[serde(default)]
    pub atba_route_mode: Option<AtbaRouteMode>,
    #[serde(default)]
    pub apex_partial_route_mode: Option<ApexPartialRouteMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicGeyserAction {
    Drop,
    Send,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateMempoolStrategy {
    /// Bundle if compatible, otherwise drop (Guerrila-safe).
    JitoBundleIfCompatible,
    /// Bundle if compatible, otherwise fall back to public blast.
    JitoBundleOrPublic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleHarmonicStateAction {
    RerouteToJito,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtbaRouteMode {
    JitoOnly,
    AnyPrivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApexPartialRouteMode {
    JitoOnly,
    AnyPrivate,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuicConfig {
    pub kamikaze_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarmonicSubmitConfig {
    #[serde(default)]
    pub cu_pad_units: Option<u32>,
    #[serde(default)]
    pub quarantine_mode: Option<QuarantineMode>,
    pub quarantine_scope: QuarantineScope,
    pub tactic_class_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineMode {
    SpecificErrorOnly,
    AnyError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineScope {
    ClassScopedWithGlobalFailsafe,
    GlobalOnly,
}

// ---------------------------------------------------------------------------
// oracle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct OracleConfig {
    #[serde(default)]
    pub tip_lut: Option<TipLutConfig>,
    #[serde(default)]
    pub guerrila: Option<OracleGuerrilaConfig>,
    #[serde(default)]
    pub opso: Option<OpsoConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TipLutConfig {
    #[serde(default)]
    pub jito_tax_multiplier_bps: Option<u64>,
    #[serde(default)]
    pub harmonic_tax_multiplier_bps: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OracleGuerrilaConfig {
    #[serde(default)]
    pub max_required_backrun_capital_sol: Option<f64>,
    #[serde(default)]
    pub retail_only_mode: Option<bool>,
    #[serde(default)]
    pub micro_market_enabled: Option<bool>,
    #[serde(default)]
    pub leader_class_count: Option<u8>,
    #[serde(default)]
    pub publish_mode: Option<PublishMode>,
    #[serde(default)]
    pub max_live_trade_input_sol: Option<f64>,
    #[serde(default)]
    pub allow_clamped_apex_partials: Option<bool>,
    #[serde(default)]
    pub allow_atba_jito_only: Option<bool>,
    #[serde(default)]
    pub obscurity_discount_enabled: Option<bool>,
    #[serde(default)]
    pub obscurity_mode: Option<ObscurityMode>,
    #[serde(default)]
    pub obscurity_initial_floor_bps: Option<u64>,
    #[serde(default)]
    pub class_decay_per_step_bps: Option<u64>,
    #[serde(default)]
    pub alpha_floor_bps: Option<u64>,
    #[serde(default)]
    pub primary_strategy_mode: Option<StrategyMode>,
    #[serde(default)]
    pub shadow_strategy_mode: Option<ShadowStrategyMode>,
    #[serde(default)]
    pub sparse_class_action: Option<SparseClassAction>,
    #[serde(default)]
    pub competition_selector_enabled: Option<bool>,
    #[serde(default)]
    pub competition_selector_mode: Option<CompetitionSelectorMode>,
    #[serde(default)]
    pub competition_selector_max_penalty_bps: Option<u64>,
    #[serde(default)]
    pub competition_selector_min_live_bid_floor_bps: Option<u64>,
    #[serde(default)]
    pub competition_selector_class_min_samples: Option<u32>,
    #[serde(default)]
    pub competition_selector_half_life_slots: Option<u64>,
    #[serde(default)]
    pub lut_source: Option<OracleLutSource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpsoConfig {
    pub enabled: bool,
    pub mode: OpsoMode,
    pub sidecar_enabled: bool,
    pub sidecar_stale_action: OpsoStaleAction,
    pub max_sidecar_staleness_ns: u64,
    pub high_alpha_override_enabled: bool,
    pub max_shade_bps: u32,
    pub require_shadow_pnl: bool,
    pub require_pm_episode_ledger: bool,
    pub sparse_class_action: SparseClassAction,
    pub live_bid_shading_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsoMode {
    Disabled,
    ObserveOnly,
    ShadowBid,
    TinyLive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsoStaleAction {
    FallbackToBaseline,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostShockMode {
    None,
    Backrun,
    Housekeeping,
    Scout,
    Continuation,
    Rebalancing,
    Flatten,
    Drop,
}

impl PostShockMode {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Backrun => 1,
            Self::Housekeeping => 2,
            Self::Scout => 3,
            Self::Continuation => 4,
            Self::Rebalancing => 5,
            Self::Flatten => 6,
            Self::Drop => 7,
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Backrun),
            2 => Some(Self::Housekeeping),
            3 => Some(Self::Scout),
            4 => Some(Self::Continuation),
            5 => Some(Self::Rebalancing),
            6 => Some(Self::Flatten),
            7 => Some(Self::Drop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishMode {
    Flat,
    BankedByLeaderClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObscurityMode {
    HeaderOnlyBitpacked,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyMode {
    Baseline,
    CompetitionAdjusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowStrategyMode {
    None,
    Baseline,
    CompetitionAdjusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SparseClassAction {
    FallbackToBaseline,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompetitionSelectorMode {
    FixedBankPenaltyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleLutSource {
    ShmPublished,
    FileFallback,
}

// ---------------------------------------------------------------------------
// quant_tactics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct QuantTacticsConfig {
    #[serde(default)]
    pub taylor_compression: Option<TaylorCompressionConfig>,
    #[serde(default)]
    pub ghost_snipe: Option<GhostSnipeConfig>,
    #[serde(default)]
    pub write_lock_saturation_discount: Option<WriteLockSaturationDiscountConfig>,
    #[serde(default)]
    pub splinter_cell_arb: Option<SplinterCellArbConfig>,
    #[serde(default)]
    pub cu_density_optimization: Option<CuDensityOptimizationConfig>,
    #[serde(default)]
    pub post_trade_exhaust_scavenging: Option<PostTradeExhaustScavengingConfig>,
    #[serde(default)]
    pub tick_boundary_sabotage: Option<TickBoundarySabotageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaylorCompressionConfig {
    pub enabled: bool,
    pub mode: TaylorCompressionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaylorCompressionMode {
    PreFilterGateOnly,
    FullReplacement,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhostSnipeConfig {
    pub enabled: bool,
    pub mode: GhostSnipeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GhostSnipeMode {
    CachePrefetchOnly,
    BlindFire,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WriteLockSaturationDiscountConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SplinterCellArbConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CuDensityOptimizationConfig {
    pub enabled: bool,
    pub mode: CuDensityMode,
    /// Variance pad (CU) added to audited baseline for runtime jitter.
    pub jito_baseline_variance_pad_cus: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuDensityMode {
    MinimumEmpiricalSafePadding,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostTradeExhaustScavengingConfig {
    pub enabled: bool,
    pub route_mode: ExhaustScavengingRouteMode,
    /// Maximum capital deployment per scavenging attempt, in SOL.
    pub max_capital_deployment_sol: f64,
    /// Minimum upstream competitor trade size (SOL) to trigger scavenging.
    #[serde(default = "default_min_competitor_size_sol")]
    pub min_competitor_size_sol: f64,
    /// Minimum post-trade convexity distortion (bps) to trigger scavenging.
    #[serde(default = "default_min_convexity_distortion_bps")]
    pub min_convexity_distortion_bps: u64,
    /// Conservative estimate of pool single-side reserve (SOL) used to compute the
    /// convexity-bps proxy when live reserve data is unavailable. A 500 SOL default
    /// covers mid-size AMM pools. Tune lower for smaller or concentrated-liquidity pools.
    #[serde(default = "default_estimated_pool_reserve_sol")]
    pub estimated_pool_reserve_sol: f64,
}

fn default_min_competitor_size_sol() -> f64 {
    50.0
}
fn default_min_convexity_distortion_bps() -> u64 {
    600
}
fn default_estimated_pool_reserve_sol() -> f64 {
    500.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExhaustScavengingRouteMode {
    DeterministicJitoBackrunOnly,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TickBoundarySabotageConfig {
    pub enabled: bool,
    pub mode: TickBoundarySabotageMode,
    /// Proximity threshold in lamports. When dist_to_tick_lamports <= this
    /// value, the shadow event fires.
    #[serde(default = "default_tick_proximity_threshold_lamports")]
    pub proximity_threshold_lamports: u64,
}

fn default_tick_proximity_threshold_lamports() -> u64 {
    100_000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TickBoundarySabotageMode {
    ShadowObserveOnly,
}

// ---------------------------------------------------------------------------
// entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct EntryConfig {
    #[serde(default)]
    pub pool_slot_dominance_gate: Option<PoolSlotDominanceGateConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PoolSlotDominanceGateConfig {
    pub enabled: bool,
    pub mode: PoolSlotDominanceMode,
    pub require_same_base_state_ts: bool,
    pub min_incremental_retained_lcb_lamports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolSlotDominanceMode {
    ObserveOnly,
    Live,
}

// ---------------------------------------------------------------------------
// execution
// ---------------------------------------------------------------------------

/// Execution-phase configuration for the money worker pipeline.
///
/// Provides paths to the signing keypair, TipLut binary files, swap template
/// parameters, guard program/vault pubkeys, and the Jito tip account.
/// All pubkey fields are base58-encoded Solana addresses.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionConfig {
    /// Path to a Solana-style JSON keypair file (64-byte array: 32 secret + 32 pubkey).
    /// Used as the fee-payer / transaction signer for all worker threads.
    pub signer_keypair_path: String,

    /// Path to the Jito TipLut binary file (16 MiB: 256 leader classes × 16384 × u32).
    #[serde(default)]
    pub jito_lut_path: Option<String>,

    /// Path to the Harmonic TipLut binary file (same format).
    #[serde(default)]
    pub harmonic_lut_path: Option<String>,

    /// Raydium V4 AMM program ID (base58).  Default: Raydium V4 mainnet.
    #[serde(default = "default_raydium_v4_program_id")]
    pub swap_program_id: String,

    /// ShadowGuard program ID (base58).
    pub shadow_guard_program_id: String,

    /// ShadowGuard vault-in account (base58).
    pub guard_vault_in_pubkey: String,

    /// ShadowGuard vault-out account (base58).
    pub guard_vault_out_pubkey: String,

    /// Jito tip account for the current epoch (base58).
    /// Rotated externally; the runtime reads once at startup.
    pub jito_tip_account: String,

    /// Base compute-unit limit per route.  Defaults to 300,000.
    #[serde(default = "default_route_base_cu_limit")]
    pub route_base_cu_limit: u32,

    /// Maximum number of nonce tuples pre-seeded per worker at startup.
    /// Ring capacity is always 128; this caps how many are filled from the
    /// signer keypair.  Defaults to 128.
    #[serde(default = "default_nonce_prefill")]
    pub nonce_prefill: usize,

    /// Fixed swap template: accounts for the DEX swap instruction.
    /// Each entry is `{ pubkey: "<base58>" | "FeePayerSigner", writable: bool }`.
    #[serde(default)]
    pub swap_template_accounts: Vec<SwapTemplateAccountEntry>,

    /// Fixed swap instruction data prefix as hex string (must include discriminator).
    /// Bytes [1..9] and [9..17] are overwritten at build time with trade_size / min_out.
    #[serde(default)]
    pub swap_ix_data_hex: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SwapTemplateAccountEntry {
    pub pubkey: String,
    pub writable: bool,
}

fn default_raydium_v4_program_id() -> String {
    // Raydium V4 AMM program on mainnet.
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8".to_string()
}
fn default_route_base_cu_limit() -> u32 {
    300_000
}
fn default_nonce_prefill() -> usize {
    127
}

// ---------------------------------------------------------------------------
// rollout
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RolloutConfig {
    pub mode: RolloutMode,
    pub no_send_backend: bool,
    #[serde(default)]
    pub sh8_afxdp_source_probe_mode: Sh8AfxdpSourceProbeMode,
    #[serde(default)]
    pub sh8_afxdp_allow_local_replay_stale_slots: bool,
    #[serde(default)]
    pub r0_allow_geyser_observe_shadow: bool,
    pub cap_rung: RolloutCapRung,
    pub max_live_trade_input_sol: f64,
    pub max_position_sol_per_mint: f64,
    pub max_notional_sol_per_entity: f64,
    pub require_shadow_guard_deployed: bool,
    #[serde(default)]
    pub shadow_guard_program_id: Option<String>,
    pub require_all_gates_green: bool,
    #[serde(default)]
    pub require_public_scout_gates_green: bool,
    #[serde(default)]
    pub gate_report_path: Option<String>,
    pub live_bid_shading_allowed: bool,
    pub dominance_live_allowed: bool,
    pub competition_adjusted_primary_allowed: bool,
    pub min_observe_candidates: u64,
    pub min_live_landed: u64,
    pub min_sample_slots: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutMode {
    Disabled,
    ObserveOnly,
    TinyLive,
    ScaledLive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutCapRung {
    R0ObserveOnly,
    R1Tiny,
    R2Small,
    R3HalfGuerrila,
    R4FullGuerrila,
}

// ---------------------------------------------------------------------------
// r1
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct R1Config {
    #[serde(default)]
    pub live_admission: R1LiveAdmissionConfig,
    #[serde(default)]
    pub public_scout: R1PublicScoutConfig,
    #[serde(default)]
    pub denylist: R1DenylistConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct R1LiveAdmissionConfig {
    #[serde(default)]
    pub mode: R1LiveAdmissionMode,
    #[serde(default)]
    pub source_required: R1AdmissionSourceRequired,
    #[serde(default = "default_true")]
    pub require_private_grade: bool,
    #[serde(default = "default_true")]
    pub require_shadow_guard: bool,
    #[serde(default = "default_true")]
    pub require_micro_market_uplift: bool,
    #[serde(default = "default_true")]
    pub require_pre_sign_egress_admission: bool,
    #[serde(default = "default_r1_max_live_trade_input_sol")]
    pub max_live_trade_input_sol: f64,
    #[serde(default = "default_r1_max_position_sol_per_mint")]
    pub max_position_sol_per_mint: f64,
    #[serde(default = "default_r1_max_notional_sol_per_entity")]
    pub max_notional_sol_per_entity: f64,
    #[serde(default = "default_r1_max_new_positions_per_window")]
    pub max_new_positions_per_window: u32,
    #[serde(default = "default_r1_window_seconds")]
    pub window_seconds: u64,
    #[serde(default = "default_true")]
    pub kill_on_health_drop: bool,
    #[serde(default = "default_true")]
    pub kill_on_fee_leak: bool,
    #[serde(default = "default_true")]
    pub kill_on_inventory_mismatch: bool,
    #[serde(default)]
    pub min_retained_ev_lamports: u64,
    #[serde(default)]
    pub min_net_after_fee_lamports: u64,
}

impl Default for R1LiveAdmissionConfig {
    fn default() -> Self {
        Self {
            mode: R1LiveAdmissionMode::AutoPrivateGrade,
            source_required: R1AdmissionSourceRequired::Afxdp,
            require_private_grade: true,
            require_shadow_guard: true,
            require_micro_market_uplift: true,
            require_pre_sign_egress_admission: true,
            max_live_trade_input_sol: default_r1_max_live_trade_input_sol(),
            max_position_sol_per_mint: default_r1_max_position_sol_per_mint(),
            max_notional_sol_per_entity: default_r1_max_notional_sol_per_entity(),
            max_new_positions_per_window: default_r1_max_new_positions_per_window(),
            window_seconds: default_r1_window_seconds(),
            kill_on_health_drop: true,
            kill_on_fee_leak: true,
            kill_on_inventory_mismatch: true,
            min_retained_ev_lamports: 0,
            min_net_after_fee_lamports: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum R1LiveAdmissionMode {
    ManualAllowlist,
    #[default]
    AutoPrivateGrade,
    PassAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum R1AdmissionSourceRequired {
    #[default]
    Afxdp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct R1PublicScoutConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: R1PublicScoutMode,
    #[serde(default = "default_r1_public_max_live_trade_input_sol")]
    pub max_live_trade_input_sol: f64,
    #[serde(default = "default_r1_public_max_notional_sol_per_entity")]
    pub max_notional_sol_per_entity: f64,
    #[serde(default = "default_true")]
    pub require_afxdp_source: bool,
    #[serde(default = "default_true")]
    pub require_shadow_guard: bool,
    #[serde(default = "default_true")]
    pub kill_on_health_drop: bool,
    #[serde(default = "default_true")]
    pub kill_on_fee_leak: bool,
    #[serde(default = "default_true")]
    pub kill_on_inventory_mismatch: bool,
    #[serde(default = "default_r1_public_max_scout_attempts_per_window")]
    pub max_scout_attempts_per_window: u32,
    #[serde(default = "default_r1_window_seconds")]
    pub window_seconds: u64,
}

impl Default for R1PublicScoutConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: R1PublicScoutMode::Disabled,
            max_live_trade_input_sol: default_r1_public_max_live_trade_input_sol(),
            max_notional_sol_per_entity: default_r1_public_max_notional_sol_per_entity(),
            require_afxdp_source: true,
            require_shadow_guard: true,
            kill_on_health_drop: true,
            kill_on_fee_leak: true,
            kill_on_inventory_mismatch: true,
            max_scout_attempts_per_window: default_r1_public_max_scout_attempts_per_window(),
            window_seconds: default_r1_window_seconds(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum R1PublicScoutMode {
    #[default]
    Disabled,
    AutoAdmission,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct R1DenylistConfig {
    #[serde(default)]
    pub wallets: Vec<String>,
    #[serde(default)]
    pub mints: Vec<String>,
    #[serde(default)]
    pub pools: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_r1_max_live_trade_input_sol() -> f64 {
    1.0
}

fn default_r1_max_position_sol_per_mint() -> f64 {
    1.0
}

fn default_r1_max_notional_sol_per_entity() -> f64 {
    2.0
}

fn default_r1_max_new_positions_per_window() -> u32 {
    1
}

fn default_r1_window_seconds() -> u64 {
    1800
}

fn default_r1_public_max_live_trade_input_sol() -> f64 {
    0.05
}

fn default_r1_public_max_notional_sol_per_entity() -> f64 {
    0.1
}

fn default_r1_public_max_scout_attempts_per_window() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// pm (position manager)
// ---------------------------------------------------------------------------

/// PM evidence plane configuration.
///
/// Controls which RPC endpoints the PM thread uses for on-chain signature
/// polling and bundle status polling.
#[derive(Debug, Clone, Deserialize)]
pub struct PmConfig {
    /// Solana RPC HTTP URL for `getSignatureStatuses`.
    pub solana_rpc_url: String,

    /// Jito block-engine REST URL for `getBundleStatuses`.
    /// e.g. `https://mainnet.block-engine.jito.wtf/api/v1/bundles`
    #[serde(default)]
    pub jito_bundle_status_url: Option<String>,

    /// Harmonic block-engine REST URL for `getBundleStatuses`.
    /// Same protocol as Jito; one RpcBundleStatusSource instance per lane.
    #[serde(default)]
    pub harmonic_bundle_status_url: Option<String>,

    /// Timeout in milliseconds for RPC calls (default: 5000).
    #[serde(default = "default_pm_rpc_timeout_ms")]
    pub rpc_timeout_ms: u64,

    /// Optional JSONL export path for PM-side shock/rollout research tape.
    /// Empty or omitted disables the recorder.
    #[serde(default)]
    pub shock_research_export_path: Option<String>,
}

fn default_pm_rpc_timeout_ms() -> u64 {
    5_000
}

// ---------------------------------------------------------------------------
// telemetry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub observe_only: Option<ObserveOnlyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObserveOnlyConfig {
    #[serde(default)]
    pub log_drop_reasons: Option<bool>,
    #[serde(default)]
    pub shadow_pnl_enabled: Option<bool>,
}

// ---------------------------------------------------------------------------
// shadow_control
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowControlPlaneConfig {
    #[serde(default)]
    pub rosetta: Option<RosettaConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RosettaConfig {
    pub enabled: bool,
    pub manifest_path: String,
    pub control_flags_path: String,
    pub raw_archive_path: String,
    pub golden_records_path: String,
    pub manifest_read_interval_shreds: u32,
    pub tripwire_window_slots: u64,
    pub grpc_only_cooldown_slots: u64,
    pub blindness_miss_threshold: u32,
    pub predictive_shift_enabled: bool,
    pub predictive_shift_max_abs_delta: u16,
    pub deep_scan_enabled: bool,
    pub executable_hash_monitor_enabled: bool,
    pub venue_quarantine_enabled: bool,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl ShadowConfig {
    /// Load config from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    /// Load config from a YAML file path.
    pub fn from_file(path: &std::path::Path) -> Result<Self, ConfigLoadError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigLoadError::Io(path.display().to_string(), e))?;
        let cfg = Self::from_yaml(&contents)
            .map_err(|e| ConfigLoadError::Parse(path.display().to_string(), e))?;
        Ok(cfg)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadError {
    #[error("failed to read config file '{0}': {1}")]
    Io(String, std::io::Error),
    #[error("failed to parse config file '{0}': {1}")]
    Parse(String, serde_yaml::Error),
}
