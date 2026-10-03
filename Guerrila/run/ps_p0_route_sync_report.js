#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const readline = require("readline");

const DEFAULT_LATENCY_BUCKETS_US = [1, 10, 100, 1000, 10000, 50000, 250000];
const AUTO_LATENCY_STAGE_ORDER = [
  "end_to_end_observe",
  "source_to_final_shadow_event",
  "source_to_nosend",
  "source_to_process_intent",
  "source_to_worker",
  "source_to_feed",
  "worker_to_nosend",
  "feed_to_worker",
  "producer_to_feed",
];

function usage() {
  console.error(
    [
      "Usage: node run\\ps_p0_route_sync_report.js --input <tape.jsonl> [options]",
      "",
      "Builds an inter-pool sync / route-lag survival study from PS-P0 tape rows.",
      "",
      "Options:",
      "  --output <report.json>              Write aggregate study report JSON",
      "  --rows-output <rows.jsonl>          Write per-impulse route_sync_impulse rows as JSONL",
      "  --latency-profile <g12_latency.json>  Optional g12_latency_by_source.json input",
      "  --latency-stage <stage|auto>        Default: auto",
      "  --latency-quantile <field>          Default: p99_ns",
      "  --execution-latency-us <value>      Manual conservative latency override",
      "  --latency-buckets-us <csv>          Default: 1,10,100,1000,10000,50000,250000",
      "",
      "Notes:",
      "  - consumes rollout_shadow + ps_p0_route_lag_markout rows",
      "  - computes per-impulse / per-pair decay metrics rather than a single global sync time",
      "  - evaluates survival using event time, slot time, wall-clock time, and optional pipeline latency",
      "  - uses current retained-EV / after-fee markout as the executable gap proxy",
      "  - gap_bps_t0 remains null until upstream tape includes honest bps inputs",
    ].join("\n"),
  );
}

function parseArgs(argv) {
  const args = {
    input: null,
    output: null,
    rowsOutput: null,
    latencyProfile: null,
    latencyStage: "auto",
    latencyQuantile: "p99_ns",
    executionLatencyUs: null,
    latencyBucketsUs: DEFAULT_LATENCY_BUCKETS_US,
  };

  for (let i = 2; i < argv.length; i += 1) {
    const current = argv[i];
    if (current === "--input" && i + 1 < argv.length) {
      args.input = argv[++i];
      continue;
    }
    if (current === "--output" && i + 1 < argv.length) {
      args.output = argv[++i];
      continue;
    }
    if (current === "--rows-output" && i + 1 < argv.length) {
      args.rowsOutput = argv[++i];
      continue;
    }
    if (current === "--latency-profile" && i + 1 < argv.length) {
      args.latencyProfile = argv[++i];
      continue;
    }
    if (current === "--latency-stage" && i + 1 < argv.length) {
      args.latencyStage = argv[++i];
      continue;
    }
    if (current === "--latency-quantile" && i + 1 < argv.length) {
      args.latencyQuantile = argv[++i];
      continue;
    }
    if (current === "--execution-latency-us" && i + 1 < argv.length) {
      args.executionLatencyUs = Number(argv[++i]);
      continue;
    }
    if (current === "--latency-buckets-us" && i + 1 < argv.length) {
      args.latencyBucketsUs = parsePositiveNumbers(argv[++i]);
      continue;
    }
    if (current === "--help" || current === "-h") {
      usage();
      process.exit(0);
    }
    throw new Error(`unknown argument: ${current}`);
  }

  if (!args.input) {
    usage();
    throw new Error("missing required --input");
  }
  if (
    args.executionLatencyUs != null &&
    (!Number.isFinite(args.executionLatencyUs) || args.executionLatencyUs < 0)
  ) {
    throw new Error("invalid --execution-latency-us");
  }
  return args;
}

function parsePositiveNumbers(value) {
  const numbers = value
    .split(",")
    .map((part) => Number(part.trim()))
    .filter((part) => Number.isFinite(part) && part >= 0);
  if (numbers.length === 0) {
    throw new Error(`invalid numeric csv: ${value}`);
  }
  return Array.from(new Set(numbers)).sort((a, b) => a - b);
}

function getOrCreate(map, key, create) {
  if (!map.has(key)) {
    map.set(key, create());
  }
  return map.get(key);
}

function sourceProfileKey(signalSource) {
  switch (signalSource) {
    case "public_geyser":
      return "public_geyser";
    case "external_geyser":
      return "external_geyser";
    case "local_validator_geyser":
      return "local_validator_geyser";
    case "afxdp":
      return "afxdp";
    case "dpdk":
      return "dpdk";
    case "triton_grpc":
      return "triton_grpc";
    default:
      return null;
  }
}

function parseLatencyProfile(profilePath) {
  if (!profilePath) {
    return null;
  }
  return JSON.parse(fs.readFileSync(profilePath, "utf8"));
}

function selectLatencyStage(sourceSnapshot, preferredStage, quantile) {
  if (!sourceSnapshot) {
    return null;
  }
  if (preferredStage !== "auto") {
    const bucket = sourceSnapshot[preferredStage];
    if (bucket && Number(bucket.count) > 0 && Number.isFinite(bucket[quantile])) {
      return preferredStage;
    }
    return null;
  }
  for (const stage of AUTO_LATENCY_STAGE_ORDER) {
    const bucket = sourceSnapshot[stage];
    if (bucket && Number(bucket.count) > 0 && Number.isFinite(bucket[quantile])) {
      return stage;
    }
  }
  return null;
}

function resolveExecutionLatency(row, args, latencyProfile) {
  if (args.executionLatencyUs != null) {
    return {
      source: "manual_override",
      stage: "manual_override",
      quantile: "manual_override",
      ns: Math.round(args.executionLatencyUs * 1000),
    };
  }
  if (!latencyProfile) {
    return null;
  }
  const profileKey = sourceProfileKey(row.signalSource);
  if (!profileKey) {
    return null;
  }
  const sourceSnapshot = latencyProfile[profileKey];
  const stage = selectLatencyStage(
    sourceSnapshot,
    args.latencyStage,
    args.latencyQuantile,
  );
  if (!stage) {
    return null;
  }
  const ns = Number(sourceSnapshot[stage][args.latencyQuantile]);
  if (!Number.isFinite(ns) || ns < 0) {
    return null;
  }
  return {
    source: profileKey,
    stage,
    quantile: args.latencyQuantile,
    ns,
  };
}

function stableSourcePoolId(row, rollout) {
  return (
    row.pool_pubkey_hex ||
    rollout.pool_pubkey_hex ||
    poolIndexFallback(row.pool_index ?? rollout.pool_index)
  );
}

function stableLinkedPoolId(row) {
  return (
    row.comparison_pool_pubkey_hex ||
    poolIndexFallback(row.comparison_pool_index)
  );
}

function poolIndexFallback(poolIndex) {
  return `pool_index:${poolIndex ?? "unknown"}`;
}

function mintPair(row, rollout) {
  const mintIn = row.mint_in_hex || rollout.mint_in_hex || null;
  const mintOut = row.mint_out_hex || rollout.mint_out_hex || null;
  if (!mintIn || !mintOut) {
    return null;
  }
  return `${mintIn}/${mintOut}`;
}

function uniqueLinkedEventKey(row) {
  return [
    row.comparison_signature_hex || "no_sig",
    row.markout_unix_ns || "no_markout_ns",
    row.comparison_pool_pubkey_hex || row.comparison_pool_index || "no_pool",
  ].join("|");
}

function dedupeLinkedEvents(rows) {
  const deduped = new Map();
  for (const row of rows) {
    const key = uniqueLinkedEventKey(row);
    const current = deduped.get(key);
    if (
      !current ||
      Number(row.horizon_index ?? 0) > Number(current.horizon_index ?? 0)
    ) {
      deduped.set(key, row);
    }
  }
  return Array.from(deduped.values()).sort((a, b) => {
    const lagDiff = Number(a.lag_unix_ns ?? 0) - Number(b.lag_unix_ns ?? 0);
    if (lagDiff !== 0) {
      return lagDiff;
    }
    return Number(a.markout_unix_ns ?? 0) - Number(b.markout_unix_ns ?? 0);
  });
}

function hasClockOrderInversion(rows) {
  return rows.some(
    (row) =>
      Number(row.markout_unix_ns ?? 0) > 0 &&
      Number(row.t0_unix_ns ?? 0) > 0 &&
      Number(row.markout_unix_ns) < Number(row.t0_unix_ns),
  );
}

function netGapAtLatencyNs(rows, latencyNs) {
  if (rows.length === 0 || latencyNs == null) {
    return null;
  }
  let selected = rows[0];
  for (const row of rows) {
    if (Number(row.lag_unix_ns ?? 0) <= latencyNs) {
      selected = row;
      continue;
    }
    break;
  }
  return Number(selected.net_markout_after_fee_lamports ?? 0);
}

function bucketSurvival(rows, latencyBucketsUs) {
  return latencyBucketsUs.map((latencyUs) => {
    const netGapLamports = netGapAtLatencyNs(rows, latencyUs * 1000);
    return {
      latency_us: latencyUs,
      net_gap_lamports: netGapLamports,
      positive: netGapLamports != null ? netGapLamports > 0 : null,
    };
  });
}

function firstPositiveIndex(rows) {
  return rows.findIndex((row) => Number(row.net_markout_after_fee_lamports ?? 0) > 0);
}

function firstIndexAtOrBelow(rows, threshold, startIndex) {
  for (let i = Math.max(startIndex, 0); i < rows.length; i += 1) {
    if (Number(rows[i].net_markout_after_fee_lamports ?? 0) <= threshold) {
      return i;
    }
  }
  return -1;
}

function analyzeImpulse(rows, rollout, args, latencyProfile) {
  const uniqueRows = dedupeLinkedEvents(rows);
  if (uniqueRows.length === 0) {
    return null;
  }

  const firstRow = uniqueRows[0];
  let peakNet = Number.NEGATIVE_INFINITY;
  let peakIndex = -1;
  let peakGross = Number.NEGATIVE_INFINITY;
  for (let i = 0; i < uniqueRows.length; i += 1) {
    const row = uniqueRows[i];
    const net = Number(row.net_markout_after_fee_lamports ?? 0);
    if (net > peakNet) {
      peakNet = net;
      peakIndex = i;
    }
    peakGross = Math.max(peakGross, Number(row.markout_lamports ?? 0));
  }

  const peakRow = uniqueRows[Math.max(peakIndex, 0)];
  const firstPositive = firstPositiveIndex(uniqueRows);
  const halfGapThreshold = peakNet > 0 ? peakNet / 2 : null;
  const halfGapIndex =
    halfGapThreshold == null ? -1 : firstIndexAtOrBelow(uniqueRows, halfGapThreshold, peakIndex + 1);
  const breakEvenIndex =
    peakNet > 0 ? firstIndexAtOrBelow(uniqueRows, 0, firstPositive + 1) : 0;
  const executionLatency = resolveExecutionLatency(
    {
      signalSource:
        rollout.signal_source || rollout.signalSource || firstRow.signal_source || null,
    },
    args,
    latencyProfile,
  );
  const ourLatencyNetGap = executionLatency
    ? netGapAtLatencyNs(uniqueRows, executionLatency.ns)
    : null;
  const survivalBuckets = bucketSurvival(uniqueRows, args.latencyBucketsUs);
  const sourcePool = stableSourcePoolId(firstRow, rollout);
  const linkedPool = stableLinkedPoolId(firstRow);
  const clockOrderInversion = hasClockOrderInversion(uniqueRows);

  return {
    record_type: "route_sync_impulse",
    impulse_id: firstRow.impulse_id ?? firstRow.observation_id,
    observation_id: firstRow.observation_id,
    shock_id: firstRow.shock_id ?? rollout.shock_id ?? null,
    signal_source:
      rollout.signal_source || rollout.signalSource || firstRow.signal_source || "unknown",
    source_pool: sourcePool,
    linked_pool: linkedPool,
    source_pool_index: firstRow.pool_index ?? rollout.pool_index ?? null,
    linked_pool_index: firstRow.comparison_pool_index ?? null,
    mint_pair: mintPair(firstRow, rollout),
    mint_in: firstRow.mint_in_hex || rollout.mint_in_hex || null,
    mint_out: firstRow.mint_out_hex || rollout.mint_out_hex || null,
    venue_a: firstRow.venue || rollout.venue || "unknown",
    venue_b: firstRow.comparison_venue || "unknown",
    route_family: firstRow.route_family_label || firstRow.route_family || "unknown",
    owner_mode:
      firstRow.selected_owner ||
      firstRow.owner_mode ||
      rollout.selected_owner ||
      rollout.owner_mode ||
      "unknown",
    impulse_tier:
      firstRow.impulse_tier || rollout.impulse_tier || "unknown",
    impulse_type:
      firstRow.impulse_type || rollout.impulse_type || "unknown",
    source_pool_price: firstRow.source_pool_price ?? null,
    linked_pool_price: firstRow.linked_pool_price ?? null,
    gap_bps_t0: firstRow.route_gap_bps_t0 ?? null,
    expected_out_a: firstRow.expected_out_a ?? null,
    expected_out_b: firstRow.expected_out_b ?? null,
    gross_gap_lamports_t0:
      firstRow.net_gap_lamports_t0 ?? Number(firstRow.markout_lamports ?? 0),
    net_gap_lamports_t0:
      firstRow.fee_slippage_adjusted_gap_lamports_t0 ??
      firstRow.net_gap_lamports_t0 ??
      Number(firstRow.net_markout_after_fee_lamports ?? 0),
    fee_slippage_adjusted_gap_lamports_t0:
      firstRow.fee_slippage_adjusted_gap_lamports_t0 ?? null,
    peak_gross_gap_lamports: Number.isFinite(peakGross) ? peakGross : 0,
    peak_net_gap_lamports: Number.isFinite(peakNet) ? peakNet : 0,
    t_first_response_us: Math.round(Number(firstRow.lag_unix_ns ?? 0) / 1000),
    t_peak_gap_us: Math.round(Number(peakRow.lag_unix_ns ?? 0) / 1000),
    t_half_gap_us:
      halfGapIndex >= 0
        ? Math.round(Number(uniqueRows[halfGapIndex].lag_unix_ns ?? 0) / 1000)
        : null,
    t_break_even_us:
      breakEvenIndex >= 0
        ? Math.round(Number(uniqueRows[breakEvenIndex].lag_unix_ns ?? 0) / 1000)
        : null,
    break_even_already_dead_on_first_response:
      Number(firstRow.net_markout_after_fee_lamports ?? 0) <= 0,
    events_to_half_gap: halfGapIndex >= 0 ? halfGapIndex + 1 : null,
    events_to_break_even: breakEvenIndex >= 0 ? breakEvenIndex + 1 : null,
    events_to_sync: breakEvenIndex >= 0 ? breakEvenIndex + 1 : null,
    linked_route_events_observed: uniqueRows.length,
    clock_order_inversion: clockOrderInversion,
    slot_delta_first_response: Number(firstRow.lag_slots ?? 0),
    slot_delta_break_even:
      breakEvenIndex >= 0 ? Number(uniqueRows[breakEvenIndex].lag_slots ?? 0) : null,
    execution_latency_source: executionLatency?.source || null,
    execution_latency_stage: executionLatency?.stage || null,
    execution_latency_quantile: executionLatency?.quantile || null,
    execution_latency_ns: executionLatency?.ns || null,
    execution_latency_us:
      executionLatency?.ns != null ? Math.round(executionLatency.ns / 1000) : null,
    net_gap_at_execution_latency_lamports: ourLatencyNetGap,
    survives_our_latency:
      ourLatencyNetGap != null ? ourLatencyNetGap > 0 : null,
    survival_buckets: survivalBuckets,
    clock_fields: {
      t0_unix_ns: firstRow.t0_unix_ns ?? null,
      first_response_unix_ns: firstRow.markout_unix_ns ?? null,
      latest_response_unix_ns:
        uniqueRows[uniqueRows.length - 1].markout_unix_ns ?? null,
      first_response_slot: firstRow.markout_slot ?? null,
      latest_response_slot:
        uniqueRows[uniqueRows.length - 1].markout_slot ?? null,
    },
  };
}

function createCurveAccumulator(latencyUs) {
  return {
    latency_us: latencyUs,
    positive_count: 0,
    non_positive_count: 0,
    evaluable_count: 0,
  };
}

function finalizeCurveAccumulator(acc) {
  return {
    latency_us: acc.latency_us,
    evaluable_count: acc.evaluable_count,
    positive_count: acc.positive_count,
    non_positive_count: acc.non_positive_count,
    positive_rate:
      acc.evaluable_count > 0 ? acc.positive_count / acc.evaluable_count : 0,
  };
}

function createGroupAccumulator() {
  return {
    impulses: 0,
    positive_on_first_response: 0,
    positive_anytime: 0,
    break_even_observed: 0,
    survives_our_latency_positive: 0,
    survives_our_latency_evaluable: 0,
    sum_t_first_response_us: 0,
    sum_t_break_even_us: 0,
    t_break_even_count: 0,
  };
}

function updateGroupAccumulator(acc, row) {
  acc.impulses += 1;
  acc.positive_on_first_response += row.net_gap_lamports_t0 > 0 ? 1 : 0;
  acc.positive_anytime += row.peak_net_gap_lamports > 0 ? 1 : 0;
  acc.break_even_observed += row.t_break_even_us != null ? 1 : 0;
  acc.sum_t_first_response_us += row.t_first_response_us;
  if (row.t_break_even_us != null) {
    acc.sum_t_break_even_us += row.t_break_even_us;
    acc.t_break_even_count += 1;
  }
  if (row.survives_our_latency != null) {
    acc.survives_our_latency_evaluable += 1;
    acc.survives_our_latency_positive += row.survives_our_latency ? 1 : 0;
  }
}

function finalizeGroupMap(map) {
  return Array.from(map.entries())
    .map(([key, acc]) => ({
      key,
      impulses: acc.impulses,
      positive_on_first_response: acc.positive_on_first_response,
      positive_anytime: acc.positive_anytime,
      break_even_observed: acc.break_even_observed,
      avg_t_first_response_us:
        acc.impulses > 0 ? Math.round(acc.sum_t_first_response_us / acc.impulses) : 0,
      avg_t_break_even_us:
        acc.t_break_even_count > 0
          ? Math.round(acc.sum_t_break_even_us / acc.t_break_even_count)
          : null,
      survives_our_latency_positive: acc.survives_our_latency_positive,
      survives_our_latency_evaluable: acc.survives_our_latency_evaluable,
      survives_our_latency_rate:
        acc.survives_our_latency_evaluable > 0
          ? acc.survives_our_latency_positive / acc.survives_our_latency_evaluable
          : 0,
    }))
    .sort(
      (a, b) =>
        b.survives_our_latency_positive - a.survives_our_latency_positive ||
        b.positive_anytime - a.positive_anytime ||
        b.impulses - a.impulses,
    );
}

async function main() {
  const args = parseArgs(process.argv);
  const inputPath = path.resolve(args.input);
  const outputPath = args.output ? path.resolve(args.output) : null;
  const rowsOutputPath = args.rowsOutput ? path.resolve(args.rowsOutput) : null;
  const latencyProfile = args.latencyProfile
    ? parseLatencyProfile(path.resolve(args.latencyProfile))
    : null;

  const observations = new Map();

  const stream = fs.createReadStream(inputPath, { encoding: "utf8" });
  const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });

  for await (const line of rl) {
    if (!line.trim()) {
      continue;
    }
    const row = JSON.parse(line);
    const observationId = row.observation_id;
    if (observationId == null) {
      continue;
    }
    const observation = getOrCreate(observations, observationId, () => ({
      rollout: null,
      routeLagRows: [],
    }));
    if (row.record_type === "rollout_shadow") {
      observation.rollout = row;
    } else if (row.record_type === "ps_p0_route_lag_markout") {
      observation.routeLagRows.push(row);
    }
  }

  const routeSyncRows = [];
  for (const observation of observations.values()) {
    if (observation.routeLagRows.length === 0) {
      continue;
    }
    const groups = new Map();
    for (const row of observation.routeLagRows) {
      const sourcePool = stableSourcePoolId(row, observation.rollout || {});
      const linkedPool = stableLinkedPoolId(row);
      const groupKey = [
        row.observation_id,
        sourcePool,
        linkedPool,
        row.route_family_label || row.route_family || "unknown",
        row.comparison_venue || "unknown",
      ].join("|");
      getOrCreate(groups, groupKey, () => []).push(row);
    }
    for (const groupRows of groups.values()) {
      const analyzed = analyzeImpulse(
        groupRows,
        observation.rollout || {},
        args,
        latencyProfile,
      );
      if (analyzed) {
        routeSyncRows.push(analyzed);
      }
    }
  }

  routeSyncRows.sort(
    (a, b) =>
      Number(b.survives_our_latency === true) - Number(a.survives_our_latency === true) ||
      b.peak_net_gap_lamports - a.peak_net_gap_lamports ||
      a.t_first_response_us - b.t_first_response_us,
  );

  const curveAccumulators = new Map(
    args.latencyBucketsUs.map((latencyUs) => [
      latencyUs,
      createCurveAccumulator(latencyUs),
    ]),
  );
  const byPair = new Map();
  const byRouteFamily = new Map();
  const byVenuePair = new Map();

  for (const row of routeSyncRows) {
    for (const bucket of row.survival_buckets) {
      const acc = curveAccumulators.get(bucket.latency_us);
      if (!acc || bucket.positive == null) {
        continue;
      }
      acc.evaluable_count += 1;
      if (bucket.positive) {
        acc.positive_count += 1;
      } else {
        acc.non_positive_count += 1;
      }
    }

    const pairKey = [
      row.venue_a,
      row.source_pool,
      row.venue_b,
      row.linked_pool,
      row.route_family,
    ].join("|");
    const routeKey = row.route_family;
    const venuePairKey = [row.venue_a, row.venue_b, row.route_family].join("|");
    updateGroupAccumulator(getOrCreate(byPair, pairKey, createGroupAccumulator), row);
    updateGroupAccumulator(
      getOrCreate(byRouteFamily, routeKey, createGroupAccumulator),
      row,
    );
    updateGroupAccumulator(
      getOrCreate(byVenuePair, venuePairKey, createGroupAccumulator),
      row,
    );
  }

  const report = {
    input_path: inputPath,
    route_sync_impulse_count: routeSyncRows.length,
    latency_profile_path: args.latencyProfile
      ? path.resolve(args.latencyProfile)
      : null,
    latency_stage_preference: args.latencyStage,
    latency_quantile: args.latencyQuantile,
    latency_buckets_us: args.latencyBucketsUs,
    study_notes: {
      focus:
        "Per impulse / linked-pair executable route-gap survival after fees, not a single global pool synchronization number.",
      executable_gap_proxy:
        "Uses ps_p0_route_lag_markout.net_markout_after_fee_lamports as the current executable after-fee gap proxy.",
      gap_bps_note:
        "gap_bps_t0 is null until upstream tape includes honest bps-grade gap inputs.",
      clock_note:
        "Survival is evaluated on event, slot, and wall-clock fields from the tape; between linked events the latest observed state is treated as persisting.",
      clock_order_note:
        "clock_order_inversion=true means the saved tape had markout_unix_ns earlier than t0_unix_ns; treat those rows as dirty-clock evidence rather than true zero-latency sync.",
      latency_note:
        "When a latency profile is supplied, survival is compared against the chosen per-source stage/quantile; otherwise survives_our_latency remains null unless a manual override is supplied.",
    },
    summary: {
      positive_on_first_response_count: routeSyncRows.filter(
        (row) => row.net_gap_lamports_t0 > 0,
      ).length,
      positive_anytime_count: routeSyncRows.filter(
        (row) => row.peak_net_gap_lamports > 0,
      ).length,
      break_even_observed_count: routeSyncRows.filter(
        (row) => row.t_break_even_us != null,
      ).length,
      survives_our_latency_positive_count: routeSyncRows.filter(
        (row) => row.survives_our_latency === true,
      ).length,
      survives_our_latency_evaluable_count: routeSyncRows.filter(
        (row) => row.survives_our_latency != null,
      ).length,
      clock_order_inversion_count: routeSyncRows.filter(
        (row) => row.clock_order_inversion === true,
      ).length,
    },
    survival_curve: Array.from(curveAccumulators.values()).map(finalizeCurveAccumulator),
    by_pair: finalizeGroupMap(byPair).slice(0, 200),
    by_route_family: finalizeGroupMap(byRouteFamily),
    by_venue_pair: finalizeGroupMap(byVenuePair),
    route_sync_impulses: routeSyncRows,
  };

  if (rowsOutputPath) {
    const rowsOutput = routeSyncRows
      .map((row) => JSON.stringify(row))
      .join("\n");
    fs.writeFileSync(
      rowsOutputPath,
      rowsOutput.length > 0 ? `${rowsOutput}\n` : "",
      "utf8",
    );
  }

  const output = `${JSON.stringify(report, null, 2)}\n`;
  if (outputPath) {
    fs.writeFileSync(outputPath, output, "utf8");
  } else {
    process.stdout.write(output);
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
