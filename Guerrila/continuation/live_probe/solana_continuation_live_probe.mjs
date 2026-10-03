import fs from "node:fs/promises";
import path from "node:path";

const WSOL_MINT = "So11111111111111111111111111111111111111112";
const RUN61_PATH = path.join(
  process.cwd(),
  "g12-qc",
  "epoch1_canonical_monetization__run61_continuous_shadow_live_simulation.json",
);
const ENV_PATH = path.join(process.cwd(), ".env");
const DEFAULT_OUTPUT_PATH = path.join(
  process.cwd(),
  "tmp",
  "solana_continuation_live_probe_report.json",
);

function quantile(values, q) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = (sorted.length - 1) * q;
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return sorted[lo];
  const weight = idx - lo;
  return sorted[lo] * (1 - weight) + sorted[hi] * weight;
}

function median(values) {
  return quantile(values, 0.5);
}

function rate(numerator, denominator) {
  return denominator > 0 ? numerator / denominator : null;
}

function round(value, digits = 6) {
  return Number.isFinite(value)
    ? Number(value.toFixed(digits))
    : value;
}

function parseEnv(text) {
  const map = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (!line || /^\s*#/.test(line)) continue;
    const idx = line.indexOf("=");
    if (idx <= 0) continue;
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    map.set(key, value);
  }
  return map;
}

function extractApiKey(rpcUrl) {
  const url = new URL(rpcUrl);
  const apiKey = url.searchParams.get("api-key");
  if (!apiKey) {
    throw new Error("HELIUS_RPC_URL is missing api-key query parameter");
  }
  return apiKey;
}

function toNumber(value) {
  if (typeof value === "number") return value;
  if (typeof value === "string" && value.trim()) return Number(value);
  return Number.NaN;
}

function classifyTrade(tx) {
  if (tx?.type !== "SWAP") return null;
  const transfers = Array.isArray(tx.tokenTransfers) ? tx.tokenTransfers : [];
  const wsolTransfers = transfers.filter((t) => t.mint === WSOL_MINT);
  const otherTransfers = transfers.filter((t) => t.mint !== WSOL_MINT);
  if (wsolTransfers.length !== 1 || otherTransfers.length !== 1) {
    return null;
  }

  const wsol = wsolTransfers[0];
  const other = otherTransfers[0];
  const solNotional = toNumber(wsol.tokenAmount);
  const tokenAmount = toNumber(other.tokenAmount);
  if (!(solNotional > 0) || !(tokenAmount > 0)) {
    return null;
  }

  const feePayer = tx.feePayer || "";
  let direction = null;
  if (other.toUserAccount === feePayer) {
    direction = "buy";
  } else if (other.fromUserAccount === feePayer) {
    direction = "sell";
  } else if (wsol.fromUserAccount === feePayer) {
    direction = "buy";
  } else if (wsol.toUserAccount === feePayer) {
    direction = "sell";
  }

  if (!direction) {
    return null;
  }

  return {
    signature: tx.signature,
    slot: tx.slot,
    timestamp: tx.timestamp,
    feePayer,
    pool: null,
    source: tx.source,
    direction,
    tokenMint: other.mint,
    solNotional,
    tokenAmount,
    priceSolPerToken: solNotional / tokenAmount,
  };
}

function percentileLabel(q) {
  return `p${Math.round(q * 100)}`;
}

function aggregateEpisodes(episodes) {
  const next1Same = episodes.filter((e) => e.sameDirectionNext1).length;
  const next1Cont = episodes.filter((e) => e.priceContinuationNext1).length;
  const next3Majority = episodes.filter((e) => e.majoritySameDirectionNext3).length;
  const sameSlotNext1 = episodes.filter((e) => e.next1SameSlot).length;
  const within60sNext1 = episodes.filter((e) => e.next1Within60s).length;
  const favBps = episodes.map((e) => e.maxFavorableMoveBps);
  const advBps = episodes.map((e) => e.maxAdverseMoveBps);

  return {
    count: episodes.length,
    sameDirectionNext1Rate: round(rate(next1Same, episodes.length), 4),
    priceContinuationNext1Rate: round(rate(next1Cont, episodes.length), 4),
    majoritySameDirectionNext3Rate: round(rate(next3Majority, episodes.length), 4),
    next1SameSlotRate: round(rate(sameSlotNext1, episodes.length), 4),
    next1Within60sRate: round(rate(within60sNext1, episodes.length), 4),
    medianMaxFavorableMoveBps: round(median(favBps), 2),
    medianMaxAdverseMoveBps: round(median(advBps), 2),
    p90MaxFavorableMoveBps: round(quantile(favBps, 0.9), 2),
    p90MaxAdverseMoveBps: round(quantile(advBps, 0.9), 2),
  };
}

function summarizePool(pool, trades, shockQuantile) {
  const ordered = [...trades].sort((a, b) => {
    if (a.timestamp !== b.timestamp) return a.timestamp - b.timestamp;
    if (a.slot !== b.slot) return a.slot - b.slot;
    return a.signature.localeCompare(b.signature);
  });
  const notionals = ordered.map((t) => t.solNotional);
  const threshold = quantile(notionals, shockQuantile);
  const shocks = [];

  for (let i = 0; i < ordered.length - 1; i += 1) {
    const trade = ordered[i];
    if (!(trade.solNotional >= threshold)) continue;
    const next1 = ordered[i + 1];
    const next3 = ordered.slice(i + 1, i + 4);
    if (!next1 || !next3.length) continue;

    const sameDirectionCount3 = next3.filter((t) => t.direction === trade.direction).length;
    const priceMovesBps = next3.map((future) => {
      const base = trade.priceSolPerToken;
      if (trade.direction === "buy") {
        return ((future.priceSolPerToken - base) / base) * 10_000;
      }
      return ((base - future.priceSolPerToken) / base) * 10_000;
    });

    shocks.push({
      signature: trade.signature,
      slot: trade.slot,
      timestamp: trade.timestamp,
      direction: trade.direction,
      tokenMint: trade.tokenMint,
      solNotional: round(trade.solNotional, 6),
      priceSolPerToken: trade.priceSolPerToken,
      sameDirectionNext1: next1.direction === trade.direction,
      priceContinuationNext1:
        trade.direction === "buy"
          ? next1.priceSolPerToken > trade.priceSolPerToken
          : next1.priceSolPerToken < trade.priceSolPerToken,
      majoritySameDirectionNext3: sameDirectionCount3 >= 2,
      next1SameSlot: next1.slot === trade.slot,
      next1Within60s: next1.timestamp - trade.timestamp <= 60,
      maxFavorableMoveBps: round(Math.max(...priceMovesBps), 2),
      maxAdverseMoveBps: round(
        Math.max(0, ...priceMovesBps.map((value) => -value)),
        2,
      ),
    });
  }

  const byDirection = {
    buy: aggregateEpisodes(shocks.filter((e) => e.direction === "buy")),
    sell: aggregateEpisodes(shocks.filter((e) => e.direction === "sell")),
  };

  return {
    pool,
    tradeCount: ordered.length,
    latestTimestamp: ordered.length ? ordered[ordered.length - 1].timestamp : null,
    latestSlot: ordered.length ? ordered[ordered.length - 1].slot : null,
    tokenMints: [...new Set(ordered.map((t) => t.tokenMint))],
    shockThresholdSol: round(threshold, 6),
    shockThresholdLabel: percentileLabel(shockQuantile),
    shockCount: shocks.length,
    aggregate: aggregateEpisodes(shocks),
    byDirection,
    sampleShocks: shocks
      .slice(0, 5)
      .map(({ signature, slot, timestamp, direction, solNotional, sameDirectionNext1, priceContinuationNext1, maxFavorableMoveBps, maxAdverseMoveBps }) => ({
        signature,
        slot,
        timestamp,
        direction,
        solNotional,
        sameDirectionNext1,
        priceContinuationNext1,
        maxFavorableMoveBps,
        maxAdverseMoveBps,
      })),
  };
}

async function fetchPoolTransactions(pool, apiKey) {
  const url = new URL(`https://api-mainnet.helius-rpc.com/v0/addresses/${pool}/transactions`);
  url.searchParams.set("api-key", apiKey);
  url.searchParams.set("type", "SWAP");
  url.searchParams.set("sort-order", "desc");

  const res = await fetch(url, {
    headers: {
      Accept: "application/json",
    },
  });

  if (!res.ok) {
    throw new Error(`Helius request failed for ${pool}: ${res.status} ${res.statusText}`);
  }

  const data = await res.json();
  if (!Array.isArray(data)) {
    throw new Error(`Unexpected Helius response for ${pool}`);
  }
  return data;
}

async function main() {
  const [envText, run61Text] = await Promise.all([
    fs.readFile(ENV_PATH, "utf8"),
    fs.readFile(RUN61_PATH, "utf8"),
  ]);

  const envMap = parseEnv(envText);
  const heliusRpcUrl = envMap.get("HELIUS_RPC_URL");
  if (!heliusRpcUrl) {
    throw new Error("HELIUS_RPC_URL not found in .env");
  }

  const apiKey = extractApiKey(heliusRpcUrl);
  const run61 = JSON.parse(run61Text);
  const continuation = run61?.owner_mode_report?.continuation;
  if (!continuation) {
    throw new Error("Continuation report missing from run61 artifact");
  }

  const pools = (continuation.top_pool_directions || [])
    .slice(0, 5)
    .map((item) => item.pool)
    .filter(Boolean);

  const shockQuantile = 0.8;
  const poolReports = [];

  for (const pool of pools) {
    const txs = await fetchPoolTransactions(pool, apiKey);
    const trades = txs
      .map((tx) => {
        const parsed = classifyTrade(tx);
        return parsed ? { ...parsed, pool } : null;
      })
      .filter(Boolean);

    if (!trades.length) {
      continue;
    }

    poolReports.push(summarizePool(pool, trades, shockQuantile));
  }

  const allEpisodes = poolReports.flatMap((pool) => {
    const samples = pool.sampleShocks || [];
    return samples;
  });

  const aggregateEpisodesExpanded = poolReports.flatMap((pool) => {
    const ordered = [];
    for (const dir of ["buy", "sell"]) {
      const details = pool.byDirection?.[dir];
      if (!details || !details.count) continue;
      ordered.push({ direction: dir, count: details.count });
    }
    return ordered;
  });

  const poolEpisodeRows = [];
  for (const pool of poolReports) {
    const txs = await fetchPoolTransactions(pool.pool, apiKey);
    const trades = txs
      .map((tx) => {
        const parsed = classifyTrade(tx);
        return parsed ? { ...parsed, pool: pool.pool } : null;
      })
      .filter(Boolean);
    const ordered = [...trades].sort((a, b) => {
      if (a.timestamp !== b.timestamp) return a.timestamp - b.timestamp;
      if (a.slot !== b.slot) return a.slot - b.slot;
      return a.signature.localeCompare(b.signature);
    });
    const threshold = quantile(
      ordered.map((t) => t.solNotional),
      shockQuantile,
    );
    for (let i = 0; i < ordered.length - 1; i += 1) {
      const trade = ordered[i];
      if (!(trade.solNotional >= threshold)) continue;
      const next1 = ordered[i + 1];
      const next3 = ordered.slice(i + 1, i + 4);
      if (!next1 || !next3.length) continue;
      const sameDirectionCount3 = next3.filter((t) => t.direction === trade.direction).length;
      const priceMovesBps = next3.map((future) => {
        const base = trade.priceSolPerToken;
        if (trade.direction === "buy") {
          return ((future.priceSolPerToken - base) / base) * 10_000;
        }
        return ((base - future.priceSolPerToken) / base) * 10_000;
      });
      poolEpisodeRows.push({
        pool: pool.pool,
        direction: trade.direction,
        sameDirectionNext1: next1.direction === trade.direction,
        priceContinuationNext1:
          trade.direction === "buy"
            ? next1.priceSolPerToken > trade.priceSolPerToken
            : next1.priceSolPerToken < trade.priceSolPerToken,
        majoritySameDirectionNext3: sameDirectionCount3 >= 2,
        next1SameSlot: next1.slot === trade.slot,
        next1Within60s: next1.timestamp - trade.timestamp <= 60,
        maxFavorableMoveBps: Math.max(...priceMovesBps),
        maxAdverseMoveBps: Math.max(0, ...priceMovesBps.map((value) => -value)),
      });
    }
  }

  const report = {
    generatedAt: new Date().toISOString(),
    historicalContext: {
      continuationCandidateCount: continuation.candidate_count,
      positiveCount: continuation.positive_count,
      bestHorizonPositiveCount: continuation.best_horizon_positive_count,
      routeUsableCount: continuation.route_usable_count,
      deployableByUsCount: continuation.deployable_by_us_count,
      routeDeployabilityCounts: continuation.route_deployability_counts,
      blockerSplitCounts: continuation.blocker_split_counts,
      mainBlocker: continuation.main_blocker,
      topPoolDirections: continuation.top_pool_directions.slice(0, 5),
    },
    liveProbe: {
      selectedPools: pools,
      analyzedPools: poolReports.length,
      shockQuantile,
      perPool: poolReports,
      aggregate: aggregateEpisodes(poolEpisodeRows),
      byDirection: {
        buy: aggregateEpisodes(poolEpisodeRows.filter((row) => row.direction === "buy")),
        sell: aggregateEpisodes(poolEpisodeRows.filter((row) => row.direction === "sell")),
      },
      notes: [
        "Shock trades are the top 20% of WSOL-notional swaps within each pool's recent Helius history.",
        "Continuation is measured two ways: same-direction next flow and favorable price movement versus the shock trade price.",
        "This live probe only covers pools whose swap stream can be reduced cleanly to WSOL-versus-token trades.",
      ],
    },
    scratch: {
      sampleShockRowsCount: allEpisodes.length,
      aggregateDirectionCounts: aggregateEpisodesExpanded,
    },
  };

  await fs.writeFile(DEFAULT_OUTPUT_PATH, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(JSON.stringify(report, null, 2));
}

main().catch((error) => {
  console.error(error.stack || String(error));
  process.exitCode = 1;
});
