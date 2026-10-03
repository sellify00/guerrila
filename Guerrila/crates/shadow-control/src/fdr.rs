use memmap2::MmapMut;
use shadow_ipc::raw_archive::{frame_hash64, RawArchiveRecord, RawArchiveSegment};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

#[derive(Clone)]
pub struct FdrFrameSample {
    pub record: RawArchiveRecord,
    pub frame_bytes: Vec<u8>,
}

#[derive(Default)]
pub struct FdrBatch {
    pub records: Vec<RawArchiveRecord>,
    pub frame_samples: Vec<FdrFrameSample>,
    pub last_slot: Option<u64>,
    pub last_unix_ns: Option<u64>,
    pub rejected_records: usize,
}

impl FdrBatch {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[derive(Clone, Copy)]
struct LatestFrameGuard {
    generation: u64,
    len: u16,
    rx_timestamp_ns: u64,
}

pub struct FdrConsumer {
    archive: RawArchiveSegment,
    umem: MmapMut,
    umem_frame_size: u32,
    max_sample_window_ns: u64,
    latest_by_offset: HashMap<u32, LatestFrameGuard>,
}

impl FdrConsumer {
    pub fn open(
        path: &Path,
        umem_path: &Path,
        umem_frames: u32,
        umem_frame_size: u32,
        max_sample_window_ns: u64,
    ) -> io::Result<Self> {
        let archive = RawArchiveSegment::open(path)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

        let file = OpenOptions::new().read(true).write(true).open(umem_path)?;
        let expected_len = umem_frames as u64 * umem_frame_size as u64;
        let found_len = file.metadata()?.len();
        if found_len < expected_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("UMEM file too small: found {found_len} bytes, expected {expected_len}"),
            ));
        }

        let umem = unsafe { MmapMut::map_mut(&file)? };
        Ok(Self {
            archive,
            umem,
            umem_frame_size,
            max_sample_window_ns,
            latest_by_offset: HashMap::new(),
        })
    }

    pub fn drain_once(&mut self) -> FdrBatch {
        let mut drained = Vec::new();
        let mut last_slot = None;
        let mut last_unix_ns = None;

        for lane in 0..self.archive.producer_count() as usize {
            while let Some(record) = self.archive.pop(lane) {
                last_slot =
                    Some(last_slot.map_or(record.slot, |current: u64| current.max(record.slot)));
                last_unix_ns = Some(last_unix_ns.map_or(record.rx_timestamp_ns, |current: u64| {
                    current.max(record.rx_timestamp_ns)
                }));
                drained.push(record);
            }
        }

        for record in &drained {
            let entry = self
                .latest_by_offset
                .entry(record.umem_offset_bytes)
                .or_insert(LatestFrameGuard {
                    generation: record.generation,
                    len: record.len,
                    rx_timestamp_ns: record.rx_timestamp_ns,
                });
            if record.generation >= entry.generation {
                *entry = LatestFrameGuard {
                    generation: record.generation,
                    len: record.len,
                    rx_timestamp_ns: record.rx_timestamp_ns,
                };
            }
        }

        let mut records = Vec::with_capacity(drained.len());
        let mut frame_samples = Vec::with_capacity(drained.len());
        let mut rejected_records = 0usize;
        for record in drained {
            if self.record_is_live(&record, last_unix_ns) {
                let start = record.umem_offset_bytes as usize;
                let end = start.saturating_add(record.len as usize);
                frame_samples.push(FdrFrameSample {
                    record,
                    frame_bytes: self.umem[start..end].to_vec(),
                });
                records.push(record);
            } else {
                rejected_records += 1;
            }
        }

        FdrBatch {
            records,
            frame_samples,
            last_slot,
            last_unix_ns,
            rejected_records,
        }
    }

    fn record_is_live(&self, record: &RawArchiveRecord, last_unix_ns: Option<u64>) -> bool {
        if record.generation == 0 || record.rx_timestamp_ns == 0 || record.frame_hash64 == 0 {
            return false;
        }

        let expected = match self.latest_by_offset.get(&record.umem_offset_bytes) {
            Some(expected) => expected,
            None => return false,
        };
        if expected.generation != record.generation
            || expected.len != record.len
            || expected.rx_timestamp_ns != record.rx_timestamp_ns
        {
            return false;
        }

        let start = record.umem_offset_bytes as usize;
        if start % self.umem_frame_size as usize != 0 {
            return false;
        }
        let end = start.saturating_add(record.len as usize);
        if end > self.umem.len() {
            return false;
        }

        if let Some(max_seen_unix_ns) = last_unix_ns {
            if max_seen_unix_ns.saturating_sub(record.rx_timestamp_ns) > self.max_sample_window_ns {
                return false;
            }
        }

        frame_hash64(&self.umem[start..end]) == record.frame_hash64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_umem(path: &Path, bytes: usize) {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.set_len(bytes as u64).unwrap();
    }

    fn write_umem_bytes(path: &Path, offset: usize, bytes: &[u8], total_len: usize) {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        mmap[..total_len].fill(0);
        mmap[offset..offset + bytes.len()].copy_from_slice(bytes);
        mmap.flush().unwrap();
    }

    #[test]
    fn g11_fdr_overwrite_guard_accepts_matching_frame() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("archive");
        let umem_path = dir.path().join("shadow_umem_0");
        let frame_size = 2048usize;
        make_umem(&umem_path, frame_size * 2);

        let frame = [0xABu8; 64];
        write_umem_bytes(&umem_path, 0, &frame, frame_size * 2);

        let archive = RawArchiveSegment::create(&archive_path, 1, 8).unwrap();
        let record = RawArchiveRecord::new_guarded(
            42,
            1,
            1_000,
            frame_hash64(&frame),
            0,
            7,
            frame.len() as u16,
            true,
            0,
        );
        assert!(archive.push(0, &record));

        let mut consumer =
            FdrConsumer::open(&archive_path, &umem_path, 2, frame_size as u32, 10_000).unwrap();
        let batch = consumer.drain_once();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.rejected_records, 0);
    }

    #[test]
    fn g11_fdr_overwrite_guard_rejects_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("archive");
        let umem_path = dir.path().join("shadow_umem_0");
        let frame_size = 2048usize;
        make_umem(&umem_path, frame_size * 2);

        let original = [0x11u8; 64];
        let overwritten = [0x22u8; 64];
        write_umem_bytes(&umem_path, 0, &original, frame_size * 2);

        let archive = RawArchiveSegment::create(&archive_path, 1, 8).unwrap();
        let record = RawArchiveRecord::new_guarded(
            42,
            1,
            1_000,
            frame_hash64(&original),
            0,
            7,
            original.len() as u16,
            true,
            0,
        );
        assert!(archive.push(0, &record));

        write_umem_bytes(&umem_path, 0, &overwritten, frame_size * 2);

        let mut consumer =
            FdrConsumer::open(&archive_path, &umem_path, 2, frame_size as u32, 10_000).unwrap();
        let batch = consumer.drain_once();
        assert!(batch.records.is_empty());
        assert_eq!(batch.rejected_records, 1);
    }

    #[test]
    fn g11_fdr_overwrite_guard_rejects_reused_generation() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("archive");
        let umem_path = dir.path().join("shadow_umem_0");
        let frame_size = 2048usize;
        make_umem(&umem_path, frame_size * 2);

        let first = [0x33u8; 64];
        let second = [0x44u8; 64];
        let archive = RawArchiveSegment::create(&archive_path, 1, 8).unwrap();

        write_umem_bytes(&umem_path, 0, &first, frame_size * 2);
        assert!(archive.push(
            0,
            &RawArchiveRecord::new_guarded(
                42,
                1,
                1_000,
                frame_hash64(&first),
                0,
                7,
                first.len() as u16,
                true,
                0,
            )
        ));

        write_umem_bytes(&umem_path, 0, &second, frame_size * 2);
        assert!(archive.push(
            0,
            &RawArchiveRecord::new_guarded(
                43,
                2,
                1_100,
                frame_hash64(&second),
                0,
                8,
                second.len() as u16,
                true,
                0,
            )
        ));

        let mut consumer =
            FdrConsumer::open(&archive_path, &umem_path, 2, frame_size as u32, 10_000).unwrap();
        let batch = consumer.drain_once();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].generation, 2);
        assert_eq!(batch.rejected_records, 1);
    }

    #[test]
    fn g11_fdr_overwrite_guard_rejects_out_of_range_offset() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("archive");
        let umem_path = dir.path().join("shadow_umem_0");
        let frame_size = 2048usize;
        make_umem(&umem_path, frame_size * 2);

        let archive = RawArchiveSegment::create(&archive_path, 1, 8).unwrap();
        assert!(archive.push(
            0,
            &RawArchiveRecord::new_guarded(
                42,
                1,
                1_000,
                0x55,
                (frame_size * 4) as u32,
                7,
                64,
                true,
                0,
            )
        ));

        let mut consumer =
            FdrConsumer::open(&archive_path, &umem_path, 2, frame_size as u32, 10_000).unwrap();
        let batch = consumer.drain_once();
        assert!(batch.records.is_empty());
        assert_eq!(batch.rejected_records, 1);
    }

    #[test]
    fn g11_fdr_overwrite_guard_rejects_stale_timestamp_window() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("archive");
        let umem_path = dir.path().join("shadow_umem_0");
        let frame_size = 2048usize;
        make_umem(&umem_path, frame_size * 4);

        let frame = [0x77u8; 64];
        write_umem_bytes(&umem_path, 0, &frame, frame_size * 4);
        write_umem_bytes(&umem_path, frame_size, &frame, frame_size * 4);

        let archive = RawArchiveSegment::create(&archive_path, 1, 8).unwrap();
        assert!(archive.push(
            0,
            &RawArchiveRecord::new_guarded(
                41,
                1,
                100,
                frame_hash64(&frame),
                0,
                1,
                frame.len() as u16,
                true,
                0,
            )
        ));
        assert!(archive.push(
            0,
            &RawArchiveRecord::new_guarded(
                42,
                2,
                10_000,
                frame_hash64(&frame),
                frame_size as u32,
                2,
                frame.len() as u16,
                true,
                0,
            )
        ));

        let mut consumer =
            FdrConsumer::open(&archive_path, &umem_path, 4, frame_size as u32, 500).unwrap();
        let batch = consumer.drain_once();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].generation, 2);
        assert_eq!(batch.rejected_records, 1);
    }

    #[test]
    fn g11_overwrite_guard_rejects_stale_umem_frame() {
        g11_fdr_overwrite_guard_rejects_stale_timestamp_window();
    }
}
