import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const ENV_PATH = path.join(ROOT, ".env");
const OUT_PATH = path.join(ROOT, "tmp", "pre_migration_entry_30m_probe_result.json");

const PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const WSOL = "So11111111111111111111111111111111111111112";

const PUMP_TRADE_EVENT = [189, 219, 127, 211, 78, 230, 97, 238];
const AMM_CREATE_POOL_EVENT = [177, 49, 12, 210, 160, 118, 167, 116];
const AMM_BUY_EVENT = [103, 244, 82, 31, 44, 245, 119, 119];
const AMM_SELL_EVENT = [62, 47, 55, 10, 165, 3, 220, 42];

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
}

function reservePrice(quoteReserves, tokenReserves) {
  return tokenReserves > 0 ? quoteReserves / tokenReserves : null;
}

function ammPrice(row) {
  return reservePrice(row.poolQuoteTokenReserves, row.poolBaseTokenReserves);
}

function executableSellAvgPriceNoFee(poolBaseTokenReserves, poolQuoteTokenReserves, baseAmountIn) {
  if (!(poolBaseTokenReserves > 0 && poolQuoteTokenReserves > 0 && baseAmountIn > 0)) return null;
  const quoteOut = (poolQuoteTokenReserves * baseAmountIn) / (poolBaseTokenReserves + baseAmountIn);
  return quoteOut / baseAmountIn;
}

function decodePumpTradeEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "pump_trade",
    mint: c.pubkey(),
    solAmount: c.u64(),
    tokenAmount: c.u64(),
    isBuy: c.bool(),
    user: c.pubkey(),
    timestamp: c.i64(),
    virtualSolReserves: c.u64(),
    virtualTokenReserves: c.u64(),
    realSolReserves: c.u64(),
    realTokenReserves: c.u64(),
  };
  row.reservePrice = reservePrice(row.virtualSolReserves, row.virtualTokenReserves);
  row.effectivePrice = row.tokenAmount > 0 ? row.solAmount / row.tokenAmount : null;
  row.side = row.isBuy ? "buy" : "sell";
  return row;
}

function decodeAmmCreatePoolEvent(buf) {
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
  };
  row.price = ammPrice(row);
  return row;
}

function decodeAmmBuyEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "amm_buy",
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
  };
  row.price = ammPrice(row);
  return row;
}

function decodeAmmSellEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "amm_sell",
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
  };
  row.price = ammPrice(row);
  return row;
}

function extractPumpTradeEvents(rawTx) {
  const events = [];
  for (const line of rawTx?.meta?.logMessages || []) {
    let b64 = null;
    if (line.startsWith("Program data: ")) b64 = line.slice("Program data: ".length).trim();
    else if (line.startsWith(`Program return: ${PUMP_PROGRAM} `)) {
      b64 = line.slice(`Program return: ${PUMP_PROGRAM} `.length).trim();
    }
    if (!b64) continue;
    try {
      const buf = Buffer.from(b64, "base64");
      if (sameDiscriminator(buf, PUMP_TRADE_EVENT)) events.push(decodePumpTradeEvent(buf));
    } catch {
      // Ignore event versions this lightweight decoder cannot parse.
    }
  }
  return events;
}

function extractAmmEvents(rawTx) {
  const events = [];
  for (const line of rawTx?.meta?.logMessages || []) {
    let b64 = null;
    if (line.startsWith("Program data: ")) b64 = line.slice("Program data: ".length).trim();
    else if (line.startsWith(`Program return: ${PUMP_AMM} `)) {
      b64 = line.slice(`Program return: ${PUMP_AMM} `.length).trim();
    }
    if (!b64) continue;
    try {
      const buf = Buffer.from(b64, "base64");
      if (sameDiscriminator(buf, AMM_CREATE_POOL_EVENT)) events.push(decodeAmmCreatePoolEvent(buf));
      else if (sameDiscriminator(buf, AMM_BUY_EVENT)) events.push(decodeAmmBuyEvent(buf));
      else if (sameDiscriminator(buf, AMM_SELL_EVENT)) events.push(decodeAmmSellEvent(buf));
    } catch {
      // Ignore event versions this lightweight decoder cannot parse.
    }
  }
  return events;
}

async function httpJson(url) {
  let lastError = null;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const res = await fetch(url, {
      headers: {
        accept: "application/json",
        "user-agent": "shadow-pre-migration-entry-30m-probe/1.0",
      },
    });
    if (res.ok) return res.json();
    lastError = new Error(`${res.status} ${res.statusText}: ${await res.text()}`);
    if (![429, 500, 502, 503, 504].includes(res.status)) throw lastError;
    await sleep(400 * 2 ** attempt);
  }
  throw lastError;
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

async function fetchAddressHistory(rpcUrl, address, { maxPages, limit = 1000, sortOrder = "asc", filters = {} }) {
  const rows = [];
  let paginationToken = null;
  let exhausted = false;
  for (let page = 0; page < maxPages; page += 1) {
    const result = await getTransactionsForAddress(rpcUrl, address, {
      limit,
      sortOrder,
      paginationToken,
      filters: { status: "succeeded", ...filters },
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
    minBlockTime: blockTimes.length ? Math.min(...blockTimes) : null,
    maxBlockTime: blockTimes.length ? Math.max(...blockTimes) : null,
  };
}

async function discoverCompletedPumpApi(pages, limit, minCreatedAgeSeconds) {
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
    if (!Array.isArray(rows) || !rows.length) break;
    for (const coin of rows) {
      const createdTimestamp = Number(coin.created_timestamp) / 1000;
      if (
        !coin.complete ||
        !coin.pool_address ||
        !coin.mint ||
        !coin.bonding_curve ||
        !Number.isFinite(createdTimestamp)
      ) {
        continue;
      }
      if (minCreatedAgeSeconds && nowTs - createdTimestamp < minCreatedAgeSeconds) continue;
      if (!out.has(coin.pool_address)) {
        out.set(coin.pool_address, {
          pool: coin.pool_address,
          mint: coin.mint,
          bondingCurve: coin.bonding_curve,
          symbol: coin.symbol,
          name: coin.name,
          createdTimestamp: Math.floor(createdTimestamp),
        });
      }
      if (out.size >= limit) break;
    }
    offset += rows.length;
    await sleep(125);
  }
  return [...out.values()];
}

function orderedEvent(rawEvent, tx, orderHint) {
  return {
    ...rawEvent,
    signature: tx.transaction?.signatures?.[0],
    slot: tx.slot,
    transactionIndex: tx.transactionIndex,
    blockTime: tx.blockTime,
    orderHint,
  };
}

function sortEventsAsc(a, b) {
  return (
    a.slot - b.slot ||
    (a.transactionIndex ?? 0) - (b.transactionIndex ?? 0) ||
    a.orderHint - b.orderHint
  );
}

function analyzeAmm(seed, historyResult, config) {
  const events = [];
  for (const [orderHint, tx] of historyResult.rows.entries()) {
    for (const event of extractAmmEvents(tx)) {
      if (event.pool === seed.pool) events.push(orderedEvent(event, tx, orderHint));
    }
  }
  events.sort(sortEventsAsc);
  const create = events.find((event) => event.event === "create_pool" && event.price > 0);
  if (!create) return { skipped: "amm_create_pool_missing" };

  const nowTs = Math.floor(Date.now() / 1000);
  const migrationAgeSeconds = nowTs - create.timestamp;
  if (migrationAgeSeconds < config.minMigrationAgeSeconds) {
    return { skipped: "migration_younger_than_horizon", migrationAgeSeconds };
  }
  if (migrationAgeSeconds > config.maxMigrationAgeSeconds) {
    return { skipped: "migration_older_than_window", migrationAgeSeconds };
  }

  const horizonTimestamp = create.timestamp + config.horizonSeconds;
  const fetchedThroughTimestamp = historyResult.exhausted
    ? Number.POSITIVE_INFINITY
    : historyResult.maxBlockTime;
  const stateEvents = [
    create,
    ...events.filter(
      (event) =>
        (event.event === "amm_buy" || event.event === "amm_sell") &&
        event.timestamp >= create.timestamp &&
        event.timestamp <= horizonTimestamp &&
        event.price > 0,
    ),
  ];
  const last = stateEvents.at(-1);
  if (Number.isFinite(fetchedThroughTimestamp) && fetchedThroughTimestamp < horizonTimestamp) {
    return {
      skipped: "amm_horizon_not_covered",
      migrationAgeSeconds,
      observedSeconds: last.timestamp - create.timestamp,
      fetchedThroughSeconds: fetchedThroughTimestamp - create.timestamp,
    };
  }
  const peakEvent = stateEvents.reduce((best, event) => (event.price > best.price ? event : best), create);
  const atHorizon = stateEvents.filter((event) => event.timestamp <= horizonTimestamp).at(-1) || create;
  return {
    create,
    stateEvents,
    migrationAgeSeconds,
    migrationPrice: create.price,
    horizonTimestamp,
    peakEvent,
    peakPrice: peakEvent.price,
    atHorizon,
    atHorizonPrice: atHorizon.price,
    lastObserved: last,
    observedSeconds: last.timestamp - create.timestamp,
    fetchedThroughSeconds: Number.isFinite(fetchedThroughTimestamp)
      ? fetchedThroughTimestamp - create.timestamp
      : null,
    windowTradeCount: stateEvents.length - 1,
  };
}

function analyzeCurveEntry(seed, historyResult, migration, config) {
  const events = [];
  for (const [orderHint, tx] of historyResult.rows.entries()) {
    for (const event of extractPumpTradeEvents(tx)) {
      if (event.mint !== seed.mint) continue;
      if (!(event.reservePrice > 0)) continue;
      const beforeByTime = event.timestamp < migration.create.timestamp;
      const sameSecondBeforeSlot =
        event.timestamp === migration.create.timestamp && tx.slot <= migration.create.slot;
      if (beforeByTime || sameSecondBeforeSlot) events.push(orderedEvent(event, tx, orderHint));
    }
  }
  events.sort(sortEventsAsc);
  const buys = events.filter((event) => event.isBuy && event.effectivePrice > 0);
  const lastBuy = buys.at(-1);
  if (!lastBuy) return { skipped: "pre_migration_buy_missing" };
  const secondsBeforeMigration = migration.create.timestamp - lastBuy.timestamp;
  if (secondsBeforeMigration > config.entryMaxSecondsBeforeMigration) {
    return { skipped: "pre_migration_buy_too_old", secondsBeforeMigration };
  }
  return {
    lastBuy,
    secondsBeforeMigration,
    entryEffectivePrice: lastBuy.effectivePrice,
    entryReservePrice: lastBuy.reservePrice,
    entryTokenAmount: lastBuy.tokenAmount,
    entryQuoteAmount: lastBuy.solAmount,
  };
}

function combine(seed, migration, entry, config) {
  const entryPrice = entry.entryEffectivePrice > 0 ? entry.entryEffectivePrice : entry.entryReservePrice;
  const peakMarkReturn = migration.peakPrice / entryPrice - 1;
  const atHorizonMarkReturn = migration.atHorizonPrice / entryPrice - 1;
  const migrationMarkReturn = migration.migrationPrice / entryPrice - 1;

  let executablePeak = null;
  let executablePeakEvent = null;
  for (const event of migration.stateEvents) {
    const avgPrice = executableSellAvgPriceNoFee(
      event.poolBaseTokenReserves,
      event.poolQuoteTokenReserves,
      entry.entryTokenAmount,
    );
    if (!(avgPrice > 0)) continue;
    if (!executablePeak || avgPrice > executablePeak.price) {
      executablePeak = { price: avgPrice, event };
      executablePeakEvent = event;
    }
  }
  const atHorizonExecutablePrice = executableSellAvgPriceNoFee(
    migration.atHorizon.poolBaseTokenReserves,
    migration.atHorizon.poolQuoteTokenReserves,
    entry.entryTokenAmount,
  );
  const executablePeakReturn = executablePeak ? executablePeak.price / entryPrice - 1 : null;
  const executableAtHorizonReturn =
    atHorizonExecutablePrice > 0 ? atHorizonExecutablePrice / entryPrice - 1 : null;

  return {
    pool: seed.pool,
    mint: seed.mint,
    bondingCurve: seed.bondingCurve,
    symbol: seed.symbol,
    migrationSignature: migration.create.signature,
    entrySignature: entry.lastBuy.signature,
    migrationTimestamp: migration.create.timestamp,
    migrationAgeSeconds: migration.migrationAgeSeconds,
    entrySecondsBeforeMigration: entry.secondsBeforeMigration,
    entryTokenAmount: entry.entryTokenAmount,
    entryQuoteAmount: entry.entryQuoteAmount,
    entryEffectivePrice: entry.entryEffectivePrice,
    entryReservePrice: entry.entryReservePrice,
    migrationPrice: migration.migrationPrice,
    migrationMarkReturn,
    peakMarkReturn,
    atHorizonMarkReturn,
    executablePeakReturnNoFee: executablePeakReturn,
    executableAtHorizonReturnNoFee: executableAtHorizonReturn,
    secondsToMarkPeak: migration.peakEvent.timestamp - migration.create.timestamp,
    secondsToExecutablePeak: executablePeakEvent
      ? executablePeakEvent.timestamp - migration.create.timestamp
      : null,
    observedSeconds: migration.observedSeconds,
    fetchedThroughSeconds: migration.fetchedThroughSeconds,
    windowTradeCount: migration.windowTradeCount,
    markProfitOffered: peakMarkReturn > config.eps,
    executableProfitOfferedNoFee: executablePeakReturn > config.eps,
    markProfitAtHorizon: atHorizonMarkReturn > config.eps,
    executableProfitAtHorizonNoFee: executableAtHorizonReturn > config.eps,
  };
}

const opts = parseArgs(process.argv.slice(2));
const config = {
  discoveryPages: numberOpt(opts, "discovery-pages", 40),
  discoveryLimit: numberOpt(opts, "discovery-limit", 60),
  minCreatedAgeSeconds: numberOpt(opts, "min-created-age-seconds", 1800),
  sampleSize: numberOpt(opts, "sample-size", 6),
  ammHistoryPages: numberOpt(opts, "amm-history-pages", 10),
  curveHistoryPages: numberOpt(opts, "curve-history-pages", 5),
  horizonSeconds: numberOpt(opts, "horizon-seconds", 1800),
  minMigrationAgeSeconds: numberOpt(opts, "min-migration-age-seconds", 1800),
  maxMigrationAgeSeconds: numberOpt(opts, "max-migration-age-seconds", 7 * 24 * 3600),
  entryLookbackSeconds: numberOpt(opts, "entry-lookback-seconds", 12 * 3600),
  entryMaxSecondsBeforeMigration: numberOpt(opts, "entry-max-seconds-before-migration", 3600),
  eps: numberOpt(opts, "eps", 0.001),
};

const env = parseEnv(await fs.readFile(ENV_PATH, "utf8"));
const rpcUrl = env.get("HELIUS_RPC_URL");
if (!rpcUrl) throw new Error("HELIUS_RPC_URL not found in .env");

const seeds = await discoverCompletedPumpApi(
  config.discoveryPages,
  config.discoveryLimit,
  config.minCreatedAgeSeconds,
);
const analyses = [];
const skipped = [];

for (const seed of seeds) {
  const poolHistory = await fetchAddressHistory(rpcUrl, seed.pool, {
    maxPages: config.ammHistoryPages,
    sortOrder: "asc",
  });
  const migration = analyzeAmm(seed, poolHistory, config);
  if (migration.skipped) {
    skipped.push({ pool: seed.pool, mint: seed.mint, reason: migration.skipped, ...migration });
    continue;
  }

  const curveHistory = await fetchAddressHistory(rpcUrl, seed.bondingCurve, {
    maxPages: config.curveHistoryPages,
    sortOrder: "desc",
    filters: {
      blockTime: {
        gte: migration.create.timestamp - config.entryLookbackSeconds,
        lte: migration.create.timestamp,
      },
    },
  });
  const entry = analyzeCurveEntry(seed, curveHistory, migration, config);
  if (entry.skipped) {
    skipped.push({ pool: seed.pool, mint: seed.mint, reason: entry.skipped, ...entry });
    continue;
  }

  analyses.push(combine(seed, migration, entry, config));
  if (analyses.length >= config.sampleSize) break;
}

const n = analyses.length;
const rate = (predicate) => (n ? round(analyses.filter(predicate).length / n, 4) : null);

const result = {
  generatedAt: new Date().toISOString(),
  method: {
    discovery: "Pump frontend completed coins -> pool_address + bonding_curve candidates",
    measurement:
      "entry = latest Pump bonding-curve buy before PumpSwap migration; exit window = PumpSwap reserve states through horizon",
    priceUnits:
      "raw quote/base units; Pump bonding-curve SOL/token and PumpSwap WSOL/token ratios are directly comparable for relative returns",
    executableEstimate:
      "No-fee constant-product average sell price for the latest pre-migration buy's token amount; actual fees, tips, latency, and slippage settings are not included.",
    horizonSeconds: config.horizonSeconds,
    entryLookbackSeconds: config.entryLookbackSeconds,
    entryMaxSecondsBeforeMigration: config.entryMaxSecondsBeforeMigration,
    eps: config.eps,
  },
  discoveredPools: seeds.length,
  sampledPools: n,
  skippedCount: skipped.length,
  rates: {
    markProfitOfferedRate: rate((row) => row.markProfitOffered),
    executableProfitOfferedNoFeeRate: rate((row) => row.executableProfitOfferedNoFee),
    markProfitAtHorizonRate: rate((row) => row.markProfitAtHorizon),
    executableProfitAtHorizonNoFeeRate: rate((row) => row.executableProfitAtHorizonNoFee),
  },
  returnsPctVsPreMigrationEntry: {
    migrationMark: summarize(analyses.map((row) => pct(row.migrationMarkReturn)), 3),
    peakMarkWithinWindow: summarize(analyses.map((row) => pct(row.peakMarkReturn)), 3),
    atHorizonMark: summarize(analyses.map((row) => pct(row.atHorizonMarkReturn)), 3),
    peakExecutableNoFeeWithinWindow: summarize(
      analyses.map((row) => pct(row.executablePeakReturnNoFee)),
      3,
    ),
    atHorizonExecutableNoFee: summarize(
      analyses.map((row) => pct(row.executableAtHorizonReturnNoFee)),
      3,
    ),
  },
  timing: {
    entrySecondsBeforeMigration: summarize(analyses.map((row) => row.entrySecondsBeforeMigration), 3),
    secondsToMarkPeak: summarize(analyses.map((row) => row.secondsToMarkPeak), 3),
    secondsToExecutablePeak: summarize(analyses.map((row) => row.secondsToExecutablePeak), 3),
    observedSeconds: summarize(analyses.map((row) => row.observedSeconds), 3),
    windowTradeCount: summarize(analyses.map((row) => row.windowTradeCount), 3),
  },
  strongestExecutablePeaksNoFee: analyses
    .slice()
    .sort((a, b) => b.executablePeakReturnNoFee - a.executablePeakReturnNoFee)
    .slice(0, 5)
    .map((row) => ({
      pool: row.pool,
      mint: row.mint,
      entrySecondsBeforeMigration: row.entrySecondsBeforeMigration,
      trades: row.windowTradeCount,
      peakExecutableNoFeePct: pct(row.executablePeakReturnNoFee),
      peakMarkPct: pct(row.peakMarkReturn),
      atHorizonExecutableNoFeePct: pct(row.executableAtHorizonReturnNoFee),
      atHorizonMarkPct: pct(row.atHorizonMarkReturn),
      secondsToExecutablePeak: row.secondsToExecutablePeak,
    })),
  analyses,
  skipped,
};

await fs.mkdir(path.dirname(OUT_PATH), { recursive: true });
await fs.writeFile(OUT_PATH, JSON.stringify(result, null, 2));
console.log(
  JSON.stringify(
    {
      outPath: OUT_PATH,
      discoveredPools: result.discoveredPools,
      sampledPools: result.sampledPools,
      skippedCount: result.skippedCount,
      rates: result.rates,
      returnsPctVsPreMigrationEntry: result.returnsPctVsPreMigrationEntry,
      timing: result.timing,
      strongestExecutablePeaksNoFee: result.strongestExecutablePeaksNoFee,
    },
    null,
    2,
  ),
);
