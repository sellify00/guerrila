import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const ENV_PATH = path.join(ROOT, ".env");
const OUTPUT_DIR = path.join(ROOT, "continuation", "active_discovery");

const WSOL_MINT = "So11111111111111111111111111111111111111112";
const DEFAULT_SEED_TOKENS = [
  WSOL_MINT,
  "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2",
  "7xKXtg2CW1dsoGqYJcW5K63Vf6kRrK4r5xL2U5Fj4Y9X",
  "DezXAZ8z7PnrnRJjz3wXBoRgixCa6YaB1pPB263Kpump",
  "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R",
];
const SUPPORTED_VENUES = new Set([
  "raydium_cpmm",
  "raydium_amm_v4",
  "raydium_clmm",
  "meteora_damm_v2",
  "orca_whirlpool",
]);

function parseArgs(argv) {
  const options = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    const raw = token.slice(2);
    const eq = raw.indexOf("=");
    if (eq >= 0) {
      options.set(raw.slice(0, eq), raw.slice(eq + 1));
      continue;
    }
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      options.set(raw, next);
      i += 1;
    } else {
      options.set(raw, "true");
    }
  }
  return options;
}

function stringOption(options, key, fallback) {
  const value = options.get(key);
  return value == null || value === "" ? fallback : value;
}

function numberOption(options, key, fallback) {
  const value = options.get(key);
  if (value == null || value === "") return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function clamp(value, lo = 0, hi = 1) {
  return Math.max(lo, Math.min(hi, value));
}

function round(value, digits = 6) {
  return Number.isFinite(value) ? Number(value.toFixed(digits)) : value;
}

function toNumber(value) {
  if (typeof value === "number") return value;
  if (typeof value === "string" && value.trim()) return Number(value);
  return Number.NaN;
}

function rate(numerator, denominator) {
  return denominator > 0 ? numerator / denominator : null;
}

function quantile(values, q) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = (sorted.length - 1) * clamp(q, 0, 1);
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return sorted[lo];
  const weight = idx - lo;
  return sorted[lo] * (1 - weight) + sorted[hi] * weight;
}

function median(values) {
  return quantile(values, 0.5);
}

function scoreLog(value, low, high) {
  value = Math.max(value, 0);
  if (value <= 0) return 0;
  low = Math.max(low, 1e-9);
  high = Math.max(high, low * 1.0001);
  const numerator = Math.log10(Math.max(value, low)) - Math.log10(low);
  const denominator = Math.log10(high) - Math.log10(low);
  return clamp(denominator ? numerator / denominator : 0);
}

function parseEnv(text) {
  const map = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (!line || /^\s*#/.test(line)) continue;
    const idx = line.indexOf("=");
    if (idx <= 0) continue;
    map.set(line.slice(0, idx).trim(), line.slice(idx + 1).trim());
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

function mapVenueFamily(dexId, labels) {
  const dex = String(dexId || "").trim().toLowerCase();
  const labelSet = new Set((labels || []).map((label) => String(label || "").trim().toLowerCase()));
  if (dex === "raydium") {
    if (labelSet.has("clmm")) return "raydium_clmm";
    if (labelSet.has("cpmm")) return "raydium_cpmm";
    return "raydium_amm_v4";
  }
  if (dex === "meteora") return "meteora_damm_v2";
  if (dex === "orca") return "orca_whirlpool";
  return "unknown";
}

function volatilityScore(row) {
  const m5 = Math.abs(toNumber(row?.priceChange?.m5) || 0);
  const h1 = Math.abs(toNumber(row?.priceChange?.h1) || 0);
  const h24 = Math.abs(toNumber(row?.priceChange?.h24) || 0);
  const weighted = Math.max(m5, h1 * 0.65, h24 * 0.2);
  return clamp(weighted / 8);
}

function turnoverScore(volume5m, liquidityUsd) {
  if (!(liquidityUsd > 0)) return 0;
  return clamp((volume5m / liquidityUsd) / 0.35);
}

function currentActivityScore(row, minLiquidityUsd, minTxnsM5) {
  const liquidityUsd = toNumber(row.liquidity_usd) || 0;
  const txCountM5 = Number(row.tx_count_5m) || 0;
  const volume5m = toNumber(row.volume_5m) || 0;
  const liquidityScore = scoreLog(liquidityUsd, minLiquidityUsd, 2_500_000);
  const eventRateScore = scoreLog(txCountM5, minTxnsM5, 500);
  const turnScore = turnoverScore(volume5m, liquidityUsd);
  return round(
    volatilityScore({
      priceChange: {
        m5: row.price_change_5m,
        h1: row.price_change_1h,
        h24: row.price_change_24h,
      },
    }) *
      liquidityScore *
      eventRateScore *
      Math.max(turnScore, 0.1),
    6,
  );
}

async function httpJson(url) {
  const res = await fetch(url, {
    headers: {
      "User-Agent": "shadow-active-continuation-discovery/1.0",
      Accept: "application/json",
    },
  });
  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`);
  }
  return res.json();
}

async function fetchDexScreenerUniverse(seedTokens) {
  const rows = [];
  const notes = [];
  const seenPairs = new Set();
  for (const token of seedTokens) {
    const url = `https://api.dexscreener.com/token-pairs/v1/solana/${encodeURIComponent(token)}`;
    try {
      const payload = await httpJson(url);
      notes.push(`dexscreener_seed_ok:${token}:${Array.isArray(payload) ? payload.length : 0}`);
      for (const row of Array.isArray(payload) ? payload : []) {
        const pairAddress = String(row?.pairAddress || "").trim();
        if (!pairAddress || seenPairs.has(pairAddress)) continue;
        seenPairs.add(pairAddress);
        rows.push(row);
      }
    } catch (error) {
      notes.push(`dexscreener_seed_fail:${token}:${error.message || String(error)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  return { rows, notes };
}

function normalizePairs(rawRows, minLiquidityUsd, minTxnsM5) {
  const normalized = [];
  for (const row of rawRows) {
    if (String(row.chainId || "") !== "solana") continue;
    const base = row.baseToken || {};
    const quote = row.quoteToken || {};
    const baseMint = String(base.address || "").trim();
    const quoteMint = String(quote.address || "").trim();
    const pairAddress = String(row.pairAddress || "").trim();
    if (!baseMint || !quoteMint || !pairAddress) continue;
    if (baseMint !== WSOL_MINT && quoteMint !== WSOL_MINT) continue;
    const liquidityUsd = toNumber(row?.liquidity?.usd) || 0;
    const volume5m = toNumber(row?.volume?.m5) || 0;
    const volume1h = toNumber(row?.volume?.h1) || 0;
    const priceChange5m = toNumber(row?.priceChange?.m5) || 0;
    const priceChange1h = toNumber(row?.priceChange?.h1) || 0;
    const priceChange24h = toNumber(row?.priceChange?.h24) || 0;
    const txCountM5 =
      (toNumber(row?.txns?.m5?.buys) || 0) + (toNumber(row?.txns?.m5?.sells) || 0);
    if (liquidityUsd < minLiquidityUsd || txCountM5 < minTxnsM5) continue;
    const venue = mapVenueFamily(row.dexId, row.labels || []);
    if (!SUPPORTED_VENUES.has(venue)) continue;
    const normalizedRow = {
      pool: pairAddress,
      venue,
      dex_id: String(row.dexId || ""),
      labels: row.labels || [],
      base_mint: baseMint,
      base_symbol: String(base.symbol || ""),
      quote_mint: quoteMint,
      quote_symbol: String(quote.symbol || ""),
      liquidity_usd: round(liquidityUsd, 2),
      volume_5m: round(volume5m, 2),
      volume_1h: round(volume1h, 2),
      price_change_5m: round(priceChange5m, 4),
      price_change_1h: round(priceChange1h, 4),
      price_change_24h: round(priceChange24h, 4),
      tx_count_5m: txCountM5,
    };
    normalizedRow.current_activity_score = currentActivityScore(
      normalizedRow,
      minLiquidityUsd,
      minTxnsM5,
    );
    normalized.push(normalizedRow);
  }
  normalized.sort((a, b) => {
    const activityDelta = b.current_activity_score - a.current_activity_score;
    if (activityDelta !== 0) return activityDelta;
    const txDelta = b.tx_count_5m - a.tx_count_5m;
    if (txDelta !== 0) return txDelta;
    const volumeDelta = b.volume_5m - a.volume_5m;
    if (volumeDelta !== 0) return volumeDelta;
    return b.liquidity_usd - a.liquidity_usd;
  });
  return normalized;
}

function classifyTrade(tx) {
  if (tx?.type !== "SWAP") return null;
  const transfers = Array.isArray(tx.tokenTransfers) ? tx.tokenTransfers : [];
  const wsolTransfers = transfers.filter((item) => item.mint === WSOL_MINT);
  const otherTransfers = transfers.filter((item) => item.mint !== WSOL_MINT);
  if (wsolTransfers.length !== 1 || otherTransfers.length !== 1) return null;

  const wsol = wsolTransfers[0];
  const other = otherTransfers[0];
  const solNotional = toNumber(wsol.tokenAmount);
  const tokenAmount = toNumber(other.tokenAmount);
  if (!(solNotional > 0) || !(tokenAmount > 0)) return null;

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
  if (!direction) return null;

  return {
    signature: tx.signature,
    slot: Number(tx.slot) || 0,
    timestamp: Number(tx.timestamp) || 0,
    direction,
    tokenMint: other.mint,
    solNotional,
    tokenAmount,
    priceSolPerToken: solNotional / tokenAmount,
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
  const payload = await res.json();
  if (!Array.isArray(payload)) {
    throw new Error(`Unexpected Helius response for ${pool}`);
  }
  return payload;
}

function aggregateEpisodes(rows) {
  const favorable = rows
    .map((row) => row.maxFavorableMoveBps)
    .filter((value) => Number.isFinite(value));
  const adverse = rows
    .map((row) => row.maxAdverseMoveBps)
    .filter((value) => Number.isFinite(value));
  return {
    count: rows.length,
    sameDirectionNext1Rate: round(
      rate(rows.filter((row) => row.sameDirectionNext1).length, rows.length),
      4,
    ),
    priceContinuationNext1Rate: round(
      rate(rows.filter((row) => row.priceContinuationNext1).length, rows.length),
      4,
    ),
    majoritySameDirectionNext3Rate: round(
      rate(rows.filter((row) => row.majoritySameDirectionNext3).length, rows.length),
      4,
    ),
    next1SameSlotRate: round(
      rate(rows.filter((row) => row.next1SameSlot).length, rows.length),
      4,
    ),
    next1Within60sRate: round(
      rate(rows.filter((row) => row.next1Within60s).length, rows.length),
      4,
    ),
    medianMaxFavorableMoveBps: round(median(favorable), 2),
    medianMaxAdverseMoveBps: round(median(adverse), 2),
    p90MaxFavorableMoveBps: round(quantile(favorable, 0.9), 2),
    p90MaxAdverseMoveBps: round(quantile(adverse, 0.9), 2),
  };
}

function computeContinuationScore(summary, latestAgeHours, minShockEpisodes) {
  const count = Number(summary?.count) || 0;
  if (count <= 0) return 0;
  const same1 = toNumber(summary.sameDirectionNext1Rate) || 0;
  const price1 = toNumber(summary.priceContinuationNext1Rate) || 0;
  const same3 = toNumber(summary.majoritySameDirectionNext3Rate) || 0;
  const favorable = toNumber(summary.medianMaxFavorableMoveBps) || 0;
  const adverse = toNumber(summary.medianMaxAdverseMoveBps) || 0;
  const edgeComponent = clamp((favorable - adverse + 5) / 15);
  let recency = 1;
  if (latestAgeHours > 72) {
    recency = 0.4;
  } else if (latestAgeHours > 24) {
    recency = 0.65;
  } else if (latestAgeHours > 6) {
    recency = 0.85;
  }
  const confidence = Math.min(1, Math.sqrt(count / Math.max(minShockEpisodes, 1)));
  const raw = 0.3 * price1 + 0.25 * same1 + 0.25 * same3 + 0.2 * edgeComponent;
  return round(10 * raw * confidence * recency, 4);
}

function analyzePoolContinuation(poolRow, txs, shockQuantile, minShockEpisodes) {
  const trades = txs
    .map((tx) => classifyTrade(tx))
    .filter(Boolean)
    .sort((a, b) => {
      if (a.timestamp !== b.timestamp) return a.timestamp - b.timestamp;
      if (a.slot !== b.slot) return a.slot - b.slot;
      return String(a.signature).localeCompare(String(b.signature));
    });
  if (!trades.length) {
    return {
      ...poolRow,
      continuation_status: "no_clean_wsol_token_swaps",
      continuation_live_score: 0,
      shock_count: 0,
      latest_age_hours: null,
      latest_timestamp: null,
      aggregate: aggregateEpisodes([]),
      best_side: null,
    };
  }

  const threshold = quantile(trades.map((trade) => trade.solNotional), shockQuantile);
  const episodes = [];
  for (let i = 0; i < trades.length - 1; i += 1) {
    const trade = trades[i];
    if (!(trade.solNotional >= threshold)) continue;
    const next1 = trades[i + 1];
    const next3 = trades.slice(i + 1, i + 4);
    if (!next1 || !next3.length) continue;
    const sameDirectionCount3 = next3.filter(
      (future) => future.direction === trade.direction,
    ).length;
    const priceMovesBps = next3.map((future) => {
      if (trade.direction === "buy") {
        return ((future.priceSolPerToken - trade.priceSolPerToken) / trade.priceSolPerToken) * 10_000;
      }
      return ((trade.priceSolPerToken - future.priceSolPerToken) / trade.priceSolPerToken) * 10_000;
    });
    episodes.push({
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

  const aggregate = aggregateEpisodes(episodes);
  const buyAggregate = aggregateEpisodes(episodes.filter((row) => row.direction === "buy"));
  const sellAggregate = aggregateEpisodes(episodes.filter((row) => row.direction === "sell"));
  const latestTimestamp = trades[trades.length - 1].timestamp;
  const latestAgeHours = (Date.now() / 1000 - latestTimestamp) / 3600;
  const buyScore = computeContinuationScore(buyAggregate, latestAgeHours, minShockEpisodes);
  const sellScore = computeContinuationScore(sellAggregate, latestAgeHours, minShockEpisodes);
  const liveScore = computeContinuationScore(aggregate, latestAgeHours, minShockEpisodes);
  return {
    ...poolRow,
    continuation_status:
      episodes.length >= minShockEpisodes ? "ok" : "insufficient_shock_sample",
    continuation_live_score: liveScore,
    shock_threshold_sol: round(threshold, 6),
    shock_count: episodes.length,
    latest_timestamp: latestTimestamp,
    latest_age_hours: round(latestAgeHours, 2),
    trade_count: trades.length,
    best_side: buyScore >= sellScore ? "buy" : "sell",
    aggregate,
    by_direction: {
      buy: buyAggregate,
      sell: sellAggregate,
    },
  };
}

function timestampTag(date = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}_${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`;
}

function renderMarkdownSummary(report) {
  const lines = [];
  lines.push("# Active Continuation Discovery");
  lines.push("");
  lines.push(`- Generated at: ${report.generated_at}`);
  lines.push(`- DexScreener raw pairs: ${report.discovery.dexscreener_raw_pair_count}`);
  lines.push(`- Active WSOL pairs kept: ${report.discovery.active_wsol_pair_count}`);
  lines.push(`- Helius-scored pools: ${report.helius_scored_pool_count}`);
  lines.push(`- Pools active in last 1 hour: ${report.activity.active_under_1h}`);
  lines.push(`- Pools active in last 6 hours: ${report.activity.active_under_6h}`);
  lines.push(`- Pools active in last 24 hours: ${report.activity.active_under_24h}`);
  lines.push(`- Continuation candidates active in last 24 hours: ${report.activity.continuation_active_under_24h}`);
  lines.push(`- Verdict: ${report.verdict}`);
  lines.push("");
  lines.push("## Top Live Candidates");
  lines.push("");
  if (!report.top_candidates.length) {
    lines.push("- None met the current freshness and continuation thresholds.");
  } else {
    for (const row of report.top_candidates) {
      lines.push(
        `- ${row.pool}: age ${row.latest_age_hours}h, venue ${row.venue}, activity ${row.current_activity_score}, continuation ${row.continuation_live_score}, best side ${row.best_side}, same-direction ${row.aggregate.sameDirectionNext1Rate}, price continuation ${row.aggregate.priceContinuationNext1Rate}`,
      );
    }
  }
  lines.push("");
  lines.push("## Freshest Pools");
  lines.push("");
  for (const row of report.freshest_pools.slice(0, 10)) {
    lines.push(
      `- ${row.pool}: age ${row.latest_age_hours}h, tx5m ${row.tx_count_5m}, venue ${row.venue}, continuation ${row.continuation_live_score}, status ${row.continuation_status}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const seedTokens = stringOption(options, "seed-tokens", DEFAULT_SEED_TOKENS.join(","))
    .split(",")
    .map((token) => token.trim())
    .filter(Boolean);
  const minLiquidityUsd = Math.max(0, numberOption(options, "min-liquidity-usd", 25_000));
  const minTxnsM5 = Math.max(1, numberOption(options, "min-txns-m5", 20));
  const topUniverse = Math.max(1, numberOption(options, "top-universe", 40));
  const topOutput = Math.max(1, numberOption(options, "top-output", 12));
  const shockQuantile = clamp(numberOption(options, "shock-quantile", 0.8), 0, 1);
  const minShockEpisodes = Math.max(1, numberOption(options, "min-shock-episodes", 4));
  const freshnessHours = Math.max(1, numberOption(options, "freshness-hours", 24));
  const strongContinuationScore = numberOption(options, "strong-continuation-score", 2);
  const envText = await fs.readFile(ENV_PATH, "utf8");
  const envMap = parseEnv(envText);
  const heliusRpcUrl = envMap.get("HELIUS_RPC_URL");
  if (!heliusRpcUrl) {
    throw new Error("HELIUS_RPC_URL not found in .env");
  }
  const apiKey = extractApiKey(heliusRpcUrl);

  const discovery = await fetchDexScreenerUniverse(seedTokens);
  const normalized = normalizePairs(discovery.rows, minLiquidityUsd, minTxnsM5).slice(0, topUniverse);

  const scored = [];
  for (const pool of normalized) {
    try {
      const txs = await fetchPoolTransactions(pool.pool, apiKey);
      scored.push(analyzePoolContinuation(pool, txs, shockQuantile, minShockEpisodes));
    } catch (error) {
      scored.push({
        ...pool,
        continuation_status: "fetch_error",
        continuation_live_score: 0,
        latest_age_hours: null,
        latest_timestamp: null,
        shock_count: 0,
        trade_count: 0,
        aggregate: aggregateEpisodes([]),
        fetch_error: error.message || String(error),
      });
    }
  }

  const byFreshness = [...scored].sort((a, b) => {
    const ageA = Number.isFinite(a.latest_age_hours) ? a.latest_age_hours : Number.POSITIVE_INFINITY;
    const ageB = Number.isFinite(b.latest_age_hours) ? b.latest_age_hours : Number.POSITIVE_INFINITY;
    if (ageA !== ageB) return ageA - ageB;
    return (b.continuation_live_score || 0) - (a.continuation_live_score || 0);
  });

  const activeUnder1h = scored.filter(
    (row) => Number.isFinite(row.latest_age_hours) && row.latest_age_hours <= 1,
  );
  const activeUnder6h = scored.filter(
    (row) => Number.isFinite(row.latest_age_hours) && row.latest_age_hours <= 6,
  );
  const activeUnder24h = scored.filter(
    (row) => Number.isFinite(row.latest_age_hours) && row.latest_age_hours <= freshnessHours,
  );
  const continuationActiveUnder24h = activeUnder24h.filter(
    (row) =>
      row.continuation_status === "ok" &&
      (row.continuation_live_score || 0) >= strongContinuationScore &&
      (row.shock_count || 0) >= minShockEpisodes,
  );

  const topCandidates = [...continuationActiveUnder24h].sort((a, b) => {
    const scoreDelta = (b.continuation_live_score || 0) - (a.continuation_live_score || 0);
    if (scoreDelta !== 0) return scoreDelta;
    return (b.current_activity_score || 0) - (a.current_activity_score || 0);
  });

  let verdict = "No active repetition detected in the scanned WSOL pool universe.";
  if (continuationActiveUnder24h.length > 0) {
    verdict = `Active repetition detected in ${continuationActiveUnder24h.length} scanned pools.`;
  } else if (activeUnder24h.length > 0) {
    verdict = `Fresh pools exist, but none currently clear the continuation threshold in the scanned universe.`;
  }

  const generatedAt = new Date().toISOString();
  const report = {
    generated_at: generatedAt,
    config: {
      seed_tokens: seedTokens,
      min_liquidity_usd: minLiquidityUsd,
      min_txns_m5: minTxnsM5,
      top_universe: topUniverse,
      shock_quantile: shockQuantile,
      min_shock_episodes: minShockEpisodes,
      freshness_hours: freshnessHours,
      strong_continuation_score: strongContinuationScore,
    },
    discovery: {
      dexscreener_raw_pair_count: discovery.rows.length,
      active_wsol_pair_count: normalized.length,
      source_notes: discovery.notes,
    },
    helius_scored_pool_count: scored.length,
    activity: {
      active_under_1h: activeUnder1h.length,
      active_under_6h: activeUnder6h.length,
      active_under_24h: activeUnder24h.length,
      continuation_active_under_24h: continuationActiveUnder24h.length,
    },
    verdict,
    top_candidates: topCandidates.slice(0, topOutput),
    freshest_pools: byFreshness.slice(0, topOutput),
    scored_pools: scored,
  };

  await fs.mkdir(OUTPUT_DIR, { recursive: true });
  const stamp = timestampTag(new Date());
  const jsonPath = path.join(OUTPUT_DIR, `active_continuation_discovery_${stamp}.json`);
  const mdPath = path.join(OUTPUT_DIR, `active_continuation_discovery_${stamp}.md`);
  const latestJsonPath = path.join(OUTPUT_DIR, "latest_active_continuation_discovery.json");
  const latestMdPath = path.join(OUTPUT_DIR, "latest_active_continuation_discovery.md");
  const body = `${JSON.stringify(report, null, 2)}\n`;
  const markdown = renderMarkdownSummary(report);
  await Promise.all([
    fs.writeFile(jsonPath, body, "utf8"),
    fs.writeFile(mdPath, markdown, "utf8"),
    fs.writeFile(latestJsonPath, body, "utf8"),
    fs.writeFile(latestMdPath, markdown, "utf8"),
  ]);

  console.log(
    JSON.stringify(
      {
        generated_at: generatedAt,
        json_path: jsonPath,
        latest_json_path: latestJsonPath,
        active_under_24h: activeUnder24h.length,
        continuation_active_under_24h: continuationActiveUnder24h.length,
        verdict,
      },
      null,
      2,
    ),
  );
}

main().catch((error) => {
  console.error(error.stack || String(error));
  process.exitCode = 1;
});
