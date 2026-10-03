# SH7-A no-send scrutiny pack

Purpose: focused QC pack for the current SH7-A host final no-send split.

Current finding:

```text
candidate substrate: green
final no-send proof: green for SH7-A observe-only/no-send
route seam: gated external/local Geyser shadow no-send admission
live safety: live_landed=0, private-grade counters=0
```

Latest host diagnostic artifact:

```text
/home/ubuntu/shadow-host-audit/sh7a-no-send/20260428_210813-geyser-shadow-nosend-stale-ok
```

Important counters:

```text
worker_pool_state_hit=4
observe_candidates=12
worker_shadow_events_published=20
worker_geyser_shadow_no_send_admitted=4
worker_candidates_entered_process_intent=4
worker_process_intent_ok=4
worker_no_send_backend_called=4
worker_final_no_send_event_published=4
worker_rollout_would_send_events_published=4
worker_rollout_live_send_suppressed_events_published=4
rollout_would_send=4
rollout_live_send_suppressed=4
observe_candidates_private_grade=0
rollout_private_grade_admissible_opportunities=0
live_landed=0
promote_allowed=false
```

Files in this pack:

```text
source/crates_shadow-tier0_src_money_worker.rs
source/crates_shadow-tier0_src_no_send_sink.rs
source/crates_shadow-tier0_src_rollout_gate_export.rs
source/crates_shadow-control_src_rollout_gate.rs
source/crates_shadow-core_src_config.rs
source/sh7a-geyser-shadow-nosend.patch
evidence/sh7a-host-final-nosend-diagnostics-summary.txt
evidence/sh7a-terminal-path-grep.txt
evidence/sh7a-host-geyser-shadow-nosend-green-summary.txt
evidence/sh7a-geyser-shadow-nosend-terminal-grep.txt
```

QC focus:

```text
1. Confirm external/local Geyser are not relabeled private-grade globally.
2. Confirm the shadow admission is gated by observe-only, no-send, sink availability, public-lane amputation, and r0_allow_geyser_observe_shadow.
3. Confirm the shadow route cannot live send and does not increment private-grade opportunity counters.
4. Confirm stale bootstrap tolerance is scoped to the admitted Geyser no-send shadow route.
```
