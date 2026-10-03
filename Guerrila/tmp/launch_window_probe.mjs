import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const ENV_PATH = path.join(ROOT, ".env");
const OUT_PATH = path.join(ROOT, "tmp", "launch_window_probe_result.json");

const WSOL = "So11111111111111111111111111111111111111112";
const PUMP_AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function parseArgs(argv) {
  const out = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const raw = argv[i];
    if (!raw.startsWith("--")) continue;
    const eq = raw.indexOf("=");
    if (eq >= 0) {
      out.set(raw.slice(2, eq), raw.slice(eq + 1));
    } else {
      const next = argv[i + 1];
      if (next && !next.startsWith("--")) {
        out.set(raw.slice(2), next);
        i += 1;
      } else {
        out.set(raw.slice(2), "true");
      }
    }
  }
  return out;
}

function numberOpt(opts, key, fallback) {
  const parsed = Number(opts.get(key));
  return Number.isFinite(parsed) ? parsed : fallback;
}

function parseEnv(text) {
  const env = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim() || line.trim().startsWith("#")) continue;
    const idx = line.indexOf("=");
    if (idx <= 0) continue;
    let value = line.slice(idx + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    env.set(line.slice(0, idx).trim(), value);
  }
  return env;
}

function pct(value) {
  return Number.isFinite(value) ? Number((value * 100).toFixed(2)) : null;
}

function round(value, digits = 6) {
  return Number.isFinite(value) ? Number(value.toFixed(digits)) : null;
}

function quantile(values, q) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = (sorted.length - 1) * Math.max(0, Math.min(1, q));
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return sorted[lo];
  return sorted[lo] * (hi - idx) + sorted[hi] * (idx - lo);
}

function summarize(values) {
  const clean = values.filter(Number.isFinite);
  return {
    n: clean.length,
    p25: round(quantile(clean, 0.25), 6),
    median: round(quantile(clean, 0.5), 6),
    p75: round(quantile(clean, 0.75), 6),
  };
}

async function heliusAddressTxs(apiKey, address, { before = null, limit = 100, type = "SWAP" } = {}) {
  const params = new URLSearchParams({
    "api-key": apiKey,
    limit: String(limit),
  });
  if (type) params.set("type", type);
  if (before) params.set("before", before);
  const url = `https://api.helius.xyz/v0/addresses/${address}/transactions?${params}`;
  let lastError = null;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const res = await fetch(url, {
      headers: {
        accept: "application/json",
        "user-agent": "shadow-launch-window-probe/1.0",
      },
    });
    if (res.ok) return res.json();
    lastError = new Error(`${res.status} ${res.statusText}: ${await res.text()}`);
    if (![429, 500, 502, 503, 504].includes(res.status)) throw lastError;
    await sleep(500 * 2 ** attempt);
  }
  throw lastError;
}

function pumpIx(tx) {
  return (tx.instructions || []).find((ix) => ix.programId === PUMP_AMM) || null;
}

function parsePumpSwap(tx) {
  if (tx.type !== "SWAP" || tx.source !== "PUMP_AMM") return null;
  const ix = pumpIx(tx);
  if (!ix || !Array.isArray(ix.accounts) || ix.accounts.length < 9) return null;

  const pool = ix.accounts[0];
  const mintA = ix.accounts[3];
  const mintB = ix.accounts[4];
  const mint = mintA === WSOL ? mintB : mintB === WSOL ? mintA : null;
  if (!pool || !mint) return null;

  const transfers = tx.tokenTransfers || [];
  const solIn = transfers
    .filter((t) => t.mint === WSOL && t.toUserAccount === pool)
    .reduce((sum, t) => sum + Number(t.tokenAmount || 0), 0);
  const solOut = transfers
    .filter((t) => t.mint === WSOL && t.fromUserAccount === pool)
    .reduce((sum, t) => sum + Number(t.tokenAmount || 0), 0);
  const tokenIn = transfers
    .filter((t) => t.mint === mint && t.toUserAccount === pool)
    .reduce((sum, t) => sum + Number(t.tokenAmount || 0), 0);
  const tokenOut = transfers
    .filter((t) => t.mint === mint && t.fromUserAccount === pool)
    .reduce((sum, t) => sum + Number(t.tokenAmount || 0), 0);

  let side = null;
  let solAmount = 0;
  let tokenAmount = 0;
  if (solIn > 0 && tokenOut > 0) {
    side = "buy";
    solAmount = solIn;
    tokenAmount = tokenOut;
  } else if (tokenIn > 0 && solOut > 0) {
    side = "sell";
    solAmount = solOut;
    tokenAmount = tokenIn;
  } else {
    return null;
  }

  const priceSol = tokenAmount > 0 ? solAmount / tokenAmount : null;
  if (!(priceSol > 0)) return null;

  return {
    signature: tx.signature,
    slot: tx.slot,
    timestamp: tx.timestamp,
    pool,
    mint,
    side,
    solAmount,
    tokenAmount,
    priceSol,
  };
}

async function fetchRecentProgramSwaps(apiKey, pages) {
  const rows = [];
  let before = null;
  for (let page = 0; page < pages; page += 1) {
    const batch = await heliusAddressTxs(apiKey, PUMP_AMM, { before, limit: 100, type: "SWAP" });
    if (!Array.isArray(batch) || batch.length === 0) break;
    for (const tx of batch) {
      const row = parsePumpSwap(tx);
      if (row) rows.push(row);
    }
    before = batch.at(-1)?.signature;
    if (!before || batch.length < 100) break;
    await sleep(125);
  }
  return rows;
}

async function fetchPoolHistory(apiKey, pool, maxPages) {
  const parsed = [];
  const seen = new Set();
  let before = null;
  let reachedStart = false;
  for (let page = 0; page < maxPages; page += 1) {
    const batch = await heliusAddressTxs(apiKey, pool, { before, limit: 100, type: "SWAP" });
    if (!Array.isArray(batch) || batch.length === 0) {
      reachedStart = true;
      break;
    }
    for (const tx of batch) {
      const row = parsePumpSwap(tx);
      if (row && row.pool === pool && !seen.has(row.signature)) {
        parsed.push(row);
        seen.add(row.signature);
      }
    }
    before = batch.at(-1)?.signature;
    if (!before || batch.length < 100) {
      reachedStart = true;
      break;
    }
    await sleep(125);
  }
  parsed.sort((a, b) => a.slot - b.slot || a.timestamp - b.timestamp);
  return { rows: parsed, reachedStart };
}

function analyzePool(history, { earlyTrades, windowSeconds, eps }) {
  const first = history[0];
  const windowRows = history
    .filter((row) => row.timestamp <= first.timestamp + windowSeconds)
    .slice(0, earlyTrades);
  if (windowRows.length < 3) return null;

  const prices = windowRows.map((row) => row.priceSol);
  const p0 = prices[0];
  const p1 = prices[1];
  const pLast = prices.at(-1);
  const peak = Math.max(...prices);
  const trough = Math.min(...prices);
  const maxPeakIdx = prices.findIndex((p) => p === peak);

  const stepReturns = [];
  for (let i = 1; i < prices.length; i += 1) {
    stepReturns.push(prices[i] / prices[i - 1] - 1);
  }

  return {
    pool: first.pool,
    mint: first.mint,
    firstTimestamp: first.timestamp,
    firstSlot: first.slot,
    tradeCountObserved: history.length,
    tradeCountWindow: windowRows.length,
    firstSide: first.side,
    firstSolAmount: round(first.solAmount, 9),
    buyShareWindow: round(windowRows.filter((row) => row.side === "buy").length / windowRows.length, 4),
    p0Sol: p0,
    nextReturn: p1 / p0 - 1,
    peakReturn: peak / p0 - 1,
    lastReturn: pLast / p0 - 1,
    troughReturn: trough / p0 - 1,
    priceEverUp: peak > p0 * (1 + eps),
    nextUp: p1 > p0 * (1 + eps),
    endsUp: pLast > p0 * (1 + eps),
    peakTradeIndex: maxPeakIdx,
    secondsToPeak: maxPeakIdx >= 0 ? windowRows[maxPeakIdx].timestamp - first.timestamp : null,
    positiveStepShare: round(stepReturns.filter((ret) => ret > eps).length / stepReturns.length, 4),
  };
}

const opts = parseArgs(process.argv.slice(2));
const recentPages = numberOpt(opts, "recent-pages", 5);
const poolLimit = numberOpt(opts, "pool-limit", 80);
const historyPages = numberOpt(opts, "history-pages", 5);
const maxAgeSeconds = numberOpt(opts, "max-age-seconds", 1800);
const earlyTrades = numberOpt(opts, "early-trades", 20);
const windowSeconds = numberOpt(opts, "window-seconds", 60);
const eps = numberOpt(opts, "eps", 0.0025);

const env = parseEnv(await fs.readFile(ENV_PATH, "utf8"));
const rpcUrl = env.get("HELIUS_RPC_URL");
const apiKey = rpcUrl ? new URL(rpcUrl).searchParams.get("api-key") : env.get("HELIUS_LASERSTREAM_API_KEY");
if (!apiKey) throw new Error("No Helius API key found in HELIUS_RPC_URL or HELIUS_LASERSTREAM_API_KEY");

const recentRows = await fetchRecentProgramSwaps(apiKey, recentPages);
let observedMaxTs = Math.max(Math.floor(Date.now() / 1000), ...recentRows.map((row) => row.timestamp));
const uniquePools = [...new Map(recentRows.map((row) => [row.pool, row])).values()].slice(0, poolLimit);

const analyses = [];
const skipped = [];
for (const [idx, seed] of uniquePools.entries()) {
  const { rows, reachedStart } = await fetchPoolHistory(apiKey, seed.pool, historyPages);
  const sameMintRows = rows.filter((row) => row.mint === seed.mint);
  if (!reachedStart) {
    skipped.push({ pool: seed.pool, mint: seed.mint, reason: "history_truncated", rows: sameMintRows.length });
    continue;
  }
  if (sameMintRows.length < 3) {
    skipped.push({ pool: seed.pool, mint: seed.mint, reason: "too_few_swaps", rows: sameMintRows.length });
    continue;
  }
  observedMaxTs = Math.max(
    observedMaxTs,
    Math.floor(Date.now() / 1000),
    ...sameMintRows.map((row) => row.timestamp),
  );
  const ageSeconds = Math.max(0, observedMaxTs - sameMintRows[0].timestamp);
  if (ageSeconds > maxAgeSeconds) {
    skipped.push({
      pool: seed.pool,
      mint: seed.mint,
      reason: "older_than_window",
      ageSeconds,
      rows: sameMintRows.length,
    });
    continue;
  }
  const analysis = analyzePool(sameMintRows, { earlyTrades, windowSeconds, eps });
  if (!analysis) {
    skipped.push({ pool: seed.pool, mint: seed.mint, reason: "too_few_window_swaps", rows: sameMintRows.length });
    continue;
  }
  analysis.ageSecondsAtScan = ageSeconds;
  analyses.push(analysis);
  await sleep(idx % 10 === 9 ? 300 : 50);
}

const n = analyses.length;
const rates = {
  priceEverUpRate: n ? analyses.filter((a) => a.priceEverUp).length / n : null,
  nextUpRate: n ? analyses.filter((a) => a.nextUp).length / n : null,
  endsUpRate: n ? analyses.filter((a) => a.endsUp).length / n : null,
};

const result = {
  generatedAt: new Date().toISOString(),
  method: {
    venue: "PumpSwap / PUMP_AMM via Helius enhanced transactions",
    recentPages,
    poolLimit,
    historyPages,
    maxAgeSeconds,
    earlyTrades,
    windowSeconds,
    epsilonReturn: eps,
    priceDefinition: "SOL moved by pool divided by non-WSOL token moved by pool per swap",
  },
  scannedProgramSwaps: recentRows.length,
  uniquePoolsSeen: uniquePools.length,
  sampledLaunchPools: n,
  skippedCount: skipped.length,
  rates: Object.fromEntries(Object.entries(rates).map(([k, v]) => [k, v == null ? null : round(v, 4)])),
  returnsPct: {
    next: summarize(analyses.map((a) => pct(a.nextReturn))),
    peak: summarize(analyses.map((a) => pct(a.peakReturn))),
    last: summarize(analyses.map((a) => pct(a.lastReturn))),
    trough: summarize(analyses.map((a) => pct(a.troughReturn))),
  },
  positiveStepShare: summarize(analyses.map((a) => a.positiveStepShare)),
  buyShareWindow: summarize(analyses.map((a) => a.buyShareWindow)),
  examples: analyses
    .slice()
    .sort((a, b) => b.peakReturn - a.peakReturn)
    .slice(0, 8)
    .map((a) => ({
      pool: a.pool,
      mint: a.mint,
      ageSecondsAtScan: a.ageSecondsAtScan,
      tradeCountWindow: a.tradeCountWindow,
      firstSide: a.firstSide,
      firstSolAmount: a.firstSolAmount,
      nextReturnPct: pct(a.nextReturn),
      peakReturnPct: pct(a.peakReturn),
      lastReturnPct: pct(a.lastReturn),
      troughReturnPct: pct(a.troughReturn),
      secondsToPeak: a.secondsToPeak,
    })),
  analyses,
  skipped: skipped.slice(0, 25),
};

await fs.writeFile(OUT_PATH, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify({
  outPath: OUT_PATH,
  sampledLaunchPools: result.sampledLaunchPools,
  rates: result.rates,
  returnsPct: result.returnsPct,
  examples: result.examples.slice(0, 3),
}, null, 2));
