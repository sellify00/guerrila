# SH8 Report — AF_XDP on self-sourced Agave traffic

## Status

- **Overall:** SH8 is **CLOSED GREEN** for observe-only AF_XDP private-grade proof: SH8-A source-proof, SH8-B transaction-derived records, and SH8-C final longer soak all passed.
- **Safety:** no-send posture held; `live_landed=0`.
- **Host:** `.57`.
- **NIC:** `eno1` only for this constrained proof; `eno2` remains ignored because it has no carrier.
- **XDP mode:** `xdpgeneric`, not native XDP, because native XDP disrupted the management NIC.
- **Private-grade result:** transaction-derived AF_XDP records reached the worker and PM private-grade counters without live send, then held over the longer SH8-C soak.
- **SH8 blocker:** closed for observe-only proof; broader G12 promotion still remains governed by its separate full-threshold gates.

## What changed

The correct source model is now documented and materially exercised:

```text
our Agave mainnet non-voting node
-> receives Solana UDP / Turbine / TVU traffic on eno1
-> AF_XDP attaches to eno1
-> Shadow workers should consume AF_XDP records
```

Latitude is only the bare-metal host/network provider. It is not the Solana feed provider.

The old SH8-A blocker is retired:

```text
Agave bootstrap / snapshot churn is no longer the limiting step.
```

The old SH8-A blocker is now retired:

```text
AF_XDP parsed shreds now emit bounded no-op source-proof records
that reach the worker under observe-only / no-send.
```

## Host fixes and findings

- Built and installed `agave-validator 3.1.14` from source because the installed Solana CLI release did not include the validator binary.
- Created `agave-mainnet-no-vote.service` for mainnet non-voting operation.
- Fixed the expected genesis hash to the full mainnet hash.
- Proved Solana UDP on `eno1` with tcpdump.
- Found `eno2` has no carrier and zero RX, so it cannot be used for SH8-A right now.
- Found native XDP can drop carrier / disrupt the management path on `eno1`; use `xdpgeneric` with rollback instead.
- Later replacement-host validation on `.221` (`259S001355`, Intel `i40e`) showed a stricter failure mode than `.57`: rescue-mode logs from the failed 2026-05-06 observe-only run show `i40e 0000:05:00.0` `AMD-Vi: IO_PAGE_FAULT` / `IOMMU Event log restarting` beginning about two seconds after `xdpgeneric` was attached to `eno1`. The run still emitted gate evidence, but the host eventually lost reachability. Treat this as host-specific proof that `.221` must not use `eno1` for AF_XDP/XDP at all.
- Added a service override with `LimitMEMLOCK=infinity` after Agave began crash-looping with `unable to set memory lock limit`.
- Kept hugepages at `vm.nr_hugepages=4096` for AF_XDP UMEM.
- Started `shadow-state` and `shadow-oracle` sidecars to satisfy startup readiness.
- Fixed the real host storage bug: `/mnt/solana-ledger` and `/mnt/solana-accounts` were previously rootfs directories on `/dev/md0`; they are now mounted on `nvme2n1` and `nvme3n1` as `sol-ledger` and `sol-accounts`.
- Preserved validator restart state on-host with `shadow-preserve-validator-state.service` under `/mnt/solana-accounts/validator-preserve`.
- Added `/home/ubuntu/sh8a_wait_ready_then_run.sh` to wait for RPC/replay readiness and auto-trigger the AF_XDP proof immediately when Agave becomes usable.
- Added Agave disk-backed accounts index and a small accounts DB cache on `.57` so the constrained 125 GiB host stops OOM-killing during accounts-index generation.
- Added a gated SH8 local-replay stale-slot bypass for AF_XDP observe-only/no-send records only; this keeps the constrained-host proof usable while Agave replay lags without widening live/public execution.

## XDP program hardening

`g1-host-pack\xdp\shadow_rx_meta.bpf.c` now passes non-Solana traffic through and redirects only packets destined to local Agave Solana ports, with an additional raw mainnet shred predicate. This avoids peer-source-port/gossip noise while keeping management/RPC traffic on `eno1` untouched.

That hardening remains valid for the original `.57` constrained proof host, but it does **not** make `eno1` safe on `.221`; the rescue-confirmed `i40e` / AMD-Vi DMA fault there means `.221` should reserve `eno1` for management and normal Agave traffic only.

The filtered object compiled on `.57` with:

```bash
cd ~/Guerrila/g1-host-pack/xdp
make clean && make
```

## Bootstrap unlock and AF_XDP chronology

### 1. Pre-unlock filtered 48-queue run

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_141959-eno1-xdpgeneric-afxdp-48q-filtered-agave-stable
```

Result:

```text
xskmap_registered=32
xsk_bound=32
bind_invalid_arg=288
worker_records_seen=65
worker_signal_afxdp=0
live_landed=0
latency_ok=true
worker_latency_proof_present=true
```

Meaning:

```text
XDP/XSKMAP path could arm safely, but Agave bootstrap was still the blocker.
```

### 2. Agave-ready auto-trigger with stale host binary

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_224855-af_xdp-after-agave-ready
```

Result:

```text
worker_records_seen=81
worker_signal_afxdp=0
xskmap_registered=32
xsk_bound=32
bind_invalid_arg=336
afxdp_* fields=None
```

Meaning:

```text
Replay readiness detection worked and SH8-A launched automatically,
but the host binary was stale and the 48-worker queue model was wrong.
```

### 3. 32-queue run with raw AF_XDP counters deployed

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_235006-af_xdp-32q-sh8-probe
```

Host binary:

```text
commit 7451372  Add AF_XDP raw gate counters
```

Config fix:

```text
shred_capture_workers: 32
```

Meaning:

```text
The host binary is no longer stale, the valid AF_XDP queue range is 0-31,
and SH8-A now has real ingress/parse counters from the host artifact.
```

### 4. SH8-A parsed-shred source-proof first green

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260429_235006-af_xdp-32q-sh8-probe
```

Host binary:

```text
commit 566d76d  Tighten SH8 parsed-shred probe cadence
```

Config fix:

```text
rollout.sh8_afxdp_source_probe_mode: parsed_shred_noop
```

Key counters:

```text
shred_capture_workers=32
xskmap_registered=32
xsk_bound=32
bind_invalid_arg=0
afxdp_frames_seen=327
afxdp_udp_8000_8025_seen=327
afxdp_xsk_rx_batches=327
afxdp_shred_parse_ok=8
afxdp_shred_parse_err=319
afxdp_feed_enqueue_ok=0
afxdp_feed_enqueue_full=0
afxdp_source_probe_emit_attempt=8
afxdp_source_probe_emit_ok=8
afxdp_source_probe_emit_full=0
afxdp_source_probe_emit_err=0
worker_signal_afxdp=2
worker_source_probe_afxdp_parsed_shred_drops=2
observe_candidates_private_grade=0
rollout_private_grade_admissible_opportunities=0
live_landed=0
worker_health_drops=0
latency_ok=true
worker_latency_proof_present=true
promote_allowed=false
all_gates_green=false
```

### 5. SH8-B stage-counter rerun localizes the next stop

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_001337-af_xdp-32q-sh8b-counters
```

Host binary:

```text
commit 1cfa2f7  Add SH8-B AF_XDP stage counters
```

Key counters:

```text
shred_capture_workers=32
xskmap_registered=32
xsk_bound=32
bind_invalid_arg=0
afxdp_frames_seen=294
afxdp_udp_8000_8025_seen=294
afxdp_xsk_rx_batches=294
afxdp_shred_parse_ok=7
afxdp_shred_parse_err=287
afxdp_reassembly_insert_ok=7
afxdp_reassembly_complete_entries=5
afxdp_fec_recovered_entries=0
afxdp_data_shred_payload_ok=5
afxdp_tx_extract_attempt=5
afxdp_tx_extract_ok=0
afxdp_tx_extract_err=5
afxdp_tx_record_emit_attempt=0
afxdp_tx_record_emit_ok=0
afxdp_tx_record_emit_full=0
afxdp_feed_enqueue_ok=0
afxdp_feed_enqueue_full=0
afxdp_source_probe_emit_ok=7
worker_signal_afxdp=3
observe_candidates_private_grade=0
rollout_private_grade_admissible_opportunities=0
live_landed=0
worker_health_drops=0
latency_ok=true
worker_latency_proof_present=true
promote_allowed=false
all_gates_green=false
```

Meaning:

```text
SH8-A stays green during the SH8-B rerun.
Parsed shreds are now entering reassembly and yielding data payloads.
The current stop is transaction extraction from those payloads:
tx_extract_attempt > 0, tx_extract_ok = 0, tx_extract_err > 0.
No real transaction-derived AF_XDP IngressRecord is emitted yet.
```

### 6. SH8-B tx diagnostics exposed false DATA payloads

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_002938-af_xdp-32q-sh8b-txdiag
```

Host binary:

```text
commit 13f9135  Add SH8-B tx extraction diagnostics
```

Key counters:

```text
afxdp_tx_extract_attempt=5
afxdp_tx_extract_ok=0
afxdp_tx_extract_err=5
afxdp_tx_extract_err_bad_entry_header=5
```

Bounded samples showed:

```text
payload_len=44
reason=bad_entry_header
slot/shred_index values were impossible for current Solana mainnet slots
```

Meaning:

```text
The extractor was not failing on valid entry payloads. The parser was accepting
coding/random UDP as DATA shreds because the legacy DATA/CODE constants were reversed.
Agave v3.1.14 defines legacy DATA as 0xa5 and legacy CODE as 0x5a.
```

### 7. Corrected shred parser moves SH8-B stop before tx extraction

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_004644-af_xdp-32q-sh8b-txdiag
```

Host binary:

```text
commit 4843f9e  Split SH8 parsed shred counters
```

Key counters:

```text
afxdp_frames_seen=296
afxdp_udp_8000_8025_seen=296
afxdp_xsk_rx_batches=296
afxdp_shred_parse_ok=10
afxdp_shred_parse_data_ok=0
afxdp_shred_parse_coding_ok=10
afxdp_shred_parse_err=286
afxdp_reassembly_insert_ok=10
afxdp_reassembly_complete_entries=0
afxdp_data_shred_payload_ok=0
afxdp_tx_extract_attempt=0
afxdp_tx_extract_ok=0
afxdp_tx_extract_err=0
afxdp_source_probe_emit_ok=10
worker_signal_afxdp=2
live_landed=0
worker_health_drops=0
```

Meaning:

```text
SH8-A source-proof still holds.
The corrected parser no longer fabricates DATA payloads from legacy coding shreds.
The current SH8-B stop is now before tx extraction: this proof saw coding shreds only.
```

### 8. SH8-B transaction-derived AF_XDP first green

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_031546-af_xdp-32q-sh8b-txdiag
```

Host binary:

```text
commit ebf0ee8  Stream SH8 transaction extraction across shreds
```

Key counters:

```text
shred_capture_workers=32
xskmap_registered=32
xsk_bound=32
bind_invalid_arg=0
afxdp_frames_seen=1493713
afxdp_udp_8000_8025_seen=1493713
afxdp_shred_parse_ok=1493710
afxdp_shred_parse_data_ok>0
afxdp_reassembly_complete_entries=316431
afxdp_data_shred_payload_ok=293519
afxdp_tx_extract_attempt=316431
afxdp_tx_extract_ok=75814
afxdp_tx_record_emit_attempt=2
afxdp_tx_record_emit_ok=2
afxdp_feed_enqueue_ok=2
worker_signal_afxdp=47
worker_stale_slot_drops=0
observe_candidates_private_grade=3
rollout_private_grade_admissible_opportunities=1
live_landed=0
worker_health_drops=0
latency_ok=true
worker_latency_proof_present=true
promote_allowed=false
all_gates_green=false
```

Meaning:

```text
The SH8-B path is now proven end to end:
AF_XDP packet -> DATA shred -> reassembly/deshred stream -> wire tx extraction
-> transaction-derived IngressRecord -> worker -> PM private-grade counters.
```

Safety stayed intact: `live_landed=0`, no live send, no queue-count change, no Geyser relabeling.

### 9. SH8-C final longer AF_XDP private-grade soak

Artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak
```

Basis:

```text
existing 32-queue filtered `xdpgeneric` AF_XDP path and SH8-B green binary
```

Key counters:

```text
afxdp_frames_seen=8577897
afxdp_udp_8000_8025_seen=8577897
afxdp_shred_parse_data_ok=6406417
afxdp_tx_extract_ok=286568
afxdp_tx_record_emit_ok=15
afxdp_feed_enqueue_ok=15
worker_signal_afxdp=66
observe_candidates_private_grade=15
rollout_private_grade_admissible_opportunities=5
rollout_would_send=3743
rollout_live_send_suppressed=3743
live_landed=0
worker_health_drops=0
latency_ok=true
worker_latency_proof_present=true
```

Meaning:

```text
The longer soak confirmed that real AF_XDP DATA shreds continue through
transaction extraction, feed enqueue, worker accounting, and PM private-grade
observe counters with live send still suppressed.
```

## Latest decisive evidence

Latest host artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak
```

Key counters:

```text
shred_capture_workers=32
afxdp_frames_seen=8577897
afxdp_udp_8000_8025_seen=8577897
afxdp_shred_parse_data_ok=6406417
afxdp_tx_extract_ok=286568
afxdp_tx_record_emit_ok=15
afxdp_feed_enqueue_ok=15
worker_signal_afxdp=66
observe_candidates_private_grade=15
rollout_private_grade_admissible_opportunities=5
rollout_would_send=3743
rollout_live_send_suppressed=3743
live_landed=0
worker_health_drops=0
latency_ok=true
worker_latency_proof_present=true
```

Interpretation:

```text
AF_XDP runtime sees real Solana UDP on the host and the filtered BPF path is working.
Queue-count mismatch is fixed: 32 workers bind cleanly.
SH8-B extraction stayed green over a longer SH8-C window.
PM recorded private-grade observe candidates and private-grade admissible opportunities from AF_XDP, not from Geyser relabeling.
Live remained suppressed and worker health stayed clean.
```

## SH8 closure criteria

SH8 observe-only AF_XDP private-grade proof is closed when:

```text
afxdp_tx_extract_ok > 0
afxdp_tx_record_emit_ok > 0
afxdp_feed_enqueue_ok > 0
worker_signal_afxdp > 0
observe_candidates_private_grade > 0
rollout_private_grade_admissible_opportunities > 0
live_landed = 0
worker_health_drops = 0
```

The SH8-C final soak artifact satisfies all of those criteria.

## Implementation notes

The SH8-B green patch made three material corrections:

1. XDP redirect was restricted to destination-port/raw-mainnet-shred traffic.
2. The host Agave service was moved to disk-backed accounts index plus a small accounts DB cache so the 125 GiB host stops OOM-killing during accounts-index build.
3. Transaction extraction now streams entry bytes across DATA shreds instead of treating every single shred payload as an independent complete entry batch.

The local-replay stale-slot bypass is gated by observe-only/no-send plus AF_XDP workers and only affects AF_XDP records on this SH8 constrained proof host.

Do not fake this by relabeling Geyser records. SH8-A, SH8-B, and SH8-C are now satisfied:

```text
worker_signal_afxdp > 0
afxdp_source_probe_emit_ok > 0
afxdp_tx_extract_ok > 0
afxdp_tx_record_emit_ok > 0
observe_candidates_private_grade > 0
rollout_private_grade_admissible_opportunities > 0
live_landed = 0
worker_health_drops = 0
```

## Next steps

1. Treat SH8 as closed for observe-only AF_XDP private-grade evidence.
2. Keep `xdpgeneric`, rollback, `shred_capture_workers: 32`, destination-only/raw-shred BPF, and observe-only/no-send unchanged for any repeat proof.
3. Broader G12 promotion remains separate and must still honor full sample/threshold gates.

Do not reopen old blockers:

```text
- no more waiting on Agave bootstrap
- no more 48-queue AF_XDP runs on this host
- no Geyser relabeling
- no SH6
- no live send
```
