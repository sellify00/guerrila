use solana_program_test::*;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::Signer,
    system_instruction,
    transaction::Transaction,
};
const SPL: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];
const CB: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];
fn gid() -> Pubkey {
    Pubkey::new_from_array([
        44, 166, 212, 243, 246, 95, 159, 181, 8, 196, 202, 112, 80, 238, 140, 127, 234, 117, 35,
        83, 58, 134, 182, 245, 66, 191, 101, 112, 196, 29, 178, 146,
    ])
}
fn eb(a: u64, b: u64, c: u64, d: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(32);
    for x in [a, b, c, d] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}
fn vault(pt: &mut ProgramTest, pk: Pubkey, amt: u64) {
    let m = Pubkey::new_unique();
    let o = Pubkey::new_unique();
    let mut d = vec![0u8; 165];
    d[0..32].copy_from_slice(&m.to_bytes());
    d[32..64].copy_from_slice(&o.to_bytes());
    d[64..72].copy_from_slice(&amt.to_le_bytes());
    d[72] = 1;
    pt.add_account(
        pk,
        Account {
            lamports: 1_000_000,
            data: d,
            owner: Pubkey::new_from_array(SPL),
            executable: false,
            rent_epoch: 0,
        },
    );
}

// Probe Harmonic shape: [SetCULimit, SetCUPrice, Guard, SystemTransfer(0)]
#[tokio::test]
async fn probe_harmonic_shape() {
    for limit in [
        880u32, 890, 895, 900, 910, 920, 930, 940, 950, 960, 970, 980, 990, 1000, 1010, 1020, 1030,
        1040, 1044, 1050, 1060,
    ] {
        let vi = Pubkey::new_unique();
        let vo = Pubkey::new_unique();
        let mut pt = ProgramTest::new("shadow_guard", gid(), None);
        vault(&mut pt, vi, 1_100_000);
        vault(&mut pt, vo, 450_000);
        let (banks, payer, bh) = pt.start().await;
        let tx = Transaction::new_signed_with_payer(
            &[
                Instruction {
                    program_id: Pubkey::new_from_array(CB),
                    accounts: vec![],
                    data: {
                        let mut d = vec![0x02];
                        d.extend_from_slice(&limit.to_le_bytes());
                        d
                    },
                },
                Instruction {
                    program_id: Pubkey::new_from_array(CB),
                    accounts: vec![],
                    data: {
                        let mut d = vec![0x03];
                        d.extend_from_slice(&1000u64.to_le_bytes());
                        d
                    },
                },
                Instruction {
                    program_id: gid(),
                    accounts: vec![
                        AccountMeta::new_readonly(vi, false),
                        AccountMeta::new_readonly(vo, false),
                    ],
                    data: eb(1_050_000, 1_150_000, 400_000, 480_000),
                },
                system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0),
            ],
            Some(&payer.pubkey()),
            &[&payer],
            bh,
        );
        let ok = banks.process_transaction(tx).await.is_ok();
        println!(
            "Harmonic CU {limit:>5}: {}",
            if ok { "PASS" } else { "FAIL" }
        );
    }
}

// Probe Jito shape: [SetCULimit, Guard, SystemTransfer(0), SystemTransfer(1000)]
#[tokio::test]
async fn probe_jito_shape() {
    let tip = Pubkey::new_unique();
    for limit in [
        880u32, 900, 920, 940, 960, 980, 1000, 1020, 1040, 1044, 1050, 1060, 1080, 1100,
    ] {
        let vi = Pubkey::new_unique();
        let vo = Pubkey::new_unique();
        let mut pt = ProgramTest::new("shadow_guard", gid(), None);
        vault(&mut pt, vi, 1_100_000);
        vault(&mut pt, vo, 450_000);
        pt.add_account(
            tip,
            Account {
                lamports: 1_000_000,
                data: vec![],
                owner: Pubkey::new_from_array([0; 32]),
                executable: false,
                rent_epoch: 0,
            },
        );
        let (banks, payer, bh) = pt.start().await;
        let tx = Transaction::new_signed_with_payer(
            &[
                Instruction {
                    program_id: Pubkey::new_from_array(CB),
                    accounts: vec![],
                    data: {
                        let mut d = vec![0x02];
                        d.extend_from_slice(&limit.to_le_bytes());
                        d
                    },
                },
                Instruction {
                    program_id: gid(),
                    accounts: vec![
                        AccountMeta::new_readonly(vi, false),
                        AccountMeta::new_readonly(vo, false),
                    ],
                    data: eb(1_050_000, 1_150_000, 400_000, 480_000),
                },
                system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0),
                system_instruction::transfer(&payer.pubkey(), &tip, 1000),
            ],
            Some(&payer.pubkey()),
            &[&payer],
            bh,
        );
        let ok = banks.process_transaction(tx).await.is_ok();
        println!("Jito CU {limit:>5}: {}", if ok { "PASS" } else { "FAIL" });
    }
}
