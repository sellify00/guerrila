import os

ASSEMBLY_FILE = "crates/shadow-core/src/assembly.rs"

with open(ASSEMBLY_FILE, "r") as f:
    text = f.read()

tests_appendix = """
    #[test]
    fn test_jito_golden_bytes() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer { current_allocation: &alloc, absolute_ceiling: u64::MAX };
        let input = ExecutionAssemblyInput {
            prepared: dummy_prepared(RouteDecision::JitoBundle, crate::routing::IntentKind::PlainWithinCap),
            recent_blockhash: [8u8; 32],
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            template_id: TemplateId::JitoPlain,
            route_base_cu_limit: 300_000,
            swap_accounts: FixedSwapAccounts { program_id: [12u8; 32], pubkeys: [[0; 32]; 8], is_writable: [false; 8], is_signer: [false; 8], num_accounts: 0 },
            swap_ix_data: FixedSwapIxData { data: [0; 64], len: 0 },
            guard_bounds: GuardBounds { min_reserve_in: 1, max_reserve_in: 2, min_reserve_out: 3, max_reserve_out: 4 },
            pool_index: 0,
            mint_out_index: 0,
            overlay_delta: OverlayDelta { pool_index: 0, reserve_in_delta: 0, reserve_out_delta: 0 },
            jito_tip_rotation: JitoTipRotation { tip_account: [7u8; 32] },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [9u8; 32],
        };

        let tpl = build_jito_template(&input).unwrap();
        
        // Construct reference bytes matching Solana Canonical Format organically
        let mut expected = vec![];
        // Header
        expected.push(1); // num_required_signatures
        expected.push(0); // num_readonly_signed
        expected.push(4); // num_readonly_unsigned (CU, SG, Swap, System)
        // Count
        expected.push(6); // Total accounts = signer + tip + 4 programs
        
        // Accounts
        expected.extend_from_slice(&[9u8; 32]); // Signer Writable
        expected.extend_from_slice(&[7u8; 32]); // Tip Account Writable
        expected.extend_from_slice(&crate::assembly::COMPUTE_BUDGET_PROGRAM_ID);
        expected.extend_from_slice(&crate::assembly::SHADOW_GUARD_PROGRAM_ID);
        expected.extend_from_slice(&[12u8; 32]); // swap program id
        expected.extend_from_slice(&crate::assembly::SYSTEM_PROGRAM_ID);
        
        // Blockhash
        expected.extend_from_slice(&[8u8; 32]);
        
        // Instructions
        expected.push(4); // ix count
        
        // ix0: CU limit
        expected.extend_from_slice(&[2, 0, 5]); // program index = 2 (compute budget)
        expected.push(0x02);
        expected.extend_from_slice(&300_000u32.to_le_bytes());
        
        // ix1: GuardBounds
        expected.extend_from_slice(&[3, 0, 32]); // program index = 3 (shadow guard)
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&2u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&4u64.to_le_bytes());
        
        // ix2: Swap
        expected.extend_from_slice(&[4, 0, 0]); // program index = 4 (swap), 0 accounts, 0 data
        
        // ix3: Jito Tip Transfer
        expected.extend_from_slice(&[5, 2, 0, 1, 12]); // program index = 5 (system), 2 accounts (signer 0, tip 1), 12 bytes data
        expected.extend_from_slice(&[2u8, 0, 0, 0]);
        expected.extend_from_slice(&100u64.to_le_bytes()); // tip_lamports dummy value = 100
        
        assert_eq!(tpl.len, expected.len());
        assert_eq!(&tpl.raw_bytes[..tpl.len], expected.as_slice());
    }

    #[test]
    fn test_harmonic_golden_bytes() {
        let alloc = AtomicU64::new(1_000_000);
        let fencer = InventoryFencer { current_allocation: &alloc, absolute_ceiling: u64::MAX };
        let input = ExecutionAssemblyInput {
            prepared: dummy_prepared(RouteDecision::HarmonicBundle, crate::routing::IntentKind::PlainWithinCap),
            recent_blockhash: [8u8; 32],
            deadline_unix_ns: u64::MAX,
            ingress_unix_ns: 0,
            template_id: TemplateId::HarmonicPlain,
            route_base_cu_limit: 300_000,
            swap_accounts: FixedSwapAccounts { program_id: [12u8; 32], pubkeys: [[0; 32]; 8], is_writable: [false; 8], is_signer: [false; 8], num_accounts: 0 },
            swap_ix_data: FixedSwapIxData { data: [0; 64], len: 0 },
            guard_bounds: GuardBounds { min_reserve_in: 1, max_reserve_in: 2, min_reserve_out: 3, max_reserve_out: 4 },
            pool_index: 0,
            mint_out_index: 0,
            overlay_delta: OverlayDelta { pool_index: 0, reserve_in_delta: 0, reserve_out_delta: 0 },
            jito_tip_rotation: JitoTipRotation { tip_account: [7u8; 32] },
            inventory_fencer: &fencer,
            fee_payer_pubkey: [9u8; 32],
        };

        let tpl = build_harmonic_template(&input).unwrap();
        
        // Construct reference bytes matching Solana Canonical Format organically
        let mut expected = vec![];
        // Header
        expected.push(1); // num_required_signatures
        expected.push(0); // num_readonly_signed
        expected.push(3); // num_readonly_unsigned (CU, SG, Swap)
        // Count
        expected.push(4); // Total accounts = signer + 3 programs
        
        // Accounts
        expected.extend_from_slice(&[9u8; 32]); // Signer Writable
        expected.extend_from_slice(&crate::assembly::COMPUTE_BUDGET_PROGRAM_ID);
        expected.extend_from_slice(&crate::assembly::SHADOW_GUARD_PROGRAM_ID);
        expected.extend_from_slice(&[12u8; 32]); // swap program id
        
        // Blockhash
        expected.extend_from_slice(&[8u8; 32]);
        
        // Instructions
        expected.push(4); // ix count
        
        // ix0: CU limit (+150 pad for Harmonic)
        expected.extend_from_slice(&[1, 0, 5]); // program index = 1 (compute budget)
        expected.push(0x02);
        expected.extend_from_slice(&300_150u32.to_le_bytes());

        // ix1: CU price
        expected.extend_from_slice(&[1, 0, 9]); // program index = 1 (compute budget)
        expected.push(0x03);
        expected.extend_from_slice(&333u64.to_le_bytes()); // dummy cu price 333
        
        // ix2: GuardBounds
        expected.extend_from_slice(&[2, 0, 32]); // program index = 2 (shadow guard)
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&2u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&4u64.to_le_bytes());
        
        // ix3: Swap
        expected.extend_from_slice(&[3, 0, 0]); // program index = 3 (swap), 0 accounts, 0 data
        
        assert_eq!(tpl.len, expected.len());
        assert_eq!(&tpl.raw_bytes[..tpl.len], expected.as_slice());
    }
}"""

text = text.replace("}\n", tests_appendix)

with open(ASSEMBLY_FILE, "w") as f:
    f.write(text)

print("Golden tests injected.")
