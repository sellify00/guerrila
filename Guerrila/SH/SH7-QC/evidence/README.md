# SH7 Evidence

Drop future SH7-A host R0 rerun bundles here.

Latest current-host summary:

```text
sh7a-current-host-summary.txt
sh7a-controlled-software-summary.txt
sh7a-host-candidate-substrate-summary.txt
../scrutiny/evidence/sh7a-host-final-nosend-diagnostics-summary.txt
../scrutiny/evidence/sh7a-terminal-path-grep.txt
../scrutiny/evidence/sh7a-host-geyser-shadow-nosend-green-summary.txt
../scrutiny/evidence/sh7a-geyser-shadow-nosend-terminal-grep.txt
sh7b-micro-market-proof-summary.txt
sh7b-micro-market-g12_gate_input.json
sh7b-micro-market-g12_gate_report.json
sh7c-shadowguard-ops-blocked-summary.txt
sh7c-prepared-funded-deploy-blocked-preflight-summary.txt
sh7c-prepared-funded-deploy-blocked-g12_gate_input.json
sh7c-prepared-funded-deploy-blocked-g12_gate_report.json
sh7c-shadowguard-green-summary.txt
sh7c-shadowguard-green-deploy.stdout
sh7c-shadowguard-green-program-show.txt
sh7c-shadowguard-green-smoke-simulation.txt
sh7c-shadowguard-green-preflight-summary.txt
sh7c-shadowguard-green-g12_gate_input.json
sh7c-shadowguard-green-g12_gate_report.json
g12-r0-long-accumulation-summary.txt
g12-r0-long-accumulation-run-status.txt
g12-r0-long-accumulation-final-summary.txt
g12-r0-long-accumulation-progress.log
g12-r0-long-accumulation-g12_gate_input.json
g12-r0-long-accumulation-g12_gate_report.json
```

Status: host candidate-substrate recovery now publishes nonzero external G12 pool/mint/vault state,
and the latest `.55` host Tier0 run proves the observe-only/no-send shadow route end-to-end:
Geyser candidates enter `process_intent`, reach the no-send backend, publish final no-send events,
and PM records `rollout_would_send > 0` plus `rollout_live_send_suppressed > 0` while
`live_landed = 0` and private-grade counters remain zero.

SH7-B micro-market proof is also closed from the same host artifact: `micro_market_observations=4`,
`micro_market_uplift_ok=true`, and `micro_market_delta_lamports=0` in both gate input and gate report.
Promotion remains false because ShadowGuard ops proof is still missing.

SH7-C ShadowGuard ops proof is now green on `.57`: program
`NDQKwYGZ7LiUaqK8FxVrxa9uCcEL4b4jHcp8mY4baLW` is deployed on mainnet-beta, template proof is green,
and the no-send simulation smoke proves pass plus abort behavior on the configured Raydium V4 vault
layout. The exported G12 report records `shadow_guard_mainnet_deployed=true`,
`shadow_guard_template_program_id_matches=true`, `live_landed=0`, and `promote_allowed=false`.
The follow-up G12-R0 long observe-only accumulation reached volume thresholds with
`sample_slots=18401`, `observe_candidates=10335`, `rollout_would_send=3743`,
`rollout_live_send_suppressed=3743`, `micro_market_observations=3743`, and `live_landed=0`.
This is a green long safety soak, but full G12 promotion remains false because the intentionally
Geyser-only run did not provide private-grade admissibility or worker latency proof:
`private_lane_admissibility_ok=false`, `observe_candidates_private_grade=0`,
`rollout_private_grade_admissible_opportunities=0`, and `worker_latency_proof_present=false`.

Expected files should include the current `g12_gate_input.json`, `g12_gate_report.json`, host run
summary, and any grep/extract output showing the SH7-A counters:

```text
worker_rollout_would_send_events_published
worker_rollout_live_send_suppressed_events_published
worker_candidates_entered_process_intent
worker_candidates_dropped_before_process_intent
worker_route_resolved_drop
worker_route_reason_public_target
rollout_events_observed
rollout_would_send
rollout_live_send_suppressed
rollout_last_no_send_reason
rollout_last_drop_reason
```

