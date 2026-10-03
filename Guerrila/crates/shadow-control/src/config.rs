use crate::rollout_gate::default_gate_report_path;
use clap::Parser;
use shadow_ipc::control_flags::ShadowControlFlagsV1;
use shadow_ipc::parsing_manifest::ParsingManifestSnapshot;
use shadow_ipc::raw_archive::raw_archive_segment_path;
use shadow_ipc::segment::SegmentSpec;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "shadow-control", about = "Shadow Engine G11 control plane")]
pub struct Cli {
    #[arg(long, default_value_t = false)]
    pub once: bool,

    #[arg(long, default_value_t = 250)]
    pub sleep_ms: u64,

    #[arg(long, default_value_t = true)]
    pub rosetta_enabled: bool,

    #[arg(long, default_value_t = 64)]
    pub manifest_read_interval_shreds: u32,

    #[arg(long, default_value_t = 2)]
    pub tripwire_window_slots: u64,

    #[arg(long, default_value_t = 1)]
    pub blindness_miss_threshold: u32,

    #[arg(long, default_value_t = 2)]
    pub golden_quorum: usize,

    #[arg(long, value_name = "PATH")]
    pub golden_dir: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub golden_records_path: Option<PathBuf>,

    #[arg(long, default_value_t = true)]
    pub predictive_shift_enabled: bool,

    #[arg(long, default_value_t = 16)]
    pub predictive_shift_max_abs_delta: u16,

    #[arg(long, default_value_t = true)]
    pub deep_scan_enabled: bool,

    #[arg(long, default_value_t = true)]
    pub executable_hash_monitor_enabled: bool,

    #[arg(long, default_value_t = true)]
    pub venue_quarantine_enabled: bool,

    #[arg(long, value_name = "PATH")]
    pub raw_archive_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub parsing_manifest_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub control_flags_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub umem_path: Option<PathBuf>,

    #[arg(long, default_value_t = 2_097_152)]
    pub umem_frames: u32,

    #[arg(long, default_value_t = 2048)]
    pub umem_frame_size: u32,

    #[arg(long, default_value_t = 1_000_000_000)]
    pub max_sample_window_ns: u64,

    #[arg(long, default_value_t = 8)]
    pub grpc_only_cooldown_slots: u64,

    #[arg(long, value_name = "PATH")]
    pub rollout_gate_input_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub rollout_gate_output_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ShadowControlConfig {
    pub once: bool,
    pub sleep_ms: u64,
    pub rosetta_enabled: bool,
    pub manifest_read_interval_shreds: u32,
    pub tripwire_window_slots: u64,
    pub blindness_miss_threshold: u32,
    pub golden_quorum: usize,
    pub golden_records_path: PathBuf,
    pub predictive_shift_enabled: bool,
    pub predictive_shift_max_abs_delta: u16,
    pub deep_scan_enabled: bool,
    pub executable_hash_monitor_enabled: bool,
    pub venue_quarantine_enabled: bool,
    pub raw_archive_path: PathBuf,
    pub parsing_manifest_path: PathBuf,
    pub control_flags_path: PathBuf,
    pub umem_path: PathBuf,
    pub umem_frames: u32,
    pub umem_frame_size: u32,
    pub max_sample_window_ns: u64,
    pub grpc_only_cooldown_slots: u64,
    pub rollout_gate_input_path: Option<PathBuf>,
    pub rollout_gate_output_path: PathBuf,
}

impl From<Cli> for ShadowControlConfig {
    fn from(value: Cli) -> Self {
        let golden_records_path = value
            .golden_records_path
            .clone()
            .or_else(|| value.golden_dir.clone())
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("testdata")
                    .join("golden")
            });
        Self {
            once: value.once,
            sleep_ms: value.sleep_ms,
            rosetta_enabled: value.rosetta_enabled,
            manifest_read_interval_shreds: value.manifest_read_interval_shreds,
            tripwire_window_slots: value.tripwire_window_slots,
            blindness_miss_threshold: value.blindness_miss_threshold,
            golden_quorum: value.golden_quorum,
            golden_records_path,
            predictive_shift_enabled: value.predictive_shift_enabled,
            predictive_shift_max_abs_delta: value.predictive_shift_max_abs_delta,
            deep_scan_enabled: value.deep_scan_enabled,
            executable_hash_monitor_enabled: value.executable_hash_monitor_enabled,
            venue_quarantine_enabled: value.venue_quarantine_enabled,
            raw_archive_path: value
                .raw_archive_path
                .unwrap_or_else(raw_archive_segment_path),
            parsing_manifest_path: value
                .parsing_manifest_path
                .unwrap_or_else(|| PathBuf::from(<ParsingManifestSnapshot as SegmentSpec>::PATH)),
            control_flags_path: value
                .control_flags_path
                .unwrap_or_else(|| PathBuf::from(<ShadowControlFlagsV1 as SegmentSpec>::PATH)),
            umem_path: value
                .umem_path
                .unwrap_or_else(|| PathBuf::from("/dev/hugepages/shadow_umem_0")),
            umem_frames: value.umem_frames,
            umem_frame_size: value.umem_frame_size,
            max_sample_window_ns: value.max_sample_window_ns,
            grpc_only_cooldown_slots: value.grpc_only_cooldown_slots,
            rollout_gate_input_path: value.rollout_gate_input_path,
            rollout_gate_output_path: value
                .rollout_gate_output_path
                .unwrap_or_else(default_gate_report_path),
        }
    }
}
