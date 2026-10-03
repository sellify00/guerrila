# SH0 Quick QC

Scope: source-grade plumbing only. This package is meant to prove that external/local Geyser stay non-private while real AF_XDP remains the only private-grade packet ingress.

## Check in this order

1. `crates__shadow-ipc__src__ingress_record.rs`
   - `SIGNAL_SOURCE_EXTERNAL_GEYSER = 5`
   - `SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER = 6`
   - `SIGNAL_SOURCE_AFXDP` remains reserved for real AF_XDP

2. `crates__shadow-core__src__config.rs`
   - `IngressConfig.grpc_source_grade`
   - `GrpcSourceGrade::{ExternalGeyser, LocalValidatorGeyser}`
   - default is `external_geyser`

3. `crates__shadow-tier0__src__main.rs`
   - gRPC worker startup resolves `grpc_source_grade`
   - `spawn_grpc_ingest_workers(...)` takes a resolved `grpc_signal_source`
   - no hardcoded `SIGNAL_SOURCE_PUBLIC_GEYSER` in the gRPC ingest path

4. `crates__shadow-tier0__src__money_worker.rs`
   - external/local Geyser map to `SignalSource::PublicGeyser`
   - external/local counters are separate from `worker_signal_public_geyser`
   - rollout base events carry `record.signal_source`
   - acceptance tests prove:
     - external Geyser -> observe-only split, no private-grade
     - local validator Geyser -> observe-only split, no private-grade
     - AF_XDP -> private-grade split

5. `crates__shadow-ipc__src__rollout_shadow.rs`
   - `RolloutShadowEvent` carries `signal_source`
   - struct size stays stable via reduced telemetry padding

6. `crates__shadow-core__src__pipeline.rs` and `crates__shadow-core__src__assembly.rs`
   - raw `signal_source` propagates through `WorkerIntent` and `ExecutionAssemblyInput`
   - no-send shadow events preserve the real source byte

7. `crates__shadow-core__src__pm__rollout_ledger.rs`
   - split counters exist:
     - `observe_candidates_external_geyser`
     - `observe_candidates_local_validator_geyser`
     - `observe_candidates_private_grade`
     - `private_grade_admissible_opportunities`
     - `geyser_shadow_opportunities`

8. `crates__shadow-tier0__src__rollout_gate_export.rs` and `crates__shadow-control__src__rollout_gate.rs`
   - G12 input/report export the source split
   - private-grade evidence is distinct from Geyser evidence

9. `config.guerrila.yaml`, `g1-host-pack__config.guerrila.yaml`, `g12-qc__config.guerrila.yaml`
   - all default `grpc_source_grade: external_geyser`

## Pass condition

- external/local Geyser never increase private-grade counters
- AF_XDP is the only source here that increases private-grade observe/admissible counters
- G12 gate artifacts can truthfully show the split without changing live-send policy
