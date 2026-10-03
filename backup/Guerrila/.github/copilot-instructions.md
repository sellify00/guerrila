# Guerrila canonical pipeline memory

Use this pipeline shape as the default standard for project work and proof interpretation:

- **Ingest:** AF_XDP.
  - Zero-copy is preferred.
  - Generic XDP is acceptable when zero-copy is not safe or not available.
- **Source:** local Agave validator.
  - Treat the local validator as the canonical source only when it is healthy and fully bootstrapped.
  - Prefer evidence from AF_XDP ingest driven by the local validator over gRPC-only or other fallback sources.
- **Target surface:** multiple venues.
  - Do not treat CLMM-only coverage as the canonical end state.
  - Prefer proofs and regressions that preserve a multi-venue target surface.
- **Egress path:** explicit TPU/QUIC or explicit public.
  - Do not treat hidden fallback paths as canonical proof.

Interpretation rules:

- If a run does not satisfy the canonical source + ingest + target + egress shape above, classify it as **partial** or **infrastructure** evidence rather than canonical main-pipeline proof.
- Local Agave health is necessary but not sufficient; the ingest evidence must still show AF_XDP/local-validator truth, not merely local RPC readiness.

Operational rule for AF_XDP zero-copy:

- Zero-copy AF_XDP can make SSH temporarily unreachable.
- Before zero-copy work on the host, have OOB ready.
- Use the host/OOB recovery and operating documentation under `g1-host-pack` as the standard reference.
- Do not treat transient SSH loss during zero-copy transition as automatic proof failure.

Packet ownership rule for same-interface AF_XDP:

- Treat same-`eno1` native AF_XDP zero-copy as a **packet-ownership blocker** for steady-state operation when local Agave is expected to remain the canonical source.
- CPU pinning helps contention, but it does **not** solve packet-path ownership if XDP/AF_XDP redirects shred traffic away from the kernel socket path Agave needs.
- Same-`eno1` native AF_XDP zero-copy is currently **bounded proof-window only**:
  - require OOB ready
  - require local Agave healthy before attach
  - require bounded attach window
  - require detach on exit
  - require local Agave health checks before and after
- Treat same-interface zero-copy proof runs as **partial infrastructure evidence**, not steady production architecture.

Required production architecture rule:

- For durable canonical operation, **Agave must own the validator NIC path** and **Shadow must own a separate or mirrored observe path**.
- Acceptable long-term solutions include:
  - second NIC or mirrored/SPAN/TAP feed
  - provider traffic mirror
  - second host as packet observer
- Do **not** assume queue pinning alone solves this when Agave and Shadow need the same shred packets.
