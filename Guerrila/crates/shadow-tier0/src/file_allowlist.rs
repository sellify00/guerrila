//! Boot-time immutable signer allowlist loaded from a binary file.
//!
//! Binary format:
//!
//! ```text
//! [u32 LE: entry count]
//! For each entry:
//!   [32 bytes: signer pubkey]
//!   [u32 LE: entity_id]
//! ```
//!
//! Loaded once at startup into a read-only `HashMap`. If the allowlist
//! file path is `None` the system operates in pass-all mode (every
//! signer matches entity 0) rather than reject-all.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use shadow_ingest::allowlist::AllowlistLookup;

/// Record size: 32-byte pubkey + 4-byte entity_id.
const ENTRY_SIZE: usize = 36;

/// Boot-time immutable signer allowlist.
pub struct FileAllowlist {
    table: HashMap<[u8; 32], u32>,
}

impl FileAllowlist {
    /// Load from binary file. Returns an error if the file is truncated
    /// or otherwise malformed.
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = std::fs::read(path)?;
        if data.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "allowlist file too short for header",
            ));
        }
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let expected = 4 + count * ENTRY_SIZE;
        if data.len() < expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "allowlist truncated: expected {expected} bytes, got {}",
                    data.len()
                ),
            ));
        }
        let mut table = HashMap::with_capacity(count);
        let mut offset = 4;
        for _ in 0..count {
            let mut key = [0u8; 32];
            key.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            let entity_id = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            offset += 4;
            table.insert(key, entity_id);
        }
        Ok(Self { table })
    }

    /// Create an empty allowlist (rejects every signer).
    pub fn empty() -> Self {
        Self {
            table: HashMap::new(),
        }
    }

    /// Number of entries loaded.
    pub fn entry_count(&self) -> usize {
        self.table.len()
    }
}

/// Write the binary allowlist format expected by [`FileAllowlist::load`].
pub fn write_allowlist<W>(mut writer: W, entries: &[([u8; 32], u32)]) -> io::Result<()>
where
    W: Write,
{
    let count = u32::try_from(entries.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many allowlist entries to encode as u32",
        )
    })?;
    writer.write_all(&count.to_le_bytes())?;
    for (pubkey, entity_id) in entries {
        writer.write_all(pubkey)?;
        writer.write_all(&entity_id.to_le_bytes())?;
    }
    Ok(())
}

impl AllowlistLookup for FileAllowlist {
    fn lookup_signer(&self, pubkey: &[u8; 32]) -> Option<u32> {
        self.table.get(pubkey).copied()
    }
}

/// Pass-all allowlist — every signer matches entity 0.
///
/// Used when no allowlist file is configured (open-access mode).
pub struct PassAllAllowlist;

impl AllowlistLookup for PassAllAllowlist {
    fn lookup_signer(&self, _pubkey: &[u8; 32]) -> Option<u32> {
        Some(0)
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allowlist.bin");

        let signer_a = [0xAAu8; 32];
        let signer_b = [0xBBu8; 32];
        let mut f = std::fs::File::create(&path).unwrap();
        write_allowlist(&mut f, &[(signer_a, 1), (signer_b, 42)]).unwrap();

        let al = FileAllowlist::load(&path).unwrap();
        assert_eq!(al.entry_count(), 2);
        assert_eq!(al.lookup_signer(&signer_a), Some(1));
        assert_eq!(al.lookup_signer(&signer_b), Some(42));
        assert_eq!(al.lookup_signer(&[0xCCu8; 32]), None);
    }

    #[test]
    fn empty_file_loads_zero_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        write_allowlist(&mut f, &[]).unwrap();

        let al = FileAllowlist::load(&path).unwrap();
        assert_eq!(al.entry_count(), 0);
        assert_eq!(al.lookup_signer(&[0xAAu8; 32]), None);
    }

    #[test]
    fn truncated_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated.bin");
        // Write count=1 but no entries.
        std::fs::write(&path, &1u32.to_le_bytes()).unwrap();
        assert!(FileAllowlist::load(&path).is_err());
    }

    #[test]
    fn too_short_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.bin");
        std::fs::write(&path, &[0u8; 2]).unwrap();
        assert!(FileAllowlist::load(&path).is_err());
    }

    #[test]
    fn pass_all_accepts_everything() {
        let al = PassAllAllowlist;
        assert_eq!(al.lookup_signer(&[0x00u8; 32]), Some(0));
        assert_eq!(al.lookup_signer(&[0xFFu8; 32]), Some(0));
    }

    #[test]
    fn empty_allowlist_rejects_everything() {
        let al = FileAllowlist::empty();
        assert_eq!(al.lookup_signer(&[0xAAu8; 32]), None);
    }
}
