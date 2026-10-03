import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const ENV_PATH = path.join(ROOT, ".env");
const OUT_PATH = path.join(ROOT, "tmp", "pumpswap_migration_3h_probe_result.json");

const PUMP_AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const WSOL = "So11111111111111111111111111111111111111112";

const CREATE_POOL_EVENT = [177, 49, 12, 210, 160, 118, 167, 116];
const BUY_EVENT = [103, 244, 82, 31, 44, 245, 119, 119];
const SELL_EVENT = [62, 47, 55, 10, 165, 3, 220, 42];

const B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function parseArgs(argv) {
  const out = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    const eq = token.indexOf("=");
    if (eq >= 0) {
      out.set(token.slice(2, eq), token.slice(eq + 1));
      continue;
    }
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      out.set(token.slice(2), next);
      i += 1;
    } else {
      out.set(token.slice(2), "true");
    }
  }
  return out;
}

function numberOpt(opts, key, fallback) {
  const parsed = Number(opts.get(key));
  return Number.isFinite(parsed) ? parsed : fallback;
}

function stringOpt(opts, key, fallback) {
  return opts.has(key) ? String(opts.get(key)) : fallback;
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

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function round(value, digits = 6) {
  return Number.isFinite(value) ? Number(value.toFixed(digits)) : null;
}

function pct(value) {
  return Number.isFinite(value) ? round(value * 100, 3) : null;
}

function quantile(values, q) {
  const clean = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!clean.length) return null;
  const idx = (clean.length - 1) * Math.max(0, Math.min(1, q));
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return clean[lo];
  return clean[lo] * (hi - idx) + clean[hi] * (idx - lo);
}

function summarize(values, digits = 6) {
  const clean = values.filter(Number.isFinite);
  return {
    n: clean.length,
    p25: round(quantile(clean, 0.25), digits),
    median: round(quantile(clean, 0.5), digits),
    p75: round(quantile(clean, 0.75), digits),
  };
}

function base58Encode(bytes) {
  let value = 0n;
  for (const byte of bytes) value = value * 256n + BigInt(byte);
  let out = "";
  while (value > 0n) {
    const mod = value % 58n;
    value /= 58n;
    out = B58_ALPHABET[Number(mod)] + out;
  }
  for (const byte of bytes) {
    if (byte === 0) out = `1${out}`;
    else break;
  }
  return out || "1";
}

function sameDiscriminator(buf, disc) {
  if (buf.length < disc.length) return false;
  return disc.every((byte, idx) => buf[idx] === byte);
}

class Cursor {
  constructor(buf) {
    this.buf = buf;
    this.o = 8;
  }

  u8() {
    const value = this.buf[this.o];
    this.o += 1;
    return value;
  }

  u16() {
    const value = this.buf.readUInt16LE(this.o);
    this.o += 2;
    return value;
  }

  u32() {
    const value = this.buf.readUInt32LE(this.o);
    this.o += 4;
    return value;
  }

  u64() {
    const value = this.buf.readBigUInt64LE(this.o);
    this.o += 8;
    return Number(value);
  }

  i64() {
    const value = this.buf.readBigInt64LE(this.o);
    this.o += 8;
    return Number(value);
  }

  bool() {
    const value = this.buf[this.o] !== 0;
    this.o += 1;
    return value;
  }

  pubkey() {
    const value = base58Encode(this.buf.subarray(this.o, this.o + 32));
    this.o += 32;
    return value;
  }

  string() {
    const len = this.u32();
    const value = this.buf.subarray(this.o, this.o + len).toString("utf8");
    this.o += len;
    return value;
  }
}

function ammPrice(row) {
  return row.poolBaseTokenReserves > 0 ? row.poolQuoteTokenReserves / row.poolBaseTokenReserves : null;
}

function decodeCreatePoolEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "create_pool",
    timestamp: c.i64(),
    index: c.u16(),
    creator: c.pubkey(),
    baseMint: c.pubkey(),
    quoteMint: c.pubkey(),
    baseMintDecimals: c.u8(),
    quoteMintDecimals: c.u8(),
    baseAmountIn: c.u64(),
    quoteAmountIn: c.u64(),
    poolBaseTokenReserves: c.u64(),
    poolQuoteTokenReserves: c.u64(),
    minimumLiquidity: c.u64(),
    initialLiquidity: c.u64(),
    lpTokenAmountOut: c.u64(),
    poolBump: c.u8(),
    pool: c.pubkey(),
    lpMint: c.pubkey(),
    userBaseTokenAccount: c.pubkey(),
    userQuoteTokenAccount: c.pubkey(),
    coinCreator: c.pubkey(),
    isMayhemMode: c.bool(),
  };
  row.price = ammPrice(row);
  return row;
}

function decodeBuyEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "buy",
    timestamp: c.i64(),
    baseAmountOut: c.u64(),
    maxQuoteAmountIn: c.u64(),
    userBaseTokenReserves: c.u64(),
    userQuoteTokenReserves: c.u64(),
    poolBaseTokenReserves: c.u64(),
    poolQuoteTokenReserves: c.u64(),
    quoteAmountIn: c.u64(),
    lpFeeBasisPoints: c.u64(),
    lpFee: c.u64(),
    protocolFeeBasisPoints: c.u64(),
    protocolFee: c.u64(),
    quoteAmountInWithLpFee: c.u64(),
    userQuoteAmountIn: c.u64(),
    pool: c.pubkey(),
    user: c.pubkey(),
    userBaseTokenAccount: c.pubkey(),
    userQuoteTokenAccount: c.pubkey(),
    protocolFeeRecipient: c.pubkey(),
    protocolFeeRecipientTokenAccount: c.pubkey(),
    coinCreator: c.pubkey(),
    coinCreatorFeeBasisPoints: c.u64(),
    coinCreatorFee: c.u64(),
    trackVolume: c.bool(),
    totalUnclaimedTokens: c.u64(),
    totalClaimedTokens: c.u64(),
    currentSolVolume: c.u64(),
    lastUpdateTimestamp: c.i64(),
    minBaseAmountOut: c.u64(),
    ixName: c.string(),
    cashbackFeeBasisPoints: c.u64(),
    cashback: c.u64(),
    buybackFeeBasisPoints: c.u64(),
    buybackFee: c.u64(),
  };
  row.price = ammPrice(row);
  return row;
}

function decodeSellEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "sell",
    timestamp: c.i64(),
    baseAmountIn: c.u64(),
    minQuoteAmountOut: c.u64(),
    userBaseTokenReserves: c.u64(),
    userQuoteTokenReserves: c.u64(),
    poolBaseTokenReserves: c.u64(),
    poolQuoteTokenReserves: c.u64(),
    quoteAmountOut: c.u64(),
    lpFeeBasisPoints: c.u64(),
    lpFee: c.u64(),
    protocolFeeBasisPoints: c.u64(),
    protocolFee: c.u64(),
    quoteAmountOutWithoutLpFee: c.u64(),
    userQuoteAmountOut: c.u64(),
    pool: c.pubkey(),
    user: c.pubkey(),
    userBaseTokenAccount: c.pubkey(),
    userQuoteTokenAccount: c.pubkey(),
    protocolFeeRecipient: c.pubkey(),
    protocolFeeRecipientTokenAccount: c.pubkey(),
    coinCreator: c.pubkey(),
    coinCreatorFeeBasisPoints: c.u64(),
    coinCreatorFee: c.u64(),
  };
  // Newer event tails may add fields. Pool reserves are already decoded above.
  row.price = ammPrice(row);
  return row;
}

function extractAmmEvents(rawTx) {
  const events = [];
  for (const line of rawTx?.meta?.logMessages || []) {
    let b64 = null;
    if (line.startsWith("Program data: ")) {
      b64 = line.slice("Program data: ".length).trim();
    } else if (line.startsWith(`Program return: ${PUMP_AMM} `)) {
      b64 = line.slice(`Program return: ${PUMP_AMM} `.length).trim();
    }
    if (!b64) continue;
    let buf;
    try {
      buf = Buffer.from(b64, "base64");
    } catch {
      continue;
    }
    try {
      if (sameDiscriminator(buf, CREATE_POOL_EVENT)) events.push(decodeCreatePoolEvent(buf));
      else if (sameDiscriminator(buf, BUY_EVENT)) events.push(decodeBuyEvent(buf));
      else if (sameDiscriminator(buf, SELL_EVENT)) events.push(decodeSellEvent(buf));
    } catch {
      // Ignore event versions this lightweight decoder cannot parse.
    }
  }
  return events;
}

function parseEnhancedPumpAmmSwap(tx) {
  const ix = (tx.instructions || []).find((item) => item.programId === PUMP_AMM);
  if (!ix || !Array.isArray(ix.accounts) || ix.accounts.length < 5) return null;
  const pool = ix.accounts[0];
  const baseMint = ix.accounts[3];
  const quoteMint = ix.accounts[4];
  if (!pool || !baseMint || !quoteMint) return null;
  return {
    pool,
    baseMint,
    quoteMint,
    mint: baseMint === WSOL ? quoteMint : baseMint,
    signature: tx.signature,
    timestamp: tx.timestamp,
    slot: tx.slot,
  };
}

async function httpJson(url) {
  let lastError = null;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const res = await fetch(url, {
      headers: {
        accept: "application/json",
        "user-agent": "shadow-pumpswap-migration-3h-probe/1.0",
      },
    });
    if (res.ok) return res.json();
    lastError = new Error(`${res.status} ${res.statusText}: ${await res.text()}`);
    if (![429, 500, 502, 503, 504].includes(res.status)) throw lastError;
    await sleep(400 * 2 ** attempt);
  }
  throw lastError;
}

async function heliusAddressTxs(apiKey, address, { limit = 100, before = null, type = null } = {}) {
  const params = new URLSearchParams({ "api-key": apiKey, limit: String(limit) });
  if (before) params.set("before", before);
  if (type) params.set("type", type);
  return httpJson(`https://api.helius.xyz/v0/addresses/${address}/transactions?${params}`);
}

async function getTransactionsForAddress(rpcUrl, address, options = {}) {
  const {
    limit = 100,
    sortOrder = "asc",
    transactionDetails = "full",
    filters = {},
    paginationToken = null,
  } = options;
  const params = [
    address,
    {
      transactionDetails,
      encoding: "json",
      maxSupportedTransactionVersion: 0,
      sortOrder,
      limit,
      commitment: "confirmed",
      filters,
      ...(paginationToken ? { paginationToken } : {}),
    },
  ];
  let lastError = null;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const res = await fetch(rpcUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "getTransactionsForAddress", params }),
    });
    if (res.ok) {
      const payload = await res.json();
      if (payload.error) throw new Error(JSON.stringify(payload.error));
      return payload.result || { data: [], paginationToken: null };
    }
    lastError = new Error(`${res.status} ${res.statusText}: ${await res.text()}`);
    if (![429, 500, 502, 503, 504].includes(res.status)) throw lastError;
    await sleep(500 * 2 ** attempt);
  }
  throw lastError;
}

async function discoverRecentPools(apiKey, pages, limit, minTxAgeSeconds = 0) {
  const out = new Map();
  let before = null;
  const nowTs = Math.floor(Date.now() / 1000);
  for (let page = 0; page < pages && out.size < limit; page += 1) {
    const rows = await heliusAddressTxs(apiKey, PUMP_AMM, { type: "SWAP", limit: 100, before });
    if (!Array.isArray(rows) || rows.length === 0) break;
    for (const tx of rows) {
      const parsed = parseEnhancedPumpAmmSwap(tx);
      if (parsed?.timestamp && minTxAgeSeconds && nowTs - parsed.timestamp < minTxAgeSeconds) continue;
      if (parsed && !out.has(parsed.pool)) out.set(parsed.pool, parsed);
      if (out.size >= limit) break;
    }
    before = rows.at(-1)?.signature;
    if (!before) break;
    await sleep(125);
  }
  return [...out.values()];
}

async function discoverCompletedPumpApi(pages, limit, minCreatedAgeSeconds = 0) {
  const out = new Map();
  const nowTs = Math.floor(Date.now() / 1000);
  let offset = 0;
  for (let page = 0; page < pages && out.size < limit; page += 1) {
    const params = new URLSearchParams({
      offset: String(offset),
      limit: "100",
      sort: "created_timestamp",
      order: "DESC",
      includeNsfw: "true",
      complete: "true",
    });
    const rows = await httpJson(`https://frontend-api-v3.pump.fun/coins?${params}`);
    if (!Array.isArray(rows) || rows.length === 0) break;
    for (const coin of rows) {
      const createdTimestamp = Number(coin.created_timestamp) / 1000;
      if (!coin.complete || !coin.pool_address || !coin.mint || !Number.isFinite(createdTimestamp)) continue;
      if (minCreatedAgeSeconds && nowTs - createdTimestamp < minCreatedAgeSeconds) continue;
      if (!out.has(coin.pool_address)) {
        out.set(coin.pool_address, {
          pool: coin.pool_address,
          mint: coin.mint,
          baseMint: coin.mint,
          quoteMint: coin.quote_mint || WSOL,
          timestamp: Math.floor(createdTimestamp),
          source: "pump-api-completed",
          symbol: coin.symbol,
          name: coin.name,
        });
      }
      if (out.size >= limit) break;
    }
    offset += rows.length;
    await sleep(125);
  }
  return [...out.values()];
}

async function fetchPoolHistory(rpcUrl, pool, maxPages) {
  const rows = [];
  let paginationToken = null;
  let exhausted = false;
  for (let page = 0; page < maxPages; page += 1) {
    const result = await getTransactionsForAddress(rpcUrl, pool, {
      limit: 1000,
      sortOrder: "asc",
      paginationToken,
      filters: { status: "succeeded" },
    });
    const batch = result.data || [];
    if (!batch.length) break;
    rows.push(...batch);
    paginationToken = result.paginationToken || null;
    if (!paginationToken) {
      exhausted = true;
      break;
    }
    await sleep(100);
  }
  const blockTimes = rows.map((row) => row.blockTime).filter(Number.isFinite);
  return {
    rows,
    exhausted,
    pageLimitHit: Boolean(paginationToken),
    maxBlockTime: blockTimes.length ? Math.max(...blockTimes) : null,
  };
}

function analyzePool(seed, historyResult, config) {
  const history = historyResult.rows;
  const events = [];
  for (const [orderHint, tx] of history.entries()) {
    const signature = tx.transaction?.signatures?.[0];
    for (const event of extractAmmEvents(tx)) {
      if (event.pool !== seed.pool) continue;
      events.push({
        ...event,
        signature,
        slot: tx.slot,
        transactionIndex: tx.transactionIndex,
        blockTime: tx.blockTime,
        orderHint,
      });
    }
  }
  events.sort(
    (a, b) =>
      a.slot - b.slot ||
      (a.transactionIndex ?? 0) - (b.transactionIndex ?? 0) ||
      a.orderHint - b.orderHint,
  );
  const create = events.find((event) => event.event === "create_pool");
  if (!create || !(create.price > 0)) return { skipped: "create_pool_event_missing" };
  const nowTs = Math.floor(Date.now() / 1000);
  const ageSeconds = nowTs - create.timestamp;
  if (ageSeconds < config.minAgeSeconds) return { skipped: "younger_than_3h", ageSeconds };
  if (ageSeconds > config.maxAgeSeconds) return { skipped: "older_than_window", ageSeconds };

  const trades = events.filter(
    (event) =>
      (event.event === "buy" || event.event === "sell") &&
      event.timestamp >= create.timestamp &&
      event.timestamp <= create.timestamp + config.horizonSeconds &&
      event.price > 0,
  );
  const stateEvents = [create, ...trades];
  const prices = stateEvents.map((event) => event.price).filter((price) => price > 0);
  const migrationPrice = create.price;
  const last = stateEvents.at(-1);
  const horizonTimestamp = create.timestamp + config.horizonSeconds;
  const fetchedThroughTimestamp = historyResult.exhausted
    ? Number.POSITIVE_INFINITY
    : historyResult.maxBlockTime;
  if (Number.isFinite(fetchedThroughTimestamp) && fetchedThroughTimestamp < horizonTimestamp) {
    return {
      skipped: "horizon_not_covered",
      ageSeconds,
      observedSeconds: last.timestamp - create.timestamp,
      fetchedThroughSeconds: fetchedThroughTimestamp - create.timestamp,
    };
  }
  const peakPrice = Math.max(...prices);
  const troughPrice = Math.min(...prices);
  const atOrLast = (seconds) =>
    stateEvents.filter((event) => event.timestamp <= create.timestamp + seconds).at(-1) || create;
  const returnVsMigration = (price) => (price > 0 ? price / migrationPrice - 1 : null);
  const peakEvent = stateEvents.find((event) => event.price === peakPrice) || create;

  return {
    pool: seed.pool,
    mint: create.baseMint === WSOL ? create.quoteMint : create.baseMint,
    baseMint: create.baseMint,
    quoteMint: create.quoteMint,
    migrationSignature: create.signature,
    migrationTimestamp: create.timestamp,
    migrationAgeSecondsAtProbe: ageSeconds,
    migrationPrice,
    horizonTimestamp,
    fetchedThroughTimestamp: Number.isFinite(fetchedThroughTimestamp) ? fetchedThroughTimestamp : null,
    fetchedThroughSeconds: Number.isFinite(fetchedThroughTimestamp)
      ? fetchedThroughTimestamp - create.timestamp
      : null,
    historyExhausted: historyResult.exhausted,
    historyPageLimitHit: historyResult.pageLimitHit,
    lastObservedTimestamp: last.timestamp,
    observedSeconds: last.timestamp - create.timestamp,
    windowTradeCount: trades.length,
    buyCount: trades.filter((event) => event.event === "buy").length,
    sellCount: trades.filter((event) => event.event === "sell").length,
    secondsToPeak: peakEvent.timestamp - create.timestamp,
    peakReturn: returnVsMigration(peakPrice),
    troughReturn: returnVsMigration(troughPrice),
    lastReturn: returnVsMigration(last.price),
    returnAt60s: returnVsMigration(atOrLast(60).price),
    returnAt5m: returnVsMigration(atOrLast(300).price),
    returnAt30m: returnVsMigration(atOrLast(1800).price),
    returnAt1h: returnVsMigration(atOrLast(3600).price),
    returnAt3h: returnVsMigration(atOrLast(config.horizonSeconds).price),
    everAboveMigration: peakPrice > migrationPrice * (1 + config.eps),
    lastAboveMigration: last.price > migrationPrice * (1 + config.eps),
  };
}

const opts = parseArgs(process.argv.slice(2));
const config = {
  discoverySource: stringOpt(opts, "discovery-source", "pump-api"),
  discoveryPages: numberOpt(opts, "discovery-pages", 8),
  discoveryLimit: numberOpt(opts, "discovery-limit", 120),
  discoveryMinTxAgeSeconds: numberOpt(opts, "discovery-min-tx-age-seconds", 3 * 3600),
  pumpApiMinCreatedAgeSeconds: numberOpt(opts, "pump-api-min-created-age-seconds", 3 * 3600),
  sampleSize: numberOpt(opts, "sample-size", 30),
  historyPages: numberOpt(opts, "history-pages", 4),
  minAgeSeconds: numberOpt(opts, "min-age-seconds", 3 * 3600),
  maxAgeSeconds: numberOpt(opts, "max-age-seconds", 72 * 3600),
  horizonSeconds: numberOpt(opts, "horizon-seconds", 3 * 3600),
  eps: numberOpt(opts, "eps", 0.001),
};

const env = parseEnv(await fs.readFile(ENV_PATH, "utf8"));
const rpcUrl = env.get("HELIUS_RPC_URL");
if (!rpcUrl) throw new Error("HELIUS_RPC_URL not found in .env");
const apiKey = new URL(rpcUrl).searchParams.get("api-key") || env.get("HELIUS_LASERSTREAM_API_KEY");
if (!apiKey) throw new Error("No Helius API key found");

const seeds =
  config.discoverySource === "pump-api"
    ? await discoverCompletedPumpApi(
        config.discoveryPages,
        config.discoveryLimit,
        config.pumpApiMinCreatedAgeSeconds,
      )
    : await discoverRecentPools(
        apiKey,
        config.discoveryPages,
        config.discoveryLimit,
        config.discoveryMinTxAgeSeconds,
      );
const analyses = [];
const skipped = [];
for (const seed of seeds) {
  const history = await fetchPoolHistory(rpcUrl, seed.pool, config.historyPages);
  const result = analyzePool(seed, history, config);
  if (result.skipped) {
    skipped.push({
      pool: seed.pool,
      mint: seed.mint,
      reason: result.skipped,
      ageSeconds: result.ageSeconds,
      observedSeconds: result.observedSeconds,
      fetchedThroughSeconds: result.fetchedThroughSeconds,
    });
  } else {
    analyses.push(result);
  }
  if (analyses.length >= config.sampleSize) break;
}

const n = analyses.length;
const rate = (predicate) => (n ? round(analyses.filter(predicate).length / n, 4) : null);

const result = {
  generatedAt: new Date().toISOString(),
  method: {
    discovery:
      config.discoverySource === "pump-api"
        ? "Pump frontend completed coins -> pool_address candidates"
        : "recent Helius enhanced PUMP_AMM swaps -> unique PumpSwap pool addresses",
    measurement:
      "chronological Helius getTransactionsForAddress on each pool; price = pool_quote_token_reserves / pool_base_token_reserves from PumpSwap CreatePool/Buy/Sell events",
    discoverySource: config.discoverySource,
    horizonSeconds: config.horizonSeconds,
    discoveryMinTxAgeSeconds: config.discoveryMinTxAgeSeconds,
    pumpApiMinCreatedAgeSeconds: config.pumpApiMinCreatedAgeSeconds,
    minAgeSeconds: config.minAgeSeconds,
    maxAgeSeconds: config.maxAgeSeconds,
    eps: config.eps,
    note: "This measures AMM price after migration/public PumpSwap trading, not bonding-curve price before migration.",
  },
  discoveredPools: seeds.length,
  sampledPools: n,
  skippedCount: skipped.length,
  rates: {
    everAboveMigrationRate: rate((row) => row.everAboveMigration),
    lastAboveMigrationRate: rate((row) => row.lastAboveMigration),
    tradeCountAtLeast1Rate: rate((row) => row.windowTradeCount >= 1),
    tradeCountAtLeast10Rate: rate((row) => row.windowTradeCount >= 10),
    tradeCountAtLeast100Rate: rate((row) => row.windowTradeCount >= 100),
  },
  returnsPctVsMigration: {
    at60s: summarize(analyses.map((row) => pct(row.returnAt60s)), 3),
    at5m: summarize(analyses.map((row) => pct(row.returnAt5m)), 3),
    at30m: summarize(analyses.map((row) => pct(row.returnAt30m)), 3),
    at1h: summarize(analyses.map((row) => pct(row.returnAt1h)), 3),
    at3hOrLast: summarize(analyses.map((row) => pct(row.returnAt3h)), 3),
    peak3h: summarize(analyses.map((row) => pct(row.peakReturn)), 3),
    lastObserved: summarize(analyses.map((row) => pct(row.lastReturn)), 3),
    trough3h: summarize(analyses.map((row) => pct(row.troughReturn)), 3),
  },
  timing: {
    migrationAgeSecondsAtProbe: summarize(analyses.map((row) => row.migrationAgeSecondsAtProbe), 3),
    secondsToPeak: summarize(analyses.map((row) => row.secondsToPeak), 3),
    observedSeconds: summarize(analyses.map((row) => row.observedSeconds), 3),
    fetchedThroughSeconds: summarize(analyses.map((row) => row.fetchedThroughSeconds), 3),
    windowTradeCount: summarize(analyses.map((row) => row.windowTradeCount), 3),
  },
  strongestMigrationPeaks: analyses
    .slice()
    .sort((a, b) => b.peakReturn - a.peakReturn)
    .slice(0, 10)
    .map((row) => ({
      pool: row.pool,
      mint: row.mint,
      ageSeconds: row.migrationAgeSecondsAtProbe,
      trades: row.windowTradeCount,
      secondsToPeak: row.secondsToPeak,
      peakPct: pct(row.peakReturn),
      at3hPct: pct(row.returnAt3h),
      lastPct: pct(row.lastReturn),
      troughPct: pct(row.troughReturn),
    })),
  analyses: analyses.map((row) => ({
    ...row,
    peakReturnPct: pct(row.peakReturn),
    returnAt3hPct: pct(row.returnAt3h),
    lastReturnPct: pct(row.lastReturn),
    troughReturnPct: pct(row.troughReturn),
  })),
  skipped: skipped.slice(0, 100),
};

await fs.writeFile(OUT_PATH, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(
  JSON.stringify(
    {
      outPath: OUT_PATH,
      discoveredPools: result.discoveredPools,
      sampledPools: result.sampledPools,
      skippedCount: result.skippedCount,
      rates: result.rates,
      returnsPctVsMigration: result.returnsPctVsMigration,
      timing: result.timing,
      strongestMigrationPeaks: result.strongestMigrationPeaks.slice(0, 5),
    },
    null,
    2,
  ),
);
