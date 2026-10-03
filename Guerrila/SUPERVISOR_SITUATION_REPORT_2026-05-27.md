# Supervisor Situation Report - 2026-05-27

## Bottom Line

Rebalancing is **execution-plumbing ready but proposal parked**.

The fresh `.15` native AF_XDP zero-copy epoch plus the normalized `66r*` chain produced:

```text
status=REBALANCING_EXECUTION_PLUMBING_READY
execution_plumbing_ready_rows=21
sender_no_send_ready_rows=12
bundle_no_submit_ready_rows=9
proposal_eligible_rebalancing_rows=0
architecture_blocker_counts={}
send_transaction_called=false
sendBundle_called=false
wire_send=0
live_landed=0
```

This is execution-plumbing readiness, not owner-mode eligibility or live approval.

## Current Lane Map

```text
Rebalancing:
  status = REBALANCING_EXECUTION_PLUMBING_READY
  role = parked at proposal stage
  proposal_eligible_rebalancing_rows = 0
  next = 66RG2 owner-selector instrumentation repair

DirectArb:
  status = DIRECTARB_STRUCTURAL_HUNT_READY
  role = sentinel only
  blocker = policy-clean roundtrip economics / habitat quality
  latest read = cap-1 and bundle work found entry-positive traps, not default-policy sendables

Backrun:
  status = BACKRUN_STRUCTURAL_HUNT_READY
  blocker = immediate target freshness / source timing on .15 AF_XDP
  latest read = AFBR1H SOURCE_LATE_DOMINANT, 10/10 valid immediate samples SOURCE_LATE

Post-shock / Continuation:
  status = signal real, architecture not certified
  next = structural instrument-readiness certificate

Housekeeping:
  status = under-tested
  next = real-trade vs saved-loss / Drop-only structural split
```

## Rebalancing Proof

```text
script=66rg_rebalancing_certificate_owner_reconciliation_20260527
artifact=/home/ubuntu/Guerrila/g1-host-pack/results/66rg_rebalancing_certificate_owner_reconciliation_20260527/final_verdict.json
final_verdict=REBALANCING_PARKED_CERTIFICATE_AMENDED
66rb_status=REBALANCING_EXECUTION_PLUMBING_READY
66rb_meaning=execution_plumbing_structural_ready
rebalancing_rows_seen=3997
exact_positive_rows=91
quote_min_out_ready_rows=324
entry_rule_ready_rows=3451
exit_rule_ready_rows=2916
pm_ready_rows=72
account_ready_rows=47
inline_ata_possible_rows=9
template_ready_rows=69
sender_no_send_ready_rows=12
bundle_no_submit_ready_rows=9
roundtrip_positive_rows=91
structural_ready_rows=21
execution_plumbing_ready_rows=21
owner_mode_match_rebalancing_rows=0
pm_owner_lock_ready_rows=0
restoration_pressure_measured_rows=0
false_restoration_gate_input_ready_rows=0
proposal_eligible_rebalancing_rows=0
```

Fresh Run65 host handback was clean:

```text
ssh_ok=true
xskmap_absent=true
sh1-agave-no-geyser-2.service=active
local_rpc_getHealth=ok
live_landed=0
would_wire_send_count=0
```

## Hard Stops

```text
No live send.
No sendBundle.
No AFBR2.
No LS12.
No cap increase.
No acquisition.
No ATA creation.
No Token-2022 explicit allow.
No policy widening from watch-only rows.
```

## Next Supervisor Action

Run `66RG2` owner-selector instrumentation repair. Do not prepare another Rebalancing proposal package until owner-mode match, PM owner lock or split-rule readiness, restoration pressure, and false-restoration inputs produce `proposal_eligible_rebalancing_rows > 0`.

Do not spend the next block on broad DirectArb-B retries unless the habitat selector produces policy-clean, roundtrip-positive rows.
