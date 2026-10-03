import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const ENV_PATH = path.join(ROOT, ".env");
const OUT_PATH = path.join(ROOT, "tmp", "bonding_curve_birth_probe_result.json");

const PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const WSOL = "So11111111111111111111111111111111111111112";

const CREATE_EVENT = [27, 114, 169, 77, 222, 235, 99, 118];
const TRADE_EVENT = [189, 219, 127, 211, 78, 230, 97, 238];

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
    if (byte === 0) out = "1" + out;
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

function decodeCreateEvent(buf) {
  const c = new Cursor(buf);
  return {
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
    quoteMint: c.o + 32 <= buf.length ? c.pubkey() : WSOL,
    virtualQuoteReserves: c.o + 8 <= buf.length ? c.u64() : null,
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
  row.priceLamportsPerRawToken = row.tokenAmount > 0 ? row.solAmount / row.tokenAmount : null;
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
      }
    } catch {
      // Ignore event versions that do not match this decoder.
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
        "user-agent": "shadow-bonding-curve-birth-probe/1.0",
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
    await sleep(125);
  }
  return out;
}

async function fetchRecentCreates(apiKey, pages, limit) {
  const rows = [];
  const seen = new Set();
  let before = null;
  for (let page = 0; page < pages && rows.length < limit; page += 1) {
    const batch = await heliusAddressTxs(apiKey, PUMP_PROGRAM, {
      type: "CREATE",
      limit: 100,
      before,
    });
    if (!Array.isArray(batch) || batch.length === 0) break;
    for (const tx of batch) {
      if (!seen.has(tx.signature)) {
        rows.push(tx);
        seen.add(tx.signature);
      }
      if (rows.length >= limit) break;
    }
    before = batch.at(-1)?.signature;
    if (!before) break;
    await sleep(125);
  }
  return rows;
}

async function fetchMintEnhancedHistory(apiKey, mint, createSignature, maxPages, pageLimit) {
  const rows = [];
  const seen = new Set();
  let before = null;
  let sawCreate = false;
  for (let page = 0; page < maxPages; page += 1) {
    const batch = await heliusAddressTxs(apiKey, mint, { limit: pageLimit, before });
    if (!Array.isArray(batch) || batch.length === 0) break;
    for (const tx of batch) {
      if (seen.has(tx.signature)) continue;
      rows.push(tx);
      seen.add(tx.signature);
      if (tx.signature === createSignature) sawCreate = true;
    }
    before = batch.at(-1)?.signature;
    if (sawCreate || !before || batch.length < pageLimit) break;
    await sleep(125);
  }
  return { rows, sawCreate };
}

function analyzeMint(create, trades, { earlyTrades, windowSeconds, eps }) {
  const sorted = [...trades].sort(
    (a, b) =>
      a.slot - b.slot ||
      a.timestamp - b.timestamp ||
      (a.orderHint ?? 0) - (b.orderHint ?? 0) ||
      a.eventIndex - b.eventIndex,
  );
  if (!sorted.length) return null;

  const first = sorted[0];
  const windowRows = sorted
    .filter((row) => row.timestamp <= first.timestamp + windowSeconds)
    .slice(0, earlyTrades);
  if (!windowRows.length) return null;

  const prices = windowRows.map((row) => row.priceLamportsPerRawToken).filter((value) => value > 0);
  if (!prices.length) return null;
  const p0 = prices[0];
  const peak = Math.max(...prices);
  const trough = Math.min(...prices);
  const peakIdx = prices.findIndex((value) => value === peak);
  const firstSellIdx = windowRows.findIndex((row) => row.side === "sell");
  const buyPrefix =
    firstSellIdx >= 0 ? windowRows.slice(0, firstSellIdx) : windowRows.filter((row) => row.side === "buy");
  const buyPrefixPrices = buyPrefix.map((row) => row.priceLamportsPerRawToken).filter((value) => value > 0);
  const buyPrefixPeak = buyPrefixPrices.length ? Math.max(...buyPrefixPrices) : null;
  const buyPrefixLast = buyPrefixPrices.length ? buyPrefixPrices.at(-1) : null;
  const buyPrefixMonotonic =
    buyPrefixPrices.length > 1
      ? buyPrefixPrices.every((value, idx) => idx === 0 || value >= buyPrefixPrices[idx - 1] * (1 - eps))
      : null;

  const next = prices.length > 1 ? prices[1] : null;
  const third = prices.length > 2 ? prices[2] : null;
  const fifth = prices.length > 4 ? prices[4] : null;
  const tenth = prices.length > 9 ? prices[9] : null;

  return {
    mint: create.mint,
    name: create.name,
    symbol: create.symbol,
    createSignature: create.signature,
    createSlot: create.slot,
    createTimestamp: create.timestamp,
    bondingCurve: create.bondingCurve,
    firstTradeSignature: first.signature,
    firstTradeSlot: first.slot,
    firstTradeTimestamp: first.timestamp,
    firstTradeSide: first.side,
    firstTradeSol: round(first.solAmount / 1e9, 9),
    firstTradeTokenRaw: first.tokenAmount,
    firstTradeIxName: first.ixName,
    observedTradeCount: sorted.length,
    windowTradeCount: windowRows.length,
    windowBuyCount: windowRows.filter((row) => row.side === "buy").length,
    windowSellCount: windowRows.filter((row) => row.side === "sell").length,
    buyPrefixTradeCount: buyPrefix.length,
    buyPrefixMonotonic,
    firstSellTradeIndex: firstSellIdx >= 0 ? firstSellIdx : null,
    firstSellSeconds: firstSellIdx >= 0 ? windowRows[firstSellIdx].timestamp - first.timestamp : null,
    firstSellSlots: firstSellIdx >= 0 ? windowRows[firstSellIdx].slot - first.slot : null,
    secondsToPeak: peakIdx >= 0 ? windowRows[peakIdx].timestamp - first.timestamp : null,
    peakTradeIndex: peakIdx >= 0 ? peakIdx : null,
    nextReturn: next ? next / p0 - 1 : null,
    trade3Return: third ? third / p0 - 1 : null,
    trade5Return: fifth ? fifth / p0 - 1 : null,
    trade10Return: tenth ? tenth / p0 - 1 : null,
    peakReturn: peak / p0 - 1,
    troughReturn: trough / p0 - 1,
    lastReturn: prices.at(-1) / p0 - 1,
    buyPrefixPeakReturn: buyPrefixPeak ? buyPrefixPeak / p0 - 1 : null,
    buyPrefixLastReturn: buyPrefixLast ? buyPrefixLast / p0 - 1 : null,
    priceEverUpWindow: peak > p0 * (1 + eps),
    priceEverUpBeforeFirstSell: buyPrefixPeak != null ? buyPrefixPeak > p0 * (1 + eps) : false,
    nextUp: next != null ? next > p0 * (1 + eps) : null,
    endsUp: prices.at(-1) > p0 * (1 + eps),
  };
}

const options = parseArgs(process.argv.slice(2));
const createLimit = numberOpt(options, "create-limit", 30);
const createPages = numberOpt(options, "create-pages", 6);
const mintPages = numberOpt(options, "mint-pages", 3);
const mintPageLimit = numberOpt(options, "mint-page-limit", 100);
const maxCreateAgeSeconds = numberOpt(options, "max-create-age-seconds", 900);
const earlyTrades = numberOpt(options, "early-trades", 20);
const windowSeconds = numberOpt(options, "window-seconds", 60);
const eps = numberOpt(options, "eps", 0.001);

const env = parseEnv(await fs.readFile(ENV_PATH, "utf8"));
const rpcUrl = env.get("HELIUS_RPC_URL");
if (!rpcUrl) throw new Error("HELIUS_RPC_URL not found in .env");
const apiKey = new URL(rpcUrl).searchParams.get("api-key") || env.get("HELIUS_LASERSTREAM_API_KEY");
if (!apiKey) throw new Error("No Helius API key found");

const createEnhanced = await fetchRecentCreates(apiKey, createPages, createLimit);
const createRaw = await rpcBatchGetTransactions(
  rpcUrl,
  createEnhanced.map((row) => row.signature),
);

const nowTs = Math.floor(Date.now() / 1000);
const creates = [];
const skipped = [];
for (const row of createEnhanced) {
  const raw = createRaw.get(row.signature);
  const createEvent = extractPumpEvents(raw).find((event) => event.event === "create");
  if (!createEvent) {
    skipped.push({ signature: row.signature, reason: "create_event_missing" });
    continue;
  }
  const ageSeconds = Math.max(0, nowTs - (createEvent.timestamp || row.timestamp || nowTs));
  if (ageSeconds > maxCreateAgeSeconds) {
    skipped.push({
      signature: row.signature,
      mint: createEvent.mint,
      reason: "older_than_window",
      ageSeconds,
    });
    continue;
  }
  creates.push({
    ...createEvent,
    signature: row.signature,
    slot: row.slot,
    timestamp: createEvent.timestamp || row.timestamp,
    ageSeconds,
  });
}

const analyses = [];
for (const create of creates) {
  const { rows, sawCreate } = await fetchMintEnhancedHistory(
    apiKey,
    create.mint,
    create.signature,
    mintPages,
    mintPageLimit,
  );
  if (!sawCreate) {
    skipped.push({
      signature: create.signature,
      mint: create.mint,
      reason: "mint_history_truncated_before_create",
      fetchedTransactions: rows.length,
    });
    continue;
  }

  const rawMap = await rpcBatchGetTransactions(
    rpcUrl,
    rows.map((row) => row.signature),
  );
  const orderBySignature = new Map(
    rows
      .slice()
      .reverse()
      .map((row, idx) => [row.signature, idx]),
  );
  const trades = [];
  for (const row of rows) {
    const raw = rawMap.get(row.signature);
    const events = extractPumpEvents(raw);
    events.forEach((event, eventIndex) => {
      if (event.event !== "trade" || event.mint !== create.mint) return;
      trades.push({
        ...event,
        signature: row.signature,
        slot: row.slot,
        timestamp: event.timestamp || row.timestamp,
        orderHint: orderBySignature.get(row.signature) ?? 0,
        eventIndex,
      });
    });
  }
  const analysis = analyzeMint(create, trades, { earlyTrades, windowSeconds, eps });
  if (!analysis) {
    skipped.push({
      signature: create.signature,
      mint: create.mint,
      reason: "no_trade_window",
      fetchedTransactions: rows.length,
      decodedTrades: trades.length,
    });
    continue;
  }
  analyses.push(analysis);
}

const n = analyses.length;
const rate = (predicate) => (n ? round(analyses.filter(predicate).length / n, 4) : null);
const withFirstSell = analyses.filter((row) => Number.isFinite(row.firstSellSeconds));

const result = {
  generatedAt: new Date().toISOString(),
  method: {
    venue: "Pump.fun bonding curve / Pump Program",
    program: PUMP_PROGRAM,
    createLimit,
    createPages,
    mintPages,
    mintPageLimit,
    maxCreateAgeSeconds,
    earlyTrades,
    windowSeconds,
    epsilonReturn: eps,
    priceDefinition:
      "TradeEvent solAmount divided by tokenAmount; returns are relative, so token decimals are not needed.",
    orderingNote:
      "Sorted by slot, event timestamp, signature, and event index; same-slot historical ordering is approximate without block transaction index.",
  },
  recentCreateTransactions: createEnhanced.length,
  freshCreatesDecoded: creates.length,
  sampledMints: n,
  skippedCount: skipped.length,
  rates: {
    firstTradeBuyRate: rate((row) => row.firstTradeSide === "buy"),
    buyPrefixAtLeast2Rate: rate((row) => row.buyPrefixTradeCount >= 2),
    buyPrefixAtLeast3Rate: rate((row) => row.buyPrefixTradeCount >= 3),
    priceEverUpBeforeFirstSellRate: rate((row) => row.priceEverUpBeforeFirstSell),
    priceEverUpWindowRate: rate((row) => row.priceEverUpWindow),
    nextUpRate: rate((row) => row.nextUp === true),
    endsUpRate: rate((row) => row.endsUp),
    firstSellWithin5sRate: rate((row) => Number.isFinite(row.firstSellSeconds) && row.firstSellSeconds <= 5),
    firstSellWithin15sRate: rate((row) => Number.isFinite(row.firstSellSeconds) && row.firstSellSeconds <= 15),
    firstSellWithin60sRate: rate((row) => Number.isFinite(row.firstSellSeconds) && row.firstSellSeconds <= 60),
  },
  returnsPct: {
    next: summarize(analyses.map((row) => pct(row.nextReturn)), 3),
    trade3: summarize(analyses.map((row) => pct(row.trade3Return)), 3),
    trade5: summarize(analyses.map((row) => pct(row.trade5Return)), 3),
    trade10: summarize(analyses.map((row) => pct(row.trade10Return)), 3),
    peakWindow: summarize(analyses.map((row) => pct(row.peakReturn)), 3),
    peakBeforeFirstSell: summarize(analyses.map((row) => pct(row.buyPrefixPeakReturn)), 3),
    lastWindow: summarize(analyses.map((row) => pct(row.lastReturn)), 3),
    troughWindow: summarize(analyses.map((row) => pct(row.troughReturn)), 3),
  },
  timing: {
    firstSellSeconds: summarize(withFirstSell.map((row) => row.firstSellSeconds), 3),
    secondsToPeak: summarize(analyses.map((row) => row.secondsToPeak), 3),
    buyPrefixTradeCount: summarize(analyses.map((row) => row.buyPrefixTradeCount), 3),
    windowTradeCount: summarize(analyses.map((row) => row.windowTradeCount), 3),
  },
  strongestBirthRamps: analyses
    .slice()
    .sort((a, b) => (b.buyPrefixPeakReturn || 0) - (a.buyPrefixPeakReturn || 0))
    .slice(0, 8)
    .map((row) => ({
      mint: row.mint,
      symbol: row.symbol,
      ageSeconds: Math.max(0, nowTs - row.createTimestamp),
      firstTradeSol: row.firstTradeSol,
      buyPrefixTradeCount: row.buyPrefixTradeCount,
      firstSellSeconds: row.firstSellSeconds,
      peakBeforeFirstSellPct: pct(row.buyPrefixPeakReturn),
      peakWindowPct: pct(row.peakReturn),
      lastWindowPct: pct(row.lastReturn),
      windowBuys: row.windowBuyCount,
      windowSells: row.windowSellCount,
    })),
  analyses: analyses.map((row) => ({
    ...row,
    nextReturnPct: pct(row.nextReturn),
    trade3ReturnPct: pct(row.trade3Return),
    trade5ReturnPct: pct(row.trade5Return),
    trade10ReturnPct: pct(row.trade10Return),
    peakReturnPct: pct(row.peakReturn),
    buyPrefixPeakReturnPct: pct(row.buyPrefixPeakReturn),
    lastReturnPct: pct(row.lastReturn),
    troughReturnPct: pct(row.troughReturn),
  })),
  skipped: skipped.slice(0, 50),
};

await fs.writeFile(OUT_PATH, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(
  JSON.stringify(
    {
      outPath: OUT_PATH,
      recentCreateTransactions: result.recentCreateTransactions,
      freshCreatesDecoded: result.freshCreatesDecoded,
      sampledMints: result.sampledMints,
      rates: result.rates,
      returnsPct: result.returnsPct,
      timing: result.timing,
      strongestBirthRamps: result.strongestBirthRamps.slice(0, 5),
    },
    null,
    2,
  ),
);
