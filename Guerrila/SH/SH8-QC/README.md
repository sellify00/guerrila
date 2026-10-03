# SH8-QC — Raw AF_XDP Shred Capture

This folder tracks SH8 evidence for self-sourced AF_XDP ingress from our own Agave node on `.57`.

Current `.57` status: **SH8 is closed green for observe-only AF_XDP private-grade proof**. SH8-A source-proof, SH8-B transaction-derived records, and SH8-C longer private-grade soak all passed with live send suppressed. Agave/bootstrap, NVMe storage, filtered generic XDP, XSKMAP pinning, queue count, parsed-shred worker reachability, DATA shred reassembly, wire transaction extraction, feed-ring emission, and PM private-grade observe counters are no longer blockers for this slice.

Latest decisive artifact:

```text
/home/ubuntu/shadow-host-audit/sh8-raw-afxdp/20260430_032838-sh8c-final-afxdp-soak
```

What is now green:

- Agave is active and past snapshot download / untar; replay readiness was detected by `/home/ubuntu/sh8a_wait_ready_then_run.sh`.
- `/mnt/solana-ledger` and `/mnt/solana-accounts` are mounted on the dedicated `3.5 TiB` NVMe devices.
- `xdpgeneric` attach on `eno1` with rollback and pinned XSKMAP is stable.
- The filtered XDP program redirects only local destination Solana UDP `8000-8025` packets that match raw mainnet shred shape/version.
- `shred_capture_workers: 32` matches the valid AF_XDP queue range on this host.
- The accepted 32-queue bind proof had `xskmap_registered=32`, `xsk_bound=32`, and `bind_invalid_arg=0`.
- Raw AF_XDP ingress/parse/extraction counters stayed green over the SH8-C soak:
  - `afxdp_frames_seen=8577897`
  - `afxdp_udp_8000_8025_seen=8577897`
  - `afxdp_shred_parse_data_ok=6406417`
  - `afxdp_tx_extract_ok=286568`
  - `afxdp_tx_record_emit_ok=15`
  - `afxdp_feed_enqueue_ok=15`
  - `worker_signal_afxdp=66`
- PM private-grade observe-only evidence stayed green:
  - `observe_candidates_private_grade=15`
  - `rollout_private_grade_admissible_opportunities=5`
- Safety stayed green: `live_landed=0`, `worker_health_drops=0`, `latency_ok=true`, `worker_latency_proof_present=true`.
- Full G12 promotion remains separate; this SH8-C soak closes the AF_XDP private-grade observe-only proof, not live promotion.

Resolved root causes:

- AF_XDP ingress is real, parsed shreds now emit bounded no-op source-proof records, and workers count those AF_XDP receipts under observe-only/no-send.
- The earlier `bad_entry_header` payload samples were caused by reversed legacy shred classification (`0x5a` is coding, `0xa5` is data); that bug is fixed.
- The source-or-destination XDP port filter admitted peer-source-port noise; the filter is now destination-only plus raw-shred gated.
- Real Solana transactions can split across shred payloads; SH8-B now streams extraction across DATA shred boundaries and emits transaction-derived AF_XDP `IngressRecord`s.

Most useful files here:

- `SH8_REPORT.md` — durable status report, chronology, and SH8 closure evidence.
- `evidence\sh8a-eno1-afxdp-start-summary.txt` — compact SH8-A first-green source-proof summary.
- `evidence\sh8b-tx-extract-stop-summary.txt` — superseded compact SH8-B stop summary.
- `evidence\sh8b-afxdp-tx-green-summary.txt` — compact SH8-B first-green transaction-derived AF_XDP summary.
- `evidence\sh8c-final-afxdp-soak-summary.txt` — compact SH8-C final longer-soak closure summary.

Do not:

- Do not relabel Geyser as private-grade.
- Do not start `r1_tiny`.
- Do not enable live send.
- Do not use native XDP on `eno1` without explicit risk acceptance and rollback.
