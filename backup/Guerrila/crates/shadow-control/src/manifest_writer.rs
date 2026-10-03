use shadow_ipc::parsing_manifest::{
    ParsingManifestEntryV1, ParsingManifestSnapshot, ParsingManifestWriteError,
};
use shadow_ipc::segment::{MappedSegment, SegmentError};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ManifestPublisher {
    seg: MappedSegment<ParsingManifestSnapshot>,
    next_epoch: u64,
}

impl ManifestPublisher {
    pub fn open_or_create(path: &Path) -> io::Result<Self> {
        let seg = match MappedSegment::<ParsingManifestSnapshot>::open_rw(path) {
            Ok(seg) => seg,
            Err(SegmentError::Io(err)) if err.kind() == io::ErrorKind::NotFound => {
                MappedSegment::<ParsingManifestSnapshot>::create_rw(path).map_err(segment_to_io)?
            }
            Err(err) => return Err(segment_to_io(err)),
        };

        let next_epoch = seg
            .payload()
            .active_manifest()
            .map(|manifest| manifest.publish_epoch.saturating_add(1))
            .unwrap_or(1);
        Ok(Self { seg, next_epoch })
    }

    pub fn current_entries(&self) -> Vec<ParsingManifestEntryV1> {
        self.seg
            .payload()
            .read_active_entries()
            .map(|entries| entries.to_vec())
            .unwrap_or_default()
    }

    pub fn current_epoch(&self) -> u64 {
        self.seg
            .payload()
            .active_manifest()
            .map(|manifest| manifest.publish_epoch)
            .unwrap_or(0)
    }

    pub fn publish(&mut self, entries: &[ParsingManifestEntryV1]) -> io::Result<u64> {
        let epoch = self.next_epoch;
        self.seg
            .payload()
            .publish_manifest(entries, epoch, now_unix_ns())
            .map_err(manifest_to_io)?;
        self.seg
            .flush()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        self.next_epoch = self.next_epoch.saturating_add(1);
        Ok(epoch)
    }
}

fn now_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

fn segment_to_io(err: SegmentError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}

fn manifest_to_io(err: ParsingManifestWriteError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}
