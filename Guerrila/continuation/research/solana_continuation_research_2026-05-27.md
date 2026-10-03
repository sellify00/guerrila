# Solana Continuation Research

Date: 2026-05-27

## Question

Can a Solana trader profit by riding post-shock momentum after a large swap or whale-like impulse?

Short answer:

- Yes, but not as a broad rule.
- The evidence in this repo says continuation is habitat-specific, route-sensitive, and often too small or too late after fees and latency.
- The best setup is not "buy every shock". It is "pre-select pools where large shocks statistically trigger same-direction follow-through, then only trade when route and inventory are already ready."

## Data Used

- Historical continuation report:
  - `g12-qc/epoch1_canonical_monetization_run55_run39__continuation_p0_markout_report.json`
- Historical live-simulation / owner-mode report:
  - `g12-qc/epoch1_canonical_monetization__run61_continuous_shadow_live_simulation.json`
- Fresh live Helius probe generated in this session:
  - `tmp/solana_continuation_live_probe_report.json`
- Probe script generated in this session:
  - `tmp/solana_continuation_live_probe.mjs`

## Historical Read

Across `11,711` observed rows, immediate continuation is weak if you treat the whole surface as tradeable:

- Positive at `50ms`: `6.14%`
- Positive at `100ms`: `7.08%`
- Positive at `250ms`: `8.85%`
- Positive at `500ms`: `12.08%`
- Positive by `next same-pool event`: `44.72%`
- Positive by `1 slot`: `5.92%`
- Positive by `2 slots`: `11.28%`

Interpretation:

- Naive "shock happened, instantly copy it" is a bad general rule.
- A lot of rows become positive only after more time or after more same-pool flow arrives.
- That means continuation exists, but it should be modeled with separate entry, hold, and exit clocks rather than as a pure sub-`100ms` race.

## Continuation As A Strategy Class

The run61 simulation tagged `979` rows as continuation candidates.

But tradeability was the real limiter:

- `108` were route-usable by us
- `1` was deployable by us
- Main blocker: `route_unavailable`
- Blocker split:
  - `871` route unavailable
  - `107` inventory insufficient
  - `1` no blocker

Interpretation:

- The repo already found continuation-like behavior.
- Most of it was not monetizable with the route / inventory posture available in that run.
- So the edge is real, but access matters as much as signal quality.

## Timing Model

Continuation should be framed with three clocks:

- `entry clock`: how quickly Shadow can recognize valid same-direction follow-through and enter
- `thesis / holding clock`: how long continuation pressure persists across later events or seconds
- `exit clock`: how quickly Shadow can flatten when target, stop, or reversal triggers

This matters because strict quote-identity drift is not the same thing as thesis decay.

- `ENTRY_IDENTITY_DRIFT` means the exact original replay quote changed.
- `THESIS_STILL_VALID` means same-direction continuation pressure still exists on a fresh check.
- `THESIS_DECAYED` means the follow-through disappeared or reversed.
- `EXIT_READY` means a fresh flatten route still exists now.
- `EXIT_NOT_READY` means exit quality or readiness is no longer acceptable.

For Continuation, strict replay identity is useful as a diagnostic. The live proof should be fresh thesis revalidation with fast entry/exit gates and bounded adverse excursion.

## Fresh Live Helius Probe

I probed the top continuation-heavy WSOL pools from the repo's own report using recent Helius `SWAP` history.

Method:

- Take the top continuation pools from run61.
- Pull recent parsed swap history from Helius.
- Keep clean WSOL-versus-token swaps.
- Define a shock as the top `20%` WSOL-notional swap within each pool's recent sample.
- Measure what happens on the next `1` to `3` swaps.

Aggregate across `5` pools and `42` shock episodes:

- Same-direction next trade: `71.43%`
- Favorable next-trade price continuation: `54.76%`
- Majority same-direction across next `3` trades: `66.67%`
- Next trade within `60s`: `100%`
- Same-slot next trade: `4.76%`
- Median max favorable move over next `3` trades: `5.35 bps`
- Median max adverse move over next `3` trades: `2.15 bps`

Direction split:

- Buy shocks:
  - Same-direction next trade: `86.67%`
  - Favorable next-trade price continuation: `50.00%`
  - Majority same-direction over next `3`: `83.33%`
- Sell shocks:
  - Same-direction next trade: `33.33%`
  - Favorable next-trade price continuation: `66.67%`
  - Majority same-direction over next `3`: `25.00%`

Interpretation:

- Buy shocks often attract more buy flow, but the next fill is not guaranteed to be better.
- Sell shocks less often get the next trade in the same direction, yet price often still continues lower.
- That looks like two different anomalies:
  - copy-wave crowding on buys
  - liquidity void / price reset on sells

## Best / Worst Live Habitats

Stronger current habitats in the fresh sample:

- `BdiLmeLxaiBbigj3Xsu7tJ7QpSCHtggeRK2huBiwiSqD`
  - Same-direction next trade: `88.89%`
  - Majority same-direction next `3`: `77.78%`
- `7FjNHJxdJE7bTkyH7Nub4ZLygnUujCtZXGRxNnYGcyYS`
  - Same-direction next trade: `81.82%`
  - Majority same-direction next `3`: `90.91%`
- `9fEF5PTvrfnHQkZFp4U8rSWmJGaYL8EMzatc5koKYoxg`
  - Same-direction next trade: `75.00%`
  - Favorable next-trade continuation: `58.33%`

Weak / noisy habitat:

- `Bg9CaaSJBPcn3HSHUUqqvCncZkkgRaz4SzdtwsVQF1YH`
  - Same-direction next trade: `50.00%`
  - Favorable next-trade continuation: `25.00%`
  - Majority same-direction next `3`: `0.00%`

Special case:

- `2Uxevuvapkt7VuTcut1iuW6qfdK2Rg5PTzXiZe9HLWBj`
  - Same-direction next trade only `33.33%`
  - But favorable next-trade continuation `83.33%`
  - That suggests price continuation without obvious same-direction trade clustering

## What This Means For Trading

The answer is not "continuation always works".

The answer is:

- continuation is real in selected pools
- it is often visible as clustered same-direction flow
- but the realized edge is usually small unless you are already prepared

Practical implications:

- Do not chase every shock globally.
- Pre-rank pools by continuation habitat.
- Use pool-specific shock thresholds, not one universal threshold.
- Require route readiness and inventory readiness before entry.
- For buy continuation, crowd-following exists, but price improvement is much weaker than flow-following.
- For sell continuation, price can keep moving even when the next trade is not another obvious sell.

## Anomalies Worth Learning

1. Flow continuation is stronger than price continuation.
   - The next trade often matches direction.
   - The next price move is much less reliable.

2. Buy-side and sell-side continuation are different phenomena.
   - Buys look like imitation / chase flow.
   - Sells look more like liquidity gapping or weak-bid repricing.

3. Pool selection matters more than global market intuition.
   - Some pools are continuation habitats.
   - Others are noise traps.

4. The signal often persists beyond the entry reaction.
   - In the live sample, every next trade came within `60s`.
   - Only a small fraction were same-slot.
   - That means the entry clock can still be fast while the thesis itself persists across later events or seconds.

5. Route access is part of the alpha.
   - Historical repo evidence found many continuation rows that were theoretically positive but not reachable with the available route posture.

## Strategy Conclusion

You can try to ride momentum after a Solana shock, but only under strict conditions:

- pool is already known to be a continuation habitat
- shock size is large relative to that pool's normal flow
- route is already usable
- input inventory is already held
- expected edge comfortably exceeds fees and slippage, and survives the entry / hold / exit timing budget

If you detect the impulse too late, the exact original quote identity may drift away. That does not automatically kill the trade, but the fresh thesis still has to revalidate with a positive entry, a positive exit, and bounded adverse excursion.

The strongest research direction from here is not broad continuation. It is:

- pool-conditioned continuation
- side-conditioned continuation
- route-conditioned continuation
- shock-size-conditioned continuation

That is where the anomalies live.
