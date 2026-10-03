//! Transport-neutral replay corpus format for deterministic G4 seal testing.
//!
//! Defines the in-memory types consumed by both the corpus builder
//! (`build_g4_corpus` binary) and the sender/seal-runner binaries.
//!
//! Key invariant: a `ReplayScenario` with `drop_plan` set to an
//! FEC-solvable configuration must produce a packet list where the
//! remaining data + coding shreds still satisfy the RS threshold
//! `received_data + received_coding >= num_data` for every affected
//! FEC set.

use serde::{Deserialize, Serialize};

use crate::recovery::encode_test_coding_shards;
use crate::shred_meta::ShredMeta;

// ── ReplayPacket ────────────────────────────────────────────────────

/// A single packet in a replay corpus, annotated with parsed metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayPacket {
    /// Monotonic index within the corpus (0-based).
    pub index: u64,
    /// Raw wire bytes of the shred (full frame including header).
    #[serde(with = "serde_bytes_hex")]
    pub data: Vec<u8>,
    /// Slot parsed from the shred header.
    pub slot: u64,
    /// Shred index within the slot.
    pub shred_index: u32,
    /// True for data shreds, false for coding shreds.
    pub is_data: bool,
    /// FEC set index this shred belongs to.
    pub fec_set_index: u32,
}

impl ReplayPacket {
    /// Construct from raw wire bytes. Returns `None` if the bytes
    /// don't parse as a valid shred.
    pub fn from_raw(index: u64, data: Vec<u8>) -> Option<Self> {
        let (meta, _payload) = ShredMeta::parse(&data).ok()?;
        Some(Self {
            index,
            data,
            slot: meta.slot,
            shred_index: meta.shred_index,
            is_data: meta.is_data,
            fec_set_index: meta.fec_set_index,
        })
    }
}

// ── Hex serde helper for Vec<u8> ────────────────────────────────────

mod serde_bytes_hex {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_str: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        serializer.serialize_str(&hex_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect()
    }
}

// ── DropPlan ────────────────────────────────────────────────────────

/// Plan for which packets to drop from a baseline corpus.
///
/// Used to simulate network loss or FEC-degraded scenarios.
/// The builder validates that the resulting corpus still satisfies
/// the FEC solvability rule when `fec_solvable` is true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropPlan {
    /// Indices into the baseline corpus to drop (0-based).
    pub drop_indices: Vec<u64>,
    /// If true, the drops were chosen to keep the FEC sets solvable
    /// (received_data + received_coding >= num_data).
    pub fec_solvable: bool,
    /// Human-readable description of the drop pattern.
    pub description: String,
}

// ── ReorderPlan ─────────────────────────────────────────────────────

/// Plan for reordering packets in a baseline corpus.
///
/// The reorder plan produces the same set of packets but in a
/// different order. Invariant: decoded output must be identical
/// because the reassembler handles out-of-order delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderPlan {
    /// Permutation: `order[i]` is the baseline index of the packet
    /// to send at position `i`.
    pub order: Vec<u64>,
    /// Human-readable description of the reorder pattern.
    pub description: String,
}

// ── FEC set geometry ────────────────────────────────────────────────

/// FEC set geometry extracted from corpus packets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FecSetGeometry {
    pub slot: u64,
    pub fec_set_index: u32,
    /// Number of data shreds declared in this FEC set.
    pub num_data: u16,
    /// Number of coding shreds declared in this FEC set.
    pub num_coding: u16,
    /// Indices of data shred packets in the corpus.
    pub data_packet_indices: Vec<u64>,
    /// Indices of coding shred packets in the corpus.
    pub coding_packet_indices: Vec<u64>,
}

impl FecSetGeometry {
    /// Data shreds present in the corpus.
    pub fn data_count(&self) -> usize {
        self.data_packet_indices.len()
    }

    /// Coding shreds present in the corpus.
    pub fn coding_count(&self) -> usize {
        self.coding_packet_indices.len()
    }

    /// Whether this FEC set is solvable given the current packets.
    pub fn is_solvable(&self) -> bool {
        if self.num_data == 0 {
            return false;
        }
        (self.data_count() + self.coding_count()) >= self.num_data as usize
    }

    /// Whether this FEC set is solvable after dropping the given indices.
    pub fn is_solvable_after_drops(&self, drops: &[u64]) -> bool {
        if self.num_data == 0 {
            return false;
        }
        let remaining_data = self
            .data_packet_indices
            .iter()
            .filter(|idx| !drops.contains(idx))
            .count();
        let remaining_coding = self
            .coding_packet_indices
            .iter()
            .filter(|idx| !drops.contains(idx))
            .count();
        (remaining_data + remaining_coding) >= self.num_data as usize
    }

    /// Maximum number of data shreds that can be dropped while
    /// keeping the FEC set solvable, assuming no coding shreds
    /// are dropped.
    pub fn max_droppable_data(&self) -> usize {
        if self.num_data == 0 {
            return 0;
        }
        let total = self.data_count() + self.coding_count();
        if total < self.num_data as usize {
            return 0; // already unsolvable
        }
        total - self.num_data as usize
    }
}

// ── ExpectedOutcome ─────────────────────────────────────────────────

/// Expected outcome for a scenario, used by the seal runner
/// to evaluate pass/fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutcome {
    /// Total packets in the scenario (after drops, before reorder).
    pub total_packets: u64,
    /// Expected data shred count.
    pub data_shreds: u64,
    /// Expected coding shred count.
    pub coding_shreds: u64,
    /// Whether FEC recovery should occur.
    pub expects_fec_recovery: bool,
    /// Number of FEC sets that should be recoverable.
    pub recoverable_fec_sets: u32,
    /// Number of data shreds that should be recovered via FEC.
    pub expected_recovered_shreds: u32,
    /// Expected decoded transaction count (0 = do not check).
    ///
    /// Synthetic corpora use fill-pattern payloads that do not contain
    /// valid wire-format transactions, so this is 0 for all synthetic
    /// scenarios. Real captured corpora should set this to the actual
    /// transaction count extracted from the data shreds.
    #[serde(default)]
    pub expected_decoded_txs: u64,
    /// Description of the expected behavior.
    pub description: String,
}

// ── ReplayScenario ──────────────────────────────────────────────────

/// A complete replay scenario: baseline corpus + optional mutations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayScenario {
    /// Scenario name (e.g. "baseline", "fec_missing_1_of_4").
    pub name: String,
    /// Baseline packets in send order.
    pub packets: Vec<ReplayPacket>,
    /// Optional drop plan (packets to omit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_plan: Option<DropPlan>,
    /// Optional reorder plan (permutation of send order).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reorder_plan: Option<ReorderPlan>,
    /// Expected outcome for seal evaluation.
    pub expected: ExpectedOutcome,
    /// FEC set geometries discovered in this corpus.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fec_sets: Vec<FecSetGeometry>,
}

impl ReplayScenario {
    /// Get the effective packet list (after drops and reorder).
    pub fn effective_packets(&self) -> Vec<&ReplayPacket> {
        let mut indices: Vec<u64> = (0..self.packets.len() as u64).collect();

        // Apply drops
        if let Some(ref plan) = self.drop_plan {
            indices.retain(|idx| !plan.drop_indices.contains(idx));
        }

        // Apply reorder
        if let Some(ref plan) = self.reorder_plan {
            // The reorder plan refers to baseline indices. After drops,
            // we filter the plan to only include surviving indices.
            let surviving: std::collections::HashSet<u64> = indices.iter().copied().collect();
            indices = plan
                .order
                .iter()
                .copied()
                .filter(|idx| surviving.contains(idx))
                .collect();
        }

        indices
            .iter()
            .filter_map(|&idx| self.packets.get(idx as usize))
            .collect()
    }
}

// ── Corpus analysis ─────────────────────────────────────────────────

/// Extract FEC set geometries from a list of replay packets.
///
/// Coding shreds carry the authoritative `num_data`/`num_coding`
/// counts. Data shreds don't carry these — their presence is
/// recorded by packet index.
pub fn extract_fec_geometries(packets: &[ReplayPacket]) -> Vec<FecSetGeometry> {
    use std::collections::BTreeMap;

    // Key: (slot, fec_set_index)
    struct SetInfo {
        num_data: u16,
        num_coding: u16,
        data_indices: Vec<u64>,
        coding_indices: Vec<u64>,
    }

    let mut sets: BTreeMap<(u64, u32), SetInfo> = BTreeMap::new();

    for pkt in packets {
        let entry = sets
            .entry((pkt.slot, pkt.fec_set_index))
            .or_insert(SetInfo {
                num_data: 0,
                num_coding: 0,
                data_indices: Vec::new(),
                coding_indices: Vec::new(),
            });

        if pkt.is_data {
            entry.data_indices.push(pkt.index);
        } else {
            entry.coding_indices.push(pkt.index);
            // Parse coding header for FEC counts
            if let Ok((meta, _)) = ShredMeta::parse(&pkt.data) {
                if meta.num_data_shreds > 0 {
                    entry.num_data = meta.num_data_shreds;
                    entry.num_coding = meta.num_coding_shreds;
                }
            }
        }
    }

    sets.into_iter()
        .map(|((slot, fec_set_index), info)| FecSetGeometry {
            slot,
            fec_set_index,
            num_data: info.num_data,
            num_coding: info.num_coding,
            data_packet_indices: info.data_indices,
            coding_packet_indices: info.coding_indices,
        })
        .collect()
}

// ── FEC-aware drop plan builder ─────────────────────────────────────

/// Error from building an FEC-aware drop plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropPlanError {
    /// Not enough coding redundancy to drop the requested number of
    /// data shreds while maintaining solvability.
    InsufficientRedundancy {
        fec_set_slot: u64,
        fec_set_index: u32,
        requested_drops: usize,
        max_droppable: usize,
    },
    /// No FEC sets found in corpus.
    NoFecSets,
}

/// Build a drop plan that removes `n` data shreds per FEC set while
/// keeping every FEC set solvable.
///
/// Strategy: for each FEC set, drop the first `n` data shreds from
/// the corpus ordering. Validates solvability before returning.
pub fn build_fec_solvable_drop_plan(
    packets: &[ReplayPacket],
    data_drops_per_set: usize,
) -> Result<DropPlan, DropPlanError> {
    let geometries = extract_fec_geometries(packets);
    if geometries.is_empty() || geometries.iter().all(|g| g.num_data == 0) {
        return Err(DropPlanError::NoFecSets);
    }

    let mut drop_indices = Vec::new();

    for geo in &geometries {
        if geo.num_data == 0 {
            continue; // Skip sets without known geometry
        }
        let max_drop = geo.max_droppable_data();
        if data_drops_per_set > max_drop {
            return Err(DropPlanError::InsufficientRedundancy {
                fec_set_slot: geo.slot,
                fec_set_index: geo.fec_set_index,
                requested_drops: data_drops_per_set,
                max_droppable: max_drop,
            });
        }
        // Drop the first N data shred packet indices
        for &idx in geo.data_packet_indices.iter().take(data_drops_per_set) {
            drop_indices.push(idx);
        }
    }

    // Validate: every FEC set must still be solvable
    for geo in &geometries {
        if geo.num_data == 0 {
            continue;
        }
        assert!(
            geo.is_solvable_after_drops(&drop_indices),
            "BUG: drop plan broke solvability for slot={} fec_set={}",
            geo.slot,
            geo.fec_set_index,
        );
    }

    let description = format!(
        "FEC-solvable: drop {} data shred(s) per FEC set ({} total drops across {} sets)",
        data_drops_per_set,
        drop_indices.len(),
        geometries.iter().filter(|g| g.num_data > 0).count(),
    );

    Ok(DropPlan {
        drop_indices,
        fec_solvable: true,
        description,
    })
}

/// Build a drop plan that makes at least one FEC set unsolvable.
///
/// Drops enough data shreds that `received_data + received_coding < num_data`.
pub fn build_fec_unsolvable_drop_plan(packets: &[ReplayPacket]) -> Result<DropPlan, DropPlanError> {
    let geometries = extract_fec_geometries(packets);
    if geometries.is_empty() || geometries.iter().all(|g| g.num_data == 0) {
        return Err(DropPlanError::NoFecSets);
    }

    let mut drop_indices = Vec::new();

    for geo in &geometries {
        if geo.num_data == 0 {
            continue;
        }
        // Drop ALL data shreds plus enough coding to go below threshold
        for &idx in &geo.data_packet_indices {
            drop_indices.push(idx);
        }
        // Also drop coding shreds until unsolvable
        // We need total_remaining < num_data
        // After dropping all data: remaining = coding_count
        // If coding_count >= num_data, drop (coding_count - num_data + 1) coding shreds
        let coding_to_drop = if geo.coding_count() >= geo.num_data as usize {
            geo.coding_count() - geo.num_data as usize + 1
        } else {
            0
        };
        for &idx in geo.coding_packet_indices.iter().take(coding_to_drop) {
            drop_indices.push(idx);
        }
    }

    let description = format!(
        "FEC-unsolvable: dropped {} packets to break recovery across {} FEC sets",
        drop_indices.len(),
        geometries.iter().filter(|g| g.num_data > 0).count(),
    );

    Ok(DropPlan {
        drop_indices,
        fec_solvable: false,
        description,
    })
}

// ── Reorder plan builder ────────────────────────────────────────────

/// Build a reorder plan that reverses the packet order within each slot.
pub fn build_reverse_reorder_plan(packets: &[ReplayPacket]) -> ReorderPlan {
    use std::collections::BTreeMap;

    let mut slot_groups: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for pkt in packets {
        slot_groups.entry(pkt.slot).or_default().push(pkt.index);
    }

    let mut order = Vec::with_capacity(packets.len());
    for (_slot, mut indices) in slot_groups {
        indices.reverse();
        order.extend(indices);
    }

    ReorderPlan {
        order,
        description: "Reverse order within each slot".into(),
    }
}

/// Build a reorder plan that interleaves shreds from different slots.
pub fn build_interleaved_reorder_plan(packets: &[ReplayPacket]) -> ReorderPlan {
    use std::collections::BTreeMap;

    let mut slot_groups: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for pkt in packets {
        slot_groups.entry(pkt.slot).or_default().push(pkt.index);
    }

    let mut iters: Vec<std::vec::IntoIter<u64>> =
        slot_groups.into_values().map(|v| v.into_iter()).collect();

    let mut order = Vec::with_capacity(packets.len());
    let mut exhausted = false;
    while !exhausted {
        exhausted = true;
        for iter in &mut iters {
            if let Some(idx) = iter.next() {
                order.push(idx);
                exhausted = false;
            }
        }
    }

    ReorderPlan {
        order,
        description: "Round-robin interleave across slots".into(),
    }
}

// ── Synthetic corpus builder ────────────────────────────────────────

/// Build a synthetic baseline corpus with RS-consistent FEC sets.
///
/// Creates `num_data` data shreds and `num_coding` coding shreds
/// per FEC set, with RS-valid coding payloads generated by the
/// recovery engine's test encoder. All shreds are in a single slot.
///
/// The `shard_payload_len` is the length of the payload *after*
/// the shred header (not total frame size).
pub fn build_synthetic_corpus(
    slot: u64,
    fec_set_index: u32,
    num_data: u16,
    num_coding: u16,
    shard_payload_len: usize,
) -> Vec<ReplayPacket> {
    let mut packets = Vec::with_capacity((num_data + num_coding) as usize);
    let mut idx = 0u64;

    // Build data shard payloads (deterministic fill pattern)
    let data_payloads: Vec<Vec<u8>> = (0..num_data as usize)
        .map(|i| {
            let fill = ((i + 1) & 0xFF) as u8;
            vec![fill; shard_payload_len]
        })
        .collect();

    // Generate RS-consistent coding payloads
    let coding_payloads = encode_test_coding_shards(&data_payloads, num_coding as usize);

    // Build data shred frames
    for (i, payload) in data_payloads.iter().enumerate() {
        let shred_index = i as u32;
        let frame = make_data_frame(slot, shred_index, fec_set_index, payload);
        packets.push(ReplayPacket {
            index: idx,
            data: frame,
            slot,
            shred_index,
            is_data: true,
            fec_set_index,
        });
        idx += 1;
    }

    // Build coding shred frames
    for (i, payload) in coding_payloads.iter().enumerate() {
        let shred_index = num_data as u32 + i as u32;
        let frame = make_coding_frame(
            slot,
            shred_index,
            fec_set_index,
            num_data,
            num_coding,
            i as u16,
            payload,
        );
        packets.push(ReplayPacket {
            index: idx,
            data: frame,
            slot,
            shred_index,
            is_data: false,
            fec_set_index,
        });
        idx += 1;
    }

    packets
}

/// Build a synthetic corpus spanning multiple FEC sets in one slot.
pub fn build_multi_fec_corpus(
    slot: u64,
    num_sets: u32,
    num_data_per_set: u16,
    num_coding_per_set: u16,
    shard_payload_len: usize,
) -> Vec<ReplayPacket> {
    let mut all_packets = Vec::new();
    let mut global_idx = 0u64;

    for set_idx in 0..num_sets {
        let fec_set_index = set_idx * num_data_per_set as u32;
        let set_packets = build_synthetic_corpus(
            slot,
            fec_set_index,
            num_data_per_set,
            num_coding_per_set,
            shard_payload_len,
        );
        for mut pkt in set_packets {
            pkt.index = global_idx;
            all_packets.push(pkt);
            global_idx += 1;
        }
    }

    all_packets
}

// ── Wire frame builders (non-test, used by corpus builder) ──────────

fn make_data_frame(slot: u64, index: u32, fec_set: u32, payload: &[u8]) -> Vec<u8> {
    let total = ShredMeta::DATA_HEADER_SIZE + payload.len();
    let mut buf = vec![0u8; total];
    buf[64] = 0x5a; // legacy data variant
    buf[65..73].copy_from_slice(&slot.to_le_bytes());
    buf[73..77].copy_from_slice(&index.to_le_bytes());
    buf[79..83].copy_from_slice(&fec_set.to_le_bytes());
    buf[ShredMeta::DATA_HEADER_SIZE..].copy_from_slice(payload);
    buf
}

fn make_coding_frame(
    slot: u64,
    index: u32,
    fec_set: u32,
    num_data: u16,
    num_coding: u16,
    position: u16,
    payload: &[u8],
) -> Vec<u8> {
    let total = ShredMeta::CODING_HEADER_SIZE + payload.len();
    let mut buf = vec![0u8; total];
    buf[64] = 0xa5; // legacy coding variant
    buf[65..73].copy_from_slice(&slot.to_le_bytes());
    buf[73..77].copy_from_slice(&index.to_le_bytes());
    buf[79..83].copy_from_slice(&fec_set.to_le_bytes());
    buf[83..85].copy_from_slice(&num_data.to_le_bytes());
    buf[85..87].copy_from_slice(&num_coding.to_le_bytes());
    buf[87..89].copy_from_slice(&position.to_le_bytes());
    buf[ShredMeta::CODING_HEADER_SIZE..].copy_from_slice(payload);
    buf
}

// ── Scenario builders ───────────────────────────────────────────────

/// Build a baseline scenario: all packets, no drops, no reorder.
pub fn build_baseline_scenario(name: &str, packets: Vec<ReplayPacket>) -> ReplayScenario {
    let data_count = packets.iter().filter(|p| p.is_data).count() as u64;
    let coding_count = packets.iter().filter(|p| !p.is_data).count() as u64;
    let total = packets.len() as u64;
    let fec_sets = extract_fec_geometries(&packets);

    ReplayScenario {
        name: name.to_string(),
        packets,
        drop_plan: None,
        reorder_plan: None,
        expected: ExpectedOutcome {
            total_packets: total,
            data_shreds: data_count,
            coding_shreds: coding_count,
            expects_fec_recovery: false,
            recoverable_fec_sets: 0,
            expected_recovered_shreds: 0,
            expected_decoded_txs: 0,
            description: "Baseline: all shreds present, no recovery needed".into(),
        },
        fec_sets,
    }
}

/// Build an FEC-degraded scenario: drop N data shreds per FEC set,
/// forcing FEC recovery.
pub fn build_fec_degraded_scenario(
    name: &str,
    packets: Vec<ReplayPacket>,
    data_drops_per_set: usize,
) -> Result<ReplayScenario, DropPlanError> {
    let fec_sets = extract_fec_geometries(&packets);
    let drop_plan = build_fec_solvable_drop_plan(&packets, data_drops_per_set)?;

    let recoverable_sets = fec_sets.iter().filter(|g| g.num_data > 0).count() as u32;
    let total_recovered = recoverable_sets * data_drops_per_set as u32;

    let effective_data = packets
        .iter()
        .filter(|p| p.is_data && !drop_plan.drop_indices.contains(&p.index))
        .count() as u64;
    let effective_coding = packets
        .iter()
        .filter(|p| !p.is_data && !drop_plan.drop_indices.contains(&p.index))
        .count() as u64;
    let effective_total = effective_data + effective_coding;

    Ok(ReplayScenario {
        name: name.to_string(),
        packets,
        drop_plan: Some(drop_plan),
        reorder_plan: None,
        expected: ExpectedOutcome {
            total_packets: effective_total,
            data_shreds: effective_data,
            coding_shreds: effective_coding,
            expects_fec_recovery: true,
            recoverable_fec_sets: recoverable_sets,
            expected_recovered_shreds: total_recovered,
            expected_decoded_txs: 0,
            description: format!(
                "FEC degraded: {} data shred(s) dropped per set, {} sets recoverable",
                data_drops_per_set, recoverable_sets,
            ),
        },
        fec_sets,
    })
}

/// Build an unsolvable FEC scenario: drop enough shreds that recovery fails.
pub fn build_fec_unsolvable_scenario(
    name: &str,
    packets: Vec<ReplayPacket>,
) -> Result<ReplayScenario, DropPlanError> {
    let fec_sets = extract_fec_geometries(&packets);
    let drop_plan = build_fec_unsolvable_drop_plan(&packets)?;

    let effective_data = packets
        .iter()
        .filter(|p| p.is_data && !drop_plan.drop_indices.contains(&p.index))
        .count() as u64;
    let effective_coding = packets
        .iter()
        .filter(|p| !p.is_data && !drop_plan.drop_indices.contains(&p.index))
        .count() as u64;
    let effective_total = effective_data + effective_coding;

    Ok(ReplayScenario {
        name: name.to_string(),
        packets,
        drop_plan: Some(drop_plan),
        reorder_plan: None,
        expected: ExpectedOutcome {
            total_packets: effective_total,
            data_shreds: effective_data,
            coding_shreds: effective_coding,
            expects_fec_recovery: false,
            recoverable_fec_sets: 0,
            expected_recovered_shreds: 0,
            expected_decoded_txs: 0,
            description: "FEC unsolvable: insufficient shreds for recovery".into(),
        },
        fec_sets,
    })
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────

    /// Build a minimal synthetic corpus: 4 data + 4 coding in one FEC set.
    fn test_corpus_4d4c() -> Vec<ReplayPacket> {
        build_synthetic_corpus(100, 0, 4, 4, 32)
    }

    /// Build a corpus with 2 FEC sets.
    fn test_corpus_2_sets() -> Vec<ReplayPacket> {
        build_multi_fec_corpus(200, 2, 4, 4, 32)
    }

    // ── ReplayPacket ────────────────────────────────────────────────

    #[test]
    fn replay_packet_from_raw_data_shred() {
        let frame = make_data_frame(42, 7, 10, &[0xAA; 32]);
        let pkt = ReplayPacket::from_raw(0, frame.clone()).unwrap();
        assert_eq!(pkt.slot, 42);
        assert_eq!(pkt.shred_index, 7);
        assert!(pkt.is_data);
        assert_eq!(pkt.fec_set_index, 10);
        assert_eq!(pkt.data, frame);
    }

    #[test]
    fn replay_packet_from_raw_coding_shred() {
        let frame = make_coding_frame(99, 4, 0, 4, 4, 0, &[0xBB; 32]);
        let pkt = ReplayPacket::from_raw(1, frame.clone()).unwrap();
        assert_eq!(pkt.slot, 99);
        assert_eq!(pkt.shred_index, 4);
        assert!(!pkt.is_data);
        assert_eq!(pkt.fec_set_index, 0);
    }

    #[test]
    fn replay_packet_from_raw_invalid() {
        assert!(ReplayPacket::from_raw(0, vec![0; 10]).is_none());
    }

    // ── FEC geometry extraction ─────────────────────────────────────

    #[test]
    fn extract_geometry_single_fec_set() {
        let corpus = test_corpus_4d4c();
        let geos = extract_fec_geometries(&corpus);
        assert_eq!(geos.len(), 1);
        assert_eq!(geos[0].slot, 100);
        assert_eq!(geos[0].fec_set_index, 0);
        assert_eq!(geos[0].num_data, 4);
        assert_eq!(geos[0].num_coding, 4);
        assert_eq!(geos[0].data_count(), 4);
        assert_eq!(geos[0].coding_count(), 4);
        assert!(geos[0].is_solvable());
    }

    #[test]
    fn extract_geometry_multi_fec_sets() {
        let corpus = test_corpus_2_sets();
        let geos = extract_fec_geometries(&corpus);
        assert_eq!(geos.len(), 2);
        for geo in &geos {
            assert_eq!(geo.num_data, 4);
            assert_eq!(geo.num_coding, 4);
            assert!(geo.is_solvable());
        }
    }

    // ── FecSetGeometry solvability ──────────────────────────────────

    #[test]
    fn geometry_solvable_after_drops() {
        let corpus = test_corpus_4d4c();
        let geos = extract_fec_geometries(&corpus);
        let geo = &geos[0];

        // Drop 0 data: solvable (trivially)
        assert!(geo.is_solvable_after_drops(&[]));

        // Drop 4 data (all): 0 data + 4 coding = 4 = num_data → solvable
        let all_data: Vec<u64> = geo.data_packet_indices.clone();
        assert!(geo.is_solvable_after_drops(&all_data));

        // Drop 4 data + 1 coding: 0 + 3 = 3 < 4 → unsolvable
        let mut too_many = all_data.clone();
        too_many.push(geo.coding_packet_indices[0]);
        assert!(!geo.is_solvable_after_drops(&too_many));
    }

    #[test]
    fn geometry_max_droppable_data() {
        let corpus = test_corpus_4d4c();
        let geos = extract_fec_geometries(&corpus);
        let geo = &geos[0];
        // 4 data + 4 coding, need 4 total → can drop 4 data (coding covers all)
        assert_eq!(geo.max_droppable_data(), 4);
    }

    // ── FEC-solvable drop plan ──────────────────────────────────────

    #[test]
    fn fec_solvable_drop_1_of_4() {
        let corpus = test_corpus_4d4c();
        let plan = build_fec_solvable_drop_plan(&corpus, 1).unwrap();
        assert!(plan.fec_solvable);
        assert_eq!(plan.drop_indices.len(), 1);

        // Verify the dropped packet is a data shred
        let dropped_pkt = &corpus[plan.drop_indices[0] as usize];
        assert!(dropped_pkt.is_data);

        // Verify remaining set is solvable
        let geos = extract_fec_geometries(&corpus);
        assert!(geos[0].is_solvable_after_drops(&plan.drop_indices));
    }

    #[test]
    fn fec_solvable_drop_max() {
        let corpus = test_corpus_4d4c();
        // 4 data + 4 coding, can drop all 4 data (4 coding >= 4 = num_data)
        let plan = build_fec_solvable_drop_plan(&corpus, 4).unwrap();
        assert!(plan.fec_solvable);
        assert_eq!(plan.drop_indices.len(), 4);

        let geos = extract_fec_geometries(&corpus);
        assert!(geos[0].is_solvable_after_drops(&plan.drop_indices));
    }

    #[test]
    fn fec_solvable_drop_too_many_errors() {
        let corpus = test_corpus_4d4c();
        // 4 data + 4 coding, max droppable = 4; requesting 5 should fail
        let err = build_fec_solvable_drop_plan(&corpus, 5).unwrap_err();
        match err {
            DropPlanError::InsufficientRedundancy {
                requested_drops: 5,
                max_droppable: 4,
                ..
            } => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn fec_solvable_multi_set_drops() {
        let corpus = test_corpus_2_sets();
        let plan = build_fec_solvable_drop_plan(&corpus, 2).unwrap();
        assert!(plan.fec_solvable);
        // 2 drops per set × 2 sets = 4 total drops
        assert_eq!(plan.drop_indices.len(), 4);

        let geos = extract_fec_geometries(&corpus);
        for geo in &geos {
            assert!(geo.is_solvable_after_drops(&plan.drop_indices));
        }
    }

    // ── FEC-unsolvable drop plan ────────────────────────────────────

    #[test]
    fn fec_unsolvable_plan_breaks_recovery() {
        let corpus = test_corpus_4d4c();
        let plan = build_fec_unsolvable_drop_plan(&corpus).unwrap();
        assert!(!plan.fec_solvable);

        let geos = extract_fec_geometries(&corpus);
        // At least one FEC set should be unsolvable
        let any_unsolvable = geos
            .iter()
            .any(|g| !g.is_solvable_after_drops(&plan.drop_indices));
        assert!(
            any_unsolvable,
            "unsolvable plan must break at least one FEC set"
        );
    }

    // ── Reorder plans ───────────────────────────────────────────────

    #[test]
    fn reverse_reorder_preserves_all_packets() {
        let corpus = test_corpus_4d4c();
        let plan = build_reverse_reorder_plan(&corpus);
        assert_eq!(plan.order.len(), corpus.len());

        // Same set of indices
        let mut sorted = plan.order.clone();
        sorted.sort();
        let expected: Vec<u64> = (0..corpus.len() as u64).collect();
        assert_eq!(sorted, expected);
    }

    #[test]
    fn interleaved_reorder_preserves_all_packets() {
        let corpus = test_corpus_2_sets();
        let plan = build_interleaved_reorder_plan(&corpus);
        assert_eq!(plan.order.len(), corpus.len());

        let mut sorted = plan.order.clone();
        sorted.sort();
        let expected: Vec<u64> = (0..corpus.len() as u64).collect();
        assert_eq!(sorted, expected);
    }

    // ── Scenario builders ───────────────────────────────────────────

    #[test]
    fn baseline_scenario_counts_correct() {
        let corpus = test_corpus_4d4c();
        let scenario = build_baseline_scenario("test_baseline", corpus);
        assert_eq!(scenario.expected.total_packets, 8);
        assert_eq!(scenario.expected.data_shreds, 4);
        assert_eq!(scenario.expected.coding_shreds, 4);
        assert!(!scenario.expected.expects_fec_recovery);
        assert_eq!(scenario.fec_sets.len(), 1);
    }

    #[test]
    fn fec_degraded_scenario_counts_correct() {
        let corpus = test_corpus_4d4c();
        let scenario = build_fec_degraded_scenario("test_fec_1_drop", corpus, 1).unwrap();
        assert_eq!(scenario.expected.total_packets, 7); // 8 - 1 drop
        assert_eq!(scenario.expected.data_shreds, 3); // 4 - 1
        assert_eq!(scenario.expected.coding_shreds, 4); // untouched
        assert!(scenario.expected.expects_fec_recovery);
        assert_eq!(scenario.expected.recoverable_fec_sets, 1);
        assert_eq!(scenario.expected.expected_recovered_shreds, 1);
    }

    #[test]
    fn fec_unsolvable_scenario_no_recovery_expected() {
        let corpus = test_corpus_4d4c();
        let scenario = build_fec_unsolvable_scenario("test_unsolvable", corpus).unwrap();
        assert!(!scenario.expected.expects_fec_recovery);
        assert_eq!(scenario.expected.recoverable_fec_sets, 0);
    }

    // ── Synthetic corpus consistency ────────────────────────────────

    #[test]
    fn synthetic_corpus_packets_parse_correctly() {
        let corpus = test_corpus_4d4c();
        assert_eq!(corpus.len(), 8);

        for (i, pkt) in corpus.iter().enumerate() {
            let (meta, _payload) = ShredMeta::parse(&pkt.data)
                .unwrap_or_else(|e| panic!("packet {} failed to parse: {:?}", i, e));
            assert_eq!(meta.slot, 100);
            assert_eq!(meta.fec_set_index, 0);
            if i < 4 {
                assert!(meta.is_data, "packet {} should be data", i);
                assert_eq!(meta.shred_index, i as u32);
            } else {
                assert!(!meta.is_data, "packet {} should be coding", i);
                assert_eq!(meta.shred_index, i as u32);
            }
        }
    }

    #[test]
    fn synthetic_corpus_rs_consistent() {
        // Verify that the coding payloads are RS-consistent with the
        // data payloads by checking that recovery succeeds when we
        // drop a data shred and use a coding shred instead.
        let corpus = build_synthetic_corpus(100, 0, 4, 4, 32);

        // Feed 3 data + 1 coding to the recovery engine
        use crate::recovery::{FecRecoveryEngine, RecoveryEngine};

        let mut eng = FecRecoveryEngine::new();

        // Data shreds 0, 1, 2 (skip 3)
        for pkt in &corpus[0..3] {
            let (meta, payload) = ShredMeta::parse(&pkt.data).unwrap();
            eng.observe_data(&meta, payload);
        }
        // Coding shred 0 (index 4)
        let (meta, payload) = ShredMeta::parse(&corpus[4].data).unwrap();
        eng.observe_coding(&meta, payload);

        assert!(
            eng.can_recover(100, 0),
            "should be recoverable with 3 data + 1 coding"
        );
        let n = eng.recover(100, 0);
        assert_eq!(n, 1, "should recover 1 missing data shred");

        let recovered = eng.take_recovered();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].shred_index, 3, "should recover data shred 3");

        // Verify recovered payload matches original
        let original_payload = &corpus[3].data[ShredMeta::DATA_HEADER_SIZE..];
        assert_eq!(recovered[0].data_payload, original_payload);
    }

    #[test]
    fn synthetic_corpus_rs_consistent_all_data_dropped() {
        // Drop ALL 4 data shreds; recover from 4 coding shreds.
        let corpus = build_synthetic_corpus(100, 0, 4, 4, 32);
        use crate::recovery::{FecRecoveryEngine, RecoveryEngine};

        let mut eng = FecRecoveryEngine::new();

        // Feed only coding shreds (indices 4..8)
        for pkt in &corpus[4..8] {
            let (meta, payload) = ShredMeta::parse(&pkt.data).unwrap();
            eng.observe_coding(&meta, payload);
        }

        assert!(eng.can_recover(100, 0));
        let n = eng.recover(100, 0);
        assert_eq!(n, 4);

        let mut recovered = eng.take_recovered();
        recovered.sort_by_key(|r| r.shred_index);
        assert_eq!(recovered.len(), 4);

        for (i, rec) in recovered.iter().enumerate() {
            assert_eq!(rec.shred_index, i as u32);
            let original_payload = &corpus[i].data[ShredMeta::DATA_HEADER_SIZE..];
            assert_eq!(
                rec.data_payload, original_payload,
                "recovered data shred {} payload mismatch",
                i
            );
        }
    }

    // ── Manifest counts are deterministic ───────────────────────────

    #[test]
    fn manifest_counts_deterministic() {
        // Build the same corpus twice and verify identical counts.
        let c1 = test_corpus_4d4c();
        let c2 = test_corpus_4d4c();

        let s1 = build_baseline_scenario("a", c1);
        let s2 = build_baseline_scenario("b", c2);

        assert_eq!(s1.expected.total_packets, s2.expected.total_packets);
        assert_eq!(s1.expected.data_shreds, s2.expected.data_shreds);
        assert_eq!(s1.expected.coding_shreds, s2.expected.coding_shreds);

        // FEC sets identical
        assert_eq!(s1.fec_sets.len(), s2.fec_sets.len());
        for (g1, g2) in s1.fec_sets.iter().zip(&s2.fec_sets) {
            assert_eq!(g1, g2);
        }
    }

    #[test]
    fn effective_packets_applies_drops() {
        let corpus = test_corpus_4d4c();
        let plan = build_fec_solvable_drop_plan(&corpus, 1).unwrap();
        let scenario = ReplayScenario {
            name: "test".into(),
            packets: corpus.clone(),
            drop_plan: Some(plan),
            reorder_plan: None,
            expected: ExpectedOutcome {
                total_packets: 7,
                data_shreds: 3,
                coding_shreds: 4,
                expects_fec_recovery: true,
                recoverable_fec_sets: 1,
                expected_recovered_shreds: 1,
                expected_decoded_txs: 0,
                description: "test".into(),
            },
            fec_sets: vec![],
        };

        let effective = scenario.effective_packets();
        assert_eq!(effective.len(), 7);
    }

    #[test]
    fn effective_packets_applies_reorder() {
        let corpus = test_corpus_4d4c();
        let reorder = build_reverse_reorder_plan(&corpus);
        let scenario = ReplayScenario {
            name: "test".into(),
            packets: corpus.clone(),
            drop_plan: None,
            reorder_plan: Some(reorder),
            expected: ExpectedOutcome {
                total_packets: 8,
                data_shreds: 4,
                coding_shreds: 4,
                expects_fec_recovery: false,
                recoverable_fec_sets: 0,
                expected_recovered_shreds: 0,
                expected_decoded_txs: 0,
                description: "test".into(),
            },
            fec_sets: vec![],
        };

        let effective = scenario.effective_packets();
        assert_eq!(effective.len(), 8);
        // First packet should be last in original (reversed within slot)
        assert_eq!(effective[0].index, 7);
    }

    // ── JSON roundtrip ──────────────────────────────────────────────

    #[test]
    fn scenario_json_roundtrip() {
        let corpus = test_corpus_4d4c();
        let scenario = build_baseline_scenario("roundtrip", corpus);
        let json = serde_json::to_string_pretty(&scenario).unwrap();
        let restored: ReplayScenario = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "roundtrip");
        assert_eq!(restored.packets.len(), 8);
        assert_eq!(restored.expected.total_packets, 8);
    }

    #[test]
    fn drop_plan_json_roundtrip() {
        let corpus = test_corpus_4d4c();
        let plan = build_fec_solvable_drop_plan(&corpus, 2).unwrap();
        let json = serde_json::to_string(&plan).unwrap();
        let restored: DropPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.drop_indices, plan.drop_indices);
        assert_eq!(restored.fec_solvable, plan.fec_solvable);
    }

    // ── Multi-set drop plan consistency ─────────────────────────────

    #[test]
    fn multi_set_drop_plan_all_solvable() {
        let corpus = build_multi_fec_corpus(300, 3, 8, 4, 64);
        // 8 data + 4 coding per set → max droppable = 4
        let plan = build_fec_solvable_drop_plan(&corpus, 3).unwrap();
        assert_eq!(plan.drop_indices.len(), 9); // 3 drops × 3 sets

        let geos = extract_fec_geometries(&corpus);
        for geo in &geos {
            assert!(
                geo.is_solvable_after_drops(&plan.drop_indices),
                "FEC set slot={} idx={} should be solvable after drops",
                geo.slot,
                geo.fec_set_index,
            );
        }
    }

    #[test]
    fn no_fec_sets_returns_error() {
        // Data-only corpus, no coding shreds → no FEC geometry
        let packets: Vec<ReplayPacket> = (0..4)
            .map(|i| {
                let frame = make_data_frame(50, i, 0, &[0xAA; 32]);
                ReplayPacket {
                    index: i as u64,
                    data: frame,
                    slot: 50,
                    shred_index: i,
                    is_data: true,
                    fec_set_index: 0,
                }
            })
            .collect();

        let err = build_fec_solvable_drop_plan(&packets, 1).unwrap_err();
        assert_eq!(err, DropPlanError::NoFecSets);
    }
}
