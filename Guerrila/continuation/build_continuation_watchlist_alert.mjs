import fs from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";

const ROOT = process.cwd();
const DISCOVERY_SCRIPT = path.join(ROOT, "continuation", "discover_active_continuation_now.mjs");
const DISCOVERY_DIR = path.join(ROOT, "continuation", "active_discovery");
const WATCHLIST_DIR = path.join(ROOT, "continuation", "watchlist");

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

function boolOption(options, key, fallback = false) {
  const value = options.get(key);
  if (value == null) return fallback;
  return !["0", "false", "no", "off"].includes(String(value).toLowerCase());
}

function round(value, digits = 6) {
  return Number.isFinite(value) ? Number(value.toFixed(digits)) : value;
}

function timestampTag(date = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}_${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`;
}

async function fileExists(filePath) {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function readJson(filePath, fallback = null) {
  try {
    return JSON.parse(await fs.readFile(filePath, "utf8"));
  } catch {
    return fallback;
  }
}

function maybeScore(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function materialScore(row) {
  return maybeScore(row.continuation_live_score);
}

function currentActivity(row) {
  return maybeScore(row.current_activity_score);
}

async function runNodeScript(scriptPath, args) {
  await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [scriptPath, ...args], {
      cwd: ROOT,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += String(chunk);
    });
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve({ stdout, stderr });
        return;
      }
      reject(
        new Error(
          `Command failed (${code}) for ${path.basename(scriptPath)}\n${stderr || stdout}`,
        ),
      );
    });
  });
}

function buildDiscoveryArgs(config) {
  return [
    "--top-universe",
    String(config.discovery_top_universe),
    "--top-output",
    String(config.discovery_top_output),
    "--min-liquidity-usd",
    String(config.min_liquidity_usd),
    "--min-txns-m5",
    String(config.min_txns_m5),
    "--freshness-hours",
    String(config.freshness_hours),
    "--min-shock-episodes",
    String(config.min_shock_episodes),
    "--strong-continuation-score",
    String(config.min_continuation_score),
  ];
}

function buildWatchlistEntries(report, config) {
  const scoredPools = Array.isArray(report.scored_pools) ? report.scored_pools : [];
  const filtered = scoredPools
    .filter((row) => row.continuation_status === "ok")
    .filter((row) => Number.isFinite(row.latest_age_hours))
    .filter((row) => row.latest_age_hours <= config.freshness_hours)
    .filter((row) => (row.shock_count || 0) >= config.min_shock_episodes)
    .filter((row) => materialScore(row) >= config.min_continuation_score)
    .sort((a, b) => {
      const scoreDelta = materialScore(b) - materialScore(a);
      if (scoreDelta !== 0) return scoreDelta;
      const activityDelta = currentActivity(b) - currentActivity(a);
      if (activityDelta !== 0) return activityDelta;
      return (b.tx_count_5m || 0) - (a.tx_count_5m || 0);
    })
    .slice(0, config.max_candidates);

  return filtered.map((row, index) => ({
    rank: index + 1,
    pool: row.pool,
    venue: row.venue,
    pair: `${row.base_symbol}/${row.quote_symbol}`,
    base_symbol: row.base_symbol,
    quote_symbol: row.quote_symbol,
    best_side: row.best_side,
    latest_age_hours: row.latest_age_hours,
    latest_timestamp: row.latest_timestamp,
    tx_count_5m: row.tx_count_5m,
    liquidity_usd: row.liquidity_usd,
    volume_5m: row.volume_5m,
    current_activity_score: row.current_activity_score,
    continuation_live_score: row.continuation_live_score,
    shock_count: row.shock_count,
    same_direction_next1_rate: row?.aggregate?.sameDirectionNext1Rate ?? null,
    price_continuation_next1_rate: row?.aggregate?.priceContinuationNext1Rate ?? null,
    majority_same_direction_next3_rate:
      row?.aggregate?.majoritySameDirectionNext3Rate ?? null,
  }));
}

function compareWatchlists(previousEntries, nextEntries, config) {
  const previousByPool = new Map(previousEntries.map((row) => [row.pool, row]));
  const nextByPool = new Map(nextEntries.map((row) => [row.pool, row]));

  const newPools = nextEntries.filter((row) => !previousByPool.has(row.pool));
  const droppedPools = previousEntries.filter((row) => !nextByPool.has(row.pool));
  const sharedPools = nextEntries.filter((row) => previousByPool.has(row.pool));

  const rankChanges = [];
  const bestSideFlips = [];
  const scoreJumps = [];
  for (const row of sharedPools) {
    const previous = previousByPool.get(row.pool);
    if (previous.rank !== row.rank) {
      rankChanges.push({
        pool: row.pool,
        from_rank: previous.rank,
        to_rank: row.rank,
      });
    }
    if (previous.best_side !== row.best_side) {
      bestSideFlips.push({
        pool: row.pool,
        from_side: previous.best_side,
        to_side: row.best_side,
      });
    }
    const delta = materialScore(row) - materialScore(previous);
    if (Math.abs(delta) >= config.alert_score_delta) {
      scoreJumps.push({
        pool: row.pool,
        delta_score: round(delta, 4),
        from_score: previous.continuation_live_score,
        to_score: row.continuation_live_score,
      });
    }
  }

  const previousTop = previousEntries[0] ?? null;
  const nextTop = nextEntries[0] ?? null;
  const topPoolChanged =
    (previousTop?.pool ?? null) !== (nextTop?.pool ?? null);

  const shouldAlert =
    newPools.length > 0 ||
    droppedPools.length > 0 ||
    topPoolChanged ||
    bestSideFlips.length > 0 ||
    rankChanges.some(
      (change) =>
        change.from_rank <= config.alert_rank_depth ||
        change.to_rank <= config.alert_rank_depth,
    ) ||
    scoreJumps.length > 0 ||
    (previousEntries.length === 0 && nextEntries.length > 0) ||
    (previousEntries.length > 0 && nextEntries.length === 0);

  return {
    previous_count: previousEntries.length,
    next_count: nextEntries.length,
    new_pools: newPools,
    dropped_pools: droppedPools,
    rank_changes: rankChanges,
    best_side_flips: bestSideFlips,
    score_jumps: scoreJumps,
    top_pool_changed: topPoolChanged,
    previous_top_pool: previousTop?.pool ?? null,
    next_top_pool: nextTop?.pool ?? null,
    should_alert: shouldAlert,
  };
}

function buildReasons(diff, watchlist, discoveryReport) {
  const reasons = [];
  if (watchlist.length === 0) {
    reasons.push("No live continuation candidates passed the watchlist filter.");
  }
  if (diff.new_pools.length) {
    reasons.push(`${diff.new_pools.length} new pool(s) entered the watchlist.`);
  }
  if (diff.dropped_pools.length) {
    reasons.push(`${diff.dropped_pools.length} pool(s) dropped from the watchlist.`);
  }
  if (diff.top_pool_changed) {
    reasons.push(`Top pool changed from ${diff.previous_top_pool || "none"} to ${diff.next_top_pool || "none"}.`);
  }
  if (diff.best_side_flips.length) {
    reasons.push(`${diff.best_side_flips.length} watchlist pool(s) flipped best-side bias.`);
  }
  if (diff.score_jumps.length) {
    reasons.push(`${diff.score_jumps.length} pool(s) moved materially in continuation score.`);
  }
  if (!reasons.length) {
    reasons.push("Watchlist composition and top ranks are unchanged.");
  }
  const activeCount = discoveryReport?.activity?.continuation_active_under_24h ?? 0;
  reasons.push(`${activeCount} active continuation-positive pool(s) were found in the current discovery universe.`);
  return reasons;
}

function renderWatchlistMarkdown(report) {
  const lines = [];
  lines.push("# Continuation Watchlist");
  lines.push("");
  lines.push(`- Generated at: ${report.generated_at}`);
  lines.push(`- Watchlist count: ${report.watchlist.length}`);
  lines.push(`- Discovery verdict: ${report.discovery_summary.verdict}`);
  lines.push(`- Active under 24h: ${report.discovery_summary.active_under_24h}`);
  lines.push(`- Continuation-positive under 24h: ${report.discovery_summary.continuation_active_under_24h}`);
  lines.push("");
  if (!report.watchlist.length) {
    lines.push("No pools currently pass the live watchlist filter.");
    return `${lines.join("\n")}\n`;
  }
  lines.push("## Candidates");
  lines.push("");
  for (const row of report.watchlist) {
    lines.push(
      `- ${row.rank}. ${row.pool}: ${row.pair}, ${row.venue}, best side ${row.best_side}, age ${row.latest_age_hours}h, continuation ${row.continuation_live_score}, activity ${row.current_activity_score}, tx5m ${row.tx_count_5m}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

function renderAlertMarkdown(alert) {
  const lines = [];
  lines.push("# Continuation Watchlist Alert");
  lines.push("");
  lines.push(`- Generated at: ${alert.generated_at}`);
  lines.push(`- Should alert: ${alert.should_alert}`);
  lines.push(`- Current watchlist count: ${alert.current_watchlist_count}`);
  lines.push(`- Previous watchlist count: ${alert.previous_watchlist_count}`);
  lines.push("");
  lines.push("## Reasons");
  lines.push("");
  for (const reason of alert.reasons) {
    lines.push(`- ${reason}`);
  }
  if (alert.current_top) {
    lines.push("");
    lines.push("## Current Top");
    lines.push("");
    lines.push(
      `- ${alert.current_top.pool}: ${alert.current_top.pair}, ${alert.current_top.venue}, best side ${alert.current_top.best_side}, continuation ${alert.current_top.continuation_live_score}, age ${alert.current_top.latest_age_hours}h`,
    );
  }
  if (alert.diff.new_pools.length) {
    lines.push("");
    lines.push("## New Pools");
    lines.push("");
    for (const row of alert.diff.new_pools) {
      lines.push(
        `- ${row.pool}: rank ${row.rank}, ${row.pair}, ${row.venue}, side ${row.best_side}, continuation ${row.continuation_live_score}`,
      );
    }
  }
  if (alert.diff.dropped_pools.length) {
    lines.push("");
    lines.push("## Dropped Pools");
    lines.push("");
    for (const row of alert.diff.dropped_pools) {
      lines.push(
        `- ${row.pool}: previous rank ${row.rank}, ${row.pair}, ${row.venue}, side ${row.best_side}, continuation ${row.continuation_live_score}`,
      );
    }
  }
  return `${lines.join("\n")}\n`;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const config = {
    run_discovery: boolOption(options, "run-discovery", true),
    min_liquidity_usd: numberOption(options, "min-liquidity-usd", 25_000),
    min_txns_m5: numberOption(options, "min-txns-m5", 20),
    discovery_top_universe: numberOption(options, "discovery-top-universe", 30),
    discovery_top_output: numberOption(options, "discovery-top-output", 15),
    freshness_hours: numberOption(options, "freshness-hours", 1),
    min_shock_episodes: numberOption(options, "min-shock-episodes", 4),
    min_continuation_score: numberOption(options, "min-continuation-score", 2),
    max_candidates: numberOption(options, "max-candidates", 8),
    alert_rank_depth: numberOption(options, "alert-rank-depth", 5),
    alert_score_delta: numberOption(options, "alert-score-delta", 1),
  };

  if (config.run_discovery) {
    await runNodeScript(DISCOVERY_SCRIPT, buildDiscoveryArgs(config));
  }

  const discoveryPath = path.join(
    DISCOVERY_DIR,
    "latest_active_continuation_discovery.json",
  );
  const discoveryReport = await readJson(discoveryPath, null);
  if (!discoveryReport) {
    throw new Error(`Missing discovery report at ${discoveryPath}`);
  }

  const watchlist = buildWatchlistEntries(discoveryReport, config);
  const statePath = path.join(WATCHLIST_DIR, "state.json");
  const previousState = await readJson(statePath, { watchlist: [] });
  const previousWatchlist = Array.isArray(previousState.watchlist)
    ? previousState.watchlist
    : [];
  const diff = compareWatchlists(previousWatchlist, watchlist, config);
  const reasons = buildReasons(diff, watchlist, discoveryReport);
  const generatedAt = new Date().toISOString();

  const watchlistReport = {
    generated_at: generatedAt,
    config,
    discovery_summary: {
      generated_at: discoveryReport.generated_at,
      verdict: discoveryReport.verdict,
      active_under_24h: discoveryReport?.activity?.active_under_24h ?? 0,
      continuation_active_under_24h:
        discoveryReport?.activity?.continuation_active_under_24h ?? 0,
      helius_scored_pool_count: discoveryReport.helius_scored_pool_count ?? 0,
    },
    watchlist,
  };

  const alertReport = {
    generated_at: generatedAt,
    should_alert: diff.should_alert,
    reasons,
    previous_watchlist_count: diff.previous_count,
    current_watchlist_count: diff.next_count,
    current_top: watchlist[0] ?? null,
    diff,
  };

  const runDir = path.join(WATCHLIST_DIR, "runs", timestampTag(new Date()));
  await fs.mkdir(runDir, { recursive: true });

  const latestWatchlistJson = path.join(WATCHLIST_DIR, "latest_watchlist.json");
  const latestWatchlistMd = path.join(WATCHLIST_DIR, "latest_watchlist.md");
  const latestAlertJson = path.join(WATCHLIST_DIR, "latest_alert.json");
  const latestAlertMd = path.join(WATCHLIST_DIR, "latest_alert.md");
  const nextState = {
    generated_at: generatedAt,
    watchlist,
  };

  await Promise.all([
    fs.mkdir(WATCHLIST_DIR, { recursive: true }),
    fs.writeFile(
      path.join(runDir, "watchlist.json"),
      `${JSON.stringify(watchlistReport, null, 2)}\n`,
      "utf8",
    ),
    fs.writeFile(
      path.join(runDir, "watchlist.md"),
      renderWatchlistMarkdown(watchlistReport),
      "utf8",
    ),
    fs.writeFile(
      path.join(runDir, "alert.json"),
      `${JSON.stringify(alertReport, null, 2)}\n`,
      "utf8",
    ),
    fs.writeFile(
      path.join(runDir, "alert.md"),
      renderAlertMarkdown(alertReport),
      "utf8",
    ),
    fs.writeFile(
      latestWatchlistJson,
      `${JSON.stringify(watchlistReport, null, 2)}\n`,
      "utf8",
    ),
    fs.writeFile(
      latestWatchlistMd,
      renderWatchlistMarkdown(watchlistReport),
      "utf8",
    ),
    fs.writeFile(
      latestAlertJson,
      `${JSON.stringify(alertReport, null, 2)}\n`,
      "utf8",
    ),
    fs.writeFile(latestAlertMd, renderAlertMarkdown(alertReport), "utf8"),
    fs.writeFile(statePath, `${JSON.stringify(nextState, null, 2)}\n`, "utf8"),
  ]);

  console.log(
    JSON.stringify(
      {
        generated_at: generatedAt,
        watchlist_count: watchlist.length,
        should_alert: diff.should_alert,
        top_pool: watchlist[0]?.pool ?? null,
        latest_watchlist_json: latestWatchlistJson,
        latest_alert_json: latestAlertJson,
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
