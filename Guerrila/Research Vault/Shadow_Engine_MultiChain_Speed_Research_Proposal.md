# Shadow Engine Multi-Chain Speed Research Proposal

**Date:** 2026-05-13  
**Status:** Research backlog / future expansion proposal  
**Current production priority remains:** Solana runtime conversion and tiny-live proof.

---

## 0. Purpose

This document tables the non-Solana networks that may be interesting for Shadow Engine after the Solana runtime funnel is stabilized.

This is **not** a proposal to pivot away from Solana now. It is a structured research queue for later: which networks might let a small, fast, low-OpEx machine exploit neglected surfaces before large professional MEV players care enough to dominate them.

The thesis is:

```text
Large chains = large gross EV, high competition, expensive access.
Small / emerging / neglected chains = lower gross EV, lower competition, possible higher attainable net EV.
```

Shadow does not need the largest market first. It needs a market where:

```text
speed advantage + neglected coverage + low fees + enough flow + weak competition
```

combine into repeatable landed profit.

---

## 1. Strategic frame

### 1.1 What we are trying to export

The exportable part of Shadow Engine is not “Solana code.” It is:

```text
fast first-seen observation
-> deterministic fixed-shape candidate classification
-> local state mirror
-> exact quote / simulator
-> no-send markout
-> HUNT / WATCH / TRAINING / DENY labels
-> tiny-live proof
-> PM / inventory / fee truth
-> latency-ablation economics
```

The chain-specific part must be rebuilt for every network.

### 1.2 What must not change

The following rules remain non-negotiable:

```text
No blind fire.
No timing-only send.
No manual pair-picking as production.
No source-grade relabeling.
No live promotion without clean net-after-fee evidence.
No bypass of inventory / PM / reconcile truth.
No double-reserving the same event across multiple owners.
```

### 1.3 Why smaller chains can matter

Large players prefer markets where:

```text
liquidity is deep
blockspace access is formalized
gross EV justifies infra spend
orderflow access is available
```

Shadow’s niche may be the opposite:

```text
small awkward trades
thin pools
new venues
migration / launchpad / memecoin surfaces
weak route coverage
under-indexed DEXs
slow public tooling
low competition
```

This is why Monad, Sei, Linea, Sonic, Berachain, and other smaller or newer networks should be researched even if current DEX volume is modest.

---

## 2. Evaluation model

Every chain should be scored on **attainable net EV**, not raw TVL or hype.

### 2.1 Net EV formula

For a chain / surface:

```text
Expected daily net EV =
  decoded_candidates_per_day
  × quote_ready_rate
  × positive_after_fee_rate
  × route_legal_rate
  × landing_rate
  × avg_net_profit_per_landed_tx
  - infra_cost
  - failed_tx_cost
  - inventory_cost
  - operational_complexity_cost
```

### 2.2 What matters more than TVL

A chain with low TVL can still be useful if it has:

```text
frequent small mispricings
low fees
low bot density
weak public indexers
fragmented liquidity
poor route coverage
fast finality / short block cycle
accessible transaction ordering
```

### 2.3 Required research fields

For each chain:

```text
chain
execution model
ordering model
block time / finality
mempool / pending tx visibility
private orderflow availability
bundle / builder / sequencer access
fees
DEX volume
active DEX families
retail / memecoin / launchpad activity
CEX-listed token depth
top route families
public RPC latency
node-running feasibility
simulator feasibility
existing bot density
hypothesized edge
first proof window
kill criteria
```

---

## 3. Chain shortlist table

| Priority | Network | Type | Why interesting | Net EV thesis | Build burden | Risk |
|---:|---|---|---|---|---|---|
| 0 | **Solana** | SVM L1 | Current engine target; lowest port burden; speed edge maps directly to shreds / accounts / DEX state | Many small txs, low fee drag, packet-level first-seen edge | Already underway | Current runtime funnel still needs proof |
| 1 | **Monad** | High-speed EVM L1 | New EVM chain, high throughput, fast blocks/finality, likely immature searcher landscape | Early land-grab before competition matures; neglected DEX/route coverage | Medium-high | Ecosystem / liquidity still proving itself |
| 1 | **Sei** | Parallel EVM L1 | Trading-oriented, low latency, parallelized EVM, potentially weaker competition | Speed-native small-trade engine on lower-liquidity chain | Medium-high | Smaller DEX volume than Base/Solana |
| 1 | **Linea** | zkEVM L2 | Smaller, less attractive to large players, protected sequencer, possible neglected DEX surface | Small but low-competition route/DEX edges | Medium | Sequencer ordering may limit speed edge |
| 2 | **Base** | OP-stack EVM L2 | Huge retail flow and DEX volume; best practical EVM benchmark | High candidate flow, many retail/memecoin opportunities | Medium | More crowded; sequencer-centric |
| 2 | **BNB Chain** | EVM L1 | Large retail flow, PancakeSwap-style surfaces, high stablecoin/DEX activity | Many small/medium retail-driven opportunities | Medium | Crowded; builder/MEV rules matter |
| 2 | **Sonic** | Fast EVM L1 | Fast EVM L1, new DeFi incentives, possible weaker bots | Speed + incentives + emerging DEXs | Medium | Hype/spec claims must be validated with flow |
| 2 | **Berachain** | EVM L1 / PoL | Proof-of-Liquidity creates unusual incentive and routing surfaces | Incentive-driven liquidity/rebalancing opportunities | Medium | Native ecosystem mechanics are special |
| 3 | **Arbitrum** | EVM L2 | Large ecosystem and liquidity | Benchmark for EVM L2 mechanics | Medium | Competitive and sequencer-shaped |
| 3 | **OP Mainnet** | EVM L2 | Useful OP-stack reference | Study ordering, stake-priority, OP family mechanics | Medium | Base looks better economically |
| 3 | **Polygon PoS** | EVM sidechain | Older broad DEX surface, lower fees | Small neglected route coverage | Medium | Mature bots; declining mindshare in places |
| 3 | **Avalanche C-Chain** | EVM L1 | Established EVM DeFi, CEX connectivity | CEX/DEX and DEX/DEX route pockets | Medium | Competition likely already present |
| 3 | **Mantle** | EVM L2 | Smaller but meaningful EVM liquidity | Neglected DEX / incentive-driven route opportunities | Medium | Need live flow validation |
| 3 | **Scroll** | zkEVM L2 | Smaller zkEVM, potentially under-covered | Low-competition DEX pockets | Medium | Volume may be thin |
| 3 | **zkSync Era** | zkEVM-ish L2 | Existing ecosystem, possible under-covered routes | Small route/bridge/liquidity edges | Medium | Ecosystem maturity mixed |
| 3 | **Blast** | EVM L2 | Incentive/yield-driven chain mechanics | Weird liquidity incentives may create rebalancing edges | Medium | Hype cycles; flow may decay |
| 4 | **Sui** | Move object-model L1 | High-throughput, object-centric, parallel-friendly architecture | New non-EVM speed experiment | High | Major architecture port; Move/object state |
| 4 | **Aptos** | Move L1 | Parallel Block-STM execution; institutional-speed story | High-performance non-EVM experiment | High | Major architecture port |
| 4 | **Tron** | TVM / stablecoin network | Massive stablecoin/payment surface | Resource/energy/stablecoin routing, not DEX-speed core | High | Different product, not Shadow’s current bot |
| 5 | **Bitcoin** | UTXO L1 | Large money, but no AMM/CLMM DeFi surface | Fee/RBF/CPFP/miner-template product, not this engine | Very high | Wrong product shape |

---

## 4. Priority network dossiers

## 4.1 Monad

### Why it is interesting

Monad is a high-speed EVM-compatible L1. Official docs describe Monad as EVM-compatible with 10,000 TPS, 400ms block frequency, and 800ms finality.

That is close enough to Solana-like speed economics that Shadow should research it seriously.

### Net EV thesis

```text
New chain
+ fast block cycle
+ EVM compatibility
+ likely immature searcher infrastructure
+ under-covered DEX routes
= possible early speed alpha
```

The key is not the headline TPS. The key is whether liquidity, retail flow, and DEX fragmentation appear before professional MEV infrastructure fully matures.

### What to measure

```text
24h / 7d DEX volume
top DEXs
pool count
long-tail pool count
pool churn
retail/memecoin launch activity
private tx / bundle access
public RPC latency
node-running feasibility
CEX listing depth of major assets
slippage distribution for 0.01 / 0.05 / 0.1 / 0.5 / 1.0 native-token trades
```

### First experiment

```text
Monad-P0-Observer:
  ingest pending/confirmed txs
  mirror top DEX pools
  simulate swaps after every observed tx
  generate HUNT/WATCH/DENY scorecard
  no send
```

### Good signs

```text
>10k useful DEX events/day
>500 quote-ready candidates/day
positive after-fee micro edges after realistic latency
repeated pool+direction patterns
low failed-simulation rate
```

### Kill signs

```text
DEX liquidity too thin
all profitable paths require private access we cannot get
fees/failed tx cost eat micro edges
no CEX/DEX or DEX/DEX markout survives delay
```

---

## 4.2 Sei

### Why it is interesting

Sei is built around high-performance trading and parallel EVM execution. Official docs describe Sei EVM as having 400ms block time, 100 MGas/s throughput, parallelized execution, and full EVM tooling compatibility.

### Net EV thesis

```text
Trading-first chain
+ parallel EVM
+ lower liquidity than Base
+ possibly lower bot density
= small ugly trades may be more attainable
```

### What to measure

```text
DEX event count
orderbook / AMM mix
top route families
retail / memecoin activity
public vs private tx paths
first-seen latency vs public indexers
failed tx fee profile
DEX liquidity fragmentation
```

### First experiment

```text
Sei-P0-Observer:
  track DEX swaps across top venues
  compute same-pool continuation markouts
  compute cross-pool route-lag markouts
  compare against 0us / 5us / 25us / 100us / 1ms artificial delays
```

### Good signs

```text
small but repeated positive after-fee edges
thin pools with weak professional bot coverage
low execution cost
fast public submission path
```

### Kill signs

```text
sequencing/order access neutralizes speed edge
too little DEX flow
positive markouts exist only at unrealistic 0us
```

---

## 4.3 Linea

### Why it is interesting

Linea is not attractive because it is huge. It is attractive because it may be small enough for major MEV shops to ignore.

Linea’s architecture is sequencer-centered: the sequencer orders transactions and builds blocks according to Linea rules. That weakens raw packet-speed edge, but smaller chain size may open neglected-DEX opportunities.

### Net EV thesis

```text
Small zkEVM
+ lower attention from large searchers
+ retail/airdrop/incentive-driven periods
+ under-covered DEXs
= possible low-competition micro portfolio
```

### What to measure

```text
top DEX volume
pool count
recurring pool+direction edges
bridge inflows/outflows
token launches
sequencer submission latency
public RPC propagation lag
private submission availability
```

### First experiment

```text
Linea-P0-Neglected-DEX:
  mirror top 20 pools
  track 7 days of swaps
  classify:
    same-pool continuation
    cross-pool lag
    stablecoin rebalancing
    bridge-flow reactions
```

### Good signs

```text
busy pockets despite low chain-wide volume
slow public routing tools
repeatable small positive marks
low failed tx cost
```

### Kill signs

```text
one sequencer path makes first-seen edge meaningless
no repeated positive net-after-fee samples
liquidity too thin even for 0.01-0.05 native-token trades
```

---

## 4.4 Base

### Why it is interesting

Base is the practical EVM benchmark: large retail flow, large DEX volume, active memecoin and long-tail trading, and enough depth for real execution.

### Net EV thesis

```text
large retail DEX flow
+ lower fees than Ethereum L1
+ many long-tail pools
= high candidate count
```

But competition will be stronger than on Linea/Sei/Monad.

### Role in research

Base should not be the “small ignored chain” thesis. It should be the **EVM benchmark chain**:

```text
Can our EVM adapter find anything on a busy chain?
Can our no-send engine classify candidates accurately?
Can our latency-ablation prove speed-exclusive opportunities?
```

### First experiment

```text
Base-P0-Benchmark:
  ingest pending + confirmed swaps
  mirror Uniswap V3 / Aerodrome / other top DEXs
  run no-send markout
  compare against Solana HUNT/WATCH/DENY workflow
```

---

## 4.5 BNB Chain

### Why it is interesting

BNB Chain has large retail flow and established DEX surfaces. It may be strong for many small and medium opportunities, but also more mature and more competed.

### Net EV thesis

```text
retail flow
+ large DEX surface
+ lower fees than Ethereum L1
= many possible attempts
```

### Main question

Can we access ordering/MEV routes well enough to compete, or are the good opportunities already captured by integrated builders/searchers?

### First experiment

```text
BNB-P0-Retail-Flow:
  monitor PancakeSwap-style DEXs
  classify memecoin/long-tail opportunities
  measure failed tx cost
  test small-trade size sweeps
```

---

## 4.6 Sonic

### Why it is interesting

Sonic is a fast EVM L1 with DeFi incentives and claimed very high throughput / sub-second finality.

### Net EV thesis

```text
fast EVM L1
+ new incentive-driven DeFi
+ less mature searcher market
= candidate for neglected-venue execution
```

### First experiment

```text
Sonic-P0-Flow-Reality:
  ignore headline TPS
  measure actual DEX event rate
  measure repeated positive markouts
  identify top pools by slippage-adjusted volume
```

### Kill criteria

```text
headline speed but insufficient real DEX flow
too few route families
all good venues already covered by aggregators/searchers
```

---

## 4.7 Berachain

### Why it is interesting

Berachain’s Proof-of-Liquidity model creates unusual liquidity and incentive mechanics. That may generate rebalancing, reward-routing, and incentive-driven flow that does not look like normal EVM DEX flow.

### Net EV thesis

```text
weird incentive design
+ native DEX/lending ecosystem
+ liquidity/reward churn
= possible non-obvious rebalancing opportunities
```

### First experiment

```text
Berachain-P0-Incentive-Flow:
  map reward vaults
  map BEX / lending / stable flows
  identify recurring liquidity rebalances
  classify as Housekeeping / Rebalancing / Continuation
```

---

## 4.8 Sui and Aptos

### Why they are interesting

Both are high-performance Move-based networks with parallel execution models. Sui has an object-centric model; Aptos uses Block-STM parallel execution.

### Why they are not first

They require a deeper port:

```text
new account/object model
new transaction format
new smart contract language
new DEX adapters
new simulator assumptions
different indexing and state mirror
```

### Research role

They are **Phase-2 non-EVM speed experiments**, not immediate ports.

### First experiment

```text
Move-P0-Feasibility:
  select either Sui or Aptos
  pick top 3 DEXs
  implement read-only pool-state mirror
  compute no-send markouts only
```

---

## 4.9 Tron

### Why it is interesting

Tron has enormous stablecoin activity and high active-address volume. But its core opportunity surface is not the same as Solana/EVM AMM MEV.

### Net EV thesis

```text
stablecoin/payment rail
+ resource/energy mechanics
+ exchange/deposit/withdrawal flows
= separate stablecoin/resource business
```

### Why it is not first

Tron is not a direct Shadow port. It likely needs a different product:

```text
energy/resource optimization
stablecoin route monitoring
bridge/exchange latency
payment-flow analytics
```

---

## 5. Research phases

## Phase 0 — Paper / data ranking

Goal: choose which chain deserves real engineering.

Deliverables:

```text
multi_chain_chain_scorecard.md
multi_chain_chain_scorecard.csv
top_dex_by_chain.csv
top_pool_by_chain.csv
mempool_access_matrix.md
submission_route_matrix.md
```

Metrics:

```text
DEX volume 24h / 7d
transactions 24h
active addresses 24h
stablecoin supply
perps volume
top DEXs
pool count
average fee
failed tx cost
public RPC latency
private tx availability
node-running feasibility
```

Pass condition:

```text
at least 2 chains show plausible:
  candidate density
  low fee drag
  route access
  under-covered surfaces
```

## Phase 1 — Read-only observer adapter

Goal: observe without sending.

For EVM-compatible chains:

```text
pending tx listener
confirmed swap listener
pool-state mirror
top DEX decoders
EVM simulator
no-send markout
latency-ablation replay
```

For non-EVM chains:

```text
chain-specific transaction decoder
state mirror
DEX-specific quote model
markout engine
```

Pass condition:

```text
>10k observed relevant events/day
>1k decoded candidates/day
>100 quote-ready candidates/day
some positive no-send after-fee samples
```

## Phase 2 — HUNT/WATCH/DENY portfolio

Goal: prove repeated patterns.

For each chain:

```text
HUNT:
  repeated positive no-send samples
  route legal
  low error noise
  size sweep survives

WATCH:
  promising but low sample count

TRAINING:
  useful for negative calibration

DENY:
  busy but worthless / toxic / unreachable
```

Pass condition:

```text
at least 3 HUNT candidates or 10 strong WATCH candidates
```

## Phase 3 — Blackhole / no-send execution proof

Goal: prove the full path without money.

```text
candidate -> quote -> policy -> build -> sign -> egress attempt -> expected blocked/blackhole outcome -> PM release
```

Pass condition:

```text
would_build > 0
would_sign > 0
would_send/egress_attempt > 0
PM reservation + release clean
fee accounting clean
no phantom inventory
```

## Phase 4 — Tiny live

Goal: one tiny real trade, then stop.

```text
one chain
one candidate class
strict cap
one tx
stop after landed or hard fail
reconcile
```

Pass condition:

```text
landed positive
or failed with full attribution and no inventory leak
```

---

## 6. Recommended research order

### Immediate Solana-first rule

Do not allocate real engineering to another chain until Solana reaches:

```text
broad census recovered
at least one HUNT candidate
blackhole/no-send path clean
one tiny live attempt reconciled
```

### After Solana tiny-live

Start with this order:

```text
1. Monad data ranking
2. Sei data ranking
3. Linea data ranking
4. Base EVM benchmark
5. Sonic / Berachain watchlist
6. BNB if retail-flow data justifies it
7. Move chain feasibility: Sui or Aptos
8. Tron only if choosing a separate stablecoin/resource product
```

### Why this order

```text
Monad / Sei:
  closest philosophical fit to speed-native chain

Linea:
  small enough to possibly be neglected

Base:
  best EVM benchmark and high flow

Sonic / Berachain:
  emerging and incentive-driven, possibly under-covered

BNB:
  high retail flow but more mature

Sui / Aptos:
  technically interesting but high port burden

Tron:
  different business
```

---

## 7. Chain-specific first questions

### Monad

```text
Is there enough DEX flow yet?
Are DEX routes under-covered?
Is transaction ordering accessible?
Can public RPC/private submission land fast enough?
Does no-send markout survive 25us / 100us / 1ms?
```

### Sei

```text
Does the trading-first architecture translate into exploitable flow?
Are AMM/DEX venues fragmented?
Can Shadow observe faster than normal indexers?
Does execution cost permit 100s of tiny attempts/day?
```

### Linea

```text
Are there neglected DEX pockets despite low volume?
Does sequencer ordering remove the speed edge?
Are bridge-flow or incentive-flow events tradable?
Can tiny trade EV overcome fees?
```

### Base

```text
Is our EVM adapter competitive in a busy environment?
Can we find long-tail opportunities not covered by big searchers?
Does high flow compensate for higher competition?
```

### Sonic

```text
Is actual DeFi flow real or mostly headline TPS?
Are incentive programs creating predictable liquidity motion?
Are DEXs under-indexed enough for small edges?
```

### Berachain

```text
Does Proof-of-Liquidity produce recurring rebalancing opportunities?
Can reward-vault flows be predicted or reacted to?
Are native DEX/lending events markout-positive?
```

### Sui / Aptos

```text
Is the non-EVM port justified by opportunity density?
Can object/Move state be mirrored cheaply?
Are DEX protocols mature enough to quote exactly?
```

### Tron

```text
Is there a stablecoin/resource optimization product worth building?
Can Energy/Bandwidth/resource mechanics create repeatable net EV?
Is this separate from Shadow DEX engine?
```

---

## 8. Data collection spec

For every candidate chain, collect at least 7 days:

```text
chain
timestamp
block_number / slot
tx_hash
source
dex
pool_id
token_in
token_out
amount_in
amount_out
pool_reserves_before
pool_reserves_after
fee_paid
gas_price / priority_fee
simulated_best_response
net_after_fee
markout_50ms
markout_100ms
markout_250ms
markout_500ms
markout_1_block
markout_2_blocks
route_family
candidate_owner_mode
```

Speed fields:

```text
first_seen_source
first_seen_ns
confirmed_seen_ns
sim_start_ns
sim_end_ns
decision_ns
egress_possible_ns
artificial_delay_us
net_after_fee_at_delay
```

---

## 9. Candidate owner modes by chain

The same owner vocabulary should be reused:

```text
DirectArb
Backrun
Continuation
Housekeeping
Rebalancing
Flatten
Drop
```

Expected best fit by chain:

| Network | Most likely owner modes |
|---|---|
| Monad | DirectArb, Continuation, Housekeeping, Rebalancing |
| Sei | Continuation, DirectArb, Rebalancing |
| Linea | Housekeeping, Rebalancing, small DirectArb |
| Base | DirectArb, Backrun, Continuation |
| BNB | Backrun, DirectArb, small retail-flow Continuation |
| Sonic | Rebalancing, DirectArb, Continuation |
| Berachain | Rebalancing, Housekeeping, incentive-flow Continuation |
| Sui | DirectArb, Rebalancing, object-state continuation |
| Aptos | DirectArb, Continuation, Rebalancing |
| Tron | Resource/Stablecoin optimization, not standard owner mode |

---

## 10. First engineering artifacts for future work

When the time comes, create:

```text
crates/shadow-evm-ingest
crates/shadow-evm-state
crates/shadow-evm-sim
crates/shadow-evm-route
crates/shadow-evm-egress
run/multichain_chain_scorecard.js
run/evm_no_send_markout_report.js
run/evm_latency_ablation_report.js
run/multichain_hunt_watch_report.js
```

For non-EVM:

```text
crates/shadow-move-ingest
crates/shadow-move-state
crates/shadow-tron-resource
```

But only after Solana proves the runtime conversion loop.

---

## 11. Go / no-go criteria

### Research go

A chain enters Phase 1 if:

```text
DEX flow or unique resource flow exists
fees allow small attempts
state/simulation is feasible
submission path is known
competition appears lower than gross EV would suggest
```

### Engineering go

A chain enters Phase 2 if:

```text
observer finds repeated positive no-send samples
candidate density is high enough
latency-ablation shows speed-exclusive windows
route legality is solvable
```

### Tiny-live go

A chain enters Phase 4 if:

```text
HUNT candidate exists
blackhole/no-send proof passes
PM/inventory/reconcile model exists
one-tx kill switch exists
failure cost is bounded
```

### Kill

Kill or pause chain if:

```text
no positive after-fee samples after 7 days
no speed-exclusive edge
access/orderflow unavailable
too much engineering for too little flow
execution failures dominate
fees eat all small trades
```

---

## 12. Practical conclusion

The best future multi-chain candidates are not necessarily the largest chains.

Priority thesis:

```text
Monad:
  early high-upside speed-native EVM

Sei:
  speed/trading-native EVM with lower competition

Linea:
  small zkEVM, possibly neglected by large players

Base:
  high-flow EVM benchmark, crowded but valuable

Sonic / Berachain:
  emerging incentive-driven watchlist

Sui / Aptos:
  high-performance non-EVM experiments after EVM adapter exists

Tron:
  separate stablecoin/resource product, not immediate Shadow port
```

The immediate rule remains:

```text
Finish Solana tiny-live proof first.
Then research Monad / Sei / Linea as the first small-but-deadly expansion set.
```

---

## 13. Reference sources

These links were used to seed the research framing. They should be refreshed before any real engineering sprint.

- Monad documentation: https://docs.monad.xyz/
- Monad user overview: https://docs.monad.xyz/introduction/monad-for-users
- Sei EVM docs: https://docs.sei.io/evm
- Sei Giga overview: https://docs.sei.io/learn/sei-giga
- Linea architecture: https://docs.linea.build/protocol/architecture
- Linea transaction finality: https://docs.linea.build/network/overview/transaction-finality
- Base DeFiLlama chain page: https://defillama.com/chain/base
- Monad DeFiLlama chain page: https://defillama.com/chain/monad
- OP Mainnet DeFiLlama chain page: https://defillama.com/chain/op-mainnet
- Tron DeFiLlama chain page: https://defillama.com/chain/tron
- Sui documentation: https://docs.sui.io/
- Aptos execution docs: https://aptos.dev/network/blockchain/execution
- Sonic docs: https://docs.soniclabs.com/
- Berachain Proof-of-Liquidity docs: https://docs.berachain.com/general/proof-of-liquidity/overview
