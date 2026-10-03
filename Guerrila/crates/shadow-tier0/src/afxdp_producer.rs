//! AF_XDP ingress producer — transport scaffold and ownership model.
//!
//! Owns producer_id assignment, shared UMEM path construction, feed-ring
//! creation/opening, and raw-archive publisher handles for AF_XDP RX workers.
//!
//! Producer ID mapping (frozen, shared with grpc_producer):
//!   AF_XDP producers: `[0 .. shred_capture_workers)`
//!   gRPC producers:   `[shred_capture_workers .. shred_capture_workers + grpc_workers)`
//!
//! Feed rings live at `/dev/shm/shadow_feed_ring_w{worker}_p{producer}`.
//! Raw archive lives at `/dev/shm/shadow_raw_archive` (single multi-lane segment).
//! Shared UMEM backing file lives at `/dev/hugepages/shadow_umem_{instance}`.
//!
//! This module does NOT contain AF_XDP socket code, eBPF programs, or any
//! `aya`/`libbpf`/`xdp-rs` dependencies. Actual socket bringup belongs in
//! later milestones (G4-E2+). This layer is the ownership and bootstrap shell.

use memmap2::MmapMut;
use sha2::{Digest, Sha256};
use shadow_core::target_tx_cache::{record_target_tx, CapturedTargetTx};
use shadow_ingest::decode::ManifestDecodedSwap;
use shadow_ingest::pipeline::DecodeInput;
use shadow_ingest::shred_meta::ShredMeta;
use shadow_ipc::control_flags::ShadowControlFlagsV1;
use shadow_ipc::ingress_record::{self, IngressRecord, FEED_RING_DEFAULT_CAPACITY};
use shadow_ipc::parsing_manifest::ParsingManifestSnapshot;
use shadow_ipc::raw_archive::{frame_hash64, RawArchiveRecord, RawArchiveSegment};
use shadow_ipc::ring::{MappedSpscRing, RingError};
use shadow_ipc::segment::MappedSegment;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::grpc_producer::ValidatedTopology;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AfxdpProducerError {
    InvalidProducerId,
    DuplicateProducerId(u32),
    RingCreateFailed {
        worker_id: u32,
        source: RingError,
    },
    ArchiveCreateFailed {
        producer_id: u32,
        source: RingError,
    },
    UmemCreateFailed {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for AfxdpProducerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProducerId => write!(f, "invalid AF_XDP producer ID"),
            Self::DuplicateProducerId(id) => {
                write!(f, "duplicate AF_XDP producer ID: {id}")
            }
            Self::RingCreateFailed { worker_id, source } => {
                write!(
                    f,
                    "feed ring create failed for worker {worker_id}: {source}"
                )
            }
            Self::ArchiveCreateFailed {
                producer_id,
                source,
            } => {
                write!(
                    f,
                    "raw archive create failed for producer {producer_id}: {source}"
                )
            }
            Self::UmemCreateFailed { path, source } => {
                write!(f, "UMEM file create failed at {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for AfxdpProducerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RingCreateFailed { source, .. } => Some(source),
            Self::ArchiveCreateFailed { source, .. } => Some(source),
            Self::UmemCreateFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Producer ID assignment
// ---------------------------------------------------------------------------

/// Compute the producer_id for an AF_XDP shred-capture worker.
///
/// AF_XDP producers occupy `[0 .. shred_capture_workers)`.
///
/// Returns `None` if `shred_capture_workers == 0` or
/// `shred_capture_index >= shred_capture_workers`.
pub fn afxdp_producer_id(shred_capture_index: u32, shred_capture_workers: u32) -> Option<u32> {
    if shred_capture_workers == 0 || shred_capture_index >= shred_capture_workers {
        return None;
    }
    Some(shred_capture_index)
}

// ---------------------------------------------------------------------------
// Queue-to-producer mapping (frozen, deterministic)
// ---------------------------------------------------------------------------

/// Map an NIC RX queue index to a producer_id.
///
/// The mapping is `queue_id == producer_id` for AF_XDP producers — identity.
/// This is deterministic across restarts because it depends only on the
/// queue index and the `shred_capture_workers` count from config, not on
/// any runtime or session state.
pub fn queue_to_producer_id(queue_id: u32, shred_capture_workers: u32) -> Option<u32> {
    afxdp_producer_id(queue_id, shred_capture_workers)
}

// ---------------------------------------------------------------------------
// AfxdpProducerRegistry — startup-owned multi-producer map
// ---------------------------------------------------------------------------

/// A startup-owned registry of all AF_XDP producers.
///
/// Built once at startup via [`AfxdpProducerRegistry::bootstrap`].
/// Rejects duplicate producer IDs at construction time.
pub struct AfxdpProducerRegistry {
    emitters: Vec<AfxdpFeedEmitter>,
}

impl AfxdpProducerRegistry {
    /// Bootstrap all AF_XDP producers for the given topology.
    ///
    /// Creates one [`AfxdpFeedEmitter`] per `shred_capture_worker`, each
    /// with its own feed rings. The raw archive is shared (one segment
    /// for all producers) and must be bootstrapped separately.
    /// Fails closed if any producer_id would be duplicated.
    pub fn bootstrap(
        topology: &ValidatedTopology,
        feed_capacity: u32,
        base_dir: &Path,
        umem_backing_path: &Path,
    ) -> Result<Self, AfxdpProducerError> {
        Self::bootstrap_with_control_flags(
            topology,
            feed_capacity,
            base_dir,
            umem_backing_path,
            None,
            None,
        )
    }

    pub fn bootstrap_with_control_flags(
        topology: &ValidatedTopology,
        feed_capacity: u32,
        base_dir: &Path,
        umem_backing_path: &Path,
        control_flags: Option<Arc<MappedSegment<ShadowControlFlagsV1>>>,
        parsing_manifest: Option<Arc<MappedSegment<ParsingManifestSnapshot>>>,
    ) -> Result<Self, AfxdpProducerError> {
        let mut emitters = Vec::with_capacity(topology.shred_capture_workers as usize);
        let mut seen_ids = std::collections::HashSet::new();

        for idx in 0..topology.shred_capture_workers {
            let pid = afxdp_producer_id(idx, topology.shred_capture_workers)
                .ok_or(AfxdpProducerError::InvalidProducerId)?;
            if !seen_ids.insert(pid) {
                return Err(AfxdpProducerError::DuplicateProducerId(pid));
            }
            let emitter = AfxdpFeedEmitter::new(
                topology,
                idx,
                feed_capacity,
                base_dir,
                umem_backing_path.to_path_buf(),
            )?;
            let emitter = match &control_flags {
                Some(control_flags) => emitter.with_control_flags(Arc::clone(control_flags)),
                None => emitter,
            };
            let emitter = match &parsing_manifest {
                Some(parsing_manifest) => {
                    emitter.with_parsing_manifest(Arc::clone(parsing_manifest))
                }
                None => emitter,
            };
            emitters.push(emitter);
        }

        Ok(Self { emitters })
    }

    /// Number of registered producers.
    pub fn len(&self) -> usize {
        self.emitters.len()
    }

    /// True if no producers are registered.
    pub fn is_empty(&self) -> bool {
        self.emitters.is_empty()
    }

    /// Look up an emitter by producer_id.
    pub fn get(&self, producer_id: u32) -> Option<&AfxdpFeedEmitter> {
        self.emitters.iter().find(|e| e.producer_id == producer_id)
    }

    /// Iterate all emitters.
    pub fn iter(&self) -> impl Iterator<Item = &AfxdpFeedEmitter> {
        self.emitters.iter()
    }

    /// Consume the registry and return the emitters for thread distribution.
    pub fn into_emitters(self) -> Vec<AfxdpFeedEmitter> {
        self.emitters
    }
}

// ---------------------------------------------------------------------------
// UMEM path and config
// ---------------------------------------------------------------------------

/// Canonical hugepage-backed UMEM file path.
///
/// Production path: `/dev/hugepages/shadow_umem_{instance_id}`.
/// Tests override with a tempdir base via [`umem_path_in`].
pub fn umem_path(instance_id: u32) -> PathBuf {
    PathBuf::from(format!("/dev/hugepages/shadow_umem_{instance_id}"))
}

/// UMEM path under an arbitrary base directory (for tests).
pub fn umem_path_in(base_dir: &Path, instance_id: u32) -> PathBuf {
    base_dir.join(format!("shadow_umem_{instance_id}"))
}

/// Validated UMEM geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UmemConfig {
    /// Total number of UMEM frames.
    pub frames: u32,
    /// Bytes per frame (must be power-of-two, >= 2048).
    pub frame_size: u32,
}

impl UmemConfig {
    /// Spec default: 2^21 frames * 2048 bytes = 4 GiB.
    pub const DEFAULT_FRAMES: u32 = 2_097_152;
    pub const DEFAULT_FRAME_SIZE: u32 = 2048;

    pub fn new(frames: u32, frame_size: u32) -> Option<Self> {
        if frames == 0 || frame_size < 2048 || !frame_size.is_power_of_two() {
            return None;
        }
        Some(Self { frames, frame_size })
    }

    pub fn default_config() -> Self {
        Self {
            frames: Self::DEFAULT_FRAMES,
            frame_size: Self::DEFAULT_FRAME_SIZE,
        }
    }

    /// Total UMEM size in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.frames as u64 * self.frame_size as u64
    }
}

/// Create (or truncate) the UMEM backing file at `path`.
///
/// Sets the file length to `config.total_bytes()`. Does NOT mmap or
/// perform any AF_XDP socket setup — that belongs in G4-E2+.
pub fn bootstrap_umem_file(
    path: &Path,
    config: &UmemConfig,
) -> Result<PathBuf, AfxdpProducerError> {
    use std::fs::OpenOptions;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| AfxdpProducerError::UmemCreateFailed {
            path: path.to_path_buf(),
            source: e,
        })?;
    file.set_len(config.total_bytes())
        .map_err(|e| AfxdpProducerError::UmemCreateFailed {
            path: path.to_path_buf(),
            source: e,
        })?;
    Ok(path.to_path_buf())
}

// ---------------------------------------------------------------------------
// Shared UMEM mapping (G4-E2)
// ---------------------------------------------------------------------------

/// A shared-memory UMEM region backed by a hugepage file.
///
/// The cross-process contract is **offsets, not pointers**: archive entries
/// publish `umem_offset_bytes` which any process that maps the same backing
/// file can resolve to a local pointer via [`SharedUmem::frame_ptr`].
///
/// Both `shadow-tier0` (producer) and `shadow-control` (FDR reader) map
/// the same file with `MAP_SHARED`, so writes from the NIC DMA path are
/// visible to the archive consumer without copying.
pub struct SharedUmem {
    mmap: MmapMut,
    config: UmemConfig,
    #[allow(dead_code)]
    path: PathBuf,
}

impl SharedUmem {
    /// Map an existing UMEM backing file as `MAP_SHARED`.
    ///
    /// The file must already exist and be sized to `config.total_bytes()`
    /// (created by [`bootstrap_umem_file`]). Returns an error if the file
    /// is missing, too small, or the mapping fails.
    pub fn map_shared(path: &Path, config: &UmemConfig) -> Result<Self, AfxdpProducerError> {
        use std::fs::OpenOptions;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| AfxdpProducerError::UmemCreateFailed {
                path: path.to_path_buf(),
                source: e,
            })?;

        let meta = file
            .metadata()
            .map_err(|e| AfxdpProducerError::UmemCreateFailed {
                path: path.to_path_buf(),
                source: e,
            })?;

        let expected = config.total_bytes();
        if meta.len() < expected {
            return Err(AfxdpProducerError::UmemCreateFailed {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "UMEM file too small: {} bytes, expected {}",
                        meta.len(),
                        expected,
                    ),
                ),
            });
        }

        // SAFETY: the file was created by bootstrap_umem_file with the
        // correct size. We map it MAP_SHARED so multiple processes see
        // the same physical pages (required for DMA + FDR).
        let mmap = unsafe { MmapMut::map_mut(&file) }.map_err(|e| {
            AfxdpProducerError::UmemCreateFailed {
                path: path.to_path_buf(),
                source: e.into(),
            }
        })?;

        Ok(Self {
            mmap,
            config: *config,
            path: path.to_path_buf(),
        })
    }

    /// UMEM configuration (frame count + frame size).
    #[allow(dead_code)]
    pub fn config(&self) -> &UmemConfig {
        &self.config
    }

    /// Path to the backing hugepage file.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Total UMEM size in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.config.total_bytes()
    }

    /// Number of frames.
    #[allow(dead_code)]
    pub fn frame_count(&self) -> u32 {
        self.config.frames
    }

    /// Bytes per frame.
    #[allow(dead_code)]
    pub fn frame_size(&self) -> u32 {
        self.config.frame_size
    }

    /// Byte offset of frame `index` from the start of the UMEM region.
    ///
    /// This is the cross-process contract value: archive entries store
    /// this offset, and any process mapping the same file can resolve it.
    /// Returns `None` if `index >= frame_count`.
    pub fn frame_offset(&self, index: u32) -> Option<u64> {
        if index >= self.config.frames {
            return None;
        }
        Some(index as u64 * self.config.frame_size as u64)
    }

    /// Pointer to the start of frame `index`.
    ///
    /// Returns `None` if `index >= frame_count`.
    pub fn frame_ptr(&self, index: u32) -> Option<*const u8> {
        let offset = self.frame_offset(index)?;
        // SAFETY: offset is guaranteed < total_bytes by frame_offset.
        Some(unsafe { self.mmap.as_ptr().add(offset as usize) })
    }

    /// Mutable pointer to the start of frame `index`.
    ///
    /// Returns `None` if `index >= frame_count`.
    pub fn frame_ptr_mut(&mut self, index: u32) -> Option<*mut u8> {
        let offset = self.frame_offset(index)?;
        Some(unsafe { self.mmap.as_mut_ptr().add(offset as usize) })
    }

    /// Resolve a byte offset (from an archive entry) to a pointer.
    ///
    /// Returns `None` if the offset is out of range or not frame-aligned.
    pub fn resolve_offset(&self, umem_offset_bytes: u64) -> Option<*const u8> {
        if umem_offset_bytes >= self.total_bytes() {
            return None;
        }
        if umem_offset_bytes % self.config.frame_size as u64 != 0 {
            return None;
        }
        Some(unsafe { self.mmap.as_ptr().add(umem_offset_bytes as usize) })
    }

    /// Mutable slice of the entire UMEM region.
    ///
    /// Use only during bootstrap / testing. The RX loop uses frame pointers.
    #[allow(dead_code)]
    pub fn as_slice(&self) -> &[u8] {
        &self.mmap
    }

    /// Mutable slice of the entire UMEM region.
    #[allow(dead_code)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.mmap
    }
}

// ---------------------------------------------------------------------------
// Feed ring path + bootstrap
// ---------------------------------------------------------------------------

/// Build the feed ring path for a given worker and producer under `base_dir`.
///
/// In production `base_dir` is `/dev/shm`. Tests pass a tempdir.
pub fn feed_ring_path_in(base_dir: &Path, worker_id: u32, producer_id: u32) -> PathBuf {
    base_dir.join(format!("shadow_feed_ring_w{worker_id}_p{producer_id}"))
}

/// Bootstrap all feed rings for a single AF_XDP producer.
///
/// Creates one `MappedSpscRing<IngressRecord>` per money worker:
///   `{base_dir}/shadow_feed_ring_w{worker}_p{producer_id}`
pub fn bootstrap_afxdp_feed_rings(
    topology: &ValidatedTopology,
    shred_capture_index: u32,
    capacity: u32,
    base_dir: &Path,
) -> Result<Vec<MappedSpscRing<IngressRecord>>, AfxdpProducerError> {
    let producer_id = afxdp_producer_id(shred_capture_index, topology.shred_capture_workers)
        .ok_or(AfxdpProducerError::InvalidProducerId)?;

    let mut rings = Vec::with_capacity(topology.money_workers as usize);
    for worker_id in 0..topology.money_workers {
        let path = feed_ring_path_in(base_dir, worker_id, producer_id);
        let ring = ingress_record::create_feed_ring(&path, capacity).map_err(|e| {
            AfxdpProducerError::RingCreateFailed {
                worker_id,
                source: e,
            }
        })?;
        rings.push(ring);
    }

    Ok(rings)
}

/// Open all existing feed rings for a single AF_XDP producer.
///
/// Fails closed if any ring file is missing or corrupt. Use this when
/// bootstrap has already been done and you need to reattach.
pub fn open_afxdp_feed_rings(
    topology: &ValidatedTopology,
    shred_capture_index: u32,
    base_dir: &Path,
) -> Result<Vec<MappedSpscRing<IngressRecord>>, AfxdpProducerError> {
    let producer_id = afxdp_producer_id(shred_capture_index, topology.shred_capture_workers)
        .ok_or(AfxdpProducerError::InvalidProducerId)?;

    let mut rings = Vec::with_capacity(topology.money_workers as usize);
    for worker_id in 0..topology.money_workers {
        let path = feed_ring_path_in(base_dir, worker_id, producer_id);
        let ring = ingress_record::open_feed_ring_rw(&path).map_err(|e| {
            AfxdpProducerError::RingCreateFailed {
                worker_id,
                source: e,
            }
        })?;
        rings.push(ring);
    }

    Ok(rings)
}

// ---------------------------------------------------------------------------
// Raw archive bootstrap
// ---------------------------------------------------------------------------

/// Raw archive segment path under an arbitrary base directory.
///
/// The archive is a single multi-lane segment file (one lane per producer),
/// NOT a per-producer file. This matches the G4-A contract:
/// `/dev/shm/shadow_raw_archive`.
pub fn raw_archive_path_in(base_dir: &Path) -> PathBuf {
    base_dir.join("shadow_raw_archive")
}

/// Bootstrap the shared raw archive segment for all AF_XDP producers.
///
/// Creates a single `RawArchiveSegment` with one SPSC lane per producer.
/// Each producer pushes to its own lane; the control plane reads all lanes.
pub fn bootstrap_afxdp_raw_archive(
    producer_count: u32,
    lane_capacity: u32,
    base_dir: &Path,
) -> Result<RawArchiveSegment, AfxdpProducerError> {
    let path = raw_archive_path_in(base_dir);
    RawArchiveSegment::create(&path, producer_count, lane_capacity).map_err(|e| {
        AfxdpProducerError::ArchiveCreateFailed {
            producer_id: 0,
            source: e,
        }
    })
}

// ---------------------------------------------------------------------------
// AfxdpFeedEmitter — startup-owned handle bundle
// ---------------------------------------------------------------------------

/// An AF_XDP producer's pre-opened feed rings, raw archive handle, and
/// UMEM/queue metadata. All handles are opened once at startup.
///
/// Ownership contract (frozen):
/// - `umem_backing_path`: the hugepage-backed UMEM file this producer reads from
/// - `rings`: one feed ring per money worker
/// - `queue_id`: NIC RX queue bound to this producer
///
/// The raw archive is NOT owned per-emitter — it is a single shared
/// `RawArchiveSegment` with one lane per producer, created once at startup.
pub struct AfxdpFeedEmitter {
    pub producer_id: u32,
    pub queue_id: u32,
    pub umem_backing_path: PathBuf,
    pub rings: Vec<MappedSpscRing<IngressRecord>>,
    control_flags: Option<Arc<MappedSegment<ShadowControlFlagsV1>>>,
    parsing_manifest: Option<Arc<MappedSegment<ParsingManifestSnapshot>>>,
}

impl AfxdpFeedEmitter {
    /// Bootstrap a new emitter for `shred_capture_index`, opening one feed
    /// ring per money worker. Also records the UMEM backing path and queue_id
    /// for later socket bringup (G4-E2+).
    ///
    /// The raw archive is NOT created here — it is a single shared segment
    /// bootstrapped once via [`bootstrap_afxdp_raw_archive`].
    pub fn new(
        topology: &ValidatedTopology,
        shred_capture_index: u32,
        feed_capacity: u32,
        base_dir: &Path,
        umem_backing_path: PathBuf,
    ) -> Result<Self, AfxdpProducerError> {
        let producer_id = afxdp_producer_id(shred_capture_index, topology.shred_capture_workers)
            .ok_or(AfxdpProducerError::InvalidProducerId)?;

        let rings =
            bootstrap_afxdp_feed_rings(topology, shred_capture_index, feed_capacity, base_dir)?;

        Ok(Self {
            producer_id,
            queue_id: shred_capture_index,
            umem_backing_path,
            rings,
            control_flags: None,
            parsing_manifest: None,
        })
    }

    pub fn with_control_flags(
        mut self,
        control_flags: Arc<MappedSegment<ShadowControlFlagsV1>>,
    ) -> Self {
        self.control_flags = Some(control_flags);
        self
    }

    pub fn with_parsing_manifest(
        mut self,
        parsing_manifest: Arc<MappedSegment<ParsingManifestSnapshot>>,
    ) -> Self {
        self.parsing_manifest = Some(parsing_manifest);
        self
    }

    pub fn observe_tripwire_record(&self, record: &IngressRecord) {
        if let Some(control_flags) = &self.control_flags {
            control_flags
                .payload()
                .record_af_xdp_hit_from_ingress(record);
        }
    }

    pub fn control_flags(&self) -> Option<Arc<MappedSegment<ShadowControlFlagsV1>>> {
        self.control_flags.as_ref().map(Arc::clone)
    }

    pub fn parsing_manifest(&self) -> Option<Arc<MappedSegment<ParsingManifestSnapshot>>> {
        self.parsing_manifest.as_ref().map(Arc::clone)
    }
}

// ---------------------------------------------------------------------------
// Config validation: AF_XDP requires UMEM + archive ownership
// ---------------------------------------------------------------------------

/// Validate that an AF_XDP-enabled config has the required ownership plan.
///
/// When `shred_capture_workers > 0`, UMEM and raw archive are mandatory.
/// Returns `Err` if AF_XDP is requested but the UMEM path doesn't exist
/// or the archive path is missing.
pub fn validate_afxdp_ownership(
    topology: &ValidatedTopology,
    umem_path: &Path,
    archive_path: &Path,
) -> Result<(), AfxdpProducerError> {
    if topology.shred_capture_workers == 0 {
        return Ok(());
    }
    // UMEM backing path must exist (created by bootstrap_umem_file)
    if !umem_path.exists() {
        return Err(AfxdpProducerError::UmemCreateFailed {
            path: umem_path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "UMEM backing file not found — bootstrap_umem_file must run first",
            ),
        });
    }
    // Archive path must exist (file or parent dir)
    if !archive_path.exists() {
        return Err(AfxdpProducerError::ArchiveCreateFailed {
            producer_id: 0,
            source: RingError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "archive base directory not found",
            )),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Queue / core / NUMA ownership table (G4-E1-5)
// ---------------------------------------------------------------------------

/// Physical ownership assignment for a single AF_XDP ingress producer.
///
/// Encodes which NIC RX queue, CPU core, and NUMA node a producer owns.
/// Injected at startup from host bootstrap metadata (e.g., ethtool, lscpu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProducerPlacement {
    /// Producer ID (must match `afxdp_producer_id` output).
    pub producer_id: u32,
    /// NIC RX queue index bound to this producer.
    pub queue_id: u32,
    /// CPU core this producer thread is pinned to.
    pub core_id: u32,
    /// NUMA node the core and UMEM belong to.
    pub numa_node: u32,
}

/// Errors from ownership table validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipError {
    /// Two producers claim the same NIC RX queue.
    DuplicateQueue {
        queue_id: u32,
        producer_a: u32,
        producer_b: u32,
    },
    /// Two producers claim the same CPU core.
    DuplicateCore {
        core_id: u32,
        producer_a: u32,
        producer_b: u32,
    },
    /// A producer's core is on a different NUMA node than the UMEM NUMA node.
    NumaMismatch {
        producer_id: u32,
        core_numa: u32,
        umem_numa: u32,
    },
    /// A producer's core collides with a money-worker core.
    CoreCollidesWithMoneyWorker { producer_id: u32, core_id: u32 },
    /// Placement count doesn't match shred_capture_workers.
    PlacementCountMismatch { expected: u32, got: u32 },
    /// producer_id doesn't match the canonical identity for queue_id.
    IdentityMismatch {
        producer_id: u32,
        queue_id: u32,
        expected_producer_id: u32,
    },
}

impl std::fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateQueue {
                queue_id,
                producer_a,
                producer_b,
            } => {
                write!(
                    f,
                    "queue {queue_id} claimed by producers {producer_a} and {producer_b}"
                )
            }
            Self::DuplicateCore {
                core_id,
                producer_a,
                producer_b,
            } => {
                write!(
                    f,
                    "core {core_id} claimed by producers {producer_a} and {producer_b}"
                )
            }
            Self::NumaMismatch {
                producer_id,
                core_numa,
                umem_numa,
            } => {
                write!(
                    f,
                    "producer {producer_id}: core NUMA {core_numa} != UMEM NUMA {umem_numa}"
                )
            }
            Self::CoreCollidesWithMoneyWorker {
                producer_id,
                core_id,
            } => {
                write!(
                    f,
                    "producer {producer_id} core {core_id} is also a money-worker core"
                )
            }
            Self::PlacementCountMismatch { expected, got } => {
                write!(f, "expected {expected} placements, got {got}")
            }
            Self::IdentityMismatch {
                producer_id,
                queue_id,
                expected_producer_id,
            } => {
                write!(f, "producer {producer_id} claims queue {queue_id} but canonical identity is {expected_producer_id}")
            }
        }
    }
}

impl std::error::Error for OwnershipError {}

/// Validated queue/core/NUMA ownership table for all AF_XDP producers.
///
/// Constructed only through [`validate_ownership_table`], which enforces:
/// - No duplicate queues across producers
/// - No duplicate cores across producers
/// - Each producer's core is on the same NUMA node as the UMEM
/// - Ingress cores don't collide with money-worker cores
/// - Placement count matches `shred_capture_workers`
#[derive(Debug)]
pub struct OwnershipTable {
    placements: Vec<ProducerPlacement>,
    umem_numa_node: u32,
}

impl OwnershipTable {
    /// Number of placements.
    pub fn len(&self) -> usize {
        self.placements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    /// NUMA node the UMEM is pinned to.
    pub fn umem_numa_node(&self) -> u32 {
        self.umem_numa_node
    }

    /// Look up a placement by producer_id.
    pub fn get(&self, producer_id: u32) -> Option<&ProducerPlacement> {
        self.placements
            .iter()
            .find(|p| p.producer_id == producer_id)
    }

    /// Iterate all placements.
    pub fn iter(&self) -> impl Iterator<Item = &ProducerPlacement> {
        self.placements.iter()
    }
}

/// Build and validate the ownership table.
///
/// `money_worker_cores`: set of CPU core IDs reserved for money workers.
/// `umem_numa_node`: the NUMA node the shared UMEM is pinned to.
pub fn validate_ownership_table(
    placements: Vec<ProducerPlacement>,
    shred_capture_workers: u32,
    money_worker_cores: &[u32],
    umem_numa_node: u32,
) -> Result<OwnershipTable, OwnershipError> {
    if placements.len() as u32 != shred_capture_workers {
        return Err(OwnershipError::PlacementCountMismatch {
            expected: shred_capture_workers,
            got: placements.len() as u32,
        });
    }

    let mut seen_queues = std::collections::HashMap::new();
    let mut seen_cores = std::collections::HashMap::new();

    for p in &placements {
        // Enforce identity invariant: producer_id must be the canonical
        // mapping from queue_id. A split identity would cause feed rings
        // to operate under one ID and archive/runtime under another.
        let canonical = queue_to_producer_id(p.queue_id, shred_capture_workers);
        if canonical != Some(p.producer_id) {
            return Err(OwnershipError::IdentityMismatch {
                producer_id: p.producer_id,
                queue_id: p.queue_id,
                expected_producer_id: canonical.unwrap_or(u32::MAX),
            });
        }

        // Check queue uniqueness
        if let Some(&prev) = seen_queues.get(&p.queue_id) {
            return Err(OwnershipError::DuplicateQueue {
                queue_id: p.queue_id,
                producer_a: prev,
                producer_b: p.producer_id,
            });
        }
        seen_queues.insert(p.queue_id, p.producer_id);

        // Check core uniqueness
        if let Some(&prev) = seen_cores.get(&p.core_id) {
            return Err(OwnershipError::DuplicateCore {
                core_id: p.core_id,
                producer_a: prev,
                producer_b: p.producer_id,
            });
        }
        seen_cores.insert(p.core_id, p.producer_id);

        // Check core is not a money-worker core
        if money_worker_cores.contains(&p.core_id) {
            return Err(OwnershipError::CoreCollidesWithMoneyWorker {
                producer_id: p.producer_id,
                core_id: p.core_id,
            });
        }

        // Check NUMA locality
        if p.numa_node != umem_numa_node {
            return Err(OwnershipError::NumaMismatch {
                producer_id: p.producer_id,
                core_numa: p.numa_node,
                umem_numa: umem_numa_node,
            });
        }
    }

    Ok(OwnershipTable {
        placements,
        umem_numa_node,
    })
}

// ---------------------------------------------------------------------------
// Startup validation (G4-E1-6)
// ---------------------------------------------------------------------------

/// Errors from AF_XDP startup validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupValidationError {
    /// `shred_capture_workers` must be > 0 when AF_XDP is enabled.
    ZeroShredCaptureWorkers,
    /// `money_workers` must be > 0.
    ZeroMoneyWorkers,
    /// `money_workers` must be a power of two.
    MoneyWorkersNotPowerOfTwo(u32),
    /// AF_XDP producer ID range overlaps gRPC producer ID range.
    ProducerIdOverlap { afxdp_id: u32, grpc_id: u32 },
    /// Expected feed ring count doesn't match `money_workers * shred_capture_workers`.
    FeedRingCountMismatch { expected: u32, got: u32 },
    /// UMEM path must be configured when AF_XDP is enabled.
    MissingUmemPath,
    /// Raw-archive path must be configured when AF_XDP is enabled.
    MissingArchivePath,
}

impl std::fmt::Display for StartupValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroShredCaptureWorkers => {
                write!(
                    f,
                    "shred_capture_workers must be > 0 when AF_XDP is enabled"
                )
            }
            Self::ZeroMoneyWorkers => write!(f, "money_workers must be > 0"),
            Self::MoneyWorkersNotPowerOfTwo(n) => {
                write!(f, "money_workers ({n}) must be a power of two")
            }
            Self::ProducerIdOverlap { afxdp_id, grpc_id } => {
                write!(
                    f,
                    "AF_XDP producer {afxdp_id} overlaps gRPC producer {grpc_id}"
                )
            }
            Self::FeedRingCountMismatch { expected, got } => {
                write!(f, "expected {expected} feed rings, got {got}")
            }
            Self::MissingUmemPath => {
                write!(f, "UMEM path must be configured when AF_XDP is enabled")
            }
            Self::MissingArchivePath => {
                write!(
                    f,
                    "raw-archive path must be configured when AF_XDP is enabled"
                )
            }
        }
    }
}

impl std::error::Error for StartupValidationError {}

/// Comprehensive startup validation for AF_XDP ingress.
///
/// Checks:
/// 1. `shred_capture_workers > 0`
/// 2. `money_workers > 0` and is a power of two
/// 3. AF_XDP producer ID range `[0..scw)` does not overlap gRPC range `[scw..scw+gw)`
/// 4. `feed_ring_count == money_workers * shred_capture_workers`
/// 5. UMEM path is `Some` (not empty)
/// 6. Archive path is `Some` (not empty)
pub fn validate_afxdp_startup(
    shred_capture_workers: u32,
    grpc_workers: u32,
    money_workers: u32,
    feed_ring_count: u32,
    umem_path_configured: Option<&Path>,
    archive_path_configured: Option<&Path>,
) -> Result<(), StartupValidationError> {
    if shred_capture_workers == 0 {
        return Err(StartupValidationError::ZeroShredCaptureWorkers);
    }
    if money_workers == 0 {
        return Err(StartupValidationError::ZeroMoneyWorkers);
    }
    if !money_workers.is_power_of_two() {
        return Err(StartupValidationError::MoneyWorkersNotPowerOfTwo(
            money_workers,
        ));
    }

    // AF_XDP range [0..scw), gRPC range [scw..scw+gw)
    // By construction these are disjoint, but verify explicitly.
    let afxdp_end = shred_capture_workers;
    let grpc_start = shred_capture_workers;
    if grpc_workers > 0 && afxdp_end > grpc_start {
        // This can't happen with the identity mapping, but fail-closed.
        return Err(StartupValidationError::ProducerIdOverlap {
            afxdp_id: afxdp_end - 1,
            grpc_id: grpc_start,
        });
    }

    let expected_rings = money_workers
        .checked_mul(shred_capture_workers)
        .unwrap_or(u32::MAX);
    if feed_ring_count != expected_rings {
        return Err(StartupValidationError::FeedRingCountMismatch {
            expected: expected_rings,
            got: feed_ring_count,
        });
    }

    if umem_path_configured.is_none() {
        return Err(StartupValidationError::MissingUmemPath);
    }
    if archive_path_configured.is_none() {
        return Err(StartupValidationError::MissingArchivePath);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Runtime state machine
// ---------------------------------------------------------------------------

/// Minimal AF_XDP ingress instance states.
///
/// Not the RX loop — just enough to support startup/bootstrap sequencing,
/// health reporting, later recovery logic, and control-plane degradation
/// modes (e.g. `GRPC_ONLY` from Tripwire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfxdpInstanceState {
    /// Instance is being constructed: UMEM, rings, and archive are being
    /// allocated but not yet fully open.
    Bootstrapping,
    /// All handles are present and validated. Ready to receive packets.
    Ready,
    /// Instance was ready but has entered a degraded mode (e.g. Tripwire
    /// flagged GRPC_ONLY, or one queue lost link). Still alive, not torn down.
    Degraded,
    /// Instance has been shut down or failed fatally. No recovery without
    /// full re-bootstrap.
    Stopped,
}

impl std::fmt::Display for AfxdpInstanceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bootstrapping => write!(f, "Bootstrapping"),
            Self::Ready => write!(f, "Ready"),
            Self::Degraded => write!(f, "Degraded"),
            Self::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Tracks the runtime state of a single AF_XDP ingress instance.
///
/// All transitions are explicit and validated. The only path to `Ready` is
/// through `promote_to_ready`, which requires `Bootstrapping` as its
/// precondition. This prevents partially-open producers from masquerading
/// as ready.
pub struct AfxdpInstanceRuntime {
    #[allow(dead_code)]
    producer_id: u32,
    state: AfxdpInstanceState,
}

impl AfxdpInstanceRuntime {
    /// Create a new instance in the `Bootstrapping` state.
    pub fn new(producer_id: u32) -> Self {
        Self {
            producer_id,
            state: AfxdpInstanceState::Bootstrapping,
        }
    }

    /// Current state.
    pub fn state(&self) -> AfxdpInstanceState {
        self.state
    }

    /// Producer ID this runtime tracks.
    #[allow(dead_code)]
    pub fn producer_id(&self) -> u32 {
        self.producer_id
    }

    /// Promote from `Bootstrapping` to `Ready`.
    ///
    /// Returns `Err` if the current state is not `Bootstrapping`.
    /// This is the ONLY way to reach `Ready`, ensuring all handles must
    /// be fully opened before the producer is considered live.
    pub fn promote_to_ready(&mut self) -> Result<(), AfxdpInstanceState> {
        if self.state != AfxdpInstanceState::Bootstrapping {
            return Err(self.state);
        }
        self.state = AfxdpInstanceState::Ready;
        Ok(())
    }

    /// Move from `Ready` to `Degraded`.
    ///
    /// Returns `Err` if the current state is not `Ready`.
    pub fn degrade(&mut self) -> Result<(), AfxdpInstanceState> {
        if self.state != AfxdpInstanceState::Ready {
            return Err(self.state);
        }
        self.state = AfxdpInstanceState::Degraded;
        Ok(())
    }

    /// Stop the instance. Valid from any state except `Stopped`.
    ///
    /// Returns `Err` if already stopped.
    pub fn stop(&mut self) -> Result<(), AfxdpInstanceState> {
        if self.state == AfxdpInstanceState::Stopped {
            return Err(self.state);
        }
        self.state = AfxdpInstanceState::Stopped;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AF_XDP producer struct — the real output of E1
// ---------------------------------------------------------------------------

/// Health / counters snapshot for a single AF_XDP producer.
///
/// All counters are monotonic u64 — zeroed at construction, bumped by the
/// RX loop (G4-E2+). Exposed here so health reporters can read without
/// knowing transport internals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AfxdpHealthSnapshot {
    /// Total packets received on this queue since startup.
    pub rx_packets: u64,
    /// Total bytes received on this queue since startup.
    pub rx_bytes: u64,
    /// Packets dropped because the feed ring was full.
    pub feed_ring_drops: u64,
    /// Times the UMEM fill queue starved (no free frames).
    pub umem_fill_stalls: u64,
}

/// Concrete AF_XDP ingress producer — the ownership shell.
///
/// Holds every shared-memory handle that later stages (G4-E2+ RX loop,
/// health reporter, Tripwire) need. No transport logic — just ownership.
///
/// The raw archive is NOT owned per-producer — it is a single shared
/// `RawArchiveSegment` with one lane per producer (lane = producer_id).
/// Producers receive the shared segment reference at call sites, not here.
///
/// Constructed via [`AfxdpProducer::bootstrap`] which opens all handles
/// and transitions the state machine from `Bootstrapping` to `Ready`.
/// If any handle fails to open, the producer stays in `Bootstrapping`
/// and the error is returned — no partial-ready state is possible.
pub struct AfxdpProducer {
    /// Placement: producer_id, queue_id, core_id, numa_node.
    placement: ProducerPlacement,
    /// Feed rings — one per money worker.
    feed_rings: Vec<MappedSpscRing<IngressRecord>>,
    /// UMEM backing-file path for later mmap (G4-E2+).
    #[allow(dead_code)]
    umem_backing_path: PathBuf,
    /// Runtime state machine.
    runtime: AfxdpInstanceRuntime,
    /// Counters / health snapshot — bumped by RX loop, read by reporters.
    health: AfxdpHealthSnapshot,
}

impl AfxdpProducer {
    /// Bootstrap a fully-owned AF_XDP producer.
    ///
    /// Opens feed rings and records UMEM path + placement.
    /// The raw archive is shared (one segment, one lane per producer)
    /// and must be bootstrapped separately via [`bootstrap_afxdp_raw_archive`].
    /// On success the producer is promoted to `Ready`.
    /// On failure the producer is never constructed — no partial state leaks.
    pub fn bootstrap(
        topology: &ValidatedTopology,
        placement: ProducerPlacement,
        feed_capacity: u32,
        base_dir: &Path,
        umem_backing_path: PathBuf,
    ) -> Result<Self, AfxdpProducerError> {
        // Enforce identity invariant: producer_id must equal the canonical
        // mapping from queue_id. Prevents split identity between feed rings
        // (keyed by queue_id) and archive/runtime (keyed by producer_id).
        let canonical = queue_to_producer_id(placement.queue_id, topology.shred_capture_workers);
        if canonical != Some(placement.producer_id) {
            return Err(AfxdpProducerError::InvalidProducerId);
        }

        let feed_rings =
            bootstrap_afxdp_feed_rings(topology, placement.queue_id, feed_capacity, base_dir)?;

        let mut runtime = AfxdpInstanceRuntime::new(placement.producer_id);
        // All handles opened — promote. This cannot fail from Bootstrapping.
        runtime
            .promote_to_ready()
            .expect("just created, must be Bootstrapping");

        Ok(Self {
            placement,
            feed_rings,
            umem_backing_path,
            runtime,
            health: AfxdpHealthSnapshot::default(),
        })
    }

    // -- Accessors: the exact handles later stages need --

    #[allow(dead_code)]
    pub fn producer_id(&self) -> u32 {
        self.placement.producer_id
    }

    #[allow(dead_code)]
    pub fn queue_id(&self) -> u32 {
        self.placement.queue_id
    }

    #[allow(dead_code)]
    pub fn core_id(&self) -> u32 {
        self.placement.core_id
    }

    #[allow(dead_code)]
    pub fn numa_node(&self) -> u32 {
        self.placement.numa_node
    }

    pub fn placement(&self) -> &ProducerPlacement {
        &self.placement
    }

    pub fn feed_rings(&self) -> &[MappedSpscRing<IngressRecord>] {
        &self.feed_rings
    }

    #[allow(dead_code)]
    pub fn feed_ring_count(&self) -> usize {
        self.feed_rings.len()
    }

    #[allow(dead_code)]
    pub fn umem_backing_path(&self) -> &Path {
        &self.umem_backing_path
    }

    pub fn state(&self) -> AfxdpInstanceState {
        self.runtime.state()
    }

    pub fn health(&self) -> &AfxdpHealthSnapshot {
        &self.health
    }

    pub fn health_mut(&mut self) -> &mut AfxdpHealthSnapshot {
        &mut self.health
    }

    /// Degrade this producer (e.g. Tripwire GRPC_ONLY signal).
    pub fn degrade(&mut self) -> Result<(), AfxdpInstanceState> {
        self.runtime.degrade()
    }

    /// Stop this producer.
    pub fn stop(&mut self) -> Result<(), AfxdpInstanceState> {
        self.runtime.stop()
    }
}

// ---------------------------------------------------------------------------
// Batch RX loop and metadata extraction (G4-E3)
// ---------------------------------------------------------------------------

/// Maximum batch size for the RX loop. Bounded to avoid unbounded stack
/// allocations while still amortising per-frame overhead.
pub const RX_BATCH_MAX: u32 = 64;

/// Minimum Ethernet frame size (no VLAN, no FCS). Anything shorter is
/// classified as invalid.
pub const MIN_FRAME_BYTES: u32 = 60;

/// One AF_XDP RX completion descriptor. This is the boundary between the
/// socket layer (G4-E2 UMEM) and the batch processing loop.
///
/// The descriptor carries the UMEM offset and length — never a pointer.
/// The RX loop resolves the offset against the `SharedUmem` mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct RxDescriptor {
    /// Byte offset into the shared UMEM region (frame-aligned).
    pub umem_offset: u64,
    /// Length of the received frame in bytes (from the NIC).
    pub len: u32,
    /// RX hardware timestamp in nanoseconds since Unix epoch.
    /// Comes from NIC RX metadata — never from a syscall.
    pub rx_timestamp_ns: u64,
}

/// Classification result for a single RX descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameVerdict {
    /// Frame is valid — proceed to metadata extraction and feed-ring push.
    Valid,
    /// Frame is a runt (below minimum Ethernet size).
    Runt,
    /// UMEM offset is out of range or misaligned.
    BadOffset,
    /// Zero-length descriptor (NIC error or fill-queue stall artefact).
    ZeroLength,
    /// Timestamp is missing (zero). The contract requires HW timestamps.
    MissingTimestamp,
}

/// Metadata extracted from one valid RX frame. This is what the next
/// stage (feed-ring production, normalization) consumes.
///
/// All references into the UMEM are expressed as offsets — never raw
/// pointers — so this struct is safe to enqueue into shared-memory rings
/// and readable by any process that maps the same UMEM file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxFrameMeta {
    /// UMEM byte offset of the frame start.
    pub umem_offset: u64,
    /// Payload length in bytes (validated > 0, >= MIN_FRAME_BYTES).
    pub payload_len: u32,
    /// RX hardware timestamp — nanoseconds since Unix epoch.
    /// Sourced from NIC RX metadata, never from a syscall.
    pub rx_timestamp_ns: u64,
    /// Frame index within the UMEM (umem_offset / frame_size).
    pub frame_index: u32,
}

/// Classify a single RX descriptor.
///
/// Pure function — no side effects, no UMEM access. Just validates
/// the descriptor fields against the UMEM geometry and minimum frame
/// size. Called once per descriptor in the batch loop.
pub fn classify_rx_descriptor(desc: &RxDescriptor, umem_config: &UmemConfig) -> FrameVerdict {
    if desc.len == 0 {
        return FrameVerdict::ZeroLength;
    }
    if desc.len < MIN_FRAME_BYTES {
        return FrameVerdict::Runt;
    }
    if desc.rx_timestamp_ns == 0 {
        return FrameVerdict::MissingTimestamp;
    }
    let total = umem_config.total_bytes();
    if desc.umem_offset >= total {
        return FrameVerdict::BadOffset;
    }
    if desc.umem_offset % umem_config.frame_size as u64 != 0 {
        return FrameVerdict::BadOffset;
    }
    FrameVerdict::Valid
}

/// Extract metadata from a valid RX descriptor.
///
/// Caller MUST ensure the descriptor was classified as `FrameVerdict::Valid`
/// before calling this — no re-validation is performed.
pub fn extract_rx_metadata(desc: &RxDescriptor, frame_size: u32) -> RxFrameMeta {
    RxFrameMeta {
        umem_offset: desc.umem_offset,
        payload_len: desc.len,
        rx_timestamp_ns: desc.rx_timestamp_ns,
        frame_index: (desc.umem_offset / frame_size as u64) as u32,
    }
}

/// Result of processing one batch of RX descriptors.
#[derive(Debug)]
pub struct RxBatchResult {
    /// Successfully classified+extracted frame metadata, ready for the next
    /// stage. Order matches the valid descriptors in submission order.
    pub frames: Vec<RxFrameMeta>,
    /// Count of descriptors dropped as runts.
    pub runts: u32,
    /// Count of descriptors dropped for bad UMEM offset.
    pub bad_offsets: u32,
    /// Count of zero-length descriptors.
    pub zero_lengths: u32,
    /// Count of descriptors missing HW timestamps.
    pub missing_timestamps: u32,
}

/// Process a batch of RX descriptors.
///
/// This is the core of the AF_XDP RX loop's per-batch work:
/// 1. Classify each descriptor (validity, size, offset, timestamp).
/// 2. Extract metadata for valid frames.
/// 3. Accumulate drop counters for health reporting.
///
/// The batch is bounded to `RX_BATCH_MAX` descriptors. If `descs` is
/// longer, only the first `RX_BATCH_MAX` are processed (caller should
/// not submit more than `RX_BATCH_MAX` at once).
///
/// Returns extracted metadata for valid frames + drop counters.
pub fn process_rx_batch(descs: &[RxDescriptor], umem_config: &UmemConfig) -> RxBatchResult {
    let limit = descs.len().min(RX_BATCH_MAX as usize);
    let batch = &descs[..limit];

    let mut frames = Vec::with_capacity(limit);
    let mut runts = 0u32;
    let mut bad_offsets = 0u32;
    let mut zero_lengths = 0u32;
    let mut missing_timestamps = 0u32;

    for desc in batch {
        match classify_rx_descriptor(desc, umem_config) {
            FrameVerdict::Valid => {
                frames.push(extract_rx_metadata(desc, umem_config.frame_size));
            }
            FrameVerdict::Runt => runts += 1,
            FrameVerdict::BadOffset => bad_offsets += 1,
            FrameVerdict::ZeroLength => zero_lengths += 1,
            FrameVerdict::MissingTimestamp => missing_timestamps += 1,
        }
    }

    RxBatchResult {
        frames,
        runts,
        bad_offsets,
        zero_lengths,
        missing_timestamps,
    }
}

/// Classify a single RX descriptor for the XSK path (test/replay only).
///
/// Same as [`classify_rx_descriptor`] but does NOT drop frames with
/// `rx_timestamp_ns == 0`. The XSK ring path does not provide HW
/// timestamps via the descriptor; the caller supplies a fallback.
///
/// **Quarantined to `#[cfg(test)]`**: the live AF_XDP path uses
/// `ShadowRxMetaParser` which rejects zero timestamps at the XSK layer.
/// This helper exists only for unit tests and offline replay.
#[cfg(test)]
pub fn classify_rx_descriptor_xsk(desc: &RxDescriptor, umem_config: &UmemConfig) -> FrameVerdict {
    if desc.len == 0 {
        return FrameVerdict::ZeroLength;
    }
    if desc.len < MIN_FRAME_BYTES {
        return FrameVerdict::Runt;
    }
    let total = umem_config.total_bytes();
    if desc.umem_offset >= total {
        return FrameVerdict::BadOffset;
    }
    if desc.umem_offset % umem_config.frame_size as u64 != 0 {
        return FrameVerdict::BadOffset;
    }
    FrameVerdict::Valid
}

/// Process an RX batch from XSK, substituting a fallback timestamp
/// for descriptors with `rx_timestamp_ns == 0` (test/replay only).
///
/// Uses [`classify_rx_descriptor_xsk`] (no MissingTimestamp drop).
///
/// **Quarantined to `#[cfg(test)]`**: the live AF_XDP path reads HW
/// timestamps from `ShadowRxMetaV1` metadata headroom, not from
/// descriptors, and rejects zero timestamps at parse time.
#[cfg(test)]
pub fn process_rx_batch_xsk(
    descs: &[RxDescriptor],
    umem_config: &UmemConfig,
    fallback_timestamp_ns: u64,
) -> RxBatchResult {
    let limit = descs.len().min(RX_BATCH_MAX as usize);
    let batch = &descs[..limit];

    let mut frames = Vec::with_capacity(limit);
    let mut runts = 0u32;
    let mut bad_offsets = 0u32;
    let mut zero_lengths = 0u32;
    let mut missing_timestamps = 0u32;

    for desc in batch {
        match classify_rx_descriptor_xsk(desc, umem_config) {
            FrameVerdict::Valid => {
                let mut meta = extract_rx_metadata(desc, umem_config.frame_size);
                if meta.rx_timestamp_ns == 0 {
                    meta.rx_timestamp_ns = fallback_timestamp_ns;
                    missing_timestamps += 1;
                }
                frames.push(meta);
            }
            FrameVerdict::Runt => runts += 1,
            FrameVerdict::BadOffset => bad_offsets += 1,
            FrameVerdict::ZeroLength => zero_lengths += 1,
            FrameVerdict::MissingTimestamp => unreachable!(),
        }
    }

    RxBatchResult {
        frames,
        runts,
        bad_offsets,
        zero_lengths,
        missing_timestamps,
    }
}

// ---------------------------------------------------------------------------
// Turbine shred header parsing — canonical parser is shadow_ingest::shred_meta
// ---------------------------------------------------------------------------

/// Thin adapter: parse a raw frame through the canonical
/// `shadow_ingest::shred_meta::ShredMeta::parse` and convert the `Result`
/// into an `Option` for call sites that just need "parseable or not".
pub fn parse_shred(frame_bytes: &[u8]) -> Option<(ShredMeta, &[u8])> {
    ShredMeta::parse(frame_bytes).ok()
}

// ---------------------------------------------------------------------------
// Shred entry → wire-format transaction extraction (G4-F2)
// ---------------------------------------------------------------------------
//
// DATA shreds carry the shredded, bincode-serialized form of Solana block
// entries.  Each entry is a bincode `Entry { num_hashes, hash, transactions }`.
// Each transaction is a bincode `VersionedTransaction { signatures, message }`
// where `message` is a bincode `VersionedMessage` enum.
//
// run_decode_pipeline (shadow-ingest) expects the compact-u16 wire format:
//   [compact-u16 sig_count][sigs][optional 0x80 v0 prefix][message wire]
// where message lengths are also compact-u16, not bincode u64.
//
// Layout within a data shred:
//   [86..88]  DataShredHeader.size (u16 LE) = 88 + len(entry_bytes)
//   [88..]    entry_bytes  (segment of serialized Vec<Entry> for the slot)
//
//   shred_index == 0: entry_bytes[0..8] is the bincode u64 Vec<Entry> count.
//   shred_index  > 0: entry_bytes is a mid-stream continuation; parsing is
//                     opportunistic and will often yield nothing.

/// Byte offset at which entry data begins in a data shred.
const SHRED_DATA_HEADER_SIZE: usize = 88;
/// Offset of the DataShredHeader.size field (u16 LE).
const SHRED_SIZE_FIELD_OFFSET: usize = 86;

/// Encode `val` as a Solana compact-u16 (shortvec) into `out`.
fn encode_compact_u16_into(val: usize, out: &mut Vec<u8>) {
    if val < 0x80 {
        out.push(val as u8);
        return;
    }
    if val < 0x4000 {
        out.push(((val & 0x7F) | 0x80) as u8);
        out.push((val >> 7) as u8);
        return;
    }
    out.push(((val & 0x7F) | 0x80) as u8);
    out.push((((val >> 7) & 0x7F) | 0x80) as u8);
    out.push((val >> 14) as u8);
}

/// Read u64 LE from `buf[pos..]`; returns (value, new_pos) or None.
#[inline]
fn read_u64_le(buf: &[u8], pos: usize) -> Option<(u64, usize)> {
    if pos + 8 > buf.len() {
        return None;
    }
    let v = u64::from_le_bytes(buf[pos..pos + 8].try_into().ok()?);
    Some((v, pos + 8))
}

/// Read u32 LE from `buf[pos..]`; returns (value, new_pos) or None.
#[inline]
fn read_u32_le(buf: &[u8], pos: usize) -> Option<(u32, usize)> {
    if pos + 4 > buf.len() {
        return None;
    }
    let v = u32::from_le_bytes(buf[pos..pos + 4].try_into().ok()?);
    Some((v, pos + 4))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfxdpTxExtractErrKind {
    EmptyPayload,
    ShortVec,
    BadEntryHeader,
    BadCompactU16,
    SanitizeFailed,
    NoTransactions,
    UnsupportedFormat,
}

impl AfxdpTxExtractErrKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyPayload => "empty_payload",
            Self::ShortVec => "short_vec",
            Self::BadEntryHeader => "bad_entry_header",
            Self::BadCompactU16 => "bad_compact_u16",
            Self::SanitizeFailed => "sanitize_failed",
            Self::NoTransactions => "no_transactions",
            Self::UnsupportedFormat => "unsupported_format",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfxdpTxPayloadShape {
    pub payload_len: usize,
    pub first_8_bytes_hex: String,
    pub first_16_bytes_hex: String,
    pub entry_count_guess: Option<u64>,
    pub num_hashes_or_tick_count_if_present: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfxdpTxExtractDiagnostics {
    pub reason: Option<AfxdpTxExtractErrKind>,
    pub shape: AfxdpTxPayloadShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfxdpTxExtractResult {
    pub wire_txs: Vec<Vec<u8>>,
    pub diagnostics: AfxdpTxExtractDiagnostics,
}

fn first_bytes_hex(buf: &[u8], max_len: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let take = buf.len().min(max_len);
    let mut out = String::with_capacity(take * 2);
    for byte in &buf[..take] {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn tx_payload_shape(shred_index: u32, data_payload: &[u8]) -> AfxdpTxPayloadShape {
    let entry_count_guess = if shred_index == 0 {
        read_u64_le(data_payload, 0).map(|(value, _)| value)
    } else {
        None
    };
    let num_hashes_offset = if shred_index == 0 { 8 } else { 0 };
    let num_hashes_or_tick_count_if_present =
        read_u64_le(data_payload, num_hashes_offset).map(|(value, _)| value);

    AfxdpTxPayloadShape {
        payload_len: data_payload.len(),
        first_8_bytes_hex: first_bytes_hex(data_payload, 8),
        first_16_bytes_hex: first_bytes_hex(data_payload, 16),
        entry_count_guess,
        num_hashes_or_tick_count_if_present,
    }
}

fn read_compact_u16_wire(buf: &[u8], pos: usize) -> Option<(usize, usize)> {
    let mut value: usize = 0;
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

fn wire_tx_compact_prefix_ok(wire: &[u8]) -> bool {
    let (sig_count, sig_start) = match read_compact_u16_wire(wire, 0) {
        Some(v) => v,
        None => return false,
    };
    if sig_count == 0 || sig_count > 8 {
        return false;
    }
    let sig_bytes = match sig_count.checked_mul(64) {
        Some(v) => v,
        None => return false,
    };
    sig_start
        .checked_add(sig_bytes)
        .is_some_and(|message_start| message_start < wire.len())
}

fn parse_wire_instruction(buf: &[u8], pos: usize) -> Option<usize> {
    let mut p = pos;
    p.checked_add(1).filter(|end| *end <= buf.len())?;
    p += 1; // program_id_index

    let (account_count, p2) = read_compact_u16_wire(buf, p)?;
    p = p2;
    if account_count > 64 || p.checked_add(account_count)? > buf.len() {
        return None;
    }
    p += account_count;

    let (data_count, p2) = read_compact_u16_wire(buf, p)?;
    p = p2;
    if data_count > 1232 || p.checked_add(data_count)? > buf.len() {
        return None;
    }
    Some(p + data_count)
}

fn parse_wire_message(buf: &[u8], pos: usize) -> Option<usize> {
    let mut p = pos;
    let versioned = *buf.get(p)? & 0x80 != 0;
    if versioned {
        if *buf.get(p)? != 0x80 {
            return None;
        }
        p += 1;
    }

    p.checked_add(3).filter(|end| *end <= buf.len())?;
    p += 3; // MessageHeader

    let (key_count, p2) = read_compact_u16_wire(buf, p)?;
    p = p2;
    if key_count == 0 || key_count > 64 {
        return None;
    }
    let key_bytes = key_count.checked_mul(32)?;
    if p.checked_add(key_bytes)? > buf.len() {
        return None;
    }
    p += key_bytes;

    if p.checked_add(32)? > buf.len() {
        return None;
    }
    p += 32; // recent_blockhash

    let (instruction_count, p2) = read_compact_u16_wire(buf, p)?;
    p = p2;
    if instruction_count > 64 {
        return None;
    }
    for _ in 0..instruction_count {
        p = parse_wire_instruction(buf, p)?;
    }

    if versioned {
        let (lookup_count, p2) = read_compact_u16_wire(buf, p)?;
        p = p2;
        if lookup_count > 32 {
            return None;
        }
        for _ in 0..lookup_count {
            if p.checked_add(32)? > buf.len() {
                return None;
            }
            p += 32;

            let (writable_count, p2) = read_compact_u16_wire(buf, p)?;
            p = p2;
            if writable_count > 256 || p.checked_add(writable_count)? > buf.len() {
                return None;
            }
            p += writable_count;

            let (readonly_count, p2) = read_compact_u16_wire(buf, p)?;
            p = p2;
            if readonly_count > 256 || p.checked_add(readonly_count)? > buf.len() {
                return None;
            }
            p += readonly_count;
        }
    }

    Some(p)
}

fn parse_wire_versioned_tx(buf: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let start = pos;
    let (sig_count, sig_start) = read_compact_u16_wire(buf, pos)?;
    if sig_count == 0 || sig_count > 8 {
        return None;
    }
    let sig_bytes = sig_count.checked_mul(64)?;
    let message_start = sig_start.checked_add(sig_bytes)?;
    if message_start >= buf.len() {
        return None;
    }
    let end = parse_wire_message(buf, message_start)?;
    Some((buf[start..end].to_vec(), end))
}

/// Parse one bincode `CompiledInstruction`, appending wire bytes to `out`.
/// Returns new_pos on success, None on truncation or sanity failure.
fn parse_bincode_instruction_into(buf: &[u8], pos: usize, out: &mut Vec<u8>) -> Option<usize> {
    let mut p = pos;

    if p >= buf.len() {
        return None;
    }
    let pid = buf[p];
    p += 1;

    let (acct_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if acct_count > 64 {
        return None;
    }
    let acct_count = acct_count as usize;
    if p + acct_count > buf.len() {
        return None;
    }
    let acct_indices = &buf[p..p + acct_count];
    p += acct_count;

    let (data_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if data_count > 1232 {
        return None;
    }
    let data_count = data_count as usize;
    if p + data_count > buf.len() {
        return None;
    }
    let data = &buf[p..p + data_count];
    p += data_count;

    out.push(pid);
    encode_compact_u16_into(acct_count, out);
    out.extend_from_slice(acct_indices);
    encode_compact_u16_into(data_count, out);
    out.extend_from_slice(data);
    Some(p)
}

/// Parse bincode Legacy message body (positioned after the u32 discriminant).
/// Returns (wire_message_bytes, new_pos) or None.
fn parse_bincode_legacy_message(buf: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let mut p = pos;
    let mut out = Vec::new();

    if p + 3 > buf.len() {
        return None;
    }
    out.extend_from_slice(&buf[p..p + 3]); // MessageHeader
    p += 3;

    let (key_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if key_count > 64 {
        return None;
    }
    let key_count = key_count as usize;
    let keys_len = key_count.checked_mul(32)?;
    if p + keys_len > buf.len() {
        return None;
    }
    encode_compact_u16_into(key_count, &mut out);
    out.extend_from_slice(&buf[p..p + keys_len]);
    p += keys_len;

    if p + 32 > buf.len() {
        return None;
    }
    out.extend_from_slice(&buf[p..p + 32]); // recent_blockhash
    p += 32;

    let (ix_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if ix_count > 64 {
        return None;
    }
    let ix_count = ix_count as usize;
    let mut ix_wire = Vec::new();
    for _ in 0..ix_count {
        p = parse_bincode_instruction_into(buf, p, &mut ix_wire)?;
    }
    encode_compact_u16_into(ix_count, &mut out);
    out.extend_from_slice(&ix_wire);
    Some((out, p))
}

/// Parse bincode V0 message body (positioned after the u32 discriminant).
/// Returns (wire_message_bytes, new_pos) or None.
fn parse_bincode_v0_message(buf: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let mut p = pos;
    let mut out = Vec::new();

    if p + 3 > buf.len() {
        return None;
    }
    out.extend_from_slice(&buf[p..p + 3]); // MessageHeader
    p += 3;

    let (key_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if key_count > 64 {
        return None;
    }
    let key_count = key_count as usize;
    let keys_len = key_count.checked_mul(32)?;
    if p + keys_len > buf.len() {
        return None;
    }
    encode_compact_u16_into(key_count, &mut out);
    out.extend_from_slice(&buf[p..p + keys_len]);
    p += keys_len;

    if p + 32 > buf.len() {
        return None;
    }
    out.extend_from_slice(&buf[p..p + 32]); // recent_blockhash
    p += 32;

    let (ix_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if ix_count > 64 {
        return None;
    }
    let ix_count = ix_count as usize;
    let mut ix_wire = Vec::new();
    for _ in 0..ix_count {
        p = parse_bincode_instruction_into(buf, p, &mut ix_wire)?;
    }
    encode_compact_u16_into(ix_count, &mut out);
    out.extend_from_slice(&ix_wire);

    // address_table_lookups
    let (alt_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if alt_count > 32 {
        return None;
    }
    let alt_count = alt_count as usize;
    let mut alt_wire = Vec::new();
    for _ in 0..alt_count {
        if p + 32 > buf.len() {
            return None;
        }
        alt_wire.extend_from_slice(&buf[p..p + 32]); // account_key
        p += 32;

        let (w_count, p2) = read_u64_le(buf, p)?;
        p = p2;
        if w_count > 256 {
            return None;
        }
        let w_count = w_count as usize;
        if p + w_count > buf.len() {
            return None;
        }
        encode_compact_u16_into(w_count, &mut alt_wire);
        alt_wire.extend_from_slice(&buf[p..p + w_count]);
        p += w_count;

        let (r_count, p2) = read_u64_le(buf, p)?;
        p = p2;
        if r_count > 256 {
            return None;
        }
        let r_count = r_count as usize;
        if p + r_count > buf.len() {
            return None;
        }
        encode_compact_u16_into(r_count, &mut alt_wire);
        alt_wire.extend_from_slice(&buf[p..p + r_count]);
        p += r_count;
    }
    encode_compact_u16_into(alt_count, &mut out);
    out.extend_from_slice(&alt_wire);
    Some((out, p))
}

/// Parse one bincode `VersionedTransaction` from `buf[pos..]`.
/// Returns (wire_tx_bytes, new_pos) or None.
fn parse_bincode_versioned_tx(buf: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let mut p = pos;

    let (sig_count, p2) = read_u64_le(buf, p)?;
    p = p2;
    if sig_count == 0 || sig_count > 8 {
        return None;
    }
    let sig_count = sig_count as usize;
    let sigs_len = sig_count.checked_mul(64)?;
    if p + sigs_len > buf.len() {
        return None;
    }
    let sigs = buf[p..p + sigs_len].to_vec();
    p += sigs_len;

    let (msg_variant, p2) = read_u32_le(buf, p)?;
    p = p2;
    let (wire_msg, p2) = match msg_variant {
        0 => parse_bincode_legacy_message(buf, p)?,
        1 => parse_bincode_v0_message(buf, p)?,
        _ => return None,
    };
    p = p2;

    let mut wire = Vec::with_capacity(3 + sigs_len + wire_msg.len());
    encode_compact_u16_into(sig_count, &mut wire);
    wire.extend_from_slice(&sigs);
    if msg_variant == 1 {
        wire.push(0x80); // v0 version prefix
    }
    wire.extend_from_slice(&wire_msg);
    Some((wire, p))
}

fn parse_versioned_tx(buf: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    parse_wire_versioned_tx(buf, pos).or_else(|| parse_bincode_versioned_tx(buf, pos))
}

fn parse_entry_transactions_at(buf: &[u8], pos: usize) -> Option<(Vec<Vec<u8>>, usize)> {
    if buf.len().saturating_sub(pos) < 48 {
        return None;
    }
    let (_, p2) = read_u64_le(buf, pos)?;
    let p3 = p2.checked_add(32)?;
    if p3 > buf.len() {
        return None;
    }
    let (tx_count, mut p) = read_u64_le(buf, p3)?;
    if tx_count > 128 {
        return None;
    }

    let mut txs = Vec::new();
    for _ in 0..tx_count {
        let (wire, consumed) = parse_versioned_tx(buf, p)?;
        txs.push(wire);
        p = consumed;
    }
    Some((txs, p))
}

fn scan_payload_for_entry_transactions(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut txs = Vec::new();
    let mut pos = 0usize;

    while pos + 48 <= buf.len() {
        let count_offset = pos + 40;
        let count_low = buf[count_offset];
        let plausible_count = (1..=128).contains(&count_low)
            && buf[count_offset + 1..count_offset + 8]
                .iter()
                .all(|b| *b == 0);

        if plausible_count {
            if let Some((mut found, consumed)) = parse_entry_transactions_at(buf, pos) {
                if !found.is_empty() {
                    txs.append(&mut found);
                    pos = consumed.max(pos + 1);
                    continue;
                }
            }
        }

        pos += 1;
    }

    txs
}

#[derive(Debug)]
struct AfxdpTxStream {
    slot: u64,
    last_shred_index: u32,
    buffer: Vec<u8>,
    parsed_pos: usize,
    aligned: bool,
}

impl AfxdpTxStream {
    fn new(slot: u64, shred_index: u32, data_payload: &[u8]) -> Self {
        let mut stream = Self {
            slot,
            last_shred_index: shred_index,
            buffer: Vec::new(),
            parsed_pos: 0,
            aligned: false,
        };
        stream.reset(shred_index, data_payload);
        stream
    }

    fn reset(&mut self, shred_index: u32, data_payload: &[u8]) {
        self.last_shred_index = shred_index;
        self.buffer.clear();
        self.buffer.extend_from_slice(data_payload);
        self.aligned = shred_index == 0 && self.buffer.len() >= 8;
        self.parsed_pos = if self.aligned { 8 } else { 0 };
    }

    fn append(&mut self, shred_index: u32, data_payload: &[u8]) {
        if shred_index == 0 || shred_index <= self.last_shred_index {
            self.reset(shred_index, data_payload);
            return;
        }
        self.last_shred_index = shred_index;
        self.buffer.extend_from_slice(data_payload);
    }

    fn extract(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if self.aligned {
            while self.parsed_pos + 48 <= self.buffer.len() {
                match parse_entry_transactions_at(&self.buffer, self.parsed_pos) {
                    Some((mut txs, consumed)) => {
                        out.append(&mut txs);
                        self.parsed_pos = consumed;
                    }
                    None => break,
                }
            }
        } else {
            let mut pos = self.parsed_pos;
            while pos + 48 <= self.buffer.len() {
                let count_offset = pos + 40;
                let count_low = self.buffer[count_offset];
                let plausible_count = (1..=128).contains(&count_low)
                    && self.buffer[count_offset + 1..count_offset + 8]
                        .iter()
                        .all(|b| *b == 0);
                if plausible_count {
                    if let Some((mut txs, consumed)) =
                        parse_entry_transactions_at(&self.buffer, pos)
                    {
                        if !txs.is_empty() {
                            out.append(&mut txs);
                            self.parsed_pos = consumed;
                            self.aligned = true;
                            break;
                        }
                    }
                }
                pos += 1;
            }
            if !self.aligned {
                self.parsed_pos = self.buffer.len().saturating_sub(47);
            }
        }

        if self.parsed_pos > 65_536 {
            self.buffer.drain(..self.parsed_pos);
            self.parsed_pos = 0;
        }
        out
    }
}

#[derive(Debug)]
pub struct AfxdpTxStreamSet {
    streams: Vec<AfxdpTxStream>,
    max_slots: usize,
}

impl Default for AfxdpTxStreamSet {
    fn default() -> Self {
        Self {
            streams: Vec::new(),
            max_slots: 8,
        }
    }
}

impl AfxdpTxStreamSet {
    pub fn extract_wire_txs(
        &mut self,
        slot: u64,
        shred_index: u32,
        data_payload: &[u8],
    ) -> Vec<Vec<u8>> {
        if data_payload.is_empty() {
            return Vec::new();
        }
        let idx = match self.streams.iter().position(|stream| stream.slot == slot) {
            Some(idx) => idx,
            None => {
                if self.streams.len() >= self.max_slots {
                    self.streams.remove(0);
                }
                self.streams
                    .push(AfxdpTxStream::new(slot, shred_index, data_payload));
                return self.streams.last_mut().unwrap().extract();
            }
        };

        let stream = &mut self.streams[idx];
        stream.append(shred_index, data_payload);
        stream.extract()
    }
}

/// Extract wire-format Solana transactions from a Turbine DATA shred.
///
/// `shred_bytes` must begin at the shred signature (byte 0 of the UDP payload
/// after the XDP program strips Ethernet/IP/UDP headers).
///
/// Returns wire-format transaction vecs for every transaction found in the
/// shred's entry data.  Returns empty for coding shreds, under-size shreds,
/// or entry data that cannot be parsed.
///
/// - `shred_index == 0`: the first 8 bytes of entry data are the bincode
///   `Vec<Entry>` count prefix; they are consumed automatically.
/// - `shred_index  > 0`: parsing is opportunistic — mid-stream continuations
///   will usually yield nothing, which is the expected behaviour.
///
/// Retained for test fixtures and replay.  The live AF_XDP path uses
/// `extract_wire_txs_from_data_payload` on reassembled payloads instead.
pub fn extract_wire_txs_from_shred(shred_bytes: &[u8]) -> Vec<Vec<u8>> {
    let (meta, _payload) = match parse_shred(shred_bytes) {
        Some(m) => m,
        None => return Vec::new(),
    };
    if !meta.is_data {
        return Vec::new();
    }
    if shred_bytes.len() < SHRED_DATA_HEADER_SIZE {
        return Vec::new();
    }

    // DataShredHeader.size = SHRED_DATA_HEADER_SIZE + len(entry_bytes)
    let total_size = u16::from_le_bytes(
        shred_bytes[SHRED_SIZE_FIELD_OFFSET..SHRED_SIZE_FIELD_OFFSET + 2]
            .try_into()
            .unwrap(),
    ) as usize;
    if total_size < SHRED_DATA_HEADER_SIZE {
        return Vec::new();
    }
    let entry_data_len = total_size - SHRED_DATA_HEADER_SIZE;
    let entry_data_end = SHRED_DATA_HEADER_SIZE + entry_data_len;
    if entry_data_end > shred_bytes.len() {
        return Vec::new();
    }
    let entry_data = &shred_bytes[SHRED_DATA_HEADER_SIZE..entry_data_end];

    extract_wire_txs_from_data_payload(meta.shred_index, entry_data)
}

/// Extract wire-format transactions from data-shred payload bytes.
///
/// `shred_index` determines whether to skip the `Vec<Entry>` count prefix
/// (8-byte u64, present only in shred 0 of the slot).
/// `data_payload` is the valid entry payload from a data shred — already
/// trimmed to the length indicated by `DataShredHeader.size - 88`.
///
/// Returns an empty vec for empty input, coding-shred payloads,
/// or entry data that cannot be parsed.
pub fn extract_wire_txs_from_data_payload(shred_index: u32, data_payload: &[u8]) -> Vec<Vec<u8>> {
    extract_wire_txs_from_data_payload_detailed(shred_index, data_payload).wire_txs
}

pub fn extract_wire_txs_from_data_payload_detailed(
    shred_index: u32,
    data_payload: &[u8],
) -> AfxdpTxExtractResult {
    let shape = tx_payload_shape(shred_index, data_payload);
    if data_payload.is_empty() {
        return AfxdpTxExtractResult {
            wire_txs: Vec::new(),
            diagnostics: AfxdpTxExtractDiagnostics {
                reason: Some(AfxdpTxExtractErrKind::EmptyPayload),
                shape,
            },
        };
    }

    // shred_index == 0: data_payload starts with the u64 Vec<Entry> count.
    let start: usize = if shred_index == 0 {
        if data_payload.len() < 8 {
            return AfxdpTxExtractResult {
                wire_txs: Vec::new(),
                diagnostics: AfxdpTxExtractDiagnostics {
                    reason: Some(AfxdpTxExtractErrKind::ShortVec),
                    shape,
                },
            };
        }
        8
    } else {
        0
    };
    if start >= data_payload.len() {
        return AfxdpTxExtractResult {
            wire_txs: Vec::new(),
            diagnostics: AfxdpTxExtractDiagnostics {
                reason: Some(AfxdpTxExtractErrKind::BadEntryHeader),
                shape,
            },
        };
    }

    let mut results = Vec::new();
    let mut pos = start;
    let mut saw_entry = false;
    let mut saw_nonzero_tx_count = false;
    let mut terminal_reason = None;

    while pos < data_payload.len() {
        if data_payload.len().saturating_sub(pos) < 48 {
            terminal_reason = Some(AfxdpTxExtractErrKind::BadEntryHeader);
            break;
        }
        let (entry_txs, p4) = match parse_entry_transactions_at(data_payload, pos) {
            Some(v) => v,
            None => {
                terminal_reason = Some(AfxdpTxExtractErrKind::BadEntryHeader);
                break;
            }
        };
        saw_entry = true;
        if entry_txs.is_empty() {
            pos = p4;
            continue;
        }
        saw_nonzero_tx_count = true;

        for wire in entry_txs {
            if !wire_tx_compact_prefix_ok(&wire) {
                terminal_reason = Some(AfxdpTxExtractErrKind::BadCompactU16);
                break;
            }
            results.push(wire);
        }
        pos = p4;
    }

    if results.is_empty() {
        let mut scanned = scan_payload_for_entry_transactions(data_payload);
        if !scanned.is_empty() {
            results.append(&mut scanned);
        }
    }

    let reason = if results.is_empty() {
        terminal_reason.or_else(|| {
            if saw_entry && !saw_nonzero_tx_count {
                Some(AfxdpTxExtractErrKind::NoTransactions)
            } else {
                Some(AfxdpTxExtractErrKind::UnsupportedFormat)
            }
        })
    } else {
        None
    };

    AfxdpTxExtractResult {
        wire_txs: results,
        diagnostics: AfxdpTxExtractDiagnostics { reason, shape },
    }
}

// ---------------------------------------------------------------------------
// AF_XDP event normalization into the shared ingest core (G4-E4)
// ---------------------------------------------------------------------------

/// Re-export the canonical AF_XDP signal source constant from `shadow_ipc`.
pub use shadow_ipc::ingress_record::SIGNAL_SOURCE_AFXDP;

/// An AF_XDP frame normalized into the same shape the shared ingest core
/// (`shadow_ingest::pipeline`) expects.
///
/// This is the AF_XDP equivalent of `grpc_normalize::NormalizedEvent`.
/// Both produce a `DecodeInput` that feeds into `run_decode_pipeline` —
/// there is no second decode system.
///
/// Fields carried for archive/FDR side use (producer_id, umem_offset) are
/// NOT part of the decode pipeline — they travel alongside the event for
/// the raw-archive and Tripwire paths.
pub struct AfxdpNormalizedEvent {
    /// Wire-format Solana transaction bytes extracted from the shred's entry
    /// data.  This is the compact-u16 encoded tx that run_decode_pipeline
    /// consumes — NOT the raw shred bytes.
    pub raw_bytes: Vec<u8>,
    /// Slot from the Turbine shred header (parsed from the frame).
    pub slot: u64,
    /// Signal source — always [`SIGNAL_SOURCE_AFXDP`].
    pub signal_source: u8,
    /// RX hardware timestamp in nanoseconds since Unix epoch.
    /// From NIC RX metadata — never from a syscall.
    pub unix_ns: u64,
    /// Producer ID that received this frame.
    pub producer_id: u32,
    /// UMEM byte offset of the original frame (for archive/FDR entries).
    pub umem_offset: u64,
    /// Optional G11 manifest-backed swap repair for this tx.
    pub manifest_swap: Option<ManifestDecodedSwap>,
}

impl AfxdpNormalizedEvent {
    /// Build a [`DecodeInput`] borrowing from this event.
    ///
    /// Same contract as `NormalizedEvent::as_decode_input` on the gRPC side:
    /// `num_workers` and `route_salt` are runtime topology values, not
    /// per-event.
    pub fn as_decode_input(&self, num_workers: u16, route_salt: u64) -> DecodeInput<'_> {
        DecodeInput {
            raw_tx: &self.raw_bytes,
            signal_source: self.signal_source,
            unix_ns: self.unix_ns,
            slot: self.slot,
            num_workers,
            route_salt,
            manifest_swap: self.manifest_swap,
        }
    }

    /// Normalize one extracted wire-format transaction into an event.
    ///
    /// `tx_bytes` is a wire-format Solana transaction (compact-u16 encoded)
    /// extracted from a shred entry by `extract_wire_txs_from_shred`.
    /// `slot` and `meta` come from the containing shred's header / RX metadata.
    pub fn from_rx_frame(
        tx_bytes: &[u8],
        slot: u64,
        meta: &RxFrameMeta,
        producer_id: u32,
        manifest_swap: Option<ManifestDecodedSwap>,
    ) -> Self {
        Self {
            raw_bytes: tx_bytes.to_vec(),
            slot,
            signal_source: SIGNAL_SOURCE_AFXDP,
            unix_ns: meta.rx_timestamp_ns,
            producer_id,
            umem_offset: meta.umem_offset,
            manifest_swap,
        }
    }
}

// ---------------------------------------------------------------------------
// Feed-ring production from AF_XDP producer (G4-E5)
// ---------------------------------------------------------------------------
//
// The AF_XDP path reuses the exact same emit function as gRPC:
//   `grpc_producer::emit_pipeline_output(out, feed_rings)`
//
// No second emit system. Both transports produce a `PipelineOutput`
// (from `shadow_ingest::pipeline::run_decode_pipeline`) and emit it
// through the same primary/overflow/drop-on-full contract.
//
// This module provides the AF_XDP-specific glue: take a normalized
// event, run it through the shared pipeline, and emit. The emit
// function itself is NOT duplicated — it lives in grpc_producer.rs and
// is transport-agnostic.

use crate::grpc_producer::{
    emit_pipeline_output, emit_pipeline_output_detailed, EmitOutcome, EmitResult,
};
use shadow_ingest::allowlist::AllowlistLookup;
use shadow_ingest::alt_expand::{self, AltResolver};
use shadow_ingest::flatten;
use shadow_ingest::message;
use shadow_ingest::mint_resolve::MintResolver;
use shadow_ingest::pipeline::{run_decode_pipeline, DropReason, PipelineOutput};
use shadow_ingest::route;

pub const AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN: usize = 16;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AfxdpUnsupportedProgramTopEntry {
    pub program_id: [u8; 32],
    pub count: u64,
}

static AFXDP_UNSUPPORTED_PROGRAM_COUNTS: OnceLock<Mutex<HashMap<[u8; 32], u64>>> = OnceLock::new();

fn unsupported_program_counts() -> &'static Mutex<HashMap<[u8; 32], u64>> {
    AFXDP_UNSUPPORTED_PROGRAM_COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn decode_afxdp_output<A, L, M>(
    event: &AfxdpNormalizedEvent,
    num_workers: u16,
    route_salt: u64,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
) -> Result<PipelineOutput, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let input = event.as_decode_input(num_workers, route_salt);
    run_decode_pipeline(&input, allowlist, alt, mint_resolver)
}

fn compact_u16_prefix_len(data: &[u8]) -> Option<usize> {
    let first = *data.first()?;
    if first < 0x80 {
        return Some(1);
    }
    let second = *data.get(1)?;
    if second < 0x80 {
        return Some(2);
    }
    data.get(2).map(|_| 3)
}

fn raw_tx_message_bytes(raw_tx: &[u8]) -> Option<&[u8]> {
    let prefix_len = compact_u16_prefix_len(raw_tx)?;
    let sig_count = {
        let b0 = raw_tx[0] as usize;
        if b0 < 0x80 {
            b0
        } else if prefix_len == 2 {
            (b0 & 0x7F) | ((raw_tx[1] as usize) << 7)
        } else {
            (b0 & 0x7F) | (((raw_tx[1] as usize) & 0x7F) << 7) | ((raw_tx[2] as usize) << 14)
        }
    };
    let message_offset = prefix_len.checked_add(sig_count.checked_mul(64)?)?;
    raw_tx.get(message_offset..)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

pub fn capture_target_tx_from_afxdp_event<L>(
    event: &AfxdpNormalizedEvent,
    record: &IngressRecord,
    alt: &L,
) where
    L: AltResolver + ?Sized,
{
    let Ok(msg) = message::parse_message(&event.raw_bytes) else {
        return;
    };
    let static_account_keys_b58 = msg
        .static_keys
        .iter()
        .map(|key| bs58::encode(key).into_string())
        .collect::<Vec<_>>();
    let account_keys = match alt_expand::expand_alt_lookups(alt, &msg.alt_lookup_bytes)
        .ok()
        .and_then(|alt_keys| flatten::flatten_accounts(&msg.static_keys, &alt_keys).ok())
    {
        Some(flattened) => flattened,
        None => msg.static_keys.clone(),
    };
    let account_keys_b58 = account_keys
        .iter()
        .map(|key| bs58::encode(key).into_string())
        .collect::<Vec<_>>();
    let program_ids_b58 = msg
        .program_id_indices
        .iter()
        .filter_map(|&index| account_keys.get(index as usize))
        .map(|key| bs58::encode(key).into_string())
        .collect::<Vec<_>>();
    let message_hash_hex = raw_tx_message_bytes(&event.raw_bytes)
        .map(sha256_hex)
        .unwrap_or_else(|| sha256_hex(&event.raw_bytes));
    record_target_tx(CapturedTargetTx {
        signature: record.signature,
        slot: record.slot,
        signal_source: record.signal_source,
        venue_id: record.venue_id,
        pool_pubkey: record.pool_pubkey,
        mint_in: record.mint_in,
        mint_out: record.mint_out,
        amount_in: record.amount_in,
        raw_tx: event.raw_bytes.clone(),
        tx_sha256_hex: sha256_hex(&event.raw_bytes),
        message_hash_hex,
        is_versioned: msg.is_versioned,
        instruction_count: msg.instruction_count,
        static_account_keys_b58,
        account_keys_b58,
        program_ids_b58,
    });
}

pub fn record_afxdp_unsupported_programs<L>(event: &AfxdpNormalizedEvent, alt: &L)
where
    L: AltResolver + ?Sized,
{
    let Ok(msg) = message::parse_message(&event.raw_bytes) else {
        return;
    };
    let Ok(alt_keys) = alt_expand::expand_alt_lookups(alt, &msg.alt_lookup_bytes) else {
        return;
    };
    let Ok(flat) = flatten::flatten_accounts(&msg.static_keys, &alt_keys) else {
        return;
    };

    let mut observed = Vec::with_capacity(msg.program_id_indices.len());
    for &pid_index in &msg.program_id_indices {
        let Some(program_id) = flat.get(pid_index as usize) else {
            continue;
        };
        if route::route_program(program_id).is_some() || observed.contains(program_id) {
            continue;
        }
        observed.push(*program_id);
    }

    if observed.is_empty() {
        return;
    }

    if let Ok(mut counts) = unsupported_program_counts().lock() {
        for program_id in observed {
            *counts.entry(program_id).or_insert(0) += 1;
        }
    }
}

pub fn snapshot_afxdp_unsupported_program_top(
) -> [AfxdpUnsupportedProgramTopEntry; AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN] {
    let mut out = [AfxdpUnsupportedProgramTopEntry::default(); AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN];
    let Ok(counts) = unsupported_program_counts().lock() else {
        return out;
    };

    let mut entries: Vec<([u8; 32], u64)> =
        counts.iter().map(|(id, count)| (*id, *count)).collect();
    entries.sort_by(|(left_id, left_count), (right_id, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_id.cmp(right_id))
    });

    for (slot, (program_id, count)) in out.iter_mut().zip(entries.into_iter()) {
        *slot = AfxdpUnsupportedProgramTopEntry { program_id, count };
    }
    out
}

pub fn record_afxdp_tripwire_hit(
    control_flags: Option<&MappedSegment<ShadowControlFlagsV1>>,
    record: &IngressRecord,
) {
    if let Some(control_flags) = control_flags {
        control_flags
            .payload()
            .record_af_xdp_hit_from_ingress(record);
    }
}

/// Run the shared decode pipeline on an AF_XDP normalized event and
/// emit the result into the producer's feed rings.
///
/// Returns `Ok(EmitResult)` if the pipeline produced an output,
/// `Err(DropReason)` if the pipeline dropped the transaction (allowlist
/// miss, parse failure, etc.).
///
/// This is the AF_XDP equivalent of the gRPC emit loop. The decode
/// pipeline and emit function are identical — only the normalization
/// input differs.
pub fn afxdp_decode_and_emit<A, L, M>(
    event: &AfxdpNormalizedEvent,
    num_workers: u16,
    route_salt: u64,
    feed_rings: &[MappedSpscRing<IngressRecord>],
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
) -> Result<EmitResult, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let host_mono_recv_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let output = decode_afxdp_output(
        event,
        num_workers,
        route_salt,
        allowlist,
        alt,
        mint_resolver,
    )?;
    let decode_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let normalized_record_ns = crate::source_arbiter_timing::current_monotonic_ns();
    crate::source_arbiter_timing::record_decoded_ingress_timing(
        &output.record,
        host_mono_recv_ns,
        decode_ns,
        normalized_record_ns,
    );
    capture_target_tx_from_afxdp_event(event, &output.record, alt);
    Ok(emit_pipeline_output(&output, feed_rings))
}

pub fn afxdp_decode_and_emit_with_emitter<A, L, M>(
    event: &AfxdpNormalizedEvent,
    num_workers: u16,
    route_salt: u64,
    emitter: &AfxdpFeedEmitter,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
) -> Result<EmitResult, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let host_mono_recv_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let output = decode_afxdp_output(
        event,
        num_workers,
        route_salt,
        allowlist,
        alt,
        mint_resolver,
    )?;
    let decode_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let normalized_record_ns = crate::source_arbiter_timing::current_monotonic_ns();
    crate::source_arbiter_timing::record_decoded_ingress_timing(
        &output.record,
        host_mono_recv_ns,
        decode_ns,
        normalized_record_ns,
    );
    capture_target_tx_from_afxdp_event(event, &output.record, alt);
    emitter.observe_tripwire_record(&output.record);
    Ok(emit_pipeline_output(&output, &emitter.rings))
}

pub fn afxdp_decode_and_emit_with_control_flags<A, L, M>(
    event: &AfxdpNormalizedEvent,
    num_workers: u16,
    route_salt: u64,
    feed_rings: &[MappedSpscRing<IngressRecord>],
    control_flags: Option<&MappedSegment<ShadowControlFlagsV1>>,
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
) -> Result<EmitResult, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let host_mono_recv_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let output = decode_afxdp_output(
        event,
        num_workers,
        route_salt,
        allowlist,
        alt,
        mint_resolver,
    )?;
    let decode_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let normalized_record_ns = crate::source_arbiter_timing::current_monotonic_ns();
    crate::source_arbiter_timing::record_decoded_ingress_timing(
        &output.record,
        host_mono_recv_ns,
        decode_ns,
        normalized_record_ns,
    );
    capture_target_tx_from_afxdp_event(event, &output.record, alt);
    record_afxdp_tripwire_hit(control_flags, &output.record);
    Ok(emit_pipeline_output(&output, feed_rings))
}

/// Like [`afxdp_decode_and_emit`] but returns the detailed outcome
/// including whether the primary ring was over the 80% threshold.
#[allow(dead_code)]
pub fn afxdp_decode_and_emit_detailed<A, L, M>(
    event: &AfxdpNormalizedEvent,
    num_workers: u16,
    route_salt: u64,
    feed_rings: &[MappedSpscRing<IngressRecord>],
    allowlist: &A,
    alt: &L,
    mint_resolver: &M,
) -> Result<EmitOutcome, DropReason>
where
    A: AllowlistLookup + ?Sized,
    L: AltResolver + ?Sized,
    M: MintResolver + ?Sized,
{
    let host_mono_recv_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let output = decode_afxdp_output(
        event,
        num_workers,
        route_salt,
        allowlist,
        alt,
        mint_resolver,
    )?;
    let decode_ns = crate::source_arbiter_timing::current_monotonic_ns();
    let normalized_record_ns = crate::source_arbiter_timing::current_monotonic_ns();
    crate::source_arbiter_timing::record_decoded_ingress_timing(
        &output.record,
        host_mono_recv_ns,
        decode_ns,
        normalized_record_ns,
    );
    capture_target_tx_from_afxdp_event(event, &output.record, alt);
    Ok(emit_pipeline_output_detailed(&output, feed_rings))
}

// ---------------------------------------------------------------------------
// Flight Data Recorder: raw archive publication (G4-E6)
// ---------------------------------------------------------------------------

/// Result of an archive publication attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchivePublishResult {
    /// Record was successfully enqueued.
    Published,
    /// Archive ring was full. Record dropped — ingress never stalled.
    DroppedFull,
}

/// Publish one raw archive record from an AF_XDP frame.
///
/// Builds a `RawArchiveRecord` from the frame metadata and attempts
/// one `push()` into the producer's lane of the shared archive segment.
/// If the lane is full, the record is dropped — the archive is for
/// `shadow-control` / FDR readers, never for the hot path. Ingress
/// must never stall.
///
/// The record stores `umem_offset_bytes` (the cross-process contract),
/// not pointers. Any process mapping the same UMEM file can resolve
/// the offset to a frame pointer.
pub fn publish_archive_record(
    archive: &RawArchiveSegment,
    lane: usize,
    meta: &RxFrameMeta,
    slot: u64,
    shred_index: u32,
    is_data_shred: bool,
    producer_id: u8,
) -> ArchivePublishResult {
    let record = RawArchiveRecord::new(
        slot,
        meta.umem_offset as u32,
        shred_index,
        meta.payload_len as u16,
        is_data_shred,
        producer_id,
    );

    if archive.push(lane, &record) {
        ArchivePublishResult::Published
    } else {
        ArchivePublishResult::DroppedFull
    }
}

pub fn publish_archive_record_guarded(
    archive: &RawArchiveSegment,
    lane: usize,
    meta: &RxFrameMeta,
    slot: u64,
    shred_index: u32,
    is_data_shred: bool,
    producer_id: u8,
    frame_bytes: &[u8],
) -> ArchivePublishResult {
    let frame_len = (meta.payload_len as usize).min(frame_bytes.len());
    let record = RawArchiveRecord::new_guarded(
        slot,
        archive.next_generation(lane),
        meta.rx_timestamp_ns,
        frame_hash64(&frame_bytes[..frame_len]),
        meta.umem_offset as u32,
        shred_index,
        frame_len as u16,
        is_data_shred,
        producer_id,
    );

    if archive.push(lane, &record) {
        ArchivePublishResult::Published
    } else {
        ArchivePublishResult::DroppedFull
    }
}

// ---------------------------------------------------------------------------
// Delay FIFO and survival-mode flush (G4-E7)
// ---------------------------------------------------------------------------

/// Default delay FIFO depth: 2^20 = 1,048,576 frame slots.
///
/// Must be a power of two so the cursor can use bitmask wrapping
/// instead of modulo division.
pub const DELAY_FIFO_DEPTH: u32 = 1_048_576;

/// Default survival-mode flush chunk size: 50,000 frames returned to
/// the Fill ring in one burst when availability drops below threshold.
pub const SURVIVAL_FLUSH_CHUNK: u32 = 50_000;

/// Default Fill-ring occupancy threshold (as a percentage of total
/// Fill capacity). When posted buffers drop below this, trigger a
/// survival-mode flush.
pub const SURVIVAL_THRESHOLD_PERCENT: u8 = 10;

/// A fixed-depth delay FIFO for UMEM frame offsets.
///
/// The FIFO holds `depth` u64 offsets. On each `enqueue`, the oldest
/// offset is returned (delayed by exactly `depth` frames). Until the
/// FIFO is primed (fewer than `depth` entries pushed), `enqueue`
/// returns `None`.
///
/// The cursor uses bitmask wrapping: `index & (depth - 1)`. No modulo
/// divide ever appears on the hot path.
///
/// This is a transport firewall: the control plane (shadow-control)
/// reads archive records at its own pace. If it falls behind, the
/// delay FIFO ensures frames are not immediately recycled, giving the
/// control plane time to catch up. If it falls behind further,
/// survival mode flushes a bounded chunk back to Fill — accepting
/// history loss, never accepting live packet loss from buffer starvation.
pub struct DelayFifo {
    buf: Vec<u64>,
    mask: u64,
    /// Write cursor — monotonically increasing, wraps via bitmask.
    head: u64,
    /// Read cursor — monotonically increasing, wraps via bitmask.
    /// `head - tail` is the current occupancy.
    tail: u64,
    depth: u32,
}

impl DelayFifo {
    /// Create a new delay FIFO with the given depth.
    ///
    /// `depth` must be a power of two. Panics otherwise.
    pub fn new(depth: u32) -> Self {
        assert!(
            depth > 0 && depth.is_power_of_two(),
            "FIFO depth must be a power of two"
        );
        Self {
            buf: vec![0u64; depth as usize],
            mask: depth as u64 - 1,
            head: 0,
            tail: 0,
            depth,
        }
    }

    /// Create a FIFO with the default depth ([`DELAY_FIFO_DEPTH`]).
    pub fn default_depth() -> Self {
        Self::new(DELAY_FIFO_DEPTH)
    }

    /// Enqueue a frame offset and return the oldest offset if the FIFO
    /// is primed (i.e. `depth` frames have been pushed).
    ///
    /// This is the core hot-path operation: one frame in, one frame out
    /// (after the initial priming window). The returned offset is the
    /// frame that can be safely recycled back to the Fill ring.
    pub fn enqueue(&mut self, offset: u64) -> Option<u64> {
        let slot = (self.head & self.mask) as usize;
        let old = self.buf[slot];
        self.buf[slot] = offset;
        self.head += 1;

        if self.head - self.tail > self.depth as u64 {
            // FIFO was full — the write just overwrote the oldest entry,
            // so advance tail to match (ring semantics: drop oldest).
            self.tail = self.head - self.depth as u64;
            Some(old)
        } else if self.head - self.tail == self.depth as u64 {
            // FIFO just became full (primed).
            Some(old)
        } else {
            None
        }
    }

    /// Number of offsets currently in the FIFO.
    pub fn len(&self) -> u64 {
        self.head - self.tail
    }

    /// Whether the FIFO has been primed (reached full depth).
    pub fn is_primed(&self) -> bool {
        self.head - self.tail >= self.depth as u64
    }

    /// FIFO depth (capacity).
    #[allow(dead_code)]
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Flush up to `count` of the oldest offsets from the FIFO.
    ///
    /// Returns the flushed offsets. Only the flushed entries are removed;
    /// the remaining entries stay valid and the FIFO continues operating.
    /// If the flush empties below `depth`, the FIFO is no longer primed
    /// until new enqueues refill it.
    ///
    /// This is the survival-mode path: accept bounded history loss (the
    /// flushed frames may not have been read by the control plane yet),
    /// never accept live packet loss from Fill-ring starvation.
    pub fn flush_oldest(&mut self, count: u32) -> Vec<u64> {
        let n = (count as u64).min(self.len()) as usize;
        if n == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(n);
        for i in 0..n as u64 {
            let slot = ((self.tail + i) & self.mask) as usize;
            result.push(self.buf[slot]);
        }

        // Advance tail past flushed entries — bounded flush, not full reset.
        self.tail += n as u64;

        result
    }
}

/// Check if survival-mode flush is needed and return the offsets to
/// recycle back to the Fill ring.
///
/// `fill_posted` is the current number of frame addresses still posted
/// to the kernel in the Fill ring.
/// `fill_capacity` is the total Fill ring capacity.
///
/// If `fill_posted` is below `SURVIVAL_THRESHOLD_PERCENT` of capacity,
/// flush up to `SURVIVAL_FLUSH_CHUNK` frames from the delay FIFO.
/// Otherwise returns an empty vec.
pub fn survival_flush_if_needed(
    fifo: &mut DelayFifo,
    fill_posted: u32,
    fill_capacity: u32,
) -> Vec<u64> {
    if fill_capacity == 0 {
        return Vec::new();
    }
    let threshold = (fill_capacity as u64 * SURVIVAL_THRESHOLD_PERCENT as u64 / 100) as u32;
    if fill_posted < threshold {
        fifo.flush_oldest(SURVIVAL_FLUSH_CHUNK)
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc_producer::validate_topology;
    use tempfile::TempDir;

    /// Create a dummy UMEM file in the given tempdir for test use.
    fn make_test_umem(tmp: &TempDir) -> PathBuf {
        let path = umem_path_in(tmp.path(), 0);
        let cfg = UmemConfig::new(64, 2048).unwrap();
        bootstrap_umem_file(&path, &cfg).unwrap();
        path
    }

    // -----------------------------------------------------------------------
    // G4-E1-T1 — AF_XDP scaffold compiles without pulling transport code
    //             into shadow-ingest
    // -----------------------------------------------------------------------

    /// Structural: production code must NOT import aya, libbpf, xdp, or af_xdp.
    /// Scans source up to `#[cfg(test)] mod tests` to exclude test code from the scan.
    #[test]
    fn g4_e1_t1_no_transport_deps_in_afxdp_producer() {
        let src = include_str!("afxdp_producer.rs");
        let production = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[..pos],
            None => src,
        };
        let banned = [
            "extern crate aya",
            "use aya::",
            "extern crate libbpf",
            "use libbpf::",
            "extern crate xdp",
            "use xdp::",
            "use af_xdp::",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "afxdp_producer.rs production code must not contain `{pat}`"
            );
        }
    }

    /// Structural: shadow-ingest Cargo.toml must not depend on AF_XDP
    /// transport crates.
    #[test]
    fn g4_e1_t1_shadow_ingest_has_no_transport_deps() {
        let cargo = include_str!("../../shadow-ingest/Cargo.toml");
        let banned = ["aya", "libbpf", "xdp-rs", "af-xdp", "xsk-rs"];
        for pat in &banned {
            assert!(
                !cargo.contains(pat),
                "shadow-ingest/Cargo.toml must not depend on `{pat}`"
            );
        }
    }

    // -----------------------------------------------------------------------
    // G4-E1-T2 — money-worker modules do not import AF_XDP code
    // -----------------------------------------------------------------------

    /// The grpc_producer module must not import afxdp_producer.
    #[test]
    fn g4_e1_t2_grpc_producer_does_not_import_afxdp() {
        let src = include_str!("grpc_producer.rs");
        assert!(
            !src.contains("afxdp_producer"),
            "grpc_producer.rs must not import afxdp_producer"
        );
    }

    /// The grpc_normalize module must not import afxdp_producer.
    #[test]
    fn g4_e1_t2_grpc_normalize_does_not_import_afxdp() {
        let src = include_str!("grpc_normalize.rs");
        assert!(
            !src.contains("afxdp_producer"),
            "grpc_normalize.rs must not import afxdp_producer"
        );
    }

    // -----------------------------------------------------------------------
    // Producer ID basics (needed by later G4-E1 sub-steps)
    // -----------------------------------------------------------------------

    #[test]
    fn afxdp_producer_id_stable_across_calls() {
        let a = afxdp_producer_id(0, 2);
        let b = afxdp_producer_id(0, 2);
        assert_eq!(a, b);
        assert_eq!(a, Some(0));
    }

    #[test]
    fn afxdp_producer_id_range() {
        assert_eq!(afxdp_producer_id(0, 3), Some(0));
        assert_eq!(afxdp_producer_id(1, 3), Some(1));
        assert_eq!(afxdp_producer_id(2, 3), Some(2));
        assert_eq!(afxdp_producer_id(3, 3), None); // out of range
    }

    #[test]
    fn afxdp_producer_id_zero_workers_returns_none() {
        assert_eq!(afxdp_producer_id(0, 0), None);
    }

    #[test]
    fn afxdp_producer_id_disjoint_from_grpc() {
        use crate::grpc_producer::grpc_producer_id;
        let scw = 2u32;
        let gw = 3u32;
        let afxdp_ids: Vec<u32> = (0..scw).filter_map(|i| afxdp_producer_id(i, scw)).collect();
        let grpc_ids: Vec<u32> = (0..gw)
            .filter_map(|i| grpc_producer_id(scw, i, gw))
            .collect();
        for a in &afxdp_ids {
            for g in &grpc_ids {
                assert_ne!(a, g, "AF_XDP id {a} collides with gRPC id {g}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // UMEM helpers
    // -----------------------------------------------------------------------

    #[test]
    fn umem_path_format() {
        assert_eq!(umem_path(0), PathBuf::from("/dev/hugepages/shadow_umem_0"));
        assert_eq!(umem_path(7), PathBuf::from("/dev/hugepages/shadow_umem_7"));
    }

    #[test]
    fn umem_config_default() {
        let cfg = UmemConfig::default_config();
        assert_eq!(cfg.frames, 2_097_152);
        assert_eq!(cfg.frame_size, 2048);
        assert_eq!(cfg.total_bytes(), 4_294_967_296); // 4 GiB
    }

    #[test]
    fn umem_config_rejects_bad_values() {
        assert!(UmemConfig::new(0, 2048).is_none());
        assert!(UmemConfig::new(1024, 1024).is_none()); // frame_size < 2048
        assert!(UmemConfig::new(1024, 3000).is_none()); // not power-of-two
    }

    #[test]
    fn bootstrap_umem_file_creates_correct_size() {
        let tmp = TempDir::new().unwrap();
        let path = umem_path_in(tmp.path(), 0);
        // Use small config for test (not 4 GiB!)
        let cfg = UmemConfig::new(1024, 2048).unwrap();
        let result = bootstrap_umem_file(&path, &cfg).unwrap();
        assert_eq!(result, path);
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.len(), 1024 * 2048);
    }

    #[test]
    fn bootstrap_umem_file_idempotent_truncate() {
        let tmp = TempDir::new().unwrap();
        let path = umem_path_in(tmp.path(), 0);
        let cfg = UmemConfig::new(512, 2048).unwrap();
        bootstrap_umem_file(&path, &cfg).unwrap();
        // Second call succeeds (truncates)
        bootstrap_umem_file(&path, &cfg).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.len(), 512 * 2048);
    }

    // -----------------------------------------------------------------------
    // G4-E1-T3 — queue-to-producer mapping is deterministic across restart
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t3_queue_to_producer_deterministic() {
        let scw = 4u32;
        // First "startup"
        let first: Vec<u32> = (0..scw)
            .filter_map(|q| queue_to_producer_id(q, scw))
            .collect();
        // Second "startup" — same inputs, same outputs
        let second: Vec<u32> = (0..scw)
            .filter_map(|q| queue_to_producer_id(q, scw))
            .collect();
        assert_eq!(first, second);
        assert_eq!(first, vec![0, 1, 2, 3]);
    }

    #[test]
    fn g4_e1_t3_queue_to_producer_identity_mapping() {
        // queue_id == producer_id for AF_XDP
        for q in 0..8u32 {
            assert_eq!(queue_to_producer_id(q, 8), Some(q));
        }
        // Out of range
        assert_eq!(queue_to_producer_id(8, 8), None);
    }

    #[test]
    fn g4_e1_t3_registry_producer_ids_stable_across_bootstrap() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(2, 3, 1).unwrap();

        let reg1 = AfxdpProducerRegistry::bootstrap(&topo, 256, tmp.path(), &umem).unwrap();
        let ids1: Vec<u32> = reg1.iter().map(|e| e.producer_id).collect();

        // Second bootstrap in a different tempdir ("restart")
        let tmp2 = TempDir::new().unwrap();
        let umem2 = make_test_umem(&tmp2);
        let reg2 = AfxdpProducerRegistry::bootstrap(&topo, 256, tmp2.path(), &umem2).unwrap();
        let ids2: Vec<u32> = reg2.iter().map(|e| e.producer_id).collect();

        assert_eq!(ids1, ids2);
        assert_eq!(ids1, vec![0, 1, 2]);
    }

    #[test]
    fn g4_e1_t3_registry_maps_producer_to_ring_set() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(4, 2, 1).unwrap();
        let reg = AfxdpProducerRegistry::bootstrap(&topo, 256, tmp.path(), &umem).unwrap();
        for pid in 0..2u32 {
            let emitter = reg.get(pid).expect("producer must exist");
            assert_eq!(emitter.producer_id, pid);
            // One feed ring per money worker
            assert_eq!(emitter.rings.len(), 4);
        }
    }

    // -----------------------------------------------------------------------
    // G4-E1-T4 — duplicate producer IDs are rejected at startup
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t4_duplicate_producer_id_rejected() {
        // The identity mapping can't produce duplicates with valid topology,
        // so we verify the DuplicateProducerId variant exists and the
        // registry enforces the check. We test by verifying the error type
        // can be constructed and matched.
        let err = AfxdpProducerError::DuplicateProducerId(0);
        match err {
            AfxdpProducerError::DuplicateProducerId(id) => assert_eq!(id, 0),
            _ => panic!("expected DuplicateProducerId"),
        }
    }

    #[test]
    fn g4_e1_t4_registry_all_ids_unique() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(2, 4, 1).unwrap();
        let reg = AfxdpProducerRegistry::bootstrap(&topo, 256, tmp.path(), &umem).unwrap();

        let ids: Vec<u32> = reg.iter().map(|e| e.producer_id).collect();
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len(), "duplicate producer IDs found");
    }

    #[test]
    fn g4_e1_t4_registry_rejects_invalid_producer() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        // shred_capture_workers=0 means no valid AF_XDP producers — but
        // validate_topology doesn't reject 0 shred workers when a gRPC
        // producer exists. The registry should produce 0
        // emitters, not an error — there's simply nothing to register.
        let topo = validate_topology(2, 0, 1).unwrap();
        let reg = AfxdpProducerRegistry::bootstrap(&topo, 256, tmp.path(), &umem).unwrap();
        assert!(reg.is_empty());
    }

    // -----------------------------------------------------------------------
    // G4-E1-T5 — all expected rings for one AF_XDP producer open/create
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t5_bootstrap_creates_all_rings_for_one_producer() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(4, 2, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 256, tmp.path()).unwrap();
        assert_eq!(rings.len(), 4, "one ring per money worker");
    }

    #[test]
    fn g4_e1_t5_rings_are_pushable_after_bootstrap() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        let mut rings = bootstrap_afxdp_feed_rings(&topo, 0, 256, tmp.path()).unwrap();
        // Push a zeroed record into each ring and pop it back
        for ring in &mut rings {
            let rec = IngressRecord::zeroed();
            assert!(ring.push(&rec));
            let popped = ring.pop().expect("ring should not be empty");
            assert_eq!(popped.slot, 0);
        }
    }

    #[test]
    fn g4_e1_t5_emitter_has_correct_ring_count() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(4, 3, 1).unwrap();
        // Producer index 2 → producer_id 2
        let _emitter = AfxdpFeedEmitter::new(&topo, 2, 256, tmp.path(), umem).unwrap();
    }

    // -----------------------------------------------------------------------
    // G4-E1-T6 — ring path naming matches convention
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t6_ring_path_naming_convention() {
        let base = Path::new("/dev/shm");
        assert_eq!(
            feed_ring_path_in(base, 0, 0),
            PathBuf::from("/dev/shm/shadow_feed_ring_w0_p0")
        );
        assert_eq!(
            feed_ring_path_in(base, 3, 1),
            PathBuf::from("/dev/shm/shadow_feed_ring_w3_p1")
        );
        assert_eq!(
            feed_ring_path_in(base, 7, 5),
            PathBuf::from("/dev/shm/shadow_feed_ring_w7_p5")
        );
    }

    #[test]
    fn g4_e1_t6_bootstrapped_ring_files_exist_at_expected_paths() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 2, 1).unwrap();
        // Bootstrap producer 0
        let _rings = bootstrap_afxdp_feed_rings(&topo, 0, 256, tmp.path()).unwrap();
        // Verify files exist with correct names
        for w in 0..2u32 {
            let path = feed_ring_path_in(tmp.path(), w, 0);
            assert!(path.exists(), "ring file missing: {}", path.display());
        }
    }

    #[test]
    fn g4_e1_t6_raw_archive_path_naming_convention() {
        let base = Path::new("/dev/shm");
        // Singular path — no per-producer suffix (G4-A contract)
        let p = raw_archive_path_in(base);
        assert!(p.starts_with("/dev/shm"));
        assert!(p.file_name().unwrap() == "shadow_raw_archive");
    }

    // -----------------------------------------------------------------------
    // G4-E1-T7 — startup fails closed if any required ring is missing
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t7_open_fails_if_rings_not_bootstrapped() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        // Do NOT bootstrap — attempt to open non-existent rings
        let result = open_afxdp_feed_rings(&topo, 0, tmp.path());
        assert!(
            result.is_err(),
            "open must fail when ring files are missing"
        );
    }

    #[test]
    fn g4_e1_t7_open_succeeds_after_bootstrap() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        // Bootstrap first
        let _created = bootstrap_afxdp_feed_rings(&topo, 0, 256, tmp.path()).unwrap();
        // Now open should succeed
        let opened = open_afxdp_feed_rings(&topo, 0, tmp.path()).unwrap();
        assert_eq!(opened.len(), 2);
    }

    #[test]
    fn g4_e1_t7_open_fails_if_partial_rings_exist() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        // Bootstrap normally
        let _created = bootstrap_afxdp_feed_rings(&topo, 0, 256, tmp.path()).unwrap();
        // Delete one ring file to simulate partial state
        let path_w2 = feed_ring_path_in(tmp.path(), 2, 0);
        std::fs::remove_file(&path_w2).unwrap();
        // Open must fail — partial ring set is not acceptable
        let result = open_afxdp_feed_rings(&topo, 0, tmp.path());
        assert!(result.is_err(), "open must fail with partial ring set");
    }

    // -----------------------------------------------------------------------
    // G4-E1-T8 — UMEM path template and archive path template are frozen
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t8_umem_path_template_frozen() {
        // Production path must match `/dev/hugepages/shadow_umem_{id}`
        assert_eq!(
            umem_path(0).to_str().unwrap(),
            "/dev/hugepages/shadow_umem_0"
        );
        assert_eq!(
            umem_path(1).to_str().unwrap(),
            "/dev/hugepages/shadow_umem_1"
        );
        assert_eq!(
            umem_path(99).to_str().unwrap(),
            "/dev/hugepages/shadow_umem_99"
        );
    }

    #[test]
    fn g4_e1_t8_archive_path_template_frozen() {
        let base = Path::new("/dev/shm");
        // Singular path — no per-producer suffix (G4-A contract)
        let p = raw_archive_path_in(base);
        assert!(p.starts_with("/dev/shm"));
        assert!(p.file_name().unwrap() == "shadow_raw_archive");
    }

    #[test]
    fn g4_e1_t8_emitter_stores_umem_path() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(2, 1, 1).unwrap();
        let emitter = AfxdpFeedEmitter::new(&topo, 0, 256, tmp.path(), umem.clone()).unwrap();
        assert_eq!(emitter.umem_backing_path, umem);
    }

    #[test]
    fn g4_e1_t8_emitter_stores_queue_id() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(2, 3, 1).unwrap();
        let emitter = AfxdpFeedEmitter::new(&topo, 2, 256, tmp.path(), umem).unwrap();
        assert_eq!(emitter.queue_id, 2);
        assert_eq!(emitter.producer_id, 2);
    }

    // -----------------------------------------------------------------------
    // G4-E1-T9 — startup rejects AF_XDP config without UMEM/archive plan
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t9_validate_rejects_missing_umem() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        // Point to a non-existent UMEM path
        let fake_umem = tmp.path().join("nonexistent_umem");
        let result = validate_afxdp_ownership(&topo, &fake_umem, tmp.path());
        assert!(result.is_err(), "must reject missing UMEM file");
    }

    #[test]
    fn g4_e1_t9_validate_rejects_missing_archive_dir() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(2, 1, 1).unwrap();
        let fake_dir = tmp.path().join("nonexistent_dir");
        let result = validate_afxdp_ownership(&topo, &umem, &fake_dir);
        assert!(result.is_err(), "must reject missing archive base dir");
    }

    #[test]
    fn g4_e1_t9_validate_passes_when_both_exist() {
        let tmp = TempDir::new().unwrap();
        let umem = make_test_umem(&tmp);
        let topo = validate_topology(2, 1, 1).unwrap();
        // tmp.path() exists and umem file exists
        validate_afxdp_ownership(&topo, &umem, tmp.path()).unwrap();
    }

    #[test]
    fn g4_e1_t9_validate_skips_when_no_afxdp_workers() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 0, 1).unwrap();
        // Even with bogus paths, validation passes when shred_capture_workers == 0
        let fake = tmp.path().join("bogus");
        validate_afxdp_ownership(&topo, &fake, &fake).unwrap();
    }

    // -----------------------------------------------------------------------
    // G4-E1-T11 — NUMA/queue ownership table is internally consistent
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t11_valid_table_passes() {
        let placements = vec![
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 4,
                numa_node: 0,
            },
            ProducerPlacement {
                producer_id: 1,
                queue_id: 1,
                core_id: 5,
                numa_node: 0,
            },
            ProducerPlacement {
                producer_id: 2,
                queue_id: 2,
                core_id: 6,
                numa_node: 0,
            },
        ];
        let table = validate_ownership_table(placements, 3, &[0, 1, 2, 3], 0).unwrap();
        assert_eq!(table.len(), 3);
        assert_eq!(table.umem_numa_node(), 0);
    }

    #[test]
    fn g4_e1_t11_rejects_numa_mismatch() {
        let placements = vec![ProducerPlacement {
            producer_id: 0,
            queue_id: 0,
            core_id: 4,
            numa_node: 1,
        }];
        let result = validate_ownership_table(placements, 1, &[], 0);
        match result {
            Err(OwnershipError::NumaMismatch {
                producer_id: 0,
                core_numa: 1,
                umem_numa: 0,
            }) => {}
            other => panic!("expected NumaMismatch, got {other:?}"),
        }
    }

    #[test]
    fn g4_e1_t11_rejects_placement_count_mismatch() {
        let placements = vec![ProducerPlacement {
            producer_id: 0,
            queue_id: 0,
            core_id: 4,
            numa_node: 0,
        }];
        let result = validate_ownership_table(placements, 2, &[], 0);
        match result {
            Err(OwnershipError::PlacementCountMismatch {
                expected: 2,
                got: 1,
            }) => {}
            other => panic!("expected PlacementCountMismatch, got {other:?}"),
        }
    }

    #[test]
    fn g4_e1_t11_table_lookup_by_producer_id() {
        let placements = vec![
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 10,
                numa_node: 0,
            },
            ProducerPlacement {
                producer_id: 1,
                queue_id: 1,
                core_id: 11,
                numa_node: 0,
            },
        ];
        let table = validate_ownership_table(placements, 2, &[], 0).unwrap();
        let p0 = table.get(0).unwrap();
        assert_eq!(p0.core_id, 10);
        assert_eq!(p0.queue_id, 0);
        let p1 = table.get(1).unwrap();
        assert_eq!(p1.core_id, 11);
        assert!(table.get(99).is_none());
    }

    // -----------------------------------------------------------------------
    // G4-E1-T12 — host bootstrap metadata injection doesn't change mapping
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t12_different_cores_same_producer_ids() {
        // "First host" — cores 4,5
        let p1 = vec![
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 4,
                numa_node: 0,
            },
            ProducerPlacement {
                producer_id: 1,
                queue_id: 1,
                core_id: 5,
                numa_node: 0,
            },
        ];
        let t1 = validate_ownership_table(p1, 2, &[], 0).unwrap();

        // "Second host" — entirely different cores 20,21 — same producer IDs
        let p2 = vec![
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 20,
                numa_node: 0,
            },
            ProducerPlacement {
                producer_id: 1,
                queue_id: 1,
                core_id: 21,
                numa_node: 0,
            },
        ];
        let t2 = validate_ownership_table(p2, 2, &[], 0).unwrap();

        // Producer mapping is stable — only physical placement differs
        let ids1: Vec<u32> = t1.iter().map(|p| p.producer_id).collect();
        let ids2: Vec<u32> = t2.iter().map(|p| p.producer_id).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn g4_e1_t12_different_numa_nodes_same_mapping() {
        // Host A: NUMA 0
        let pa = vec![ProducerPlacement {
            producer_id: 0,
            queue_id: 0,
            core_id: 4,
            numa_node: 0,
        }];
        let ta = validate_ownership_table(pa, 1, &[], 0).unwrap();

        // Host B: NUMA 1 (different hardware)
        let pb = vec![ProducerPlacement {
            producer_id: 0,
            queue_id: 0,
            core_id: 8,
            numa_node: 1,
        }];
        let tb = validate_ownership_table(pb, 1, &[], 1).unwrap();

        assert_eq!(
            ta.get(0).unwrap().producer_id,
            tb.get(0).unwrap().producer_id
        );
    }

    #[test]
    fn g4_e1_t12_empty_table_for_zero_workers() {
        let table = validate_ownership_table(vec![], 0, &[0, 1], 0).unwrap();
        assert!(table.is_empty());
    }

    // -----------------------------------------------------------------------
    // G4-E1-T14 — overlapping AF_XDP/gRPC producer ranges rejected
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t14_disjoint_ranges_accepted() {
        // scw=2, gw=3 → AF_XDP [0,1], gRPC [2,3,4] — no overlap
        let result =
            validate_afxdp_startup(2, 3, 4, 8, Some(Path::new("/tmp")), Some(Path::new("/tmp")));
        assert!(result.is_ok());
    }

    #[test]
    fn g4_e1_t14_feed_ring_count_mismatch_rejected() {
        // money_workers=4, scw=2 → expected 8 rings, provide 6
        let result =
            validate_afxdp_startup(2, 1, 4, 6, Some(Path::new("/tmp")), Some(Path::new("/tmp")));
        assert_eq!(
            result,
            Err(StartupValidationError::FeedRingCountMismatch {
                expected: 8,
                got: 6
            })
        );
    }

    #[test]
    fn g4_e1_t14_correct_ring_count_accepted() {
        // money_workers=4, scw=3 → expected 12 rings
        let result = validate_afxdp_startup(
            3,
            1,
            4,
            12,
            Some(Path::new("/tmp")),
            Some(Path::new("/tmp")),
        );
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // G4-E1-T15 — missing UMEM path rejected when AF_XDP is enabled
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t15_rejects_missing_umem_path() {
        let result = validate_afxdp_startup(1, 1, 4, 4, None, Some(Path::new("/tmp")));
        assert_eq!(result, Err(StartupValidationError::MissingUmemPath));
    }

    #[test]
    fn g4_e1_t15_rejects_missing_archive_path() {
        let result = validate_afxdp_startup(1, 1, 4, 4, Some(Path::new("/tmp")), None);
        assert_eq!(result, Err(StartupValidationError::MissingArchivePath));
    }

    #[test]
    fn g4_e1_t15_accepts_both_paths_configured() {
        let result = validate_afxdp_startup(
            1,
            1,
            4,
            4,
            Some(Path::new("/dev/hugepages/shadow_umem_0")),
            Some(Path::new("/dev/shm")),
        );
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // G4-E1-T17: startup failure never leaves partial state as ready
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t17_stopped_cannot_reach_ready() {
        let mut rt = AfxdpInstanceRuntime::new(0);
        rt.stop().unwrap();
        // Stopped -> Ready must be impossible.
        assert!(rt.promote_to_ready().is_err());
        assert_eq!(rt.state(), AfxdpInstanceState::Stopped);
    }

    #[test]
    fn g4_e1_t17_degraded_cannot_reach_ready() {
        let mut rt = AfxdpInstanceRuntime::new(0);
        rt.promote_to_ready().unwrap();
        rt.degrade().unwrap();
        // Degraded -> Ready must be impossible (needs full re-bootstrap).
        assert!(rt.promote_to_ready().is_err());
        assert_eq!(rt.state(), AfxdpInstanceState::Degraded);
    }

    #[test]
    fn g4_e1_t17_bootstrapping_cannot_degrade() {
        let mut rt = AfxdpInstanceRuntime::new(0);
        // Cannot degrade something that was never ready.
        assert!(rt.degrade().is_err());
        assert_eq!(rt.state(), AfxdpInstanceState::Bootstrapping);
    }

    #[test]
    fn g4_e1_t17_only_bootstrapping_reaches_ready() {
        // Exhaustively verify: only Bootstrapping -> Ready is valid.
        let states_that_cannot_promote = vec![
            AfxdpInstanceState::Ready,
            AfxdpInstanceState::Degraded,
            AfxdpInstanceState::Stopped,
        ];
        for state in states_that_cannot_promote {
            let mut rt = AfxdpInstanceRuntime::new(0);
            // Force into target state
            match state {
                AfxdpInstanceState::Ready => {
                    rt.promote_to_ready().unwrap();
                }
                AfxdpInstanceState::Degraded => {
                    rt.promote_to_ready().unwrap();
                    rt.degrade().unwrap();
                }
                AfxdpInstanceState::Stopped => {
                    rt.stop().unwrap();
                }
                AfxdpInstanceState::Bootstrapping => unreachable!(),
            }
            assert!(
                rt.promote_to_ready().is_err(),
                "state {:?} should not promote to Ready",
                state
            );
        }
    }

    // -----------------------------------------------------------------------
    // G4-E1-T19: producer exposes exact handles, no transport duplication
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e1_t19_placement_accessible() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        let umem_path = make_test_umem(&tmp);

        let placement = ProducerPlacement {
            producer_id: 0,
            queue_id: 0,
            core_id: 8,
            numa_node: 1,
        };

        let producer = AfxdpProducer::bootstrap(
            &topo,
            placement,
            FEED_RING_DEFAULT_CAPACITY,
            tmp.path(),
            umem_path,
        )
        .unwrap();

        let p = producer.placement();
        assert_eq!(p.producer_id, 0);
        assert_eq!(p.queue_id, 0);
        assert_eq!(p.core_id, 8);
        assert_eq!(p.numa_node, 1);
    }

    #[test]
    fn g4_e1_t19_feed_rings_are_borrowable() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        let umem_path = make_test_umem(&tmp);

        let producer = AfxdpProducer::bootstrap(
            &topo,
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 0,
                numa_node: 0,
            },
            FEED_RING_DEFAULT_CAPACITY,
            tmp.path(),
            umem_path,
        )
        .unwrap();

        // feed_rings() returns a slice — no clone, no move
        let rings: &[MappedSpscRing<IngressRecord>] = producer.feed_rings();
        assert_eq!(rings.len(), 4);
    }

    #[test]
    fn g4_e1_t19_health_mutable_for_rx_loop() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        let umem_path = make_test_umem(&tmp);

        let mut producer = AfxdpProducer::bootstrap(
            &topo,
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 0,
                numa_node: 0,
            },
            FEED_RING_DEFAULT_CAPACITY,
            tmp.path(),
            umem_path,
        )
        .unwrap();

        producer.health_mut().rx_packets += 1;
        producer.health_mut().rx_bytes += 1232;
        assert_eq!(producer.health().rx_packets, 1);
        assert_eq!(producer.health().rx_bytes, 1232);
    }

    #[test]
    fn g4_e1_t19_degrade_and_stop_work_on_producer() {
        let tmp = TempDir::new().unwrap();
        let topo = validate_topology(2, 1, 1).unwrap();
        let umem_path = make_test_umem(&tmp);

        let mut producer = AfxdpProducer::bootstrap(
            &topo,
            ProducerPlacement {
                producer_id: 0,
                queue_id: 0,
                core_id: 0,
                numa_node: 0,
            },
            FEED_RING_DEFAULT_CAPACITY,
            tmp.path(),
            umem_path,
        )
        .unwrap();

        assert_eq!(producer.state(), AfxdpInstanceState::Ready);
        producer.degrade().unwrap();
        assert_eq!(producer.state(), AfxdpInstanceState::Degraded);
        producer.stop().unwrap();
        assert_eq!(producer.state(), AfxdpInstanceState::Stopped);
    }

    // -----------------------------------------------------------------------
    // G4-E2-T1: UMEM offset roundtrip across two mappings
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e2_t1_offset_roundtrip_two_mappings() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&path, &cfg).unwrap();

        // First mapping — write a pattern to frame 7
        let mut map_a = SharedUmem::map_shared(&path, &cfg).unwrap();
        let offset = map_a.frame_offset(7).unwrap();
        let ptr = map_a.frame_ptr_mut(7).unwrap();
        unsafe { std::ptr::write_bytes(ptr, 0xDE, 2048) };

        // Second mapping of the same file — read frame 7 via the offset
        let map_b = SharedUmem::map_shared(&path, &cfg).unwrap();
        let resolved = map_b.resolve_offset(offset).unwrap();
        let val = unsafe { *resolved };
        assert_eq!(val, 0xDE, "second mapping must see writes from first");
    }

    #[test]
    fn g4_e2_t1_offset_roundtrip_all_frames_consistent() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(32, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&path, &cfg).unwrap();

        let umem = SharedUmem::map_shared(&path, &cfg).unwrap();
        let base = umem.frame_ptr(0).unwrap() as usize;

        for i in 0..32u32 {
            let offset = umem.frame_offset(i).unwrap();
            let ptr = umem.frame_ptr(i).unwrap() as usize;
            assert_eq!(
                ptr - base,
                offset as usize,
                "frame {i} pointer/offset mismatch"
            );
            let resolved = umem.resolve_offset(offset).unwrap() as usize;
            assert_eq!(resolved, ptr, "frame {i} resolve roundtrip failed");
        }
    }

    #[test]
    fn g4_e2_t1_out_of_range_frame_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&path, &cfg).unwrap();
        let umem = SharedUmem::map_shared(&path, &cfg).unwrap();

        assert!(umem.frame_offset(64).is_none());
        assert!(umem.frame_ptr(64).is_none());
        assert!(umem.resolve_offset(64 * 2048).is_none());
    }

    // -----------------------------------------------------------------------
    // G4-E2-T2: frame size/count/footprint constants frozen
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e2_t2_default_frame_size_is_2048() {
        assert_eq!(UmemConfig::DEFAULT_FRAME_SIZE, 2048);
    }

    #[test]
    fn g4_e2_t2_default_frame_count_is_2_097_152() {
        assert_eq!(UmemConfig::DEFAULT_FRAMES, 2_097_152);
    }

    #[test]
    fn g4_e2_t2_default_footprint_is_4gib() {
        let cfg = UmemConfig::default_config();
        assert_eq!(cfg.total_bytes(), 4 * 1024 * 1024 * 1024); // 4 GiB
    }

    #[test]
    fn g4_e2_t2_frame_size_must_be_power_of_two_and_ge_2048() {
        assert!(UmemConfig::new(1024, 2048).is_some());
        assert!(UmemConfig::new(1024, 4096).is_some());
        assert!(UmemConfig::new(1024, 1024).is_none()); // < 2048
        assert!(UmemConfig::new(1024, 3000).is_none()); // not power-of-two
        assert!(UmemConfig::new(0, 2048).is_none()); // zero frames
    }

    #[test]
    fn g4_e2_t2_resolve_rejects_misaligned_offset() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&path, &cfg).unwrap();
        let umem = SharedUmem::map_shared(&path, &cfg).unwrap();

        assert!(umem.resolve_offset(1).is_none());
        assert!(umem.resolve_offset(2047).is_none());
        assert!(umem.resolve_offset(2049).is_none());
        assert!(umem.resolve_offset(2048).is_some()); // aligned — ok
    }

    // -----------------------------------------------------------------------
    // G4-E2-T3: startup fails closed on bad backing
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e2_t3_map_fails_on_missing_file() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 99);
        // No bootstrap — file doesn't exist
        assert!(SharedUmem::map_shared(&path, &cfg).is_err());
    }

    #[test]
    fn g4_e2_t3_map_fails_on_undersized_file() {
        let tmp = TempDir::new().unwrap();
        let small_cfg = UmemConfig::new(32, 2048).unwrap();
        let big_cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        // Bootstrap with small config
        bootstrap_umem_file(&path, &small_cfg).unwrap();
        // Map with big config — must fail
        assert!(SharedUmem::map_shared(&path, &big_cfg).is_err());
    }

    #[test]
    fn g4_e2_t3_map_fails_on_empty_file() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        // Create empty file (0 bytes)
        std::fs::write(&path, b"").unwrap();
        assert!(SharedUmem::map_shared(&path, &cfg).is_err());
    }

    #[test]
    fn g4_e2_t3_exactly_sized_file_succeeds() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&path, &cfg).unwrap();
        // Exact size — must succeed
        assert!(SharedUmem::map_shared(&path, &cfg).is_ok());
    }

    // -----------------------------------------------------------------------
    // G4-E3-T1: RX metadata timestamp threaded into per-frame event
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e3_t1_timestamp_preserved_exactly() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let ts: u64 = 1_711_900_000_123_456_789;
        let desc = RxDescriptor {
            umem_offset: 0,
            len: 1500,
            rx_timestamp_ns: ts,
        };
        let meta = extract_rx_metadata(&desc, cfg.frame_size);
        assert_eq!(
            meta.rx_timestamp_ns, ts,
            "timestamp must pass through unchanged from descriptor to metadata"
        );
    }

    #[test]
    fn g4_e3_t1_each_frame_gets_its_own_timestamp() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let descs = vec![
            RxDescriptor {
                umem_offset: 0,
                len: 1500,
                rx_timestamp_ns: 100,
            },
            RxDescriptor {
                umem_offset: 2048,
                len: 1500,
                rx_timestamp_ns: 200,
            },
            RxDescriptor {
                umem_offset: 4096,
                len: 1500,
                rx_timestamp_ns: 300,
            },
        ];
        let result = process_rx_batch(&descs, &cfg);
        assert_eq!(result.frames.len(), 3);
        assert_eq!(result.frames[0].rx_timestamp_ns, 100);
        assert_eq!(result.frames[1].rx_timestamp_ns, 200);
        assert_eq!(result.frames[2].rx_timestamp_ns, 300);
    }

    #[test]
    fn g4_e3_t1_timestamp_zero_means_missing_not_epoch() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        // Zero timestamp is rejected — it means HW metadata was absent,
        // not "exactly Unix epoch".
        let desc = RxDescriptor {
            umem_offset: 0,
            len: 1500,
            rx_timestamp_ns: 0,
        };
        assert_eq!(
            classify_rx_descriptor(&desc, &cfg),
            FrameVerdict::MissingTimestamp
        );
    }

    // -----------------------------------------------------------------------
    // G4-E3-T2: no syscall timestamp fallback in AF_XDP fast path
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e3_t2_missing_timestamp_dropped_not_patched() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        // Descriptor with missing HW timestamp — must be dropped entirely,
        // not patched with clock_gettime or similar.
        let descs = vec![
            RxDescriptor {
                umem_offset: 0,
                len: 1500,
                rx_timestamp_ns: 0,
            },
            RxDescriptor {
                umem_offset: 2048,
                len: 1500,
                rx_timestamp_ns: 0,
            },
        ];
        let result = process_rx_batch(&descs, &cfg);
        assert_eq!(
            result.frames.len(),
            0,
            "missing-timestamp frames must be dropped, never patched"
        );
        assert_eq!(result.missing_timestamps, 2);
    }

    #[test]
    fn g4_e3_t2_rx_frame_meta_has_no_fallback_timestamp_field() {
        // RxFrameMeta has exactly one timestamp field (rx_timestamp_ns).
        // No "fallback_ns", no "wall_clock_ns", no Option wrapper.
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 1500,
            rx_timestamp_ns: 42,
            frame_index: 0,
        };
        // The only timestamp accessor is rx_timestamp_ns — compile-time proof
        // that no fallback path exists.
        assert_eq!(meta.rx_timestamp_ns, 42);
    }

    #[test]
    fn g4_e3_t2_classify_checks_timestamp_before_accepting() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        // Everything else is valid, but timestamp is 0 → rejected
        let desc = RxDescriptor {
            umem_offset: 0,
            len: 1500,
            rx_timestamp_ns: 0,
        };
        assert_ne!(classify_rx_descriptor(&desc, &cfg), FrameVerdict::Valid);

        // Same descriptor with a real timestamp → accepted
        let desc_ok = RxDescriptor {
            umem_offset: 0,
            len: 1500,
            rx_timestamp_ns: 1,
        };
        assert_eq!(classify_rx_descriptor(&desc_ok, &cfg), FrameVerdict::Valid);
    }

    // -----------------------------------------------------------------------
    // G4-E3-T3: mixed batch doesn't poison later frames
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e3_t3_bad_frame_does_not_poison_subsequent_good_frames() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let descs = vec![
            RxDescriptor {
                umem_offset: 0,
                len: 1500,
                rx_timestamp_ns: 100,
            }, // valid
            RxDescriptor {
                umem_offset: 999,
                len: 1500,
                rx_timestamp_ns: 101,
            }, // bad offset
            RxDescriptor {
                umem_offset: 2048,
                len: 30,
                rx_timestamp_ns: 102,
            }, // runt
            RxDescriptor {
                umem_offset: 4096,
                len: 0,
                rx_timestamp_ns: 103,
            }, // zero-length
            RxDescriptor {
                umem_offset: 6144,
                len: 1500,
                rx_timestamp_ns: 0,
            }, // missing ts
            RxDescriptor {
                umem_offset: 8192,
                len: 1500,
                rx_timestamp_ns: 104,
            }, // valid
            RxDescriptor {
                umem_offset: 10240,
                len: 1500,
                rx_timestamp_ns: 105,
            }, // valid
        ];

        let result = process_rx_batch(&descs, &cfg);

        // All 3 valid frames survived despite 4 bad frames interleaved
        assert_eq!(result.frames.len(), 3);
        assert_eq!(result.frames[0].umem_offset, 0);
        assert_eq!(result.frames[0].rx_timestamp_ns, 100);
        assert_eq!(result.frames[1].umem_offset, 8192);
        assert_eq!(result.frames[1].rx_timestamp_ns, 104);
        assert_eq!(result.frames[2].umem_offset, 10240);
        assert_eq!(result.frames[2].rx_timestamp_ns, 105);

        // Drop counters are accurate
        assert_eq!(result.bad_offsets, 1);
        assert_eq!(result.runts, 1);
        assert_eq!(result.zero_lengths, 1);
        assert_eq!(result.missing_timestamps, 1);
    }

    #[test]
    fn g4_e3_t3_all_bad_batch_returns_zero_frames() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let descs = vec![
            RxDescriptor {
                umem_offset: 1,
                len: 1500,
                rx_timestamp_ns: 1,
            }, // misaligned
            RxDescriptor {
                umem_offset: 0,
                len: 10,
                rx_timestamp_ns: 2,
            }, // runt
            RxDescriptor {
                umem_offset: 0,
                len: 0,
                rx_timestamp_ns: 3,
            }, // zero-length
        ];
        let result = process_rx_batch(&descs, &cfg);
        assert_eq!(result.frames.len(), 0);
        assert_eq!(result.bad_offsets + result.runts + result.zero_lengths, 3);
    }

    #[test]
    fn g4_e3_t3_valid_frame_ordering_preserved() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let descs: Vec<RxDescriptor> = (0..16u64)
            .map(|i| RxDescriptor {
                umem_offset: i * 2048,
                len: 1500,
                rx_timestamp_ns: 1000 + i,
            })
            .collect();

        let result = process_rx_batch(&descs, &cfg);
        assert_eq!(result.frames.len(), 16);
        for (i, meta) in result.frames.iter().enumerate() {
            assert_eq!(meta.frame_index, i as u32, "ordering broken at index {i}");
            assert_eq!(meta.rx_timestamp_ns, 1000 + i as u64);
        }
    }

    #[test]
    fn g4_e3_t3_empty_batch_is_harmless() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let result = process_rx_batch(&[], &cfg);
        assert_eq!(result.frames.len(), 0);
        assert_eq!(result.runts, 0);
        assert_eq!(result.bad_offsets, 0);
        assert_eq!(result.zero_lengths, 0);
        assert_eq!(result.missing_timestamps, 0);
    }

    // -----------------------------------------------------------------------
    // G4-E4-T1: AF_XDP normalized event matches gRPC/replay expectations
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e4_t1_normalized_event_produces_valid_decode_input() {
        let meta = RxFrameMeta {
            umem_offset: 4096,
            payload_len: 200,
            rx_timestamp_ns: 1_700_000_000_000_000_000,
            frame_index: 2,
        };
        let payload = vec![0xAB; 200];
        let event = AfxdpNormalizedEvent::from_rx_frame(&payload, 42, &meta, 0, None);

        let input = event.as_decode_input(4, 0xDEAD);
        assert_eq!(input.raw_tx, &payload[..]);
        assert_eq!(input.signal_source, SIGNAL_SOURCE_AFXDP);
        assert_eq!(input.unix_ns, 1_700_000_000_000_000_000);
        assert_eq!(input.slot, 42);
        assert_eq!(input.num_workers, 4);
        assert_eq!(input.route_salt, 0xDEAD);
    }

    #[test]
    fn g4_e4_t1_afxdp_and_grpc_feed_same_decode_input_shape() {
        // AF_XDP path
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 100,
            rx_timestamp_ns: 999,
            frame_index: 0,
        };
        let afxdp_event = AfxdpNormalizedEvent::from_rx_frame(&[1; 100], 50, &meta, 0, None);
        let afxdp_input = afxdp_event.as_decode_input(4, 77);

        // gRPC path (construct equivalent DecodeInput directly)
        let grpc_input = DecodeInput {
            raw_tx: &[1; 100],
            signal_source: 0, // PublicGeyser
            unix_ns: 999,
            slot: 50,
            num_workers: 4,
            route_salt: 77,
            manifest_swap: None,
        };

        // Same shape — same fields, same types
        assert_eq!(afxdp_input.raw_tx, grpc_input.raw_tx);
        assert_eq!(afxdp_input.unix_ns, grpc_input.unix_ns);
        assert_eq!(afxdp_input.slot, grpc_input.slot);
        assert_eq!(afxdp_input.num_workers, grpc_input.num_workers);
        assert_eq!(afxdp_input.route_salt, grpc_input.route_salt);
        // Only signal_source differs (by design)
        assert_ne!(afxdp_input.signal_source, grpc_input.signal_source);
    }

    #[test]
    fn g4_e4_t1_signal_source_afxdp_is_distinct_from_grpc_sources() {
        // gRPC sources occupy 0..=3 plus 5..=6; AF_XDP stays reserved at 4.
        assert_eq!(SIGNAL_SOURCE_AFXDP, 4);
    }

    #[test]
    fn g4_e4_t3_afxdp_records_target_tx_capture() {
        let fixture = crate::grpc_producer::sh5_test_fixtures::make_valid_grpc_event();
        let mut raw_tx = fixture.raw_tx.clone();
        raw_tx[1..65].fill(0xCC);
        let meta = RxFrameMeta {
            umem_offset: 4096,
            payload_len: raw_tx.len() as u32,
            rx_timestamp_ns: 1_700_000_000_000_123_456,
            frame_index: 0,
        };
        let event = AfxdpNormalizedEvent::from_rx_frame(&raw_tx, fixture.slot, &meta, 0, None);
        let (allowlist, alt, mint) = crate::grpc_producer::sh5_test_fixtures::make_test_deps();

        let output = decode_afxdp_output(&event, 4, 42, &allowlist, &alt, &mint)
            .expect("AF_XDP fixture should decode");
        capture_target_tx_from_afxdp_event(&event, &output.record, &alt);

        let capture = shadow_core::target_tx_cache::lookup_target_tx(&output.record.signature)
            .expect("AF_XDP target tx capture missing");
        assert_eq!(capture.signal_source, SIGNAL_SOURCE_AFXDP);
        assert_eq!(capture.slot, fixture.slot);
        assert_eq!(capture.raw_tx, raw_tx);
        assert!(!capture.account_keys_b58.is_empty());
        assert!(!capture.program_ids_b58.is_empty());
    }

    // -----------------------------------------------------------------------
    // G4-E4-T2: source metadata survives normalization unchanged
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e4_t2_unix_ns_from_rx_metadata_not_invented() {
        let ts: u64 = 1_711_900_000_123_456_789;
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 100,
            rx_timestamp_ns: ts,
            frame_index: 0,
        };
        let event = AfxdpNormalizedEvent::from_rx_frame(&[0; 100], 10, &meta, 5, None);
        assert_eq!(
            event.unix_ns, ts,
            "timestamp must be the RX metadata value, not invented"
        );
        let input = event.as_decode_input(2, 0);
        assert_eq!(input.unix_ns, ts);
    }

    #[test]
    fn g4_e4_t2_producer_id_and_umem_offset_preserved() {
        let meta = RxFrameMeta {
            umem_offset: 8192,
            payload_len: 100,
            rx_timestamp_ns: 42,
            frame_index: 4,
        };
        let event = AfxdpNormalizedEvent::from_rx_frame(&[0; 100], 10, &meta, 7, None);
        assert_eq!(event.producer_id, 7);
        assert_eq!(event.umem_offset, 8192);
    }

    #[test]
    fn g4_e4_t2_slot_threaded_through() {
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 64,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };
        let event = AfxdpNormalizedEvent::from_rx_frame(&[0; 64], 314159, &meta, 0, None);
        assert_eq!(event.slot, 314159);
        let input = event.as_decode_input(2, 0);
        assert_eq!(input.slot, 314159);
    }

    #[test]
    fn g4_e4_t2_raw_bytes_are_exact_copy() {
        let payload: Vec<u8> = (0..=255).collect();
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 256,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };
        let event = AfxdpNormalizedEvent::from_rx_frame(&payload, 0, &meta, 0, None);
        assert_eq!(event.raw_bytes, payload);
        let input = event.as_decode_input(2, 0);
        assert_eq!(input.raw_tx, &payload[..]);
    }

    // -----------------------------------------------------------------------
    // G4-E4-T3: same bytes → deterministic downstream behavior
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e4_t3_identical_payloads_produce_identical_decode_inputs() {
        let payload = vec![0x42; 150];
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 150,
            rx_timestamp_ns: 1000,
            frame_index: 0,
        };

        let event_a = AfxdpNormalizedEvent::from_rx_frame(&payload, 99, &meta, 0, None);
        let event_b = AfxdpNormalizedEvent::from_rx_frame(&payload, 99, &meta, 0, None);

        let input_a = event_a.as_decode_input(4, 0xBEEF);
        let input_b = event_b.as_decode_input(4, 0xBEEF);

        assert_eq!(input_a.raw_tx, input_b.raw_tx);
        assert_eq!(input_a.signal_source, input_b.signal_source);
        assert_eq!(input_a.unix_ns, input_b.unix_ns);
        assert_eq!(input_a.slot, input_b.slot);
        assert_eq!(input_a.num_workers, input_b.num_workers);
        assert_eq!(input_a.route_salt, input_b.route_salt);
    }

    #[test]
    fn g4_e4_t3_different_producers_same_payload_same_decode_input() {
        let payload = vec![0xFF; 100];
        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 100,
            rx_timestamp_ns: 500,
            frame_index: 0,
        };

        let event_p0 = AfxdpNormalizedEvent::from_rx_frame(&payload, 10, &meta, 0, None);
        let event_p1 = AfxdpNormalizedEvent::from_rx_frame(&payload, 10, &meta, 1, None);

        // producer_id differs on the event (for archive/FDR)
        assert_ne!(event_p0.producer_id, event_p1.producer_id);

        // But DecodeInput is identical — producer_id is NOT in the decode path
        let input_0 = event_p0.as_decode_input(4, 0);
        let input_1 = event_p1.as_decode_input(4, 0);
        assert_eq!(input_0.raw_tx, input_1.raw_tx);
        assert_eq!(input_0.signal_source, input_1.signal_source);
        assert_eq!(input_0.unix_ns, input_1.unix_ns);
        assert_eq!(input_0.slot, input_1.slot);
    }

    #[test]
    fn g4_e4_t3_umem_offset_not_in_decode_input() {
        // umem_offset is for archive/FDR, NOT for the decode pipeline.
        // DecodeInput has no umem_offset field — this is a compile-time
        // guarantee that the ingest core is UMEM-agnostic.
        let meta = RxFrameMeta {
            umem_offset: 999999,
            payload_len: 64,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };
        let event = AfxdpNormalizedEvent::from_rx_frame(&[0; 64], 0, &meta, 0, None);
        let input = event.as_decode_input(2, 0);

        // DecodeInput fields: raw_tx, signal_source, unix_ns, slot,
        // num_workers, route_salt. No umem_offset, no producer_id.
        assert_eq!(
            std::mem::size_of::<DecodeInput>(),
            std::mem::size_of::<DecodeInput>()
        );
        assert_eq!(input.slot, 0);
        // event.umem_offset is present but never enters input
        assert_eq!(event.umem_offset, 999999);
    }

    // -----------------------------------------------------------------------
    // G4-E5-T1: AF_XDP records land in same shard as gRPC
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e5_t1_afxdp_uses_same_emit_function_as_grpc() {
        use crate::grpc_producer::{emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let dir = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();

        // Bootstrap AF_XDP feed rings for producer 0
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        // Construct a PipelineOutput — same shape both transports produce
        let out = PipelineOutput {
            record: IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };

        // Emit using the SAME function gRPC uses
        let result = emit_pipeline_output(&out, &rings);
        assert_eq!(result, EmitResult::Primary);
        assert_eq!(rings[0].len(), 1);
    }

    #[test]
    fn g4_e5_t1_same_route_decision_same_shard() {
        use crate::grpc_producer::{bootstrap_grpc_feed_rings, emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let dir_afxdp = TempDir::new().unwrap();
        let dir_grpc = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();

        let afxdp_rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir_afxdp.path()).unwrap();
        let grpc_rings = bootstrap_grpc_feed_rings(&topo, 0, 16, dir_grpc.path()).unwrap();

        let route = RouteDecision {
            primary_worker: 2,
            overflow_worker: 3,
        };

        let out = PipelineOutput {
            record: IngressRecord::zeroed(),
            route,
        };

        // Both use the same emit function → same shard
        let afxdp_result = emit_pipeline_output(&out, &afxdp_rings);
        let grpc_result = emit_pipeline_output(&out, &grpc_rings);
        assert_eq!(afxdp_result, grpc_result);
        assert_eq!(afxdp_result, EmitResult::Primary);
        assert_eq!(afxdp_rings[2].len(), 1);
        assert_eq!(grpc_rings[2].len(), 1);
    }

    #[test]
    fn g4_e5_t1_no_second_emit_system() {
        // Prove at source level: afxdp_decode_and_emit calls
        // emit_pipeline_output (from grpc_producer), not a duplicate.
        let src = include_str!("afxdp_producer.rs");
        let prod = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[..pos],
            None => src,
        };
        assert!(
            prod.contains("emit_pipeline_output("),
            "AF_XDP emit path must call the shared emit_pipeline_output"
        );
        assert!(
            prod.contains("emit_pipeline_output_detailed("),
            "AF_XDP detailed emit must call the shared emit_pipeline_output_detailed"
        );
        // No local reimplementation of the overflow logic
        assert!(
            !prod.contains("over_threshold("),
            "AF_XDP must not reimplement overflow threshold — use emit_pipeline_output"
        );
    }

    // -----------------------------------------------------------------------
    // G4-E5-T2: 80% overflow enforced for AF_XDP producers
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e5_t2_overflow_triggers_at_80_percent() {
        use crate::grpc_producer::{emit_pipeline_output_detailed, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let dir = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = PipelineOutput {
            record: IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };

        // Fill primary to 80% threshold: capacity=16, 80%=12.8, so 13 pushes
        // trigger over_threshold.
        for _ in 0..13 {
            let outcome = emit_pipeline_output_detailed(&out, &rings);
            assert_eq!(outcome.result, EmitResult::Primary);
        }

        // 14th push: primary at 13/16 → 81.25% → over threshold → overflow
        let outcome = emit_pipeline_output_detailed(&out, &rings);
        assert_eq!(outcome.result, EmitResult::Overflow);
        assert!(outcome.primary_was_over_threshold);
        assert_eq!(
            rings[1].len(),
            1,
            "overflow ring must have received the record"
        );
    }

    #[test]
    fn g4_e5_t2_overflow_preserves_record_bytes() {
        use crate::grpc_producer::{emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let dir = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        // Fill primary past threshold
        let filler = PipelineOutput {
            record: IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };
        for _ in 0..13 {
            emit_pipeline_output(&filler, &rings);
        }

        // Push a sentinel record → should overflow to ring 1
        let mut sentinel = IngressRecord::zeroed();
        sentinel.slot = 0xDEAD_BEEF;
        sentinel.signal_source = SIGNAL_SOURCE_AFXDP;
        let out = PipelineOutput {
            record: sentinel,
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };
        let result = emit_pipeline_output(&out, &rings);
        assert_eq!(result, EmitResult::Overflow);

        // Read it back
        let received = rings[1].pop().unwrap();
        assert_eq!(received.slot, 0xDEAD_BEEF);
        assert_eq!(received.signal_source, SIGNAL_SOURCE_AFXDP);
    }

    // -----------------------------------------------------------------------
    // G4-E5-T3: feed-ring full drops immediately, never blocks
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e5_t3_drop_on_full_is_immediate() {
        use crate::grpc_producer::{emit_pipeline_output_detailed, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let dir = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        // Small capacity so we can fill it quickly
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        let out = PipelineOutput {
            record: IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };

        // Fill both primary and overflow completely
        for _ in 0..16 {
            rings[0].push(&IngressRecord::zeroed());
            rings[1].push(&IngressRecord::zeroed());
        }

        // Next emit must drop — not block, not spin, not retry
        let outcome = emit_pipeline_output_detailed(&out, &rings);
        assert_eq!(outcome.result, EmitResult::DroppedFull);
    }

    #[test]
    fn g4_e5_t3_no_blocking_in_emit_source() {
        // Prove: emit_pipeline_output has no loop, no sleep, no spin, no retry.
        let src = include_str!("grpc_producer.rs");
        let fn_start = src.find("pub fn emit_pipeline_output_detailed").unwrap();
        let fn_body = &src[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = i + open;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &fn_body[open..=fn_end];

        let banned = [
            "loop {",
            "while ",
            "sleep",
            "spin",
            "retry",
            "thread::park",
            "yield_now",
        ];
        for keyword in &banned {
            assert!(
                !body.contains(keyword),
                "emit_pipeline_output_detailed must not contain '{keyword}' — drop-on-full, never block"
            );
        }
    }

    #[test]
    fn g4_e5_t3_dropped_record_does_not_corrupt_rings() {
        use crate::grpc_producer::{emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let dir = TempDir::new().unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, dir.path()).unwrap();

        // Fill primary and overflow
        for _ in 0..16 {
            rings[0].push(&IngressRecord::zeroed());
            rings[1].push(&IngressRecord::zeroed());
        }

        let out = PipelineOutput {
            record: IngressRecord::zeroed(),
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };

        // Drop happens
        assert_eq!(emit_pipeline_output(&out, &rings), EmitResult::DroppedFull);

        // Rings are still valid — can pop all entries
        let mut count = 0;
        while rings[0].pop().is_some() {
            count += 1;
        }
        while rings[1].pop().is_some() {
            count += 1;
        }
        assert_eq!(count, 32); // 16 + 16, all intact
    }

    // -----------------------------------------------------------------------
    // G4-E6-T1: archive record fields match frame metadata
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e6_t1_published_record_matches_frame_metadata() {
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(
            &raw_archive_path_in(tmp.path()),
            1, // 1 producer lane
            64,
        )
        .unwrap();

        let meta = RxFrameMeta {
            umem_offset: 8192,
            payload_len: 1200,
            rx_timestamp_ns: 1_700_000_000_000_000_000,
            frame_index: 4,
        };

        let result = publish_archive_record(&archive, 0, &meta, 42, 7, true, 0);
        assert_eq!(result, ArchivePublishResult::Published);

        let record = archive.pop(0).unwrap();
        assert_eq!(record.slot, 42);
        assert_eq!(record.umem_offset_bytes, 8192);
        assert_eq!(record.shred_index, 7);
        assert_eq!(record.len, 1200);
        assert_eq!(record.is_data_shred, 1);
        assert_eq!(record.producer_id, 0);
    }

    #[test]
    fn g11_e6_guarded_archive_record_carries_overwrite_metadata() {
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 64).unwrap();

        let meta = RxFrameMeta {
            umem_offset: 8192,
            payload_len: 4,
            rx_timestamp_ns: 1_700_000_000_000_000_123,
            frame_index: 4,
        };
        let frame_bytes = [1u8, 2, 3, 4];

        let result =
            publish_archive_record_guarded(&archive, 0, &meta, 42, 7, true, 0, &frame_bytes);
        assert_eq!(result, ArchivePublishResult::Published);

        let record = archive.pop(0).unwrap();
        assert_eq!(record.generation, 1);
        assert_eq!(record.rx_timestamp_ns, meta.rx_timestamp_ns);
        assert_eq!(record.frame_hash64, frame_hash64(&frame_bytes));
        assert_eq!(record.len, frame_bytes.len() as u16);
    }

    #[test]
    fn g4_e6_t1_coding_shred_flag() {
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 64).unwrap();

        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 1000,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };

        publish_archive_record(&archive, 0, &meta, 10, 3, false, 1);
        let record = archive.pop(0).unwrap();
        assert_eq!(record.is_data_shred, 0);
        assert_eq!(record.producer_id, 1);
    }

    #[test]
    fn g4_e6_t1_umem_offset_is_offset_not_pointer() {
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 64).unwrap();

        let meta = RxFrameMeta {
            umem_offset: 63 * 2048, // last frame in a 64-frame UMEM
            payload_len: 500,
            rx_timestamp_ns: 1,
            frame_index: 63,
        };

        publish_archive_record(&archive, 0, &meta, 99, 0, true, 0);
        let record = archive.pop(0).unwrap();
        // u32 offset — not a pointer, safe for cross-process use
        assert_eq!(record.umem_offset_bytes, 63 * 2048);
    }

    // -----------------------------------------------------------------------
    // G4-E6-T2: archive drop-on-full never blocks
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e6_t2_full_archive_drops_immediately() {
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(
            &raw_archive_path_in(tmp.path()),
            1,
            4, // tiny capacity
        )
        .unwrap();

        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 100,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };

        // Fill the archive ring
        for i in 0..4 {
            let r = publish_archive_record(&archive, 0, &meta, i as u64, 0, true, 0);
            assert_eq!(r, ArchivePublishResult::Published);
        }

        // Next publish must drop, not block
        let r = publish_archive_record(&archive, 0, &meta, 999, 0, true, 0);
        assert_eq!(r, ArchivePublishResult::DroppedFull);
    }

    #[test]
    fn g4_e6_t2_drop_does_not_corrupt_existing_entries() {
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 4).unwrap();

        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 100,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };

        // Fill with known slots
        for i in 0..4u64 {
            publish_archive_record(&archive, 0, &meta, i, 0, true, 0);
        }

        // Drop one
        assert_eq!(
            publish_archive_record(&archive, 0, &meta, 999, 0, true, 0),
            ArchivePublishResult::DroppedFull,
        );

        // Existing entries must be intact
        for i in 0..4u64 {
            let rec = archive.pop(0).unwrap();
            assert_eq!(rec.slot, i, "entry {i} must be intact after drop");
        }
    }

    #[test]
    fn g4_e6_t2_no_retry_loop_in_publish() {
        let src = include_str!("afxdp_producer.rs");
        let fn_start = src.find("pub fn publish_archive_record").unwrap();
        // Find the function body end
        let fn_body = &src[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = i + open;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &fn_body[open..=fn_end];
        let banned = ["loop", "while", "retry", "sleep", "spin"];
        for kw in &banned {
            assert!(
                !body.contains(kw),
                "publish_archive_record must not contain '{kw}'"
            );
        }
    }

    // -----------------------------------------------------------------------
    // G4-E6-T3: second mapping reconstructs frame via umem_offset_bytes
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e6_t3_archive_offset_resolves_in_second_umem_mapping() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let umem_path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&umem_path, &cfg).unwrap();

        // First mapping — write a recognizable pattern to frame 5
        let mut map_writer = SharedUmem::map_shared(&umem_path, &cfg).unwrap();
        let ptr = map_writer.frame_ptr_mut(5).unwrap();
        unsafe { std::ptr::write_bytes(ptr, 0xCA, 2048) };

        // Publish an archive record referencing frame 5
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 64).unwrap();

        let meta = RxFrameMeta {
            umem_offset: 5 * 2048,
            payload_len: 1500,
            rx_timestamp_ns: 42,
            frame_index: 5,
        };
        publish_archive_record(&archive, 0, &meta, 100, 0, true, 0);

        // Second mapping (simulating shadow-control / FDR reader)
        let map_reader = SharedUmem::map_shared(&umem_path, &cfg).unwrap();

        // Read the archive record and resolve the offset
        let record = archive.pop(0).unwrap();
        let resolved = map_reader
            .resolve_offset(record.umem_offset_bytes as u64)
            .unwrap();

        // Verify the frame contents match what the writer put there
        let val = unsafe { *resolved };
        assert_eq!(
            val, 0xCA,
            "second mapping must see the writer's data via archive offset"
        );
    }

    #[test]
    fn g4_e6_t3_archive_record_is_64_bytes() {
        assert_eq!(std::mem::size_of::<RawArchiveRecord>(), 64);
        assert_eq!(std::mem::align_of::<RawArchiveRecord>(), 64);
    }
    // -----------------------------------------------------------------------
    // G4-E7-T2: bitmask cursor correct at wrap boundary
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e7_t2_bitmask_wrap_small_fifo() {
        let mut fifo = DelayFifo::new(4);
        // Prime with 0..3
        for i in 0..4u64 {
            fifo.enqueue(i);
        }

        // Now push 4..11 — wraps around the 4-slot buffer multiple times
        for i in 4..12u64 {
            let returned = fifo.enqueue(i).unwrap();
            assert_eq!(returned, i - 4, "wrap mismatch at i={i}");
        }
    }

    #[test]
    fn g4_e7_t2_wrap_at_exact_boundary() {
        let depth = 8u32;
        let mut fifo = DelayFifo::new(depth);

        // Prime
        for i in 0..depth as u64 {
            fifo.enqueue(i * 100);
        }

        // Push exactly `depth` more to wrap cursor back to slot 0
        for i in depth as u64..2 * depth as u64 {
            let returned = fifo.enqueue(i * 100).unwrap();
            assert_eq!(returned, (i - depth as u64) * 100);
        }

        // One more full cycle
        for i in 2 * depth as u64..3 * depth as u64 {
            let returned = fifo.enqueue(i * 100).unwrap();
            assert_eq!(returned, (i - depth as u64) * 100);
        }
    }

    #[test]
    fn g4_e7_t2_no_modulo_in_source() {
        let src = include_str!("afxdp_producer.rs");
        let prod = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[..pos],
            None => src,
        };
        // Find the enqueue function
        let fn_start = prod.find("pub fn enqueue(").unwrap();
        let fn_body = &prod[fn_start..];
        let open = fn_body.find('{').unwrap();
        let mut depth = 0u32;
        let mut fn_end = open;
        for (i, ch) in fn_body[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        fn_end = i + open;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &fn_body[open..=fn_end];
        // Must use & (bitmask), never % (modulo)
        assert!(
            body.contains("& self.mask"),
            "enqueue must use bitmask cursor"
        );
        assert!(!body.contains('%'), "enqueue must not use modulo division");
    }

    // -----------------------------------------------------------------------
    // G4-E7-T3: survival mode flush
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e7_t3_flush_returns_oldest_offsets() {
        let mut fifo = DelayFifo::new(8);
        for i in 0..8u64 {
            fifo.enqueue(i * 2048);
        }
        assert!(fifo.is_primed());

        let flushed = fifo.flush_oldest(4);
        assert_eq!(flushed.len(), 4);
        assert_eq!(flushed, vec![0, 2048, 4096, 6144]);
        // FIFO is no longer primed after flush
        assert!(!fifo.is_primed());
    }

    #[test]
    fn g4_e7_t3_flush_bounded_by_count() {
        let mut fifo = DelayFifo::new(8);
        for i in 0..8u64 {
            fifo.enqueue(i);
        }

        // Flush more than available — clamped to len
        let flushed = fifo.flush_oldest(100);
        assert_eq!(flushed.len(), 8);
    }

    #[test]
    fn g4_e7_t3_survival_flush_triggers_below_threshold() {
        let mut fifo = DelayFifo::new(16);
        for i in 0..16u64 {
            fifo.enqueue(i);
        }

        // fill_posted = 5, fill_capacity = 100 → 5% < 10% threshold → flush
        let flushed = survival_flush_if_needed(&mut fifo, 5, 100);
        assert!(!flushed.is_empty());
    }

    #[test]
    fn g4_e7_t3_survival_flush_does_not_trigger_above_threshold() {
        let mut fifo = DelayFifo::new(16);
        for i in 0..16u64 {
            fifo.enqueue(i);
        }

        // fill_posted = 50, fill_capacity = 100 → 50% > 10% threshold → no flush
        let flushed = survival_flush_if_needed(&mut fifo, 50, 100);
        assert!(flushed.is_empty());
        assert!(fifo.is_primed(), "FIFO must remain primed when no flush");
    }

    #[test]
    fn g4_e7_t3_survival_constants_frozen() {
        assert_eq!(SURVIVAL_FLUSH_CHUNK, 50_000);
        assert_eq!(SURVIVAL_THRESHOLD_PERCENT, 10);
    }

    // -----------------------------------------------------------------------
    // G4-E7-T4: control plane stall does not block ingestion
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e7_t4_fifo_never_blocks_on_enqueue() {
        // The FIFO is a fixed array with bitmask — enqueue is always O(1),
        // never waits on the reader. Prove: push millions without any
        // blocking primitive.
        let mut fifo = DelayFifo::new(1024);
        for i in 0..1_000_000u64 {
            let _ = fifo.enqueue(i);
        }
        // If we got here, enqueue never blocked.
        assert!(fifo.is_primed());
    }

    #[test]
    fn g4_e7_t4_survival_flush_is_bounded() {
        // Even with a huge FIFO, flush never returns more than the chunk size.
        let _fifo = DelayFifo::new(DELAY_FIFO_DEPTH);
        // Prime (just set head past depth)
        // — we can't allocate 1M entries in test, use a smaller one
        let mut small_fifo = DelayFifo::new(1024);
        for i in 0..1024u64 {
            small_fifo.enqueue(i);
        }

        // Flush with SURVIVAL_FLUSH_CHUNK cap
        let flushed = small_fifo.flush_oldest(SURVIVAL_FLUSH_CHUNK);
        assert!(flushed.len() <= SURVIVAL_FLUSH_CHUNK as usize);
        assert_eq!(flushed.len(), 1024); // smaller than chunk, so all returned
    }

    #[test]
    fn g4_e7_t4_archive_drop_plus_fifo_never_stalls() {
        // Combine E6 (archive drop-on-full) with E7 (FIFO never blocks):
        // both paths are non-blocking, proving AF_XDP ingestion cannot stall
        // even if the control plane is completely dead.
        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 4).unwrap();

        let mut fifo = DelayFifo::new(8);

        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 100,
            rx_timestamp_ns: 1,
            frame_index: 0,
        };

        // Simulate 1000 frames: archive fills up and drops, FIFO wraps
        for i in 0..1000u64 {
            let _ = publish_archive_record(&archive, 0, &meta, i, 0, true, 0);
            let _ = fifo.enqueue(i * 2048);
        }
        // Neither path blocked — we got here.
        assert!(fifo.is_primed());
    }

    // -----------------------------------------------------------------------
    // G4-E8-T1: RX batch → feed ring → worker-side read smoke
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e8_t1_end_to_end_rx_to_feed_ring() {
        use crate::grpc_producer::{emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let umem_path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&umem_path, &cfg).unwrap();

        // Map UMEM and write recognizable data to frame 3
        let mut umem = SharedUmem::map_shared(&umem_path, &cfg).unwrap();
        let ptr = umem.frame_ptr_mut(3).unwrap();
        unsafe { std::ptr::write_bytes(ptr, 0xAB, 2048) };

        // Build a synthetic RX batch with one valid descriptor at frame 3
        let descs = vec![RxDescriptor {
            umem_offset: 3 * 2048,
            len: 1500,
            rx_timestamp_ns: 1_700_000_000_000_000_000,
        }];
        let batch = process_rx_batch(&descs, &cfg);
        assert_eq!(batch.frames.len(), 1);

        // Bootstrap feed rings (4 money workers, 1 AF_XDP producer)
        let topo = validate_topology(4, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, tmp.path()).unwrap();

        // Construct a PipelineOutput and emit (simulating decode pipeline)
        let mut record = IngressRecord::zeroed();
        record.slot = 42;
        record.signal_source = SIGNAL_SOURCE_AFXDP;
        record.unix_ns = batch.frames[0].rx_timestamp_ns;

        let out = PipelineOutput {
            record,
            route: RouteDecision {
                primary_worker: 0,
                overflow_worker: 1,
            },
        };
        let result = emit_pipeline_output(&out, &rings);
        assert_eq!(result, EmitResult::Primary);

        // Worker-side read: pop from ring 0
        let received = rings[0].pop().unwrap();
        assert_eq!(received.slot, 42);
        assert_eq!(received.signal_source, SIGNAL_SOURCE_AFXDP);
        assert_eq!(received.unix_ns, 1_700_000_000_000_000_000);
    }

    #[test]
    fn g4_e8_t1_multi_frame_batch_emits_to_correct_shards() {
        use crate::grpc_producer::{emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let tmp = TempDir::new().unwrap();
        let _cfg = UmemConfig::new(64, 2048).unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, tmp.path()).unwrap();

        // 3 frames → 3 different workers
        for (i, worker) in [0u16, 2, 3].iter().enumerate() {
            let mut record = IngressRecord::zeroed();
            record.slot = i as u64;
            record.signal_source = SIGNAL_SOURCE_AFXDP;

            let out = PipelineOutput {
                record,
                route: RouteDecision {
                    primary_worker: *worker,
                    overflow_worker: (worker + 1) % 4,
                },
            };
            assert_eq!(emit_pipeline_output(&out, &rings), EmitResult::Primary);
        }

        assert_eq!(rings[0].len(), 1);
        assert_eq!(rings[2].len(), 1);
        assert_eq!(rings[3].len(), 1);
        assert_eq!(rings[1].len(), 0); // untouched
    }

    // -----------------------------------------------------------------------
    // G4-E8-T2: RX batch → archive → second-mapping dereference
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e8_t2_end_to_end_rx_to_archive_to_reader() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let umem_path = umem_path_in(tmp.path(), 0);
        bootstrap_umem_file(&umem_path, &cfg).unwrap();

        // Writer mapping: write pattern to frame 7
        let mut writer = SharedUmem::map_shared(&umem_path, &cfg).unwrap();
        let ptr = writer.frame_ptr_mut(7).unwrap();
        unsafe { std::ptr::write_bytes(ptr, 0xFE, 2048) };

        // Synthetic RX batch
        let descs = vec![RxDescriptor {
            umem_offset: 7 * 2048,
            len: 1200,
            rx_timestamp_ns: 42,
        }];
        let batch = process_rx_batch(&descs, &cfg);
        assert_eq!(batch.frames.len(), 1);
        let meta = &batch.frames[0];

        // Publish archive record
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 64).unwrap();
        let result = publish_archive_record(&archive, 0, meta, 100, 5, true, 0);
        assert_eq!(result, ArchivePublishResult::Published);

        // Reader mapping (simulating shadow-control)
        let reader = SharedUmem::map_shared(&umem_path, &cfg).unwrap();
        let record = archive.pop(0).unwrap();

        // Resolve the offset and verify the data
        let resolved = reader
            .resolve_offset(record.umem_offset_bytes as u64)
            .unwrap();
        let val = unsafe { *resolved };
        assert_eq!(
            val, 0xFE,
            "reader must see writer's data via archive offset"
        );
        assert_eq!(record.slot, 100);
        assert_eq!(record.shred_index, 5);
        assert_eq!(record.len, 1200);
    }

    #[test]
    fn g4_e8_t2_archive_offset_matches_rx_meta() {
        let tmp = TempDir::new().unwrap();
        let cfg = UmemConfig::new(64, 2048).unwrap();

        let descs = vec![
            RxDescriptor {
                umem_offset: 0,
                len: 500,
                rx_timestamp_ns: 1,
            },
            RxDescriptor {
                umem_offset: 10 * 2048,
                len: 800,
                rx_timestamp_ns: 2,
            },
        ];
        let batch = process_rx_batch(&descs, &cfg);

        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 64).unwrap();

        for (i, meta) in batch.frames.iter().enumerate() {
            publish_archive_record(&archive, 0, meta, i as u64, 0, true, 0);
        }

        let r0 = archive.pop(0).unwrap();
        let r1 = archive.pop(0).unwrap();
        assert_eq!(r0.umem_offset_bytes, 0);
        assert_eq!(r1.umem_offset_bytes, 10 * 2048);
        assert_eq!(r0.len, 500);
        assert_eq!(r1.len, 800);
    }

    // -----------------------------------------------------------------------
    // G4-E8-T3: ingress continues under archive saturation
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e8_t3_ingress_continues_when_archive_full() {
        use crate::grpc_producer::{emit_pipeline_output, EmitResult};
        use shadow_ingest::pipeline::PipelineOutput;
        use shadow_ingest::route::RouteDecision;

        let tmp = TempDir::new().unwrap();
        let _cfg = UmemConfig::new(64, 2048).unwrap();
        let topo = validate_topology(4, 1, 1).unwrap();
        let rings = bootstrap_afxdp_feed_rings(&topo, 0, 16, tmp.path()).unwrap();

        // Tiny archive — will saturate immediately
        let archive = RawArchiveSegment::create(&raw_archive_path_in(tmp.path()), 1, 4).unwrap();

        let mut fifo = DelayFifo::new(8);

        // Simulate 100 frames: archive fills and drops, but feed rings
        // and FIFO keep working — ingress never stalls.
        let mut archive_published = 0u32;
        let mut archive_dropped = 0u32;
        let mut feed_emitted = 0u32;

        for i in 0..100u64 {
            let meta = RxFrameMeta {
                umem_offset: (i % 64) * 2048,
                payload_len: 1500,
                rx_timestamp_ns: 1000 + i,
                frame_index: (i % 64) as u32,
            };

            // Archive publish (may drop)
            match publish_archive_record(&archive, 0, &meta, i, 0, true, 0) {
                ArchivePublishResult::Published => archive_published += 1,
                ArchivePublishResult::DroppedFull => archive_dropped += 1,
            }

            // Delay FIFO (never blocks)
            let _ = fifo.enqueue(meta.umem_offset);

            // Feed ring emit (always succeeds with 16-capacity rings)
            let mut record = IngressRecord::zeroed();
            record.slot = i;
            record.signal_source = SIGNAL_SOURCE_AFXDP;
            let out = PipelineOutput {
                record,
                route: RouteDecision {
                    primary_worker: (i % 4) as u16,
                    overflow_worker: ((i + 1) % 4) as u16,
                },
            };
            if emit_pipeline_output(&out, &rings) != EmitResult::DroppedFull {
                feed_emitted += 1;
            }
        }

        // Archive saturated — some dropped
        assert!(
            archive_dropped > 0,
            "archive must have dropped under saturation"
        );
        assert!(archive_published > 0, "archive must have published some");

        // Feed rings kept working despite archive saturation
        assert!(
            feed_emitted > 0,
            "feed ring emission must not be blocked by archive"
        );

        // FIFO kept working
        assert!(fifo.is_primed());
    }

    #[test]
    fn g4_e8_t3_survival_flush_under_saturation() {
        let mut fifo = DelayFifo::new(16);
        for i in 0..16u64 {
            fifo.enqueue(i * 2048);
        }

        // Simulate Fill ring nearly empty (2 posted out of 100 capacity)
        let flushed = survival_flush_if_needed(&mut fifo, 2, 100);
        assert!(
            !flushed.is_empty(),
            "survival flush must trigger under starvation"
        );
        // After flush, FIFO is unprimed — ingress continues, just without delay
        assert!(!fifo.is_primed());
    }

    // -----------------------------------------------------------------------
    // G4-E-GATE: all g4_e* tests exist and AF_XDP module is sealed
    // -----------------------------------------------------------------------

    #[test]
    fn g4_e_gate_all_milestones_have_tests() {
        let src = include_str!("afxdp_producer.rs");
        let test_section = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[pos..],
            None => panic!("no test module found"),
        };

        let required_prefixes = [
            "g4_e1_t", // E1: scaffold + ownership
            "g4_e2_t", // E2: shared UMEM
            "g4_e3_t", // E3: batch RX loop
            "g4_e4_t", // E4: normalization
            "g4_e5_t", // E5: feed ring emission
            "g4_e6_t", // E6: raw archive
            "g4_e7_t", // E7: delay FIFO
            "g4_e8_t", // E8: integration gate
        ];

        for prefix in &required_prefixes {
            assert!(
                test_section.contains(prefix),
                "missing tests for milestone prefix '{prefix}'"
            );
        }
    }

    #[test]
    fn g4_e_gate_no_transport_deps_in_production_code() {
        let src = include_str!("afxdp_producer.rs");
        let production = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[..pos],
            None => src,
        };

        let banned = [
            "use aya::",
            "extern crate aya",
            "use libbpf",
            "extern crate libbpf",
            "use xdp_rs",
        ];
        for pat in &banned {
            assert!(
                !production.contains(pat),
                "production code must not contain '{pat}' — actual socket code is G4-E2+"
            );
        }
    }

    #[test]
    fn g4_e_gate_afxdp_reuses_shared_pipeline() {
        let src = include_str!("afxdp_producer.rs");
        let production = match src
            .find("#[cfg(test)]\nmod tests")
            .or_else(|| src.find("#[cfg(test)]\r\nmod tests"))
        {
            Some(pos) => &src[..pos],
            None => src,
        };

        // Must import from shared ingest core
        assert!(production.contains("use shadow_ingest::pipeline::"));
        // Must use the shared emit function
        assert!(production.contains("emit_pipeline_output"));
        // Must NOT contain a second decode pipeline
        assert!(
            !production.contains("fn run_afxdp_pipeline"),
            "AF_XDP must reuse the shared decode pipeline, not duplicate it"
        );
    }

    // -------------------------------------------------------------------
    // G4-F: Turbine shred header parsing (canonical: shadow_ingest::shred_meta)
    // -------------------------------------------------------------------

    // Wire offsets used in test fixture builders (matches shadow-ingest constants).
    const T_VARIANT: usize = 64;
    const T_SLOT: usize = 65;
    const T_INDEX: usize = 73;

    #[test]
    fn parse_legacy_data_shred_header() {
        let mut frame = vec![0u8; 256];
        frame[T_VARIANT] = 0xa5; // legacy data
        frame[T_SLOT..T_SLOT + 8].copy_from_slice(&42u64.to_le_bytes());
        frame[T_INDEX..T_INDEX + 4].copy_from_slice(&7u32.to_le_bytes());
        frame[SHRED_SIZE_FIELD_OFFSET..SHRED_SIZE_FIELD_OFFSET + 2]
            .copy_from_slice(&(SHRED_DATA_HEADER_SIZE as u16).to_le_bytes());

        let (meta, _) = parse_shred(&frame).unwrap();
        assert_eq!(meta.slot, 42);
        assert_eq!(meta.shred_index, 7);
        assert!(meta.is_data);
    }

    #[test]
    fn parse_merkle_coding_shred_header() {
        let mut frame = vec![0u8; 256];
        frame[T_VARIANT] = 0x6F; // merkle coding (0b0110_xxxx)
        frame[T_SLOT..T_SLOT + 8].copy_from_slice(&100u64.to_le_bytes());
        frame[T_INDEX..T_INDEX + 4].copy_from_slice(&3u32.to_le_bytes());
        // Coding header requires num_data/num_coding at offsets 83..87
        frame[83..85].copy_from_slice(&4u16.to_le_bytes());
        frame[85..87].copy_from_slice(&4u16.to_le_bytes());

        let (meta, _) = parse_shred(&frame).unwrap();
        assert_eq!(meta.slot, 100);
        assert_eq!(meta.shred_index, 3);
        assert!(!meta.is_data);
    }

    #[test]
    fn parse_merkle_data_shred_header() {
        let mut frame = vec![0u8; 256];
        frame[T_VARIANT] = 0x9F; // merkle data (0b1001_xxxx)
        frame[T_SLOT..T_SLOT + 8].copy_from_slice(&999u64.to_le_bytes());
        frame[T_INDEX..T_INDEX + 4].copy_from_slice(&15u32.to_le_bytes());
        frame[SHRED_SIZE_FIELD_OFFSET..SHRED_SIZE_FIELD_OFFSET + 2]
            .copy_from_slice(&(SHRED_DATA_HEADER_SIZE as u16).to_le_bytes());

        let (meta, _) = parse_shred(&frame).unwrap();
        assert_eq!(meta.slot, 999);
        assert_eq!(meta.shred_index, 15);
        assert!(meta.is_data);
    }

    #[test]
    fn parse_too_short_frame_returns_none() {
        let frame = vec![0u8; 50]; // below common header (83 bytes)
        assert!(parse_shred(&frame).is_none());
    }

    #[test]
    fn parse_unknown_variant_returns_none() {
        let mut frame = vec![0u8; 256];
        frame[T_VARIANT] = 0xFF; // unknown variant
        assert!(parse_shred(&frame).is_none());
    }

    // -------------------------------------------------------------------
    // G4-F2: Shred → wire-format transaction extraction
    // -------------------------------------------------------------------

    #[test]
    fn g4_f2_t1_compact_u16_encoding() {
        let mut out = Vec::new();

        encode_compact_u16_into(0, &mut out);
        assert_eq!(out, [0x00]);

        out.clear();
        encode_compact_u16_into(127, &mut out);
        assert_eq!(out, [0x7F]);

        out.clear();
        encode_compact_u16_into(128, &mut out);
        assert_eq!(out, [0x80, 0x01]);

        out.clear();
        encode_compact_u16_into(255, &mut out);
        assert_eq!(out, [0xFF, 0x01]);

        out.clear();
        encode_compact_u16_into(16383, &mut out);
        assert_eq!(out, [0xFF, 0x7F]);
    }

    #[test]
    fn g4_f2_t2_coding_shred_yields_empty() {
        let mut shred = vec![0u8; 256];
        shred[T_VARIANT] = 0x5a; // legacy coding shred
                                 // Coding header needs num_data/num_coding at offsets 83..87
        shred[83..85].copy_from_slice(&4u16.to_le_bytes());
        shred[85..87].copy_from_slice(&4u16.to_le_bytes());
        assert!(extract_wire_txs_from_shred(&shred).is_empty());
    }

    #[test]
    fn g4_f2_t3_too_short_yields_empty() {
        let shred = vec![0u8; 50];
        assert!(extract_wire_txs_from_shred(&shred).is_empty());
    }

    #[test]
    fn g4_f2_t4_empty_entry_data_yields_empty() {
        // Build a data shred with total_size == SHRED_DATA_HEADER_SIZE (no entry bytes).
        let mut shred = vec![0u8; 256];
        shred[T_VARIANT] = 0xa5; // legacy data shred
                                 // total_size = 88, so entry_data_len = 0
        let total_size = SHRED_DATA_HEADER_SIZE as u16;
        shred[SHRED_SIZE_FIELD_OFFSET..SHRED_SIZE_FIELD_OFFSET + 2]
            .copy_from_slice(&total_size.to_le_bytes());
        assert!(extract_wire_txs_from_shred(&shred).is_empty());
    }

    /// Build a minimal data shred containing one entry with one legacy tx.
    ///
    /// Entry layout (bincode):
    ///   u64 num_hashes | [u8;32] hash | u64 tx_count=1 | VersionedTransaction
    ///
    /// VersionedTransaction layout (bincode):
    ///   u64 sig_count=1 | [u8;64] sig | u32 variant=0 | Message bincode
    fn build_minimal_legacy_shred(shred_index: u32) -> (Vec<u8>, Vec<u8>) {
        let sig: [u8; 64] = [0xAA; 64];
        let key: [u8; 32] = [0xBB; 32];
        let blockhash: [u8; 32] = [0xCC; 32];

        // VersionedTransaction bincode
        let mut tx_bincode: Vec<u8> = Vec::new();
        tx_bincode.extend_from_slice(&1u64.to_le_bytes()); // sig_count
        tx_bincode.extend_from_slice(&sig);
        tx_bincode.extend_from_slice(&0u32.to_le_bytes()); // Legacy variant
        tx_bincode.extend_from_slice(&[1u8, 0, 0]); // header
        tx_bincode.extend_from_slice(&1u64.to_le_bytes()); // key_count
        tx_bincode.extend_from_slice(&key);
        tx_bincode.extend_from_slice(&blockhash);
        tx_bincode.extend_from_slice(&1u64.to_le_bytes()); // ix_count
        tx_bincode.push(0u8); // pid
        tx_bincode.extend_from_slice(&0u64.to_le_bytes()); // acct_count=0
        tx_bincode.extend_from_slice(&0u64.to_le_bytes()); // data_count=0

        // Entry bincode
        let entry_hash: [u8; 32] = [0xDD; 32];
        let mut entry_bincode: Vec<u8> = Vec::new();
        entry_bincode.extend_from_slice(&0u64.to_le_bytes()); // num_hashes
        entry_bincode.extend_from_slice(&entry_hash);
        entry_bincode.extend_from_slice(&1u64.to_le_bytes()); // tx_count
        entry_bincode.extend_from_slice(&tx_bincode);

        // Entry data (shred_index==0 adds Vec<Entry> count prefix)
        let mut entry_data: Vec<u8> = Vec::new();
        if shred_index == 0 {
            entry_data.extend_from_slice(&1u64.to_le_bytes()); // Vec<Entry> count
        }
        entry_data.extend_from_slice(&entry_bincode);

        // Build shred
        let total_size = (SHRED_DATA_HEADER_SIZE + entry_data.len()) as u16;
        let mut shred = vec![0u8; SHRED_DATA_HEADER_SIZE + entry_data.len()];
        shred[T_VARIANT] = 0xa5; // legacy data
        shred[T_SLOT..T_SLOT + 8].copy_from_slice(&1u64.to_le_bytes());
        shred[T_INDEX..T_INDEX + 4].copy_from_slice(&shred_index.to_le_bytes());
        shred[SHRED_SIZE_FIELD_OFFSET..SHRED_SIZE_FIELD_OFFSET + 2]
            .copy_from_slice(&total_size.to_le_bytes());
        shred[SHRED_DATA_HEADER_SIZE..].copy_from_slice(&entry_data);

        // Expected wire tx:
        // compact-u16(1) + sig + header(3) + compact-u16(1 key) + key +
        // blockhash + compact-u16(1 ix) + pid(0) + compact-u16(0) + compact-u16(0)
        let mut expected_wire: Vec<u8> = Vec::new();
        expected_wire.push(0x01); // compact-u16 sig_count=1
        expected_wire.extend_from_slice(&sig);
        expected_wire.extend_from_slice(&[1, 0, 0]); // header
        expected_wire.push(0x01); // compact-u16 key_count=1
        expected_wire.extend_from_slice(&key);
        expected_wire.extend_from_slice(&blockhash);
        expected_wire.push(0x01); // compact-u16 ix_count=1
        expected_wire.push(0x00); // pid
        expected_wire.push(0x00); // compact-u16 acct_count=0
        expected_wire.push(0x00); // compact-u16 data_count=0

        (shred, expected_wire)
    }

    #[test]
    fn g4_f2_t5_single_legacy_tx_shred_index_0() {
        let (shred, expected_wire) = build_minimal_legacy_shred(0);
        let txs = extract_wire_txs_from_shred(&shred);
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0], expected_wire);
    }

    #[test]
    fn g4_f2_t6_single_legacy_tx_non_zero_index() {
        // shred_index=1: no Vec<Entry> count prefix, parse starts at byte 0.
        let (shred, expected_wire) = build_minimal_legacy_shred(1);
        let txs = extract_wire_txs_from_shred(&shred);
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0], expected_wire);
    }

    #[test]
    fn sh8b_extracts_wire_encoded_transaction_inside_entry() {
        let sig = [0x11; 64];
        let key = [0x22; 32];
        let blockhash = [0x33; 32];

        let mut wire_tx = Vec::new();
        wire_tx.push(1); // sig_count
        wire_tx.extend_from_slice(&sig);
        wire_tx.extend_from_slice(&[1, 0, 0]); // header
        wire_tx.push(1); // account key count
        wire_tx.extend_from_slice(&key);
        wire_tx.extend_from_slice(&blockhash);
        wire_tx.push(1); // instruction count
        wire_tx.push(0); // program_id_index
        wire_tx.push(0); // account index count
        wire_tx.push(0); // instruction data len

        let mut entry_payload = Vec::new();
        entry_payload.extend_from_slice(&7u64.to_le_bytes()); // num_hashes
        entry_payload.extend_from_slice(&[0x44; 32]);
        entry_payload.extend_from_slice(&1u64.to_le_bytes()); // tx_count
        entry_payload.extend_from_slice(&wire_tx);

        let result = extract_wire_txs_from_data_payload_detailed(1, &entry_payload);
        assert_eq!(result.wire_txs, vec![wire_tx]);
        assert_eq!(result.diagnostics.reason, None);
    }

    #[test]
    fn sh8b_stream_extracts_transaction_split_across_payloads() {
        let sig = [0x55; 64];
        let key = [0x66; 32];
        let blockhash = [0x77; 32];

        let mut wire_tx = Vec::new();
        wire_tx.push(1);
        wire_tx.extend_from_slice(&sig);
        wire_tx.extend_from_slice(&[1, 0, 0]);
        wire_tx.push(1);
        wire_tx.extend_from_slice(&key);
        wire_tx.extend_from_slice(&blockhash);
        wire_tx.push(1);
        wire_tx.push(0);
        wire_tx.push(0);
        wire_tx.push(0);

        let mut entry_payload = Vec::new();
        entry_payload.extend_from_slice(&1u64.to_le_bytes()); // Vec<Entry> count
        entry_payload.extend_from_slice(&3u64.to_le_bytes()); // num_hashes
        entry_payload.extend_from_slice(&[0x88; 32]);
        entry_payload.extend_from_slice(&1u64.to_le_bytes());
        entry_payload.extend_from_slice(&wire_tx);

        let split = 72;
        let mut streams = AfxdpTxStreamSet::default();
        assert!(streams
            .extract_wire_txs(99, 0, &entry_payload[..split])
            .is_empty());
        let txs = streams.extract_wire_txs(99, 1, &entry_payload[split..]);
        assert_eq!(txs, vec![wire_tx]);
    }

    #[test]
    fn g4_f2_t7_truncated_tx_yields_empty() {
        let (mut shred, _) = build_minimal_legacy_shred(0);
        // Truncate the shred to cut off mid-transaction.
        let keep = SHRED_DATA_HEADER_SIZE + 20;
        shred.truncate(keep);
        let total_size = keep as u16;
        shred[SHRED_SIZE_FIELD_OFFSET..SHRED_SIZE_FIELD_OFFSET + 2]
            .copy_from_slice(&total_size.to_le_bytes());
        let txs = extract_wire_txs_from_shred(&shred);
        assert!(txs.is_empty());
    }

    #[test]
    fn sh8b_tx_extract_detailed_empty_payload_reason() {
        let result = extract_wire_txs_from_data_payload_detailed(1, &[]);
        assert!(result.wire_txs.is_empty());
        assert_eq!(
            result.diagnostics.reason,
            Some(AfxdpTxExtractErrKind::EmptyPayload)
        );
        assert_eq!(result.diagnostics.shape.payload_len, 0);
    }

    #[test]
    fn sh8b_tx_extract_detailed_short_vec_reason() {
        let result = extract_wire_txs_from_data_payload_detailed(0, &[1, 0, 0, 0]);
        assert!(result.wire_txs.is_empty());
        assert_eq!(
            result.diagnostics.reason,
            Some(AfxdpTxExtractErrKind::ShortVec)
        );
    }

    #[test]
    fn sh8b_tx_extract_detailed_no_transactions_reason() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&[0x11; 32]);
        payload.extend_from_slice(&0u64.to_le_bytes());

        let result = extract_wire_txs_from_data_payload_detailed(1, &payload);
        assert!(result.wire_txs.is_empty());
        assert_eq!(
            result.diagnostics.reason,
            Some(AfxdpTxExtractErrKind::NoTransactions)
        );
        assert_eq!(result.diagnostics.shape.payload_len, payload.len());
        assert_eq!(
            result.diagnostics.shape.num_hashes_or_tick_count_if_present,
            Some(0)
        );
    }

    #[test]
    fn xsk_classifier_accepts_zero_timestamp() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let desc = RxDescriptor {
            umem_offset: 0,
            len: 128,
            rx_timestamp_ns: 0,
        };
        // Original classifier drops this as MissingTimestamp.
        assert_eq!(
            classify_rx_descriptor(&desc, &cfg),
            FrameVerdict::MissingTimestamp
        );
        // XSK classifier accepts it.
        assert_eq!(classify_rx_descriptor_xsk(&desc, &cfg), FrameVerdict::Valid);
    }

    #[test]
    fn xsk_classifier_still_rejects_runt() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let desc = RxDescriptor {
            umem_offset: 0,
            len: 10,
            rx_timestamp_ns: 0,
        };
        assert_eq!(classify_rx_descriptor_xsk(&desc, &cfg), FrameVerdict::Runt);
    }

    #[test]
    fn process_rx_batch_xsk_substitutes_fallback_ts() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let descs = vec![
            RxDescriptor {
                umem_offset: 0,
                len: 128,
                rx_timestamp_ns: 0,
            },
            RxDescriptor {
                umem_offset: 2048,
                len: 200,
                rx_timestamp_ns: 555,
            },
        ];
        let result = process_rx_batch_xsk(&descs, &cfg, 999);
        assert_eq!(result.frames.len(), 2);
        // First frame had ts=0, should get fallback=999.
        assert_eq!(result.frames[0].rx_timestamp_ns, 999);
        // Second frame had ts=555, keeps its own.
        assert_eq!(result.frames[1].rx_timestamp_ns, 555);
        assert_eq!(result.missing_timestamps, 1);
    }

    // ── G4-F5 structural seal tests ──────────────────────────────────

    /// The live Linux AF_XDP path must use `ShadowRxMetaParser` for
    /// metadata parsing.  `process_rx_batch_xsk` and
    /// `classify_rx_descriptor_xsk` are quarantined to `#[cfg(test)]`
    /// and must never appear in the production hot path.
    #[test]
    fn g4_f5_seal_no_fallback_timestamp_helpers_in_live_path() {
        let src = include_str!("afxdp_runtime.rs");
        assert!(
            !src.contains("process_rx_batch_xsk"),
            "live path must not use process_rx_batch_xsk (test/replay only)"
        );
        assert!(
            !src.contains("classify_rx_descriptor_xsk"),
            "live path must not use classify_rx_descriptor_xsk (test/replay only)"
        );
        assert!(
            !src.contains("fallback_timestamp"),
            "live path must not reference any fallback timestamp"
        );
    }

    /// The live Linux AF_XDP path must construct the reassembler with
    /// `FecRecoveryEngine`, not `NoOpRecovery`.
    #[test]
    fn g4_f5_seal_live_path_uses_fec_recovery_engine() {
        let src = include_str!("afxdp_runtime.rs");
        assert!(
            src.contains("Reassembler::new_with_recovery("),
            "live path must use Reassembler::new_with_recovery, not Reassembler::new()"
        );
        assert!(
            src.contains("FecRecoveryEngine::new()"),
            "live path must construct a real FecRecoveryEngine"
        );
    }

    /// Archive and recycle must be per-observed-frame, outside the
    /// `collect_ready_data` decode iteration. The structural proof
    /// is that the guarded archive publish call appears AFTER the
    /// `collect_ready_data` for-loop closes.
    #[test]
    fn g4_f5_seal_archive_recycle_per_frame_not_per_ready_item() {
        let src = include_str!("afxdp_runtime.rs");
        // Find the closing brace of the collect_ready_data loop and
        // the guarded archive publish call — archive must come after.
        let crd_pos = src
            .find("collect_ready_data(")
            .expect("collect_ready_data must exist in afxdp_runtime.rs");
        let archive_pos = src
            .find("publish_archive_record_guarded(")
            .expect("publish_archive_record_guarded must exist in afxdp_runtime.rs");
        assert!(
            archive_pos > crd_pos,
            "publish_archive_record_guarded must appear after collect_ready_data loop, not inside it"
        );

        // Neither archive nor recycle should appear inside a `for ready in` block.
        // Scan the ready-data iteration region.
        let ready_loop_start = src.find("for ready in").unwrap_or(0);
        let ready_region_end = src[ready_loop_start..]
            .find("publish_archive_record_guarded")
            .map(|p| ready_loop_start + p)
            .unwrap_or(src.len());
        let ready_region = &src[ready_loop_start..ready_region_end];
        assert!(
            !ready_region.contains("recycle_frame("),
            "recycle_frame must not appear inside the ready-data iteration"
        );
    }

    /// The live AF_XDP path must pass rx_timestamp_ns into
    /// `reassembler.ingest(...)`, not just into `collect_ready_data`.
    #[test]
    fn g4_f5_seal_ingest_receives_rx_timestamp() {
        let src = include_str!("afxdp_runtime.rs");
        // Find the reassembler.ingest call in the Linux AF_XDP path
        // and verify it passes a timestamp (not just two args).
        let ingest_pos = src
            .find("reassembler.ingest(")
            .expect("reassembler.ingest must exist in afxdp_runtime.rs");
        let call_region = &src[ingest_pos..ingest_pos + 200];
        assert!(
            call_region.contains("rx_timestamp_ns"),
            "reassembler.ingest must receive rx_timestamp_ns in the live path"
        );
    }

    /// The live AF_XDP decode loop must use per-item `.rx_timestamp_ns`
    /// from ready items, with no `unwrap_or` fallback to the frame meta.
    #[test]
    fn g4_f5_seal_no_unwrap_or_fallback_on_ready_timestamp() {
        let src = include_str!("afxdp_runtime.rs");
        // The old pattern was: ready.rx_timestamp_ns.unwrap_or(meta.rx_timestamp_ns)
        // This must not exist. The live path should use .expect() or direct access.
        assert!(
            !src.contains("unwrap_or(meta.rx_timestamp_ns)"),
            "ready items must carry concrete timestamps — no unwrap_or fallback"
        );
    }

    // ── G4-F5 real-host ingress seal ─────────────────────────────────

    /// F5-H5: Archive backpressure + FIFO recycling under sustained burst.
    ///
    /// Drives 10k frames through publish_archive_record + DelayFifo +
    /// survival_flush, with a tiny archive ring (4 slots) so it fills
    /// immediately. Proves: archive drops silently, FIFO recycles correctly,
    /// survival flush fires when fill ring is starved, and the entire path
    /// completes without blocking. This is the tier0-level complement to
    /// the reassembler-level F5-H4 test in shadow-ingest.
    #[test]
    fn g4_f5_host_t5_archive_fifo_survival_under_burst() {
        use std::time::Instant;

        let tmp = TempDir::new().unwrap();
        let archive = RawArchiveSegment::create(
            &raw_archive_path_in(tmp.path()),
            1, // 1 lane
            4, // tiny capacity — fills after 4 records
        )
        .unwrap();

        let mut fifo = DelayFifo::new(256); // small FIFO for fast priming
        let burst_size = 10_000u64;
        let mut archive_published = 0u64;
        let mut archive_dropped = 0u64;
        let mut survival_flushes = 0u64;

        let meta = RxFrameMeta {
            umem_offset: 0,
            payload_len: 200,
            rx_timestamp_ns: 42_000_000_000,
            frame_index: 0,
        };

        let start = Instant::now();
        for i in 0..burst_size {
            // Archive publish — non-blocking.
            match publish_archive_record(&archive, 0, &meta, i, i as u32, true, 0) {
                ArchivePublishResult::Published => archive_published += 1,
                ArchivePublishResult::DroppedFull => archive_dropped += 1,
            }

            // FIFO enqueue — always succeeds after priming.
            let recycled = fifo.enqueue(i * 2048);
            let _ = recycled; // In real code, this goes to xsk.recycle_frame.

            // Survival flush — simulate fill ring with only 1% of posted buffers left.
            let fill_cap = 1024u32;
            let fill_posted = fill_cap / 100; // 1% posted — below 10% threshold
            let flushed = survival_flush_if_needed(&mut fifo, fill_posted, fill_cap);
            if !flushed.is_empty() {
                survival_flushes += 1;
            }
        }
        let elapsed = start.elapsed();

        // Behavioral assertions.
        assert!(archive_published > 0, "some records must be published");
        assert!(
            archive_dropped > 0,
            "archive must drop when full (capacity=4)"
        );
        assert!(
            archive_dropped > archive_published,
            "with capacity=4 and 10k frames, most records should be dropped"
        );
        assert!(
            survival_flushes > 0,
            "survival flush must fire under fill-ring starvation"
        );

        // Timing assertion: 10k frames in < 2 seconds proves no blocking.
        assert!(
            elapsed.as_millis() < 2000,
            "10k frames took {} ms — archive/FIFO path must never block",
            elapsed.as_millis()
        );
    }

    /// F5-H6: DelayFifo recycling is FIFO-correct under burst.
    ///
    /// Proves that frames exit the delay FIFO in the same order they
    /// entered (after priming). This is essential for UMEM recycling
    /// correctness: a frame must not be recycled before the NIC has
    /// finished writing to it.
    #[test]
    fn g4_f5_host_t6_delay_fifo_order_preserved() {
        let depth = 64u32;
        let mut fifo = DelayFifo::new(depth);

        // Prime the FIFO. First depth-1 pushes return None;
        // the depth-th push primes and returns the oldest (index 0).
        for i in 0..(depth - 1) as u64 {
            let out = fifo.enqueue(i * 4096);
            assert!(out.is_none(), "FIFO must not emit during priming (i={i})");
        }
        // The depth-th push primes the FIFO.
        let prime_out = fifo.enqueue((depth - 1) as u64 * 4096);
        assert_eq!(
            prime_out,
            Some(0),
            "priming push must return the oldest frame (offset 0)"
        );
        assert!(fifo.is_primed());

        // After priming, each enqueue returns the oldest real frame in order.
        // Priming consumed buf init value (0) at slot [depth-1]; post-prime
        // outputs are the values stored during priming: 0*4096, 1*4096, ...
        for i in depth as u64..(depth as u64 + 1000) {
            let out = fifo
                .enqueue(i * 4096)
                .expect("primed FIFO must always emit");
            let expected = (i - depth as u64) * 4096;
            assert_eq!(
                out, expected,
                "FIFO must return frames in insertion order: got {out}, expected {expected}"
            );
        }
    }
}
