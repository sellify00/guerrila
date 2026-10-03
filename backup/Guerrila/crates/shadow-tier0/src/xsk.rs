//! AF_XDP / XSK transport body for Shadow Tier-0.
//!
//! Design boundary:
//! - transport only: socket + UMEM registration + ring mmap + RX polling + fill recycle
//! - no Turbine / Solana / Raydium parsing here
//! - no feed-ring emission here
//! - assumes an external XDP program is already loaded and attached to the NIC
//! - optionally registers the created XSK fd into a pinned XSKMAP
//!
//! This module is meant to replace the current bind-retry scaffold in
//! `shadow-tier0/src/main.rs` and feed `afxdp_producer.rs` with real RX data.
//!
//! Important metadata contract:
//! ----------------------------
//! The current `shadow-tier0` scaffold expects hardware RX timestamps to be
//! available per frame, but AF_XDP does not magically synthesize those in
//! userspace. The attached XDP program must read the NIC RX timestamp via
//! XDP RX-metadata support and write a small fixed header into metadata
//! headroom before redirecting the frame into the XSK.
//!
//! This file therefore expects the XDP program to prepend `ShadowRxMetaV1`
//! immediately before the packet start inside the UMEM chunk. `xdp_desc.addr`
//! points at the packet start (not the chunk start), so this module aligns the
//! address down to the chunk boundary for the project's cross-process
//! `umem_offset_bytes` contract and decodes the metadata from the headroom.
//!
//! The returned `XskRxFrame` keeps:
//! - `frame_offset`: absolute, frame-aligned offset in the shared UMEM file
//! - `packet_offset`: byte offset from frame start to packet data
//! - `packet_len`
//! - `rx_timestamp_ns`
//!
//! This preserves the existing Shadow contracts:
//! - archive / delay FIFO / recycle operate on frame-aligned offsets
//! - packet parsing remains in `afxdp_producer.rs`
//! - no transport code leaks into `shadow-ingest`

use crate::afxdp_producer::{RxDescriptor, SharedUmem, UmemConfig};
#[cfg(target_os = "linux")]
use libbpf_rs::{MapCore, MapFlags, MapHandle};
use std::ffi::CString;
use std::io;
use std::mem::size_of;
use std::os::fd::RawFd;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::atomic::{fence, Ordering};

// -----------------------------------------------------------------------------
// Linux UAPI constants and structs (from linux/if_xdp.h and linux/bpf.h)
// -----------------------------------------------------------------------------

const AF_XDP: i32 = 44;
const PF_XDP: i32 = AF_XDP;
const SOL_XDP: i32 = 283;

const XDP_SHARED_UMEM: u16 = 1 << 0;
const XDP_COPY: u16 = 1 << 1;
const XDP_ZEROCOPY: u16 = 1 << 2;
const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;
const XDP_USE_SG: u16 = 1 << 4;

const XDP_RING_NEED_WAKEUP: u32 = 1 << 0;

const XDP_MMAP_OFFSETS: i32 = 1;
const XDP_RX_RING: i32 = 2;
const XDP_TX_RING: i32 = 3;
const XDP_UMEM_REG: i32 = 4;
const XDP_UMEM_FILL_RING: i32 = 5;
const XDP_UMEM_COMPLETION_RING: i32 = 6;
const XDP_OPTIONS: i32 = 8;

const XDP_OPTIONS_ZEROCOPY: u32 = 1 << 0;

const XDP_PGOFF_RX_RING: libc::off_t = 0;
const XDP_UMEM_PGOFF_FILL_RING: libc::off_t = 0x1_0000_0000u64 as libc::off_t;

const XDP_PKT_CONTD: u32 = 1 << 0;

#[cfg(not(target_os = "linux"))]
const BPF_MAP_UPDATE_ELEM: libc::c_uint = 2;
#[cfg(not(target_os = "linux"))]
const BPF_OBJ_GET: libc::c_uint = 7;
#[cfg(not(target_os = "linux"))]
const BPF_ANY: u64 = 0;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SockaddrXdp {
    sxdp_family: u16,
    sxdp_flags: u16,
    sxdp_ifindex: u32,
    sxdp_queue_id: u32,
    sxdp_shared_umem_fd: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct XdpRingOffset {
    producer: u64,
    consumer: u64,
    desc: u64,
    flags: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct XdpMmapOffsets {
    rx: XdpRingOffset,
    tx: XdpRingOffset,
    fr: XdpRingOffset,
    cr: XdpRingOffset,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct XdpUmemReg {
    addr: u64,
    len: u64,
    chunk_size: u32,
    headroom: u32,
    flags: u32,
    tx_metadata_len: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct XdpOptions {
    flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct XdpDesc {
    addr: u64,
    len: u32,
    options: u32,
}

#[cfg(not(target_os = "linux"))]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfAttrObjGet {
    pathname: u64,
    bpf_fd: u32,
    file_flags: u32,
    path_fd: u32,
}

#[cfg(not(target_os = "linux"))]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfAttrMapUpdateElem {
    map_fd: u32,
    _pad: u32,
    key: u64,
    value: u64,
    flags: u64,
}

// -----------------------------------------------------------------------------
// Public config / metadata types
// -----------------------------------------------------------------------------

/// Disjoint frame subrange of the shared UMEM file assigned to one queue.
///
/// This lets multiple AF_XDP producers use the same hugepage-backed UMEM file
/// without handing the same frames to multiple queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLease {
    pub start_frame: u32,
    pub frame_count: u32,
}

impl FrameLease {
    pub fn whole(umem: &UmemConfig) -> Self {
        Self {
            start_frame: 0,
            frame_count: umem.frames,
        }
    }

    /// Split the shared UMEM evenly by queue index.
    ///
    /// The last queue receives any remainder.
    pub fn split_evenly(
        umem: &UmemConfig,
        queue_id: u32,
        queue_count: u32,
    ) -> Result<Self, XskError> {
        if queue_count == 0 {
            return Err(XskError::InvalidLease("queue_count must be > 0"));
        }
        if queue_id >= queue_count {
            return Err(XskError::InvalidLease("queue_id >= queue_count"));
        }

        let base = umem.frames / queue_count;
        let rem = umem.frames % queue_count;
        let extra_before = queue_id.min(rem);
        let start_frame = queue_id * base + extra_before;
        let frame_count = base + if queue_id < rem { 1 } else { 0 };

        if frame_count == 0 {
            return Err(XskError::InvalidLease("computed zero-frame lease"));
        }

        Ok(Self {
            start_frame,
            frame_count,
        })
    }

    #[inline(always)]
    pub fn start_offset_bytes(&self, umem: &UmemConfig) -> u64 {
        self.start_frame as u64 * umem.frame_size as u64
    }

    #[inline(always)]
    pub fn len_bytes(&self, umem: &UmemConfig) -> u64 {
        self.frame_count as u64 * umem.frame_size as u64
    }

    #[inline(always)]
    pub fn contains_abs_offset(&self, umem: &UmemConfig, abs_offset: u64) -> bool {
        let start = self.start_offset_bytes(umem);
        let end = start + self.len_bytes(umem);
        abs_offset >= start && abs_offset < end
    }
}

/// RX-only XSK bind configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XskBindConfig {
    pub ifindex: u32,
    pub queue_id: u32,
    pub rx_entries: u32,
    pub fill_entries: u32,
    /// Headroom reserved in the registered UMEM region.
    ///
    /// The attached XDP program must use this headroom to stash RX metadata.
    pub headroom: u32,
    /// Flags for `sockaddr_xdp.sxdp_flags`.
    pub bind_flags: u16,
    /// The disjoint frame subrange this socket may use.
    pub lease: FrameLease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XskBindMode {
    ZeroCopy,
    Copy,
}

impl XskBindMode {
    #[inline(always)]
    fn bind_flags(self) -> u16 {
        match self {
            Self::ZeroCopy => XDP_ZEROCOPY | XDP_USE_NEED_WAKEUP,
            Self::Copy => XDP_COPY | XDP_USE_NEED_WAKEUP,
        }
    }
}

impl XskBindConfig {
    pub const DEFAULT_RX_ENTRIES: u32 = 4096;
    pub const DEFAULT_FILL_ENTRIES: u32 = 4096;
    pub const DEFAULT_HEADROOM: u32 = 256;

    pub fn rx_only(ifindex: u32, queue_id: u32, lease: FrameLease) -> Self {
        Self::rx_only_with_bind_mode(ifindex, queue_id, lease, XskBindMode::ZeroCopy)
    }

    pub fn rx_only_with_bind_mode(
        ifindex: u32,
        queue_id: u32,
        lease: FrameLease,
        bind_mode: XskBindMode,
    ) -> Self {
        Self {
            ifindex,
            queue_id,
            rx_entries: Self::DEFAULT_RX_ENTRIES,
            fill_entries: Self::DEFAULT_FILL_ENTRIES,
            headroom: Self::DEFAULT_HEADROOM,
            bind_flags: bind_mode.bind_flags(),
            lease,
        }
    }
}

/// Metadata written by the external XDP program into metadata headroom.
///
/// The XDP program should:
/// 1. call `bpf_xdp_metadata_rx_timestamp()`
/// 2. `bpf_xdp_adjust_meta()` to make room in the headroom
/// 3. write this header immediately before packet data
/// 4. redirect via `bpf_redirect_map(xsks_map, queue_id, 0)`
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowRxMetaV1 {
    pub magic: u32,
    pub version: u16,
    pub header_len: u16,
    pub rx_timestamp_ns: u64,
}

pub const SHADOW_RX_META_MAGIC: u32 = 0x314d5853; // "SXM1" in little-endian memory order
pub const SHADOW_RX_META_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedRxMeta {
    pub rx_timestamp_ns: u64,
    pub metadata_len: u16,
}

/// Parser for the custom metadata headroom immediately preceding packet bytes.
pub trait RxMetadataParser {
    fn parse(
        &self,
        umem: &SharedUmem,
        frame_offset: u64,
        packet_offset: u16,
        frame_size: u32,
    ) -> Result<DecodedRxMeta, XskError>;
}

/// Default Shadow metadata parser.
pub struct ShadowRxMetaParser;

impl RxMetadataParser for ShadowRxMetaParser {
    fn parse(
        &self,
        umem: &SharedUmem,
        frame_offset: u64,
        packet_offset: u16,
        frame_size: u32,
    ) -> Result<DecodedRxMeta, XskError> {
        let header_len = size_of::<ShadowRxMetaV1>();
        let packet_offset = packet_offset as usize;
        if packet_offset < header_len {
            return Err(XskError::MissingMetadataHeadroom {
                packet_offset: packet_offset as u16,
                required: header_len as u16,
            });
        }

        let frame_ptr = umem
            .resolve_offset(frame_offset)
            .ok_or(XskError::BadUmemOffset(frame_offset))?;
        let frame = unsafe { slice::from_raw_parts(frame_ptr, frame_size as usize) };

        let meta_start = packet_offset - header_len;
        let meta = unsafe {
            ptr::read_unaligned(
                frame[meta_start..meta_start + header_len].as_ptr() as *const ShadowRxMetaV1
            )
        };

        if meta.magic != SHADOW_RX_META_MAGIC {
            return Err(XskError::BadMetadataMagic(meta.magic));
        }
        if meta.version != SHADOW_RX_META_VERSION {
            return Err(XskError::BadMetadataVersion(meta.version));
        }
        if meta.header_len < header_len as u16 || meta.header_len as usize > packet_offset {
            return Err(XskError::BadMetadataLen(meta.header_len));
        }
        if meta.rx_timestamp_ns == 0 {
            return Err(XskError::MissingHardwareTimestamp);
        }

        Ok(DecodedRxMeta {
            rx_timestamp_ns: meta.rx_timestamp_ns,
            metadata_len: meta.header_len,
        })
    }
}

/// One drained RX frame, normalized for the existing Shadow ingest shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XskRxFrame {
    /// Absolute, frame-aligned byte offset in the shared UMEM file.
    pub frame_offset: u64,
    /// Byte offset from `frame_offset` to the first packet byte.
    pub packet_offset: u16,
    /// Length of the packet bytes (not including metadata headroom).
    pub packet_len: u32,
    /// Length of the custom metadata prefix immediately before packet bytes.
    pub metadata_len: u16,
    /// RX hardware timestamp propagated from the XDP metadata header.
    pub rx_timestamp_ns: u64,
}

impl XskRxFrame {
    /// Convert to the existing `afxdp_producer::RxDescriptor` contract.
    ///
    /// Important: `umem_offset` is the frame start, not the packet start.
    #[inline(always)]
    pub fn as_rx_descriptor(&self) -> RxDescriptor {
        RxDescriptor {
            umem_offset: self.frame_offset,
            len: self.packet_len,
            rx_timestamp_ns: self.rx_timestamp_ns,
        }
    }

    /// Borrow the packet payload as a slice from the shared UMEM mapping.
    pub fn payload<'a>(&self, umem: &'a SharedUmem) -> Option<&'a [u8]> {
        let frame_ptr = umem.resolve_offset(self.frame_offset)?;
        let start = unsafe { frame_ptr.add(self.packet_offset as usize) };
        Some(unsafe { slice::from_raw_parts(start, self.packet_len as usize) })
    }

    /// Borrow the metadata prefix as a slice from the shared UMEM mapping.
    pub fn metadata<'a>(&self, umem: &'a SharedUmem) -> Option<&'a [u8]> {
        if self.metadata_len == 0 || self.packet_offset < self.metadata_len {
            return None;
        }
        let frame_ptr = umem.resolve_offset(self.frame_offset)?;
        let start = unsafe { frame_ptr.add((self.packet_offset - self.metadata_len) as usize) };
        Some(unsafe { slice::from_raw_parts(start, self.metadata_len as usize) })
    }
}

#[derive(Debug, Default)]
pub struct XskDrainBatch {
    pub frames: Vec<XskRxFrame>,
    pub multi_buffer_drops: u32,
    pub bad_bounds_drops: u32,
    pub metadata_drops: u32,
}

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub enum XskError {
    InvalidConfig(&'static str),
    InvalidLease(&'static str),
    CString(std::ffi::NulError),
    Io(io::Error),
    BadUmemOffset(u64),
    MissingMetadataHeadroom { packet_offset: u16, required: u16 },
    BadMetadataMagic(u32),
    BadMetadataVersion(u16),
    BadMetadataLen(u16),
    MissingHardwareTimestamp,
    PollTimeout,
    NotZeroCopy,
    BpfMapOpen(io::Error),
    BpfMapUpdate(io::Error),
}

impl std::fmt::Display for XskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(msg) => write!(f, "invalid XSK config: {msg}"),
            Self::InvalidLease(msg) => write!(f, "invalid UMEM lease: {msg}"),
            Self::CString(e) => write!(f, "invalid C string: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::BadUmemOffset(off) => write!(f, "bad UMEM offset: {off}"),
            Self::MissingMetadataHeadroom {
                packet_offset,
                required,
            } => write!(
                f,
                "packet offset {packet_offset} too small for metadata header ({required} bytes)"
            ),
            Self::BadMetadataMagic(m) => write!(f, "bad RX metadata magic: 0x{m:08x}"),
            Self::BadMetadataVersion(v) => write!(f, "bad RX metadata version: {v}"),
            Self::BadMetadataLen(v) => write!(f, "bad RX metadata header_len: {v}"),
            Self::MissingHardwareTimestamp => write!(f, "missing RX hardware timestamp"),
            Self::PollTimeout => write!(f, "poll timeout"),
            Self::NotZeroCopy => {
                write!(f, "XSK bind succeeded but socket is not in zero-copy mode")
            }
            Self::BpfMapOpen(e) => write!(f, "failed to open pinned XSKMAP: {e}"),
            Self::BpfMapUpdate(e) => write!(f, "failed to update XSKMAP: {e}"),
        }
    }
}

impl std::error::Error for XskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CString(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::BpfMapOpen(e) => Some(e),
            Self::BpfMapUpdate(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for XskError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<std::ffi::NulError> for XskError {
    fn from(value: std::ffi::NulError) -> Self {
        Self::CString(value)
    }
}

// -----------------------------------------------------------------------------
// Mmap-backed ring helpers
// -----------------------------------------------------------------------------

struct RingMmap {
    ptr: NonNull<libc::c_void>,
    len: usize,
}

impl RingMmap {
    unsafe fn map(fd: RawFd, pgoff: libc::off_t, len: usize) -> Result<Self, XskError> {
        let raw = libc::mmap(
            ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            pgoff,
        );
        if raw == libc::MAP_FAILED {
            return Err(XskError::Io(io::Error::last_os_error()));
        }
        Ok(Self {
            ptr: NonNull::new_unchecked(raw),
            len,
        })
    }

    #[inline(always)]
    fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr() as *mut u8
    }
}

impl Drop for RingMmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr(), self.len);
        }
    }
}

struct RingBase {
    map: RingMmap,
    producer: *mut u32,
    consumer: *mut u32,
    flags: *mut u32,
    entries: u32,
    mask: u32,
}

impl RingBase {
    unsafe fn new<T>(
        fd: RawFd,
        pgoff: libc::off_t,
        off: &XdpRingOffset,
        entries: u32,
    ) -> Result<(Self, *mut T), XskError> {
        let len = off.desc as usize + entries as usize * size_of::<T>();
        let map = RingMmap::map(fd, pgoff, len)?;
        let base = map.as_ptr();
        let producer = base.add(off.producer as usize) as *mut u32;
        let consumer = base.add(off.consumer as usize) as *mut u32;
        let flags = base.add(off.flags as usize) as *mut u32;
        let desc = base.add(off.desc as usize) as *mut T;
        Ok((
            Self {
                map,
                producer,
                consumer,
                flags,
                entries,
                mask: entries - 1,
            },
            desc,
        ))
    }

    #[inline(always)]
    fn producer(&self) -> u32 {
        unsafe { ptr::read_volatile(self.producer) }
    }

    #[inline(always)]
    fn consumer(&self) -> u32 {
        unsafe { ptr::read_volatile(self.consumer) }
    }

    #[inline(always)]
    fn set_producer(&self, value: u32) {
        fence(Ordering::Release);
        unsafe { ptr::write_volatile(self.producer, value) };
    }

    #[inline(always)]
    fn set_consumer(&self, value: u32) {
        fence(Ordering::Release);
        unsafe { ptr::write_volatile(self.consumer, value) };
    }

    #[inline(always)]
    fn needs_wakeup(&self) -> bool {
        let flags = unsafe { ptr::read_volatile(self.flags) };
        (flags & XDP_RING_NEED_WAKEUP) != 0
    }
}

struct FillRing {
    base: RingBase,
    desc: *mut u64,
}

impl FillRing {
    unsafe fn new(fd: RawFd, off: &XdpRingOffset, entries: u32) -> Result<Self, XskError> {
        let (base, desc) = RingBase::new(fd, XDP_UMEM_PGOFF_FILL_RING, off, entries)?;
        Ok(Self { base, desc })
    }

    #[inline(always)]
    fn free_slots(&self) -> u32 {
        self.base.entries - self.base.producer().wrapping_sub(self.base.consumer())
    }

    #[inline(always)]
    fn push(&mut self, rel_addr: u64) -> bool {
        let prod = self.base.producer();
        let cons = self.base.consumer();
        if prod.wrapping_sub(cons) >= self.base.entries {
            return false;
        }
        let idx = prod & self.base.mask;
        unsafe { ptr::write_volatile(self.desc.add(idx as usize), rel_addr) };
        self.base.set_producer(prod.wrapping_add(1));
        true
    }

    fn push_many<I: IntoIterator<Item = u64>>(&mut self, iter: I) -> u32 {
        let mut pushed = 0u32;
        for rel in iter {
            if !self.push(rel) {
                break;
            }
            pushed += 1;
        }
        pushed
    }
}

struct RxRing {
    base: RingBase,
    desc: *mut XdpDesc,
}

impl RxRing {
    unsafe fn new(fd: RawFd, off: &XdpRingOffset, entries: u32) -> Result<Self, XskError> {
        let (base, desc) = RingBase::new(fd, XDP_PGOFF_RX_RING, off, entries)?;
        Ok(Self { base, desc })
    }

    #[inline(always)]
    fn available(&self) -> u32 {
        self.base.producer().wrapping_sub(self.base.consumer())
    }

    fn pop_many(&mut self, max_batch: u32, out: &mut Vec<XdpDesc>) -> u32 {
        let cons = self.base.consumer();
        let prod = self.base.producer();
        let avail = prod.wrapping_sub(cons).min(max_batch);
        if avail == 0 {
            return 0;
        }

        out.clear();
        out.reserve(avail as usize);
        for i in 0..avail {
            let idx = cons.wrapping_add(i) & self.base.mask;
            let d = unsafe { ptr::read_volatile(self.desc.add(idx as usize)) };
            out.push(d);
        }
        fence(Ordering::Acquire);
        self.base.set_consumer(cons.wrapping_add(avail));
        avail
    }
}

// -----------------------------------------------------------------------------
// XSK socket
// -----------------------------------------------------------------------------

pub struct XskSocket {
    fd: RawFd,
    cfg: XskBindConfig,
    umem_cfg: UmemConfig,
    fill: FillRing,
    rx: RxRing,
    next_fresh_rel_offset: u64,
}

impl XskSocket {
    pub fn bind(
        cfg: XskBindConfig,
        umem: &SharedUmem,
        umem_cfg: &UmemConfig,
    ) -> Result<Self, XskError> {
        if cfg.ifindex == 0 {
            return Err(XskError::InvalidConfig("ifindex must be > 0"));
        }
        if cfg.rx_entries == 0 || !cfg.rx_entries.is_power_of_two() {
            return Err(XskError::InvalidConfig(
                "rx_entries must be a non-zero power of two",
            ));
        }
        if cfg.fill_entries == 0 || !cfg.fill_entries.is_power_of_two() {
            return Err(XskError::InvalidConfig(
                "fill_entries must be a non-zero power of two",
            ));
        }
        if cfg.lease.frame_count == 0 {
            return Err(XskError::InvalidLease("frame_count must be > 0"));
        }
        if cfg.lease.start_frame >= umem_cfg.frames {
            return Err(XskError::InvalidLease("start_frame out of bounds"));
        }
        if cfg.lease.start_frame + cfg.lease.frame_count > umem_cfg.frames {
            return Err(XskError::InvalidLease("lease exceeds UMEM geometry"));
        }

        let fd = unsafe { libc::socket(PF_XDP, libc::SOCK_RAW, 0) };
        if fd < 0 {
            return Err(XskError::Io(io::Error::last_os_error()));
        }

        let result = (|| {
            set_sockopt_u32(fd, XDP_RX_RING, cfg.rx_entries)?;
            set_sockopt_u32(fd, XDP_UMEM_FILL_RING, cfg.fill_entries)?;
            // Kernel 6.8+ xsk_validate_queues() requires both fq_tmp AND cq_tmp
            // to be set, even for RX-only sockets. Set a minimal completion ring.
            set_sockopt_u32(fd, XDP_UMEM_COMPLETION_RING, cfg.fill_entries)?;

            let base_ptr = umem.as_slice().as_ptr();
            let lease_start = cfg.lease.start_offset_bytes(umem_cfg);
            let lease_len = cfg.lease.len_bytes(umem_cfg);
            let lease_ptr = unsafe { base_ptr.add(lease_start as usize) };

            let reg = XdpUmemReg {
                addr: lease_ptr as u64,
                len: lease_len,
                chunk_size: umem_cfg.frame_size,
                headroom: cfg.headroom,
                flags: 0,
                tx_metadata_len: 0,
            };
            set_sockopt_blob(fd, XDP_UMEM_REG, &reg)?;

            let offsets = get_mmap_offsets(fd)?;
            let fill = unsafe { FillRing::new(fd, &offsets.fr, cfg.fill_entries)? };
            let rx = unsafe { RxRing::new(fd, &offsets.rx, cfg.rx_entries)? };

            let addr = SockaddrXdp {
                sxdp_family: AF_XDP as u16,
                sxdp_flags: cfg.bind_flags,
                sxdp_ifindex: cfg.ifindex,
                sxdp_queue_id: cfg.queue_id,
                sxdp_shared_umem_fd: 0,
            };

            let rc = unsafe {
                libc::bind(
                    fd,
                    &addr as *const SockaddrXdp as *const libc::sockaddr,
                    size_of::<SockaddrXdp>() as libc::socklen_t,
                )
            };
            if rc != 0 {
                return Err(XskError::Io(io::Error::last_os_error()));
            }

            // If zero-copy was requested, verify the bound socket actually got it.
            if (cfg.bind_flags & XDP_ZEROCOPY) != 0 {
                let opts = get_xdp_options(fd)?;
                if (opts.flags & XDP_OPTIONS_ZEROCOPY) == 0 {
                    return Err(XskError::NotZeroCopy);
                }
            }

            Ok(Self {
                fd,
                cfg,
                umem_cfg: *umem_cfg,
                fill,
                rx,
                next_fresh_rel_offset: 0,
            })
        })();

        if result.is_err() {
            unsafe { libc::close(fd) };
        }
        result
    }

    #[inline(always)]
    pub fn fd(&self) -> RawFd {
        self.fd
    }

    #[inline(always)]
    pub fn ifindex(&self) -> u32 {
        self.cfg.ifindex
    }

    #[inline(always)]
    pub fn queue_id(&self) -> u32 {
        self.cfg.queue_id
    }

    #[inline(always)]
    pub fn fill_free_slots(&self) -> u32 {
        self.fill.free_slots()
    }

    #[inline(always)]
    pub fn fill_posted(&self) -> u32 {
        self.cfg.fill_entries - self.fill_free_slots()
    }

    #[inline(always)]
    pub fn fill_capacity(&self) -> u32 {
        self.cfg.fill_entries
    }

    /// Prime the Fill ring with as many never-used frames from this lease as fit.
    pub fn prime_fill_ring(&mut self) -> Result<u32, XskError> {
        let mut pushed = 0u32;
        let lease_bytes = self.cfg.lease.len_bytes(&self.umem_cfg);
        while self.next_fresh_rel_offset < lease_bytes {
            if !self.fill.push(self.next_fresh_rel_offset) {
                break;
            }
            self.next_fresh_rel_offset += self.umem_cfg.frame_size as u64;
            pushed += 1;
        }
        if self.fill.base.needs_wakeup() {
            self.kick_rx_path()?;
        }
        Ok(pushed)
    }

    /// Recycle one frame-aligned absolute UMEM offset back to the Fill ring.
    ///
    /// Returns `Ok(false)` if the Fill ring is currently full.
    pub fn recycle_frame(&mut self, abs_frame_offset: u64) -> Result<bool, XskError> {
        if abs_frame_offset % self.umem_cfg.frame_size as u64 != 0 {
            return Err(XskError::BadUmemOffset(abs_frame_offset));
        }
        if !self
            .cfg
            .lease
            .contains_abs_offset(&self.umem_cfg, abs_frame_offset)
        {
            return Err(XskError::BadUmemOffset(abs_frame_offset));
        }
        let rel = abs_frame_offset - self.cfg.lease.start_offset_bytes(&self.umem_cfg);
        let ok = self.fill.push(rel);
        if ok && self.fill.base.needs_wakeup() {
            self.kick_rx_path()?;
        }
        Ok(ok)
    }

    /// Recycle a bounded slice of absolute frame offsets.
    pub fn recycle_many(&mut self, offsets: &[u64]) -> Result<u32, XskError> {
        let mut pushed = 0u32;
        for &off in offsets {
            if !self.recycle_frame(off)? {
                break;
            }
            pushed += 1;
        }
        Ok(pushed)
    }

    /// Poll and drain up to `max_batch` RX descriptors.
    ///
    /// `timeout_ms < 0` blocks indefinitely, `timeout_ms == 0` is non-blocking.
    pub fn poll_and_drain<P: RxMetadataParser>(
        &mut self,
        umem: &SharedUmem,
        parser: &P,
        max_batch: u32,
        timeout_ms: i32,
    ) -> Result<XskDrainBatch, XskError> {
        if self.rx.available() == 0 {
            let rc = poll_fd(self.fd, timeout_ms)?;
            if rc == 0 {
                return Ok(XskDrainBatch::default());
            }
        }

        let mut raw_descs = Vec::with_capacity(max_batch.min(self.cfg.rx_entries) as usize);
        let _n = self.rx.pop_many(max_batch, &mut raw_descs);

        let mut batch = XskDrainBatch {
            frames: Vec::with_capacity(raw_descs.len()),
            ..XskDrainBatch::default()
        };

        for desc in raw_descs {
            if (desc.options & XDP_PKT_CONTD) != 0 {
                batch.multi_buffer_drops += 1;
                continue;
            }

            let frame_size = self.umem_cfg.frame_size as u64;
            let frame_start_rel = (desc.addr / frame_size) * frame_size;
            let packet_offset = (desc.addr - frame_start_rel) as u16;
            if packet_offset as u32 + desc.len > self.umem_cfg.frame_size {
                batch.bad_bounds_drops += 1;
                continue;
            }

            let abs_frame_offset =
                self.cfg.lease.start_offset_bytes(&self.umem_cfg) + frame_start_rel;
            let meta = match parser.parse(
                umem,
                abs_frame_offset,
                packet_offset,
                self.umem_cfg.frame_size,
            ) {
                Ok(m) => m,
                Err(XskError::MissingMetadataHeadroom { .. })
                | Err(XskError::BadMetadataMagic(_))
                | Err(XskError::BadMetadataVersion(_))
                | Err(XskError::BadMetadataLen(_))
                | Err(XskError::MissingHardwareTimestamp) => {
                    batch.metadata_drops += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };

            batch.frames.push(XskRxFrame {
                frame_offset: abs_frame_offset,
                packet_offset,
                packet_len: desc.len,
                metadata_len: meta.metadata_len,
                rx_timestamp_ns: meta.rx_timestamp_ns,
            });
        }

        Ok(batch)
    }

    /// Open a pinned XSKMAP and update `queue_id -> this_socket_fd`.
    ///
    /// Use this if the external XDP program has already been loaded and pinned
    /// with an `xsks_map`, but userspace still needs to insert the queue's XSK fd.
    pub fn register_pinned_xskmap<P: AsRef<Path>>(
        &self,
        pinned_map_path: P,
    ) -> Result<(), XskError> {
        #[cfg(target_os = "linux")]
        {
            let map = MapHandle::from_pinned_path(pinned_map_path.as_ref())
                .map_err(|e| XskError::BpfMapOpen(io::Error::other(e.to_string())))?;
            let key = self.cfg.queue_id.to_ne_bytes();
            let value = (self.fd as u32).to_ne_bytes();
            map.update(&key, &value, MapFlags::ANY)
                .map_err(|e| XskError::BpfMapUpdate(io::Error::other(e.to_string())))?;
            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        {
            let map_fd = open_pinned_bpf_map(pinned_map_path.as_ref())?;
            let result = update_xskmap(map_fd, self.cfg.queue_id, self.fd);
            unsafe { libc::close(map_fd) };
            result
        }
    }

    fn kick_rx_path(&self) -> Result<(), XskError> {
        let rc = unsafe {
            libc::recvfrom(
                self.fd,
                ptr::null_mut(),
                0,
                libc::MSG_DONTWAIT,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if rc >= 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        match err.kind() {
            io::ErrorKind::WouldBlock => Ok(()),
            _ => Err(XskError::Io(err)),
        }
    }
}

impl Drop for XskSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

// -----------------------------------------------------------------------------
// Convenience helpers
// -----------------------------------------------------------------------------

pub fn ifindex_from_name(name: &str) -> Result<u32, XskError> {
    let c = CString::new(name)?;
    let idx = unsafe { libc::if_nametoindex(c.as_ptr()) };
    if idx == 0 {
        Err(XskError::Io(io::Error::last_os_error()))
    } else {
        Ok(idx)
    }
}

fn set_sockopt_u32(fd: RawFd, opt: i32, value: u32) -> Result<(), XskError> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            SOL_XDP,
            opt,
            &value as *const u32 as *const libc::c_void,
            size_of::<u32>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(XskError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

fn set_sockopt_blob<T>(fd: RawFd, opt: i32, value: &T) -> Result<(), XskError> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            SOL_XDP,
            opt,
            value as *const T as *const libc::c_void,
            size_of::<T>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(XskError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

fn get_mmap_offsets(fd: RawFd) -> Result<XdpMmapOffsets, XskError> {
    let mut offsets = XdpMmapOffsets::default();
    let mut len = size_of::<XdpMmapOffsets>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            SOL_XDP,
            XDP_MMAP_OFFSETS,
            &mut offsets as *mut XdpMmapOffsets as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(XskError::Io(io::Error::last_os_error()));
    }
    Ok(offsets)
}

fn get_xdp_options(fd: RawFd) -> Result<XdpOptions, XskError> {
    let mut opts = XdpOptions::default();
    let mut len = size_of::<XdpOptions>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            SOL_XDP,
            XDP_OPTIONS,
            &mut opts as *mut XdpOptions as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(XskError::Io(io::Error::last_os_error()));
    }
    Ok(opts)
}

fn poll_fd(fd: RawFd, timeout_ms: i32) -> Result<i32, XskError> {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let rc = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, timeout_ms) };
    if rc < 0 {
        return Err(XskError::Io(io::Error::last_os_error()));
    }
    Ok(rc)
}

#[cfg(not(target_os = "linux"))]
fn open_pinned_bpf_map(path: &Path) -> Result<RawFd, XskError> {
    let cpath = CString::new(path.to_string_lossy().as_bytes()).map_err(XskError::CString)?;
    let attr = BpfAttrObjGet {
        pathname: cpath.as_ptr() as u64,
        bpf_fd: 0,
        file_flags: 0,
        path_fd: 0,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_OBJ_GET,
            &attr as *const BpfAttrObjGet,
            size_of::<BpfAttrObjGet>(),
        ) as RawFd
    };
    if fd < 0 {
        return Err(XskError::BpfMapOpen(io::Error::last_os_error()));
    }
    Ok(fd)
}

#[cfg(not(target_os = "linux"))]
fn update_xskmap(map_fd: RawFd, queue_id: u32, xsk_fd: RawFd) -> Result<(), XskError> {
    let key = queue_id;
    let value = xsk_fd as u32;
    let attr = BpfAttrMapUpdateElem {
        map_fd: map_fd as u32,
        _pad: 0,
        key: (&key as *const u32) as u64,
        value: (&value as *const u32) as u64,
        flags: BPF_ANY,
    };
    let rc = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_MAP_UPDATE_ELEM,
            &attr as *const BpfAttrMapUpdateElem,
            size_of::<BpfAttrMapUpdateElem>(),
        )
    };
    if rc != 0 {
        return Err(XskError::BpfMapUpdate(io::Error::last_os_error()));
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Small self-contained tests for pure helpers only.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_lease_whole_covers_entire_umem() {
        let cfg = UmemConfig::new(1024, 2048).unwrap();
        let lease = FrameLease::whole(&cfg);
        assert_eq!(lease.start_frame, 0);
        assert_eq!(lease.frame_count, 1024);
        assert_eq!(lease.start_offset_bytes(&cfg), 0);
        assert_eq!(lease.len_bytes(&cfg), 1024 * 2048);
    }

    #[test]
    fn frame_lease_split_evenly_partitions() {
        let cfg = UmemConfig::new(10, 2048).unwrap();
        let a = FrameLease::split_evenly(&cfg, 0, 3).unwrap();
        let b = FrameLease::split_evenly(&cfg, 1, 3).unwrap();
        let c = FrameLease::split_evenly(&cfg, 2, 3).unwrap();

        assert_eq!(
            a,
            FrameLease {
                start_frame: 0,
                frame_count: 4
            }
        );
        assert_eq!(
            b,
            FrameLease {
                start_frame: 4,
                frame_count: 3
            }
        );
        assert_eq!(
            c,
            FrameLease {
                start_frame: 7,
                frame_count: 3
            }
        );
    }

    #[test]
    fn rx_only_defaults_are_sane() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let lease = FrameLease::whole(&cfg);
        let bind = XskBindConfig::rx_only(7, 0, lease);
        assert_eq!(bind.ifindex, 7);
        assert_eq!(bind.queue_id, 0);
        assert_eq!(bind.rx_entries, XskBindConfig::DEFAULT_RX_ENTRIES);
        assert_eq!(bind.fill_entries, XskBindConfig::DEFAULT_FILL_ENTRIES);
        assert_eq!(bind.headroom, XskBindConfig::DEFAULT_HEADROOM);
        assert_eq!(bind.bind_flags, XDP_ZEROCOPY | XDP_USE_NEED_WAKEUP);
    }

    #[test]
    fn rx_only_copy_mode_sets_copy_bind_flags() {
        let cfg = UmemConfig::new(64, 2048).unwrap();
        let lease = FrameLease::whole(&cfg);
        let bind = XskBindConfig::rx_only_with_bind_mode(7, 0, lease, XskBindMode::Copy);
        assert_eq!(bind.bind_flags, XDP_COPY | XDP_USE_NEED_WAKEUP);
    }

    #[test]
    fn xsk_rx_frame_descriptor_preserves_aligned_offset() {
        let f = XskRxFrame {
            frame_offset: 4096,
            packet_offset: 128,
            packet_len: 1500,
            metadata_len: 16,
            rx_timestamp_ns: 123,
        };
        let d = f.as_rx_descriptor();
        assert_eq!(d.umem_offset, 4096);
        assert_eq!(d.len, 1500);
        assert_eq!(d.rx_timestamp_ns, 123);
    }

    #[test]
    fn shadow_meta_header_layout_stable() {
        assert_eq!(size_of::<ShadowRxMetaV1>(), 16);
    }
}
