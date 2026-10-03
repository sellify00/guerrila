use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use clap::Parser;
use shadow_tier0::file_allowlist::write_allowlist;

#[derive(Parser, Debug)]
#[command(
    name = "write-allowlist",
    about = "Write the Tier0 binary allowlist from a signer/entity CSV"
)]
struct Cli {
    /// CSV input with columns signer_pubkey,entity_id.
    #[arg(long)]
    input: PathBuf,

    /// Output binary allowlist path.
    #[arg(long)]
    output: PathBuf,
}

fn main() {
    let cli = Cli::parse();
    let entries = load_csv_entries(&cli.input).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to parse {}: {err}", cli.input.display());
        std::process::exit(1);
    });

    if entries.is_empty() {
        eprintln!(
            "ERROR: {} contained no allowlist entries; refusing to write an empty allowlist",
            cli.input.display()
        );
        std::process::exit(1);
    }

    let parent = cli.output.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).unwrap_or_else(|err| {
        eprintln!(
            "ERROR: failed to create output directory {}: {err}",
            parent.display()
        );
        std::process::exit(1);
    });

    let mut out = File::create(&cli.output).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to create {}: {err}", cli.output.display());
        std::process::exit(1);
    });

    write_allowlist(&mut out, &entries).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to write {}: {err}", cli.output.display());
        std::process::exit(1);
    });

    println!(
        "Wrote {} allowlist entr{} to {}",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" },
        cli.output.display()
    );
}

fn load_csv_entries(path: &Path) -> io::Result<Vec<([u8; 32], u32)>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut seen = HashMap::<[u8; 32], u32>::new();
    let mut entries = Vec::new();

    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if line_no == 1 && is_header(trimmed) {
            continue;
        }

        let (signer_raw, entity_raw) = parse_csv_row(trimmed, line_no)?;
        let signer = parse_signer_pubkey(signer_raw, line_no)?;
        let entity_id = parse_entity_id(entity_raw, line_no)?;

        if let Some(existing) = seen.insert(signer, entity_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "line {line_no}: duplicate signer {} (existing entity_id={existing}, new entity_id={entity_id})",
                    signer_raw
                ),
            ));
        }

        entries.push((signer, entity_id));
    }

    Ok(entries)
}

fn is_header(line: &str) -> bool {
    let normalized: Vec<_> = line
        .split(',')
        .map(|part| part.trim().to_ascii_lowercase())
        .collect();
    normalized.len() == 2
        && normalized[0] == "signer_pubkey"
        && (normalized[1] == "entity_id" || normalized[1] == "entity")
}

fn parse_csv_row(line: &str, line_no: usize) -> io::Result<(&str, &str)> {
    let mut parts = line.split(',');
    let signer = parts.next().map(str::trim).unwrap_or_default();
    let entity = parts.next().map(str::trim).unwrap_or_default();
    if signer.is_empty() || entity.is_empty() || parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "line {line_no}: expected exactly two comma-separated columns: signer_pubkey,entity_id"
            ),
        ));
    }
    Ok((signer, entity))
}

fn parse_signer_pubkey(value: &str, line_no: usize) -> io::Result<[u8; 32]> {
    let decoded = bs58::decode(value).into_vec().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("line {line_no}: invalid base58 signer pubkey {value}: {err}"),
        )
    })?;
    let decoded_len = decoded.len();
    let signer: [u8; 32] = decoded.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "line {line_no}: signer pubkey {value} decoded to {} bytes, expected 32",
                decoded_len
            ),
        )
    })?;
    Ok(signer)
}

fn parse_entity_id(value: &str, line_no: usize) -> io::Result<u32> {
    value.parse::<u32>().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("line {line_no}: invalid entity_id {value}: {err}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_ingest::allowlist::AllowlistLookup;
    use shadow_tier0::file_allowlist::FileAllowlist;

    #[test]
    fn write_allowlist_roundtrips_through_file_allowlist_load() {
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("allowlist.csv");
        let bin_path = dir.path().join("allowlist.bin");
        let signer_a = bs58::encode([0x11u8; 32]).into_string();
        let signer_b = bs58::encode([0x22u8; 32]).into_string();
        std::fs::write(
            &csv_path,
            format!("signer_pubkey,entity_id\n{signer_a},7\n{signer_b},42\n"),
        )
        .unwrap();

        let entries = load_csv_entries(&csv_path).unwrap();
        let mut out = File::create(&bin_path).unwrap();
        write_allowlist(&mut out, &entries).unwrap();

        let allowlist = FileAllowlist::load(&bin_path).unwrap();
        assert_eq!(allowlist.entry_count(), 2);
        assert_eq!(allowlist.lookup_signer(&[0x11u8; 32]), Some(7));
        assert_eq!(allowlist.lookup_signer(&[0x22u8; 32]), Some(42));
        assert_eq!(allowlist.lookup_signer(&[0x33u8; 32]), None);
    }

    #[test]
    fn csv_loader_rejects_duplicate_signers() {
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("allowlist.csv");
        let signer = bs58::encode([0x44u8; 32]).into_string();
        std::fs::write(&csv_path, format!("{signer},1\n{signer},2\n")).unwrap();

        let err = load_csv_entries(&csv_path).unwrap_err();
        assert!(err.to_string().contains("duplicate signer"));
    }
}
