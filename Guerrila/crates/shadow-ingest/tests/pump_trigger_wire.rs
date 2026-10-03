use shadow_ingest::decode::pumpfun_amm::PUMPFUN_AMM_PROGRAM_ID;
use shadow_ingest::message::parse_message;

#[test]
fn historical_trigger_parses_as_outer_jupiter_route_with_unresolved_lookups() {
    // Signed transaction at slot 446328956, index 510. The fixture generator
    // verifies its Ed25519 signature; no execution metadata is in these bytes.
    let wire = include_bytes!("../testdata/trigger_446328956.bin");
    let parsed = parse_message(wire).unwrap();
    let jupiter = [
        4, 121, 213, 91, 242, 49, 192, 110, 238, 116, 197, 110, 206, 104, 21, 7, 253, 177, 178,
        222, 163, 244, 142, 81, 2, 177, 205, 162, 86, 188, 19, 143,
    ];

    assert_eq!(wire.len(), 1082);
    assert!(parsed.is_versioned);
    assert_eq!(parsed.static_keys.len(), 19);
    assert_eq!(parsed.instruction_count, 5);
    assert_eq!(parsed.header.num_required_signatures, 1);
    assert!(!parsed.alt_lookup_bytes.is_empty());
    assert!(!parsed.static_keys.contains(&PUMPFUN_AMM_PROGRAM_ID));
    let outer_programs: Vec<_> = parsed
        .program_id_indices
        .iter()
        .map(|&index| parsed.static_keys[index as usize])
        .collect();
    assert_eq!(outer_programs.iter().filter(|p| **p == jupiter).count(), 1);
    assert!(!outer_programs.contains(&PUMPFUN_AMM_PROGRAM_ID));
}
