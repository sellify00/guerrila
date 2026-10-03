import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const ENV_PATH = path.join(ROOT, ".env");
const OUT_PATH = path.join(ROOT, "tmp", "bonding_curve_3h_probe_result.json");

const PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const WSOL = "So11111111111111111111111111111111111111112";

const CREATE_EVENT = [27, 114, 169, 77, 222, 235, 99, 118];
const TRADE_EVENT = [189, 219, 127, 211, 78, 230, 97, 238];
const COMPLETE_EVENT = [95, 114, 97, 156, 212, 46, 152, 8];

const B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function parseArgs(argv) {
  const options = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    const eq = token.indexOf("=");
    if (eq >= 0) {
      options.set(token.slice(2, eq), token.slice(eq + 1));
      continue;
    }
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      options.set(token.slice(2), next);
      i += 1;
    } else {
      options.set(token.slice(2), "true");
    }
  }
  return options;
}

function numberOpt(options, key, fallback) {
  const parsed = Number(options.get(key));
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

function reservePrice(quoteReserves, tokenReserves) {
  return tokenReserves > 0 ? quoteReserves / tokenReserves : null;
}

function decodeCreateEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "create",
    name: c.string(),
    symbol: c.string(),
    uri: c.string(),
    mint: c.pubkey(),
    bondingCurve: c.pubkey(),
    user: c.pubkey(),
    creator: c.pubkey(),
    timestamp: c.i64(),
    virtualTokenReserves: c.u64(),
    virtualSolReserves: c.u64(),
    realTokenReserves: c.u64(),
    tokenTotalSupply: c.u64(),
    tokenProgram: c.pubkey(),
    isMayhemMode: c.bool(),
    isCashbackEnabled: c.bool(),
  };
  row.quoteMint = c.o + 32 <= buf.length ? c.pubkey() : WSOL;
  row.virtualQuoteReserves = c.o + 8 <= buf.length ? c.u64() : row.virtualSolReserves;
  row.createReservePrice = reservePrice(row.virtualQuoteReserves, row.virtualTokenReserves);
  return row;
}

function decodeCompleteEvent(buf) {
  const c = new Cursor(buf);
  return {
    event: "complete",
    user: c.pubkey(),
    mint: c.pubkey(),
    bondingCurve: c.pubkey(),
    timestamp: c.i64(),
    quoteMint: c.o + 32 <= buf.length ? c.pubkey() : WSOL,
  };
}

function decodeTradeEvent(buf) {
  const c = new Cursor(buf);
  const row = {
    event: "trade",
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
    feeRecipient: c.pubkey(),
    feeBasisPoints: c.u64(),
    fee: c.u64(),
    creator: c.pubkey(),
    creatorFeeBasisPoints: c.u64(),
    creatorFee: c.u64(),
    trackVolume: c.bool(),
    totalUnclaimedTokens: c.u64(),
    totalClaimedTokens: c.u64(),
    currentSolVolume: c.u64(),
    lastUpdateTimestamp: c.i64(),
    ixName: c.string(),
    mayhemMode: c.bool(),
    cashbackFeeBasisPoints: c.u64(),
    cashback: c.u64(),
    buybackFeeBasisPoints: c.u64(),
    buybackFee: c.u64(),
  };
  const shareholders = c.o + 4 <= buf.length ? c.u32() : 0;
  for (let i = 0; i < shareholders && c.o + 34 <= buf.length; i += 1) {
    c.pubkey();
    c.u16();
  }
  row.quoteMint = c.o + 32 <= buf.length ? c.pubkey() : WSOL;
  row.quoteAmount = c.o + 8 <= buf.length ? c.u64() : row.solAmount;
  row.virtualQuoteReserves = c.o + 8 <= buf.length ? c.u64() : row.virtualSolReserves;
  row.realQuoteReserves = c.o + 8 <= buf.length ? c.u64() : row.realSolReserves;
  row.reservePrice = reservePrice(row.virtualQuoteReserves, row.virtualTokenReserves);
  row.effectivePrice = row.tokenAmount > 0 ? row.quoteAmount / row.tokenAmount : null;
  row.side = row.isBuy ? "buy" : "sell";
  return row;
}

function extractPumpEvents(rawTx) {
  const events = [];
  const logs = rawTx?.meta?.logMessages || [];
  for (const line of logs) {
    let b64 = null;
    if (line.startsWith("Program data: ")) {
      b64 = line.slice("Program data: ".length).trim();
    } else if (line.startsWith(`Program return: ${PUMP_PROGRAM} `)) {
      b64 = line.slice(`Program return: ${PUMP_PROGRAM} `.length).trim();
    }
    if (!b64) continue;
    let buf;
    try {
      buf = Buffer.from(b64, "base64");
    } catch {
      continue;
    }
    try {
      if (sameDiscriminator(buf, CREATE_EVENT)) {
        events.push(decodeCreateEvent(buf));
      } else if (sameDiscriminator(buf, TRADE_EVENT)) {
        events.push(decodeTradeEvent(buf));
      } else if (sameDiscriminator(buf, COMPLETE_EVENT)) {
        events.push(decodeCompleteEvent(buf));
      }
    } catch {
      // Ignore unknown event versions.
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
        "user-agent": "shadow-bonding-curve-3h-probe/1.0",
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

async function rpcBatchGetTransactions(rpcUrl, signatures, batchSize = 25) {
  const unique = [...new Set(signatures.filter(Boolean))];
  const out = new Map();
  for (let i = 0; i < unique.length; i += batchSize) {
    const chunk = unique.slice(i, i + batchSize);
    const body = chunk.map((signature, idx) => ({
      jsonrpc: "2.0",
      id: idx,
      method: "getTransaction",
      params: [
        signature,
        {
          encoding: "json",
          maxSupportedTransactionVersion: 0,
          commitment: "confirmed",
        },
      ],
    }));
    let lastError = null;
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const res = await fetch(rpcUrl, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      if (res.ok) {
        const payload = await res.json();
        for (const item of Array.isArray(payload) ? payload : [payload]) {
          out.set(chunk[item.id], item.result || null);
        }
        lastError = null;
        break;
      }
      lastError = new Error(`${res.status} ${res.statusText}: ${await res.text()}`);
      if (![429, 500, 502, 503, 504].includes(res.status)) throw lastError;
      await sleep(500 * 2 ** attempt);
    }
    if (lastError) throw lastError;
    await sleep(100);
  }
  return out;
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
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "getTransactionsForAddress",
        params,
      }),
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

async function fetchCandidateCreates(rpcUrl, config) {
  const candidates = [];
  const skipped = [];
  const seen = new Set();
  let paginationToken = null;
  let pages = 0;
  let scannedCreates = 0;
  const nowTs = Math.floor(Date.now() / 1000);
  const timeFilter = {
    gte: nowTs - config.maxCreateAgeSeconds,
    lte: nowTs - config.minCreateAgeSeconds,
  };

  while (pages < config.createPages && candidates.length < config.sampleSize) {
    pages += 1;
    const result = await getTransactionsForAddress(rpcUrl, PUMP_PROGRAM, {
      limit: 100,
      sortOrder: "asc",
      paginationToken,
      filters: {
        blockTime: timeFilter,
        status: "succeeded",
      },
    });
    const batch = result.data || [];
    if (!batch.length) break;
    paginationToken = result.paginationToken || null;

    for (const row of batch) {
      const signature = row.transaction?.signatures?.[0];
      if (!signature || seen.has(signature)) continue;
      seen.add(signature);
      scannedCreates += 1;
      const create = extractPumpEvents(row).find((event) => event.event === "create");
      if (!create) continue;
      const ageSeconds = nowTs - create.timestamp;
      if (ageSeconds < config.minCreateAgeSeconds || ageSeconds > config.maxCreateAgeSeconds) {
        skipped.push({
          signature,
          mint: create.mint,
          reason: "outside_age_window_after_decode",
          ageSeconds,
        });
        continue;
      }
      candidates.push({
        ...create,
        signature,
        slot: row.slot,
        transactionIndex: row.transactionIndex,
        timestamp: create.timestamp || row.blockTime,
        ageSeconds,
      });
      if (candidates.length >= config.sampleSize) break;
    }
    if (!paginationToken) break;
    await sleep(125);
  }
  return { candidates, skipped, scannedCreates, createPagesRead: pages };
}

async function fetchMintWindow(rpcUrl, mint, createTs, horizonSeconds, maxPages) {
  const rows = [];
  let paginationToken = null;
  for (let page = 0; page < maxPages; page += 1) {
    const result = await getTransactionsForAddress(rpcUrl, mint, {
      limit: 100,
      sortOrder: "asc",
      paginationToken,
      filters: {
        blockTime: {
          gte: createTs,
          lte: createTs + horizonSeconds,
        },
        status: "succeeded",
      },
    });
    const batch = result.data || [];
    if (!batch.length) break;
    rows.push(...batch);
    paginationToken = result.paginationToken || null;
    if (!paginationToken) break;
    await sleep(125);
  }
  return { rows, fetchedWindowRows: rows.length };
}

function analyzeWindow(create, trades, completes, config) {
  const sorted = [...trades].sort(
    (a, b) =>
      a.slot - b.slot ||
      a.timestamp - b.timestamp ||
      (a.orderHint ?? 0) - (b.orderHint ?? 0) ||
      a.eventIndex - b.eventIndex,
  );
  if (!sorted.length || !(create.createReservePrice > 0)) return null;

  const first = sorted[0];
  const last = sorted.at(-1);
  const prices = sorted.map((row) => row.reservePrice).filter((value) => value > 0);
  const createPrice = create.createReservePrice;
  const firstPrice = first.reservePrice;
  const lastPrice = last.reservePrice;
  const peakPrice = Math.max(...prices);
  const troughPrice = Math.min(...prices);
  const peakTradeIndex = sorted.findIndex((row) => row.reservePrice === peakPrice);
  const firstSellIndex = sorted.findIndex((row) => row.side === "sell");
  const firstSell = firstSellIndex >= 0 ? sorted[firstSellIndex] : null;
  const completed = completes
    .filter((row) => row.mint === create.mint)
    .sort((a, b) => a.timestamp - b.timestamp)[0] || null;

  const returnVs = (price, base) => (price > 0 && base > 0 ? price / base - 1 : null);
  const tradeAt = (idx) => (sorted.length > idx ? sorted[idx] : null);
  const priceAtSeconds = (seconds) => {
    const at = sorted.filter((row) => row.timestamp <= create.timestamp + seconds).at(-1);
    return at ? at.reservePrice : null;
  };

  return {
    mint: create.mint,
    name: create.name,
    symbol: create.symbol,
    createSignature: create.signature,
    createSlot: create.slot,
    createTimestamp: create.timestamp,
    ageSecondsAtProbe: create.ageSeconds,
    bondingCurve: create.bondingCurve,
    quoteMint: create.quoteMint,
    firstTradeSignature: first.signature,
    firstTradeSlot: first.slot,
    firstTradeTimestamp: first.timestamp,
    firstTradeSecondsAfterCreate: first.timestamp - create.timestamp,
    firstTradeSide: first.side,
    firstTradeSol: round(first.solAmount / 1e9, 9),
    firstSellSecondsAfterCreate: firstSell ? firstSell.timestamp - create.timestamp : null,
    firstSellSecondsAfterFirstTrade: firstSell ? firstSell.timestamp - first.timestamp : null,
    firstSellTradeIndex: firstSellIndex >= 0 ? firstSellIndex : null,
    completedWithinWindow: Boolean(completed && completed.timestamp <= create.timestamp + config.horizonSeconds),
    completeSecondsAfterCreate: completed ? completed.timestamp - create.timestamp : null,
    windowTradeCount: sorted.length,
    windowBuyCount: sorted.filter((row) => row.side === "buy").length,
    windowSellCount: sorted.filter((row) => row.side === "sell").length,
    observedSeconds: last.timestamp - create.timestamp,
    secondsToPeak: peakTradeIndex >= 0 ? sorted[peakTradeIndex].timestamp - create.timestamp : null,
    peakTradeIndex,
    createPrice,
    firstPrice,
    lastPrice,
    peakPrice,
    troughPrice,
    firstTradeReturnVsCreate: returnVs(firstPrice, createPrice),
    peakReturnVsCreate: returnVs(peakPrice, createPrice),
    lastReturnVsCreate: returnVs(lastPrice, createPrice),
    troughReturnVsCreate: returnVs(troughPrice, createPrice),
    peakReturnVsFirstTrade: returnVs(peakPrice, firstPrice),
    lastReturnVsFirstTrade: returnVs(lastPrice, firstPrice),
    troughReturnVsFirstTrade: returnVs(troughPrice, firstPrice),
    trade2ReturnVsCreate: returnVs(tradeAt(1)?.reservePrice, createPrice),
    trade3ReturnVsCreate: returnVs(tradeAt(2)?.reservePrice, createPrice),
    trade5ReturnVsCreate: returnVs(tradeAt(4)?.reservePrice, createPrice),
    trade10ReturnVsCreate: returnVs(tradeAt(9)?.reservePrice, createPrice),
    returnAt60sVsCreate: returnVs(priceAtSeconds(60), createPrice),
    returnAt5mVsCreate: returnVs(priceAtSeconds(300), createPrice),
    returnAt30mVsCreate: returnVs(priceAtSeconds(1800), createPrice),
    returnAt1hVsCreate: returnVs(priceAtSeconds(3600), createPrice),
    returnAt3hVsCreate: returnVs(priceAtSeconds(config.horizonSeconds), createPrice),
    everAboveCreate: peakPrice > createPrice * (1 + config.eps),
    everAboveFirstTrade: peakPrice > firstPrice * (1 + config.eps),
    lastAboveCreate: lastPrice > createPrice * (1 + config.eps),
    lastAboveFirstTrade: lastPrice > firstPrice * (1 + config.eps),
  };
}

const options = parseArgs(process.argv.slice(2));
const config = {
  sampleSize: numberOpt(options, "sample-size", 30),
  createPages: numberOpt(options, "create-pages", 80),
  mintPages: numberOpt(options, "mint-pages", 40),
  minCreateAgeSeconds: numberOpt(options, "min-age-seconds", 3 * 3600),
  maxCreateAgeSeconds: numberOpt(options, "max-age-seconds", 12 * 3600),
  horizonSeconds: numberOpt(options, "horizon-seconds", 3 * 3600),
  eps: numberOpt(options, "eps", 0.001),
};

const env = parseEnv(await fs.readFile(ENV_PATH, "utf8"));
const rpcUrl = env.get("HELIUS_RPC_URL");
if (!rpcUrl) throw new Error("HELIUS_RPC_URL not found in .env");
const apiKey = new URL(rpcUrl).searchParams.get("api-key") || env.get("HELIUS_LASERSTREAM_API_KEY");
if (!apiKey) throw new Error("No Helius API key found");

const candidateScan = await fetchCandidateCreates(rpcUrl, config);
const analyses = [];
const skipped = [...candidateScan.skipped];

for (const create of candidateScan.candidates) {
  const history = await fetchMintWindow(
    rpcUrl,
    create.mint,
    create.timestamp,
    config.horizonSeconds,
    config.mintPages,
  );

  const orderBySignature = new Map(
    history.rows.map((row, idx) => [row.transaction?.signatures?.[0], idx]),
  );

  const trades = [];
  const completes = [];
  for (const row of history.rows) {
    const signature = row.transaction?.signatures?.[0];
    const events = extractPumpEvents(row);
    events.forEach((event, eventIndex) => {
      if (event.mint !== create.mint) return;
      const enriched = {
        ...event,
        signature,
        slot: row.slot,
        transactionIndex: row.transactionIndex,
        timestamp: event.timestamp || row.blockTime,
        orderHint: orderBySignature.get(signature) ?? 0,
        eventIndex,
      };
      if (event.event === "trade") trades.push(enriched);
      if (event.event === "complete") completes.push(enriched);
    });
  }

  const analysis = analyzeWindow(create, trades, completes, config);
  if (!analysis) {
    skipped.push({
      signature: create.signature,
      mint: create.mint,
      reason: "no_decoded_trades",
      fetchedWindowRows: history.fetchedWindowRows,
      decodedTradeCount: trades.length,
    });
    continue;
  }
  analyses.push(analysis);
}

const n = analyses.length;
const rate = (predicate) => (n ? round(analyses.filter(predicate).length / n, 4) : null);
const tradeCountAtLeast = (count) => rate((row) => row.windowTradeCount >= count);

const result = {
  generatedAt: new Date().toISOString(),
  method: {
    venue: "Pump.fun bonding curve / Pump Program",
    program: PUMP_PROGRAM,
    sampleSizeRequested: config.sampleSize,
    minCreateAgeSeconds: config.minCreateAgeSeconds,
    maxCreateAgeSeconds: config.maxCreateAgeSeconds,
    horizonSeconds: config.horizonSeconds,
    createPages: config.createPages,
    mintPages: config.mintPages,
    epsilonReturn: config.eps,
    priceDefinition:
      "Post-trade curve state price = virtual quote reserves / virtual token reserves from Pump TradeEvent. Create baseline uses Pump CreateEvent virtual reserves.",
    scopeLimit:
      "This follows Pump bonding-curve events in the first 3h. Tokens that migrate to AMM inside the window are flagged; post-migration AMM price is not stitched into the curve series.",
  },
  candidateScan: {
    scannedCreateTransactions: candidateScan.scannedCreates,
    createPagesRead: candidateScan.createPagesRead,
    candidateCreates: candidateScan.candidates.length,
  },
  sampledMints: n,
  skippedCount: skipped.length,
  rates: {
    everAboveCreateRate: rate((row) => row.everAboveCreate),
    everAboveFirstTradeRate: rate((row) => row.everAboveFirstTrade),
    lastAboveCreateRate: rate((row) => row.lastAboveCreate),
    lastAboveFirstTradeRate: rate((row) => row.lastAboveFirstTrade),
    firstTradeBuyRate: rate((row) => row.firstTradeSide === "buy"),
    firstSellWithin5sRate: rate(
      (row) => Number.isFinite(row.firstSellSecondsAfterFirstTrade) && row.firstSellSecondsAfterFirstTrade <= 5,
    ),
    firstSellWithin60sRate: rate(
      (row) => Number.isFinite(row.firstSellSecondsAfterFirstTrade) && row.firstSellSecondsAfterFirstTrade <= 60,
    ),
    completedWithin3hRate: rate((row) => row.completedWithinWindow),
    tradeCountAtLeast5Rate: tradeCountAtLeast(5),
    tradeCountAtLeast20Rate: tradeCountAtLeast(20),
    tradeCountAtLeast100Rate: tradeCountAtLeast(100),
  },
  returnsPctVsCreate: {
    firstTrade: summarize(analyses.map((row) => pct(row.firstTradeReturnVsCreate)), 3),
    trade2: summarize(analyses.map((row) => pct(row.trade2ReturnVsCreate)), 3),
    trade3: summarize(analyses.map((row) => pct(row.trade3ReturnVsCreate)), 3),
    trade5: summarize(analyses.map((row) => pct(row.trade5ReturnVsCreate)), 3),
    trade10: summarize(analyses.map((row) => pct(row.trade10ReturnVsCreate)), 3),
    at60s: summarize(analyses.map((row) => pct(row.returnAt60sVsCreate)), 3),
    at5m: summarize(analyses.map((row) => pct(row.returnAt5mVsCreate)), 3),
    at30m: summarize(analyses.map((row) => pct(row.returnAt30mVsCreate)), 3),
    at1h: summarize(analyses.map((row) => pct(row.returnAt1hVsCreate)), 3),
    at3hOrLastInWindow: summarize(analyses.map((row) => pct(row.returnAt3hVsCreate)), 3),
    peak3h: summarize(analyses.map((row) => pct(row.peakReturnVsCreate)), 3),
    lastObservedIn3h: summarize(analyses.map((row) => pct(row.lastReturnVsCreate)), 3),
    trough3h: summarize(analyses.map((row) => pct(row.troughReturnVsCreate)), 3),
  },
  returnsPctVsFirstTrade: {
    peak3h: summarize(analyses.map((row) => pct(row.peakReturnVsFirstTrade)), 3),
    lastObservedIn3h: summarize(analyses.map((row) => pct(row.lastReturnVsFirstTrade)), 3),
    trough3h: summarize(analyses.map((row) => pct(row.troughReturnVsFirstTrade)), 3),
  },
  timing: {
    firstTradeSecondsAfterCreate: summarize(analyses.map((row) => row.firstTradeSecondsAfterCreate), 3),
    firstSellSecondsAfterFirstTrade: summarize(
      analyses.map((row) => row.firstSellSecondsAfterFirstTrade),
      3,
    ),
    secondsToPeakAfterCreate: summarize(analyses.map((row) => row.secondsToPeak), 3),
    observedSeconds: summarize(analyses.map((row) => row.observedSeconds), 3),
    windowTradeCount: summarize(analyses.map((row) => row.windowTradeCount), 3),
    windowBuyCount: summarize(analyses.map((row) => row.windowBuyCount), 3),
    windowSellCount: summarize(analyses.map((row) => row.windowSellCount), 3),
  },
  strongest3hPeaks: analyses
    .slice()
    .sort((a, b) => b.peakReturnVsCreate - a.peakReturnVsCreate)
    .slice(0, 10)
    .map((row) => ({
      mint: row.mint,
      symbol: row.symbol,
      ageSecondsAtProbe: row.ageSecondsAtProbe,
      firstTradeSol: row.firstTradeSol,
      windowTrades: row.windowTradeCount,
      buys: row.windowBuyCount,
      sells: row.windowSellCount,
      secondsToPeak: row.secondsToPeak,
      peakVsCreatePct: pct(row.peakReturnVsCreate),
      lastVsCreatePct: pct(row.lastReturnVsCreate),
      troughVsCreatePct: pct(row.troughReturnVsCreate),
      completedWithinWindow: row.completedWithinWindow,
    })),
  analyses: analyses.map((row) => ({
    ...row,
    firstTradeReturnVsCreatePct: pct(row.firstTradeReturnVsCreate),
    peakReturnVsCreatePct: pct(row.peakReturnVsCreate),
    lastReturnVsCreatePct: pct(row.lastReturnVsCreate),
    troughReturnVsCreatePct: pct(row.troughReturnVsCreate),
    peakReturnVsFirstTradePct: pct(row.peakReturnVsFirstTrade),
    lastReturnVsFirstTradePct: pct(row.lastReturnVsFirstTrade),
    troughReturnVsFirstTradePct: pct(row.troughReturnVsFirstTrade),
  })),
  skipped: skipped.slice(0, 100),
};

await fs.writeFile(OUT_PATH, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(
  JSON.stringify(
    {
      outPath: OUT_PATH,
      candidateScan: result.candidateScan,
      sampledMints: result.sampledMints,
      skippedCount: result.skippedCount,
      rates: result.rates,
      returnsPctVsCreate: result.returnsPctVsCreate,
      returnsPctVsFirstTrade: result.returnsPctVsFirstTrade,
      timing: result.timing,
      strongest3hPeaks: result.strongest3hPeaks.slice(0, 5),
    },
    null,
    2,
  ),
);
