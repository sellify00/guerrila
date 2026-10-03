import fs from "node:fs/promises";
import path from "node:path";

const WSOL_MINT = "So11111111111111111111111111111111111111112";
const DEFAULT_ROWS_PATH = path.join(process.cwd(), "continuation", "run80d", "rows.jsonl");
const DEFAULT_ENV_PATH = path.join(process.cwd(), ".env");
const DEFAULT_STUDIES_ROOT = path.join(process.cwd(), "continuation", "studies");

function parseArgs(argv) {
  const options = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    const trimmed = token.slice(2);
    if (!trimmed) continue;
    const eq = trimmed.indexOf("=");
    if (eq >= 0) {
      options.set(trimmed.slice(0, eq), trimmed.slice(eq + 1));
      continue;
    }
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      options.set(trimmed, next);
      i += 1;
    } else {
      options.set(trimmed, "true");
    }
  }
  return options;
}

function numberOption(options, key, fallback) {
  const raw = options.get(key);
  if (raw == null) return fallback;
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function stringOption(options, key, fallback) {
  const raw = options.get(key);
  return raw == null || raw === "" ? fallback : raw;
}

function boolOption(options, key, fallback = false) {
  const raw = options.get(key);
  if (raw == null) return fallback;
  return !["0", "false", "no", "off"].includes(String(raw).toLowerCase());
}

function clamp(value, lo = 0, hi = 1) {
  return Math.max(lo, Math.min(hi, value));
}

function round(value, digits = 6) {
  return Number.isFinite(value) ? Number(value.toFixed(digits)) : value;
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

function rate(numerator, denominator) {
  return denominator > 0 ? numerator / denominator : null;
}

function average(values) {
  if (!values.length) return 0;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function sampleStdDev(values) {
  if (values.length <= 1) return 0;
  const mean = average(values);
  const variance =
    values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / values.length;
  return Math.sqrt(variance);
}

function range(values) {
  if (!values.length) return 0;
  return Math.max(...values) - Math.min(...values);
}

function toNumber(value) {
  if (typeof value === "number") return value;
  if (typeof value === "string" && value.trim()) return Number(value);
  return Number.NaN;
}

function safeFloat(value, fallback = 0) {
  const parsed = toNumber(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function safeInt(value, fallback = 0) {
  const parsed = Number.parseInt(String(value ?? ""), 10);
  return Number.isFinite(parsed) ? parsed : fallback;
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

function habitatScore(row) {
  const scores = row?.scores ?? {};
  const market = row?.market_metrics ?? {};
  return safeFloat(scores.habitat_score ?? market.external_habitat_score);
}

function readinessScore(row) {
  const scores = row?.scores ?? {};
  return safeFloat(scores.readiness_score);
}

function classifyTrade(tx) {
  if (tx?.type !== "SWAP") return null;
  const transfers = Array.isArray(tx.tokenTransfers) ? tx.tokenTransfers : [];
  const wsolTransfers = transfers.filter((item) => item.mint === WSOL_MINT);
  const otherTransfers = transfers.filter((item) => item.mint !== WSOL_MINT);
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
    slot: safeInt(tx.slot),
    timestamp: safeInt(tx.timestamp),
    direction,
    tokenMint: other.mint,
    solNotional,
    tokenAmount,
    priceSolPerToken: solNotional / tokenAmount,
  };
}

function aggregateEpisodes(rows) {
  const favorable = rows
    .map((row) => safeFloat(row.maxFavorableMoveBps, Number.NaN))
    .filter((value) => Number.isFinite(value));
  const adverse = rows
    .map((row) => safeFloat(row.maxAdverseMoveBps, Number.NaN))
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

function computeSideScore(summary, latestAgeHours, minEpisodes) {
  const count = safeInt(summary?.count);
  if (count <= 0) return 0;
  const same1 = safeFloat(summary.sameDirectionNext1Rate);
  const price1 = safeFloat(summary.priceContinuationNext1Rate);
  const same3 = safeFloat(summary.majoritySameDirectionNext3Rate);
  const favorable = safeFloat(summary.medianMaxFavorableMoveBps);
  const adverse = safeFloat(summary.medianMaxAdverseMoveBps);
  const edgeComponent = clamp((favorable - adverse + 5) / 15);
  let recency = 1;
  if (latestAgeHours > 72) {
    recency = 0.4;
  } else if (latestAgeHours > 24) {
    recency = 0.65;
  } else if (latestAgeHours > 6) {
    recency = 0.85;
  }
  const confidence = Math.min(1, Math.sqrt(count / Math.max(minEpisodes, 1)));
  const raw = 0.3 * price1 + 0.25 * same1 + 0.25 * same3 + 0.2 * edgeComponent;
  return round(10 * raw * confidence * recency, 4);
}

async function fetchPoolTransactions(pool, apiKey) {
  const url = new URL(
    `https://api-mainnet.helius-rpc.com/v0/addresses/${pool}/transactions`,
  );
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

function deepClone(value) {
  return JSON.parse(JSON.stringify(value));
}

function analyzePool(row, trades, options, nowMs) {
  const ordered = [...trades].sort((a, b) => {
    if (a.timestamp !== b.timestamp) return a.timestamp - b.timestamp;
    if (a.slot !== b.slot) return a.slot - b.slot;
    return String(a.signature).localeCompare(String(b.signature));
  });
  const latestTimestamp = ordered.length ? safeInt(ordered[ordered.length - 1].timestamp) : 0;
  const latestAgeHours = latestTimestamp ? (nowMs / 1000 - latestTimestamp) / 3600 : null;
  const notionals = ordered.map((trade) => trade.solNotional);
  const threshold = safeFloat(quantile(notionals, options.shockQuantile));
  const episodes = [];

  if (threshold > 0) {
    for (let i = 0; i < ordered.length - 1; i += 1) {
      const trade = ordered[i];
      if (trade.solNotional < threshold) continue;
      const next1 = ordered[i + 1];
      const next3 = ordered.slice(i + 1, i + 4);
      if (!next1 || !next3.length) continue;
      const sameDirectionCount3 = next3.filter(
        (future) => future.direction === trade.direction,
      ).length;
      const movesBps = [];
      for (const future of next3) {
        const base = safeFloat(trade.priceSolPerToken);
        const futurePrice = safeFloat(future.priceSolPerToken);
        if (!(base > 0) || !(futurePrice > 0)) continue;
        if (trade.direction === "buy") {
          movesBps.push(((futurePrice - base) / base) * 10_000);
        } else {
          movesBps.push(((base - futurePrice) / base) * 10_000);
        }
      }
      if (!movesBps.length) continue;
      episodes.push({
        signature: trade.signature,
        slot: trade.slot,
        timestamp: trade.timestamp,
        direction: trade.direction,
        tokenMint: trade.tokenMint,
        solNotional: round(trade.solNotional, 6),
        sameDirectionNext1: next1.direction === trade.direction,
        priceContinuationNext1:
          trade.direction === "buy"
            ? next1.priceSolPerToken > trade.priceSolPerToken
            : next1.priceSolPerToken < trade.priceSolPerToken,
        majoritySameDirectionNext3: sameDirectionCount3 >= 2,
        next1SameSlot: next1.slot === trade.slot,
        next1Within60s: next1.timestamp - trade.timestamp <= 60,
        maxFavorableMoveBps: round(Math.max(...movesBps), 2),
        maxAdverseMoveBps: round(Math.max(0, ...movesBps.map((value) => -value)), 2),
      });
    }
  }

  const aggregate = aggregateEpisodes(episodes);
  const buyRows = episodes.filter((row) => row.direction === "buy");
  const sellRows = episodes.filter((row) => row.direction === "sell");
  const byDirection = {
    buy: aggregateEpisodes(buyRows),
    sell: aggregateEpisodes(sellRows),
  };
  const buyScore = computeSideScore(
    byDirection.buy,
    safeFloat(latestAgeHours, 9999),
    options.minShockEpisodes,
  );
  const sellScore = computeSideScore(
    byDirection.sell,
    safeFloat(latestAgeHours, 9999),
    options.minShockEpisodes,
  );
  const liveScore = computeSideScore(
    aggregate,
    safeFloat(latestAgeHours, 9999),
    options.minShockEpisodes,
  );
  const deployableScore = round(
    liveScore * 0.7 + readinessScore(row) * 10 * 0.2 + habitatScore(row) * 10 * 0.1,
    4,
  );
  const bestSide = buyScore >= sellScore ? "buy" : "sell";

  let status = "ok";
  if (episodes.length < options.minShockEpisodes) {
    status = "insufficient_shock_sample";
  }
  if (!ordered.length) {
    status = "no_clean_wsol_token_swaps";
  }

  return {
    status,
    trade_count: ordered.length,
    shock_quantile: round(options.shockQuantile, 4),
    shock_threshold_sol: round(threshold, 6),
    shock_count: episodes.length,
    latest_timestamp: latestTimestamp || null,
    latest_age_hours: round(latestAgeHours, 2),
    best_side: bestSide,
    aggregate,
    by_direction: byDirection,
    sample_shocks: episodes.slice(0, 5),
    ranking_reason: `live=${liveScore} deployable=${deployableScore} shock_count=${episodes.length} best_side=${bestSide}`,
    scores: {
      continuation_live_score: liveScore,
      continuation_deployable_score: deployableScore,
      continuation_buy_score: buyScore,
      continuation_sell_score: sellScore,
    },
  };
}

function mergeRowWithLive(row, live, capturedAtIso) {
  const merged = deepClone(row);
  merged.scores = {
    ...(merged.scores ?? {}),
    ...live.scores,
  };
  merged.continuation_live = {
    status: live.status,
    trade_count: live.trade_count,
    shock_quantile: live.shock_quantile,
    shock_threshold_sol: live.shock_threshold_sol,
    shock_count: live.shock_count,
    latest_timestamp: live.latest_timestamp,
    latest_age_hours: live.latest_age_hours,
    best_side: live.best_side,
    aggregate: live.aggregate,
    by_direction: live.by_direction,
    sample_shocks: live.sample_shocks,
    ranking_reason: live.ranking_reason,
  };
  merged.evidence = {
    ...(merged.evidence ?? {}),
    continuation_live_source: "helius_enhanced_transactions_by_address",
    continuation_live_endpoint:
      "https://api-mainnet.helius-rpc.com/v0/addresses/<pool>/transactions",
    continuation_live_generated_at: capturedAtIso,
  };
  return merged;
}

async function loadJsonlRows(filePath) {
  const raw = await fs.readFile(filePath, "utf8");
  return raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

async function fileExists(filePath) {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function loadStudyConfig(studyRoot) {
  const configPath = path.join(studyRoot, "study_config.json");
  if (!(await fileExists(configPath))) return null;
  return JSON.parse(await fs.readFile(configPath, "utf8"));
}

async function listSnapshotFiles(studyRoot) {
  const snapshotDir = path.join(studyRoot, "snapshots");
  if (!(await fileExists(snapshotDir))) return [];
  const names = await fs.readdir(snapshotDir);
  return names
    .filter((name) => name.endsWith(".json"))
    .sort()
    .map((name) => path.join(snapshotDir, name));
}

async function loadSnapshots(studyRoot) {
  const files = await listSnapshotFiles(studyRoot);
  const snapshots = [];
  for (const filePath of files) {
    snapshots.push(JSON.parse(await fs.readFile(filePath, "utf8")));
  }
  return snapshots;
}

function simplifyRow(row, rank) {
  const aggregate = row?.continuation_live?.aggregate ?? {};
  const scores = row?.scores ?? {};
  return {
    pool: row.pool,
    rank,
    best_side: row?.continuation_live?.best_side ?? null,
    shock_count: row?.continuation_live?.shock_count ?? null,
    latest_timestamp: row?.continuation_live?.latest_timestamp ?? null,
    continuation_live_score: scores.continuation_live_score ?? null,
    continuation_deployable_score: scores.continuation_deployable_score ?? null,
    same_direction_next1_rate: aggregate.sameDirectionNext1Rate ?? null,
    price_continuation_next1_rate: aggregate.priceContinuationNext1Rate ?? null,
    majority_same_direction_next3_rate: aggregate.majoritySameDirectionNext3Rate ?? null,
    median_max_favorable_move_bps: aggregate.medianMaxFavorableMoveBps ?? null,
    median_max_adverse_move_bps: aggregate.medianMaxAdverseMoveBps ?? null,
  };
}

function stabilityLabel(stats) {
  if (
    stats.rank_range === 0 &&
    stats.best_side_flips === 0 &&
    stats.live_score_range <= 0.05 &&
    stats.same_direction_rate_range <= 0.05 &&
    stats.price_continuation_rate_range <= 0.05
  ) {
    return "stable";
  }
  if (
    stats.rank_range <= 1 &&
    stats.best_side_flips <= 1 &&
    stats.live_score_range <= 0.2 &&
    stats.same_direction_rate_range <= 0.15 &&
    stats.price_continuation_rate_range <= 0.15
  ) {
    return "moderate_drift";
  }
  return "unstable";
}

function computeStudySummary(config, baselineRows, snapshots) {
  const baselineRankMap = new Map(
    baselineRows.map((row, index) => [row.pool, index + 1]),
  );
  const candidatePools = [...new Set(baselineRows.map((row) => row.pool))];
  const snapshotOrders = snapshots.map((snapshot) => snapshot.ranking_order);
  const sameRankOrderAllSnapshots = snapshotOrders.every(
    (order) => JSON.stringify(order) === JSON.stringify(snapshotOrders[0] ?? []),
  );
  const poolStats = [];

  for (const pool of candidatePools) {
    const series = [];
    for (const snapshot of snapshots) {
      const row = snapshot.rows.find((item) => item.pool === pool);
      if (!row) continue;
      const rank = snapshot.ranking_order.indexOf(pool) + 1;
      const aggregate = row?.continuation_live?.aggregate ?? {};
      const scores = row?.scores ?? {};
      series.push({
        captured_at: snapshot.captured_at,
        rank,
        best_side: row?.continuation_live?.best_side ?? null,
        shock_count: row?.continuation_live?.shock_count ?? null,
        latest_timestamp: row?.continuation_live?.latest_timestamp ?? null,
        live_score: scores.continuation_live_score ?? null,
        deployable_score: scores.continuation_deployable_score ?? null,
        same_direction_next1_rate: aggregate.sameDirectionNext1Rate ?? null,
        price_continuation_next1_rate: aggregate.priceContinuationNext1Rate ?? null,
        majority_same_direction_next3_rate:
          aggregate.majoritySameDirectionNext3Rate ?? null,
      });
    }

    const ranks = series.map((row) => row.rank).filter(Number.isFinite);
    const liveScores = series
      .map((row) => row.live_score)
      .filter((value) => Number.isFinite(value));
    const deployableScores = series
      .map((row) => row.deployable_score)
      .filter((value) => Number.isFinite(value));
    const sameDirectionRates = series
      .map((row) => row.same_direction_next1_rate)
      .filter((value) => Number.isFinite(value));
    const priceContinuationRates = series
      .map((row) => row.price_continuation_next1_rate)
      .filter((value) => Number.isFinite(value));
    const majorityRates = series
      .map((row) => row.majority_same_direction_next3_rate)
      .filter((value) => Number.isFinite(value));
    const bestSides = series.map((row) => row.best_side);
    let bestSideFlips = 0;
    for (let i = 1; i < bestSides.length; i += 1) {
      if (bestSides[i] !== bestSides[i - 1]) {
        bestSideFlips += 1;
      }
    }
    const stats = {
      pool,
      pair: baselineRows.find((row) => row.pool === pool)?.pair ?? null,
      baseline_rank: baselineRankMap.get(pool) ?? null,
      snapshot_count: series.length,
      avg_rank: round(average(ranks), 4),
      rank_stddev: round(sampleStdDev(ranks), 4),
      rank_range: range(ranks),
      top1_hits: series.filter((row) => row.rank === 1).length,
      top3_hits: series.filter((row) => row.rank <= 3).length,
      best_side_mode: bestSides[0] ?? null,
      best_side_unique: [...new Set(bestSides.filter(Boolean))],
      best_side_flips: bestSideFlips,
      live_score_avg: round(average(liveScores), 4),
      live_score_range: round(range(liveScores), 4),
      deployable_score_avg: round(average(deployableScores), 4),
      deployable_score_range: round(range(deployableScores), 4),
      same_direction_rate_avg: round(average(sameDirectionRates), 4),
      same_direction_rate_range: round(range(sameDirectionRates), 4),
      price_continuation_rate_avg: round(average(priceContinuationRates), 4),
      price_continuation_rate_range: round(range(priceContinuationRates), 4),
      majority_rate_avg: round(average(majorityRates), 4),
      majority_rate_range: round(range(majorityRates), 4),
      latest_timestamp_unique_count: new Set(
        series.map((row) => row.latest_timestamp).filter(Boolean),
      ).size,
      stability: "pending",
    };
    stats.stability = stabilityLabel(stats);
    poolStats.push(stats);
  }

  const stablePools = poolStats.filter((row) => row.stability === "stable").length;
  const unstablePools = poolStats.filter((row) => row.stability === "unstable").length;
  const freshDataObserved = poolStats.some((row) => row.latest_timestamp_unique_count > 1);

  let studyStability = "materially_changed";
  if (sameRankOrderAllSnapshots && unstablePools === 0 && stablePools === poolStats.length) {
    studyStability = "highly_consistent";
  } else if (unstablePools === 0 && poolStats.every((row) => row.rank_range <= 1)) {
    studyStability = "consistent_with_moderate_drift";
  }

  return {
    generated_at: new Date().toISOString(),
    study_label: config.label,
    iterations_captured: snapshots.length,
    interval_seconds: config.interval_seconds,
    candidate_pool_count: candidatePools.length,
    same_rank_order_all_snapshots: sameRankOrderAllSnapshots,
    fresh_data_observed: freshDataObserved,
    stable_pool_count: stablePools,
    unstable_pool_count: unstablePools,
    stability_classification: studyStability,
    snapshot_timeline: snapshots.map((snapshot) => ({
      snapshot_number: snapshot.snapshot_number,
      captured_at: snapshot.captured_at,
      ranking_order: snapshot.ranking_order,
      top_pool: snapshot.ranking_order[0] ?? null,
    })),
    pools: poolStats,
  };
}

function renderSummaryMarkdown(config, summary, snapshots) {
  const lines = [];
  lines.push(`# Continuation Persistence Study`);
  lines.push("");
  lines.push(`- Label: ${config.label}`);
  lines.push(`- Study root: ${config.study_root}`);
  lines.push(`- Snapshots captured: ${summary.iterations_captured}`);
  lines.push(`- Interval seconds: ${config.interval_seconds}`);
  lines.push(`- Candidate pools: ${summary.candidate_pool_count}`);
  lines.push(`- Stability classification: ${summary.stability_classification}`);
  lines.push(`- Same rank order across snapshots: ${summary.same_rank_order_all_snapshots}`);
  lines.push(`- Fresh market updates observed: ${summary.fresh_data_observed}`);
  lines.push("");
  lines.push("## Snapshot Timeline");
  lines.push("");
  for (const snapshot of summary.snapshot_timeline) {
    lines.push(
      `- #${snapshot.snapshot_number} at ${snapshot.captured_at}: ${snapshot.ranking_order.join(" > ")}`,
    );
  }
  lines.push("");
  lines.push("## Pool Stability");
  lines.push("");
  for (const row of summary.pools) {
    lines.push(
      `- ${row.pool}: ${row.stability}, avg rank ${row.avg_rank}, rank range ${row.rank_range}, ` +
        `best-side flips ${row.best_side_flips}, live score range ${row.live_score_range}, ` +
        `same-direction range ${row.same_direction_rate_range}, price-continuation range ${row.price_continuation_rate_range}`,
    );
  }
  lines.push("");
  lines.push("## Latest Snapshot");
  lines.push("");
  const latest = snapshots[snapshots.length - 1];
  if (latest) {
    for (const [index, row] of latest.rows.entries()) {
      const aggregate = row?.continuation_live?.aggregate ?? {};
      const scores = row?.scores ?? {};
      lines.push(
        `- ${index + 1}. ${row.pool}: deployable ${scores.continuation_deployable_score}, live ${scores.continuation_live_score}, ` +
          `best side ${row?.continuation_live?.best_side}, same-direction ${aggregate.sameDirectionNext1Rate}, ` +
          `price continuation ${aggregate.priceContinuationNext1Rate}`,
      );
    }
  }
  return `${lines.join("\n")}\n`;
}

async function writeJsonl(filePath, rows) {
  const body = rows.map((row) => JSON.stringify(row)).join("\n");
  await fs.writeFile(filePath, `${body}${body ? "\n" : ""}`, "utf8");
}

function timestampForPath(date = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return [
    date.getFullYear(),
    pad(date.getMonth() + 1),
    pad(date.getDate()),
    "_",
    pad(date.getHours()),
    pad(date.getMinutes()),
    pad(date.getSeconds()),
  ].join("");
}

async function rerankRows(baseRows, apiKey, options, capturedAtIso) {
  const nowMs = Date.parse(capturedAtIso);
  const ranked = [];
  for (const row of baseRows) {
    const transactions = await fetchPoolTransactions(row.pool, apiKey);
    const trades = transactions
      .map((tx) => classifyTrade(tx))
      .filter(Boolean);
    const live = analyzePool(row, trades, options, nowMs);
    ranked.push(mergeRowWithLive(row, live, capturedAtIso));
  }
  ranked.sort((a, b) => {
    const scoreDelta =
      safeFloat(b?.scores?.continuation_deployable_score) -
      safeFloat(a?.scores?.continuation_deployable_score);
    if (scoreDelta !== 0) return scoreDelta;
    const liveDelta =
      safeFloat(b?.scores?.continuation_live_score) -
      safeFloat(a?.scores?.continuation_live_score);
    if (liveDelta !== 0) return liveDelta;
    const shockDelta =
      safeInt(b?.continuation_live?.shock_count) - safeInt(a?.continuation_live?.shock_count);
    if (shockDelta !== 0) return shockDelta;
    return habitatScore(b) - habitatScore(a);
  });
  return ranked;
}

function computeSnapshotSummary(rows) {
  const top = rows[0];
  return {
    candidate_pool_count: rows.length,
    top_pool: top?.pool ?? null,
    top_score: top?.scores?.continuation_deployable_score ?? null,
    stable_best_side_bias: rows.every(
      (row) => row?.continuation_live?.best_side === rows[0]?.continuation_live?.best_side,
    ),
    statuses: [...new Set(rows.map((row) => row?.continuation_live?.status ?? "unknown"))],
  };
}

async function sleep(ms) {
  await new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function main() {
  const argMap = parseArgs(process.argv.slice(2));
  const now = new Date();
  const label = stringOption(argMap, "label", "continuation_persistence");
  const intervalSeconds = Math.max(0, numberOption(argMap, "interval-seconds", 20));
  const iterations = Math.max(1, numberOption(argMap, "iterations", 3));
  const topN = Math.max(1, numberOption(argMap, "top-n", 5));
  const maxHabitats = Math.max(1, numberOption(argMap, "max-habitats", 20));
  const shockQuantile = clamp(numberOption(argMap, "shock-quantile", 0.8), 0, 1);
  const minShockEpisodes = Math.max(1, numberOption(argMap, "min-shock-episodes", 4));
  const appendMode = boolOption(argMap, "append", false);
  const envPath = stringOption(argMap, "env-path", DEFAULT_ENV_PATH);
  const sourceRowsPath = stringOption(argMap, "rows-path", DEFAULT_ROWS_PATH);
  const defaultStudyRoot = path.join(DEFAULT_STUDIES_ROOT, `${label}_${timestampForPath(now)}`);
  const studyRoot = path.resolve(stringOption(argMap, "study-root", defaultStudyRoot));
  const configPath = path.join(studyRoot, "study_config.json");
  const baselineCopyPath = path.join(studyRoot, "baseline_rows.jsonl");
  const snapshotsDir = path.join(studyRoot, "snapshots");

  const env = parseEnv(await fs.readFile(envPath, "utf8"));
  const heliusRpcUrl = env.get("HELIUS_RPC_URL");
  if (!heliusRpcUrl) {
    throw new Error(`Missing HELIUS_RPC_URL in ${envPath}`);
  }
  const apiKey = extractApiKey(heliusRpcUrl);

  await fs.mkdir(studyRoot, { recursive: true });
  await fs.mkdir(snapshotsDir, { recursive: true });

  const existingConfig = await loadStudyConfig(studyRoot);
  const previousSnapshots = appendMode ? await loadSnapshots(studyRoot) : [];
  if (!appendMode && previousSnapshots.length) {
    throw new Error(`Study root already contains snapshots: ${studyRoot}`);
  }

  const baseRows = appendMode && (await fileExists(baselineCopyPath))
    ? await loadJsonlRows(baselineCopyPath)
    : (await loadJsonlRows(sourceRowsPath)).slice(0, maxHabitats);

  const config = existingConfig ?? {
    label,
    created_at: new Date().toISOString(),
    study_root: studyRoot,
    source_rows_path: path.resolve(sourceRowsPath),
    interval_seconds: intervalSeconds,
    top_n: topN,
    max_habitats: maxHabitats,
    shock_quantile: shockQuantile,
    min_shock_episodes: minShockEpisodes,
    append_mode: appendMode,
  };

  const effectiveOptions = {
    shockQuantile: shockQuantile,
    minShockEpisodes,
    topN,
  };

  if (!(await fileExists(baselineCopyPath))) {
    await writeJsonl(baselineCopyPath, baseRows);
  }
  await fs.writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");

  const snapshots = [...previousSnapshots];
  let nextSnapshotNumber = previousSnapshots.length + 1;

  for (let iteration = 0; iteration < iterations; iteration += 1) {
    const capturedAtIso = new Date().toISOString();
    const ranked = await rerankRows(baseRows, apiKey, effectiveOptions, capturedAtIso);
    const selected = ranked.slice(0, topN);
    const snapshot = {
      snapshot_number: nextSnapshotNumber,
      captured_at: capturedAtIso,
      ranking_order: ranked.map((row) => row.pool),
      rows: ranked,
      selected,
      compact_rows: ranked.map((row, index) => simplifyRow(row, index + 1)),
      summary: computeSnapshotSummary(ranked),
    };
    const fileName = `${String(nextSnapshotNumber).padStart(3, "0")}_${timestampForPath(new Date(capturedAtIso))}.json`;
    await fs.writeFile(
      path.join(snapshotsDir, fileName),
      `${JSON.stringify(snapshot, null, 2)}\n`,
      "utf8",
    );
    snapshots.push(snapshot);
    nextSnapshotNumber += 1;
    if (iteration + 1 < iterations && intervalSeconds > 0) {
      await sleep(intervalSeconds * 1000);
    }
  }

  const latest = snapshots[snapshots.length - 1];
  await writeJsonl(path.join(studyRoot, "latest_rows.jsonl"), latest?.rows ?? []);
  await fs.writeFile(
    path.join(studyRoot, "latest_selected_habitats.json"),
    `${JSON.stringify(latest?.selected ?? [], null, 2)}\n`,
    "utf8",
  );

  const summary = computeStudySummary(config, baseRows, snapshots);
  await fs.writeFile(
    path.join(studyRoot, "study_summary.json"),
    `${JSON.stringify(summary, null, 2)}\n`,
    "utf8",
  );
  await fs.writeFile(
    path.join(studyRoot, "study_summary.md"),
    renderSummaryMarkdown(config, summary, snapshots),
    "utf8",
  );

  console.log(
    JSON.stringify(
      {
        study_root: studyRoot,
        iterations_captured: summary.iterations_captured,
        stability_classification: summary.stability_classification,
        same_rank_order_all_snapshots: summary.same_rank_order_all_snapshots,
        fresh_data_observed: summary.fresh_data_observed,
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
