# G1 Closure Bundle — g1-host-a (2026-04-17)

**Host:** g1-host-a (historical G1 host)
**Sealed by:** Guerrila engineering session
**Date:** 2026-04-17

---

## Supervisor Call

G1: DONE
G4: DONE
G5: DONE

---

## G4 Real-Host Ingress Seal (4/4 pass)

All results archived in `g4_seal_results/`:

| Scenario | Frames | Parse | Data | Coding | FEC Recovered | Archive OK | Archive Drop | Verdict |
|---|---|---|---|---|---|---|---|---|
| baseline | 128 | 100% | 64 | 64 | 64 | 128 | 0 | ALL PASS (7/7) |
| reorder | 128 | 100% | 64 | 64 | — | 128 | 0 | ALL PASS (7/7) |
| fec | 124 | 100% | 60 | 64 | 48 | 124 | 0 | ALL PASS (8/8) |
| archive_pressure | 6022 | 100% | 3014 | 3008 | — | 32 | 5990 | ALL PASS (10/10) |

Archive pressure: 401 PPS sustained, ring cap=32, survival flushes prove non-blocking ingress.

### Host Setup
- veth pair: `seal_rx` (main ns, 10.99.2.1) <-> `seal_tx` (`seal_ns`, 10.99.2.2)
- XDP program: `shadow_rx_meta.bpf.c` loaded on seal_rx (prog id 565)
- XSKMAP pinned at `/sys/fs/bpf/shadow_xsks_map`
- Copy mode (veth does not support zero-copy)

### Key Fixes During G4 Seal
1. XDP_UMEM_COMPLETION_RING required by kernel 6.8 even for RX-only
2. XSKMAP + bpf_redirect_map required for zero-copy bind path
3. bpf_xdp_adjust_meta vs adjust_head metadata placement
4. ETH+IP+UDP header stripping for AF_XDP full Ethernet frames
5. observe_rx_timestamp moved to fire for all frames
6. Survival flush as recycle evidence (DelayFifo 1M depth never primes in short runs)
7. Replay sender off-by-one fix

---

## G5 Host Seals (2/2 pass)

### Strace Smoke (PASS)
- `strace -f -e trace=clock_gettime,gettimeofday,time` on money-worker threads
- Result: **zero hits** for banned runtime network/clock syscalls
- Structural regression guard already in test suite (g4_g1f_t5 + g4_g2_4_t1)

### Dequeue-to-Egress Microbench (PASS)
- p99 = **8,979 ns (9.0 us)** against 150 us budget
- Well within the Guerrila target envelope

---

## Commit SHAs (G4 seal session on g1-host-a)

```
f7f546c  temp debug tracing to poll_and_drain
590bca7  fix debug dump slice panic in metadata read
c4fce1e  revert debug tracing (clean tree)
5036ad0  fix replay sender off-by-one (last packet skipped)
0591f2f  strip ETH+IP+UDP headers before shred parse + move rx_timestamp
508bbe1  count survival flushes as recycle evidence in pressure verdict
dcd680a  archive G4 seal results (server commit)
```

---

## Test Counts at Closure

| Crate | Tests | Status |
|---|---|---|
| shadow-core | 385 | PASS |
| shadow-ipc | 245 (7 ignored) | PASS |
| shadow-ingest | 152 | PASS |
| shadow-state | 152 | PASS |
| shadow-tier0 | 575 | PASS |
| shadow-harmonic | 177 | PASS |
| shadow-egress-jito | 104 (1 ignored) | PASS |
| shadow-guard | 13 | PASS |
| **Total** | **1,803** | **0 failures** |

---

## Server Artifacts (on g1-host-a)

- Binaries: `~/Guerrila/target/release-guerrila/`
- G4 seal results: `~/Guerrila/g1-host-pack/g4_seal_results/` (synced to local repo)
- G5 strace log: `~/Guerrila/g1-host-pack/results/11_g5_strace_smoke/`
- G5 microbench: `~/Guerrila/g1-host-pack/results/12_g5_dequeue_egress/`
- XDP program: `/sys/fs/bpf/shadow_xsks_map`
- veth pair: seal_rx / seal_tx (still active)

**Note:** G5 seal results remain on server only. Pull before decommissioning:
```bash
ssh root@<historical-g1-host>
tar czf /tmp/g5_seals.tar.gz ~/Guerrila/g1-host-pack/results/11_g5_strace_smoke/ ~/Guerrila/g1-host-pack/results/12_g5_dequeue_egress/
```
