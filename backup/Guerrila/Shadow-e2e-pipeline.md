Below is the clean end-to-end mental model of Shadow as it exists conceptually.

The **canonical runtime funnel** is:

```text
wire
-> decode
-> worker record
-> pool tag map
-> route
-> quote
-> policy
-> build/sign
-> egress
-> reconcile
```

That is also the project’s own green-stage rule: a crate/component is not “working” unless the downstream stage it feeds is moving in the same no-restart window. 

## 1. `shadow-ingest`

Role:

```text
raw packet / shred / transaction ingestion
```

It owns the low-level ingest logic:

```text
AF_XDP packets
-> shred parsing
-> reassembly
-> FEC recovery where possible
-> tx extraction
-> initial transaction normalization
-> IngressRecord emission
```

In our canonical pipeline, this is where owned AF_XDP/local packet visibility becomes usable engine input.

Important distinction:

```text
shadow-ingest does not decide trades.
It creates normalized candidate/event records.
```

The checklist describes this as frozen `IngressRecord` / `RawArchiveRecord` contracts, feed rings, raw archive paths, canonical signal-source constants, shared decode pipeline, canonical shred parser, AF_XDP live path, reassembler, FEC recovery, and reinjection into normal decode flow. 

## 2. `shadow-ipc`

Role:

```text
shared memory contracts / rings / fixed binary layouts
```

This is the nervous system between processes.

It defines things like:

```text
feed rings
egress rings
NACK rings
health segments
pool/mint/ALT arenas
blockhash segment
leader schedule segment
risk flags
control flags
raw archive
quote sidecars
TPU/Jito/Harmonic ring contracts
```

Think of `shadow-ipc` as the **language** all daemons speak through `/dev/shm`.

It must stay fixed-layout and versioned because the hot path cannot tolerate dynamic schemas or ambiguous data structures. The project docs list `shadow-ipc` as owning versioned `/dev/shm` headers, SPSC rings, 8-byte tag maps, RCU-style double buffering, leader schedule, NACK ring, egress health, pool/mint state, risk flags, control flags, and UMEM/raw archive surfaces. 

## 3. `shadow-state`

Role:

```text
state publisher / local market truth
```

It takes Agave/RPC/Geyser/account information and publishes worker-readable state:

```text
pool state
mint state
vault reserves
ALT cache
leader schedule
health snapshot
transfer-fee metadata
CLMM quote sidecar responses
hot-pool hydration
```

In the canonical setup:

```text
local Agave / local RPC / shadow-state
= state truth
```

`shadow-state` is not supposed to be the hot trading loop. It prepares the data that Tier0 reads.

The project docs describe `shadow-state` as publishing pools, mints, ALTs, health, and leader schedule tags; maintaining monotonic `snapshot_slot`; handling Token-2022 fee updates; dynamic pool/mint append; fork rollback; and exposing freshness metrics. 

## 4. `shadow-core`

Role:

```text
shared trading logic / policy / math / assembly / PM model primitives
```

This is the library that contains the “rules of the engine.”

It includes:

```text
routing types
quote math
policy decisions
economics checks
template assembly
inventory/fence logic
PM/reconcile primitives
rollout ledger logic
owner-mode scoring primitives
```

It is used by several binaries.

A useful mental split:

```text
shadow-core = pure logic
shadow-tier0 = live hot-path application of that logic
```

So if we implement quote math, route policy, template assembly, inventory reservation, or rollout ledger semantics, those usually live in `shadow-core`.

The checklist describes the money-worker core as using fast staleness gates, pool/mint lookup, thread-local overlay, forward projection quote logic, Token-2022 fee discount, route fork, LUT selection, fee clamp, inventory reservation/rollback, template build, signing, and egress enqueue. Many of those shared mechanisms are implemented through `shadow-core` and consumed by Tier0. 

## 5. `shadow-tier0`

Role:

```text
hot-path execution engine / money worker
```

This is the main low-latency runtime.

It consumes:

```text
IngressRecord from feed ring
pool/mint/ALT/blockhash/health from shadow-state via IPC
oracle LUTs / sidecars
inventory/fence state
egress health
```

Then it does:

```text
pool state lookup
route decision
quote request / quote read
policy and cap checks
template build
sign
PM reserve or no-send classification
egress enqueue
diagnostic export
```

Tier0 is the place where a market event becomes:

```text
drop
or no-send event
or egress envelope
```

The hot-path rule is strict:

```text
no network I/O in money worker
no RPC call in money worker
no blocking
no dynamic model inference
no worker-side route search
```

The checklist explicitly describes the worker loop as O(1), no alloc, no lock, no syscall; consuming fixed shared state; using thread-local dominance; building Jito/Harmonic envelopes; applying route matrix, freshness, inventory, template, fee, and egress gates. 

## 6. `shadow-oracle`

Role:

```text
off-path strategy / bidding / score publisher
```

Oracle is not the hot path. It prepares compact advisory state for Tier0.

It owns things like:

```text
leader-class micro-market LUTs
tip/bid banks
competition-adjusted banks
crowd heat
obscurity discounting
OPSO / ownership sidecar
mode scoring sidecars
```

Tier0 reads fixed-width outputs from Oracle. It should never ask Oracle to do dynamic work inside the money loop.

The docs describe `shadow-oracle` as publishing dual banked LUTs, competition-adjusted strategy banks with the same worker-visible footprint, crowd-heat feedback, leader-class identity, and sidecar surfaces for OPSO/Rebalancing observe-only scoring. 

## 7. `shadow-egress-*`

Role:

```text
out-of-worker send proxies
```

Existing/current egress crates include:

```text
shadow-egress-jito
shadow-harmonic
shadow-egress-tpu
```

Their job is:

```text
read signed envelopes from egress ring
send or no-send depending mode
publish result / NACK / status
update egress health
feed PM/reconcile path
```

The important architecture rule:

```text
Tier0 builds/signs/enqueues.
Egress proxy sends/dequeues/status-polls.
Tier0 does not open sockets.
```

For Jito, the project docs describe a dedicated proxy with Searcher API sessions, egress rings, dequeue-to-bundle-send loop, NACK emission, and coarse egress health publication. 

For Harmonic, same pattern: regional gRPC proxy, fanout on proxy cores, Death Clamp, NACK emission, and health publication. 

For TPU/QUIC, we have just started this same shape: `shadow-egress-tpu` is the new public submit proxy. It should eventually own:

```text
leader/contact cache
TPU/QUIC client
TPU ring dequeue
wire send
status polling
PM release/reconcile events
```

But current state is dry-run/no-send scaffold, not tiny-live.

## 8. `shadow-pm`

Role:

```text
position manager / inventory truth / reconcile authority
```

PM is the accountant and truth layer.

It owns:

```text
pending table
reservation truth
NACK consumption
bundle status
signature status
release
reconcile
inventory delta
harmonic/Jito terminal handling
phantom inventory prevention
```

PM answers:

```text
Did we reserve?
Did we send?
Did it land?
Did it fail?
Did we release capital?
Did inventory match chain truth?
Did we leak fees?
```

The docs describe `shadow-pm` as owning inventory truth, pending table keyed by signature, NACK polling, Jito/Harmonic terminal paths, delta reconciliation, release on failure/expiry/revert/landed success, and risk flag publication. 

## 9. `shadow-control`

Role:

```text
control plane / proof reports / Rosetta / diagnostics / gate reports
```

Control is not the trading engine. It is how we prove what happened.

It owns or helps with:

```text
g12_gate_input.json
g12_gate_report.json
rollout gate schemas
Rosetta Stone auto-heal
manifest/control flags
proof classifications
run reports
golden records / parser repair
```

In practice, when we ask:

```text
why did this run fail?
what counter moved?
what is the current blocker?
```

we are often reading `shadow-control` outputs.

The docs describe G11 `shadow-control` as implementing shared-memory control-plane surfaces, Flight Data Recorder guard, Tripwire downgrade, predictive shift, deep scan, executable-hash quarantine, Rosetta sink wiring, and end-to-end degrade/heal/resume proof. 

## 10. End-to-end Shadow flow

Canonical runtime:

```text
1. Local Agave receives Solana network traffic
   - maintains ledger/accounts/state
   - local RPC gives blockhash, account reads, leader/contact/status truth

2. AF_XDP captures live UDP/shred traffic
   - packet-level owned ingest
   - earlier than external gRPC

3. shadow-ingest parses/reassembles/decodes
   - packet/shred -> tx/event -> IngressRecord

4. shadow-ipc feed ring carries records
   - producer writes
   - Tier0 workers consume

5. shadow-state publishes state
   - pool/mint/ALT/health/blockhash/leader/quote sidecars

6. shadow-tier0 consumes event + state
   - pool tag lookup
   - route decision
   - quote
   - policy
   - build/sign
   - PM/no-send decision
   - egress enqueue

7. shadow-oracle provides advisory LUTs/sidecars
   - leader class
   - bidding/tip banks
   - ownership/competition/crowd sidecars

8. egress proxy drains chosen route
   - Jito / Harmonic / TPU / explicit public
   - no-send or live depending mode
   - sends or records would-send
   - emits result/NACK/status

9. shadow-pm reconciles
   - reservation
   - release
   - landed/fail/timeout
   - inventory truth
   - fee truth

10. shadow-control reports
   - gate input/report
   - blocker classification
   - run artifacts
   - safety proof
```

## One-line mental model

```text
shadow-ingest sees the market.
shadow-state knows the state.
shadow-oracle prices/scales advice.
shadow-tier0 decides/builds/signs.
shadow-egress-* sends or no-sends.
shadow-pm reconciles truth.
shadow-control proves what happened.
shadow-ipc connects all of them.
shadow-core is the shared rules/math/policy library.
```

## Where we currently are

We are no longer just proving raw ingestion. The runtime funnel has moved into:

```text
multi-venue candidate selection
venue-correct execution templates
TPU/QUIC no-send enqueue/dequeue proof
owner-mode scoring beyond CLMM/backrun
```

The checklist explicitly says the active rule is to broaden after the funnel moves: Run39 broad-surface discovery exists, current discovery recognizes Raydium V4, Raydium CPMM, Raydium CLMM, Pump.fun AMM/PumpSwap, and Meteora DAMM v2, while venue-correct execution templates for non-CLMM are still open. 
