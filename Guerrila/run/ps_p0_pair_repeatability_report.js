#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const readline = require("readline");

function usage() {
  console.error(
    [
      "Usage: node run\\ps_p0_pair_repeatability_report.js --tape <shock.jsonl> --pairs <run26_same_pool_d_by_pair.json> [options]",
      "",
      "Builds a focused repeatability report for isolated same-pool Raydium leads.",
      "",
      "Options:",
      "  --output <report.json>                 Optional output path",
      "  --max-live-trade-lamports <value>     Default: 50000000 (0.05 SOL)",
      "  --max-notional-lamports <value>       Default: 100000000 (0.1 SOL)",
      "  --required-source <label>             Default: afxdp",
    ].join("\n"),
  );
}

function parseArgs(argv) {
  const args = {
    tape: null,
    pairs: null,
    output: null,
    maxLiveTradeLamports: 50_000_000,
    maxNotionalLamports: 100_000_000,
    requiredSource: "afxdp",
  };
  for (let i = 2; i < argv.length; i += 1) {
    const current = argv[i];
    if (current === "--tape" && i + 1 < argv.length) {
      args.tape = argv[++i];
      continue;
    }
    if (current === "--pairs" && i + 1 < argv.length) {
      args.pairs = argv[++i];
      continue;
    }
    if (current === "--output" && i + 1 < argv.length) {
      args.output = argv[++i];
      continue;
    }
    if (current === "--max-live-trade-lamports" && i + 1 < argv.length) {
      args.maxLiveTradeLamports = Number(argv[++i]);
      continue;
    }
    if (current === "--max-notional-lamports" && i + 1 < argv.length) {
      args.maxNotionalLamports = Number(argv[++i]);
      continue;
    }
    if (current === "--required-source" && i + 1 < argv.length) {
      args.requiredSource = argv[++i];
      continue;
    }
    if (current === "--help" || current === "-h") {
      usage();
      process.exit(0);
    }
    throw new Error(`unknown argument: ${current}`);
  }
  if (!args.tape || !args.pairs) {
    usage();
    throw new Error("missing required arguments");
  }
  return args;
}

function createStats(values) {
  const sorted = [...values].sort((a, b) => a - b);
  if (sorted.length === 0) {
    return {
      count: 0,
      min: 0,
      p50: 0,
      p90: 0,
      max: 0,
      avg: 0,
    };
  }
  const quantile = (q) => {
    if (sorted.length === 1) {
      return sorted[0];
    }
    const idx = (sorted.length - 1) * q;
    const lo = Math.floor(idx);
    const hi = Math.ceil(idx);
    if (lo === hi) {
      return sorted[lo];
    }
    const weight = idx - lo;
    return Math.round(sorted[lo] * (1 - weight) + sorted[hi] * weight);
  };
  return {
    count: sorted.length,
    min: sorted[0],
    p50: quantile(0.5),
    p90: quantile(0.9),
    max: sorted[sorted.length - 1],
    avg: Math.round(sorted.reduce((sum, value) => sum + value, 0) / sorted.length),
  };
}

function groupCounts(rows, field) {
  const map = new Map();
  for (const row of rows) {
    const key = row[field] ?? "null";
    map.set(key, (map.get(key) || 0) + 1);
  }
  return Array.from(map.entries())
    .map(([key, count]) => ({ key, count }))
    .sort((a, b) => b.count - a.count || String(a.key).localeCompare(String(b.key)));
}

function classifyObservation(t0RetainedEv, bestForwardMarkout, bestNetAfterFee, landableUnderCap) {
  const positiveBeforeFees = t0RetainedEv > 0 || bestForwardMarkout > 0;
  if (bestNetAfterFee > 0) {
    return landableUnderCap ? "D" : "C";
  }
  if (positiveBeforeFees) {
    return "B";
  }
  return "A";
}

function pairKey(venue, poolPubkey, mintIn, mintOut) {
  const pair = [mintIn || "null", mintOut || "null"].sort();
  return [venue || "unknown", poolPubkey || "null", pair[0], pair[1]].join("|");
}

function isPublicScoutCompatibleRouteFamily(routeFamily) {
  return routeFamily === "public_scout" || routeFamily === "public_rpc";
}

function repeatabilityLabel(summary) {
  const repeatedD = summary.independent_window.class_counts.D > 0;
  const positiveTinyCap = summary.independent_window.positive_after_fee_and_tiny_cap_count > 0;
  const positiveAny =
    summary.independent_window.positive_t0_after_fee_count > 0 ||
    summary.independent_window.positive_after_fee_count > 0;
  const dominanceBlocked = summary.independent_window.dominance_would_drop_count > 0;
  const sourcePass = summary.independent_window.required_source_pass_count > 0;
  const publicRoutePass = summary.independent_window.public_route_compatible_count > 0;
  const pmTruthAvailable =
    summary.independent_window.final_outcome_distribution.some(
      (row) => row.key !== "pending_observe_only" && row.key !== "null",
    );

  if (repeatedD && positiveTinyCap && !dominanceBlocked && sourcePass && publicRoutePass && pmTruthAvailable) {
    return "HUNT";
  }
  if (repeatedD || positiveTinyCap) {
    return "WATCH";
  }
  if (positiveAny || summary.independent_window.observed_count > 0) {
    return "TRAINING";
  }
  return "DENY";
}

async function loadSamePoolRows(tapePath) {
  const observations = new Map();
  const rl = readline.createInterface({
    input: fs.createReadStream(tapePath, { encoding: "utf8" }),
    crlfDelay: Infinity,
  });

  for await (const line of rl) {
    if (!line.trim()) {
      continue;
    }
    const row = JSON.parse(line);
    if (row.observation_id == null) {
      continue;
    }
    const id = String(row.observation_id);
    if (!observations.has(id)) {
      observations.set(id, {
        rollout: null,
        finalOutcome: null,
        samePoolMarkouts: [],
      });
    }
    const observation = observations.get(id);
    switch (row.record_type) {
      case "rollout_shadow":
        observation.rollout = row;
        break;
      case "ps_p0_markout":
        observation.samePoolMarkouts.push(row);
        break;
      case "ps_p0_final_outcome":
        observation.finalOutcome = row;
        break;
      default:
        break;
    }
  }

  const grouped = new Map();
  for (const [observationId, observation] of observations.entries()) {
    if (!observation.rollout || observation.samePoolMarkouts.length === 0) {
      continue;
    }
    let bestForwardMarkout = Number.NEGATIVE_INFINITY;
    let bestNetAfterFee = Number.NEGATIVE_INFINITY;
    let bestForwardHorizon = null;
    let bestNetAfterFeeHorizon = null;
    for (const markout of observation.samePoolMarkouts) {
      const forward = Number(markout.markout_lamports || 0);
      const net = Number(markout.net_markout_after_fee_lamports || 0);
      if (forward > bestForwardMarkout) {
        bestForwardMarkout = forward;
        bestForwardHorizon = markout.horizon || null;
      }
      if (net > bestNetAfterFee) {
        bestNetAfterFee = net;
        bestNetAfterFeeHorizon = markout.horizon || null;
      }
    }

    const rollout = observation.rollout;
    const firstMarkout = observation.samePoolMarkouts[0];
    const landed = observation.finalOutcome?.landed === true;
    const landableUnderCap =
      landed || rollout.would_send === true || rollout.live_send_suppressed === true;
    const t0RetainedEv = Number(
      firstMarkout.t0_retained_ev_lamports_lcb ?? rollout.t0_retained_ev_lamports_lcb ?? 0,
    );
    const motifRow = {
      observation_id: observationId,
      venue: firstMarkout.venue || rollout.venue || "unknown",
      pool_pubkey: firstMarkout.pool_pubkey_hex || rollout.pool_pubkey_hex || null,
      mint_in: firstMarkout.mint_in_hex || rollout.mint_in_hex || null,
      mint_out: firstMarkout.mint_out_hex || rollout.mint_out_hex || null,
      pool_index: firstMarkout.pool_index || null,
      best_forward_markout_lamports:
        bestForwardMarkout === Number.NEGATIVE_INFINITY ? 0 : bestForwardMarkout,
      best_net_after_fee_lamports:
        bestNetAfterFee === Number.NEGATIVE_INFINITY ? 0 : bestNetAfterFee,
      best_forward_horizon: bestForwardHorizon,
      best_net_after_fee_horizon: bestNetAfterFeeHorizon,
      t0_retained_ev_lamports_lcb: t0RetainedEv,
      landable_under_cap: landableUnderCap,
      classification: classifyObservation(
        t0RetainedEv,
        bestForwardMarkout === Number.NEGATIVE_INFINITY ? 0 : bestForwardMarkout,
        bestNetAfterFee === Number.NEGATIVE_INFINITY ? 0 : bestNetAfterFee,
        landableUnderCap,
      ),
      rollout,
      finalOutcome: observation.finalOutcome || rollout,
    };
    const key = pairKey(motifRow.venue, motifRow.pool_pubkey, motifRow.mint_in, motifRow.mint_out);
    if (!grouped.has(key)) {
      grouped.set(key, []);
    }
    grouped.get(key).push(motifRow);
  }

  return grouped;
}

async function main() {
  const args = parseArgs(process.argv);
  const tapePath = path.resolve(args.tape);
  const pairsPath = path.resolve(args.pairs);
  const outputPath = args.output ? path.resolve(args.output) : null;

  const pairsInput = JSON.parse(fs.readFileSync(pairsPath, "utf8"));
  const grouped = await loadSamePoolRows(tapePath);

  const pairReports = pairsInput.groupsByDurablePoolMintPair.map((pair) => {
    const key = pairKey(pair.venue, pair.poolPubkey, pair.mintPair[0], pair.mintPair[1]);
    const rows = grouped.get(key) || [];
    const rolloutRows = rows.map((row) => row.rollout);
    const finalRows = rows.map((row) => row.finalOutcome);
    const observedCount = rows.length;
    const classCounts = { A: 0, B: 0, C: 0, D: 0 };
    for (const row of rows) {
      classCounts[row.classification] += 1;
    }

    const positiveT0AfterFeeCount = rolloutRows.filter(
      (row) => Number(row.net_markout_after_fee_lamports || 0) > 0,
    ).length;
    const positiveAfterFeeCount = rows.filter(
      (row) => Number(row.best_net_after_fee_lamports || 0) > 0,
    ).length;
    const positiveAfterFeeAndTinyCapCount = rows.filter((row) => {
      const trade = Number(row.rollout.trade_size_lamports || 0);
      const reserve = Number(row.rollout.would_reserve_lamports || 0);
      return (
        Number(row.best_net_after_fee_lamports || 0) > 0 &&
        trade <= args.maxLiveTradeLamports &&
        reserve <= args.maxNotionalLamports
      );
    }).length;
    const dominanceWouldDropCount = rolloutRows.filter(
      (row) => row.drop_reason === "would_drop_dominance_gate",
    ).length;
    const requiredSourcePassCount = rolloutRows.filter(
      (row) => row.signal_source === args.requiredSource,
    ).length;
    const publicRouteCompatibleCount = rolloutRows.filter((row) =>
      isPublicScoutCompatibleRouteFamily(row.route_family_label),
    ).length;

    const summary = {
      key,
      venue: pair.venue,
      pool_pubkey: pair.poolPubkey,
      mint_pair: pair.mintPair,
      policy: {
        max_live_trade_lamports: args.maxLiveTradeLamports,
        max_notional_lamports: args.maxNotionalLamports,
        required_source: args.requiredSource,
      },
      historical_repeatability: pair.crossRun,
      independent_window: {
        repeated_in_window: observedCount > 0,
        repeated_d_in_window: classCounts.D > 0,
        observed_count: observedCount,
        class_counts: classCounts,
        positive_t0_after_fee_count: positiveT0AfterFeeCount,
        positive_after_fee_count: positiveAfterFeeCount,
        positive_after_fee_and_tiny_cap_count: positiveAfterFeeAndTinyCapCount,
        landable_under_cap_count: rows.filter((row) => row.landable_under_cap).length,
        dominance_would_drop_count: dominanceWouldDropCount,
        required_source_pass_count: requiredSourcePassCount,
        public_route_compatible_count: publicRouteCompatibleCount,
        source_distribution: groupCounts(rolloutRows, "signal_source"),
        route_family_distribution: groupCounts(rolloutRows, "route_family_label"),
        selected_owner_distribution: groupCounts(rolloutRows, "selected_owner"),
        final_outcome_distribution: groupCounts(finalRows, "final_realized_outcome"),
        net_after_fee_lamports: createStats(
          rows.map((row) => Number(row.best_net_after_fee_lamports || 0)),
        ),
        t0_net_after_fee_lamports: createStats(
          rolloutRows.map((row) => Number(row.net_markout_after_fee_lamports || 0)),
        ),
      },
    };

    summary.label = repeatabilityLabel(summary);
    summary.rationale = [];
    if (summary.independent_window.observed_count === 0) {
      summary.rationale.push("The pair did not appear in this independent window.");
    } else {
      summary.rationale.push(
        `${summary.independent_window.observed_count} same-pool observations matched this durable pool+mints pair in the new window.`,
      );
    }
    if (summary.independent_window.class_counts.D > 0) {
      summary.rationale.push(
        `${summary.independent_window.class_counts.D} observations still classified as D in the new window.`,
      );
    } else if (summary.independent_window.observed_count > 0) {
      summary.rationale.push("The pair repeated, but no new D observations appeared.");
    }
    if (summary.independent_window.positive_after_fee_and_tiny_cap_count === 0) {
      summary.rationale.push(
        "No repeated same-pool observations were both positive after fee and compatible with the current tiny public-scout cap envelope.",
      );
    }
    if (summary.independent_window.dominance_would_drop_count > 0) {
      summary.rationale.push(
        `${summary.independent_window.dominance_would_drop_count} repeated observations still carried would_drop_dominance_gate.`,
      );
    }
    if (summary.independent_window.required_source_pass_count === 0 && summary.independent_window.observed_count > 0) {
      summary.rationale.push(
        `All repeated observations came from a non-${args.requiredSource} source, so the current source requirement still does not pass.`,
      );
    }
    if (summary.independent_window.public_route_compatible_count === 0 && summary.independent_window.observed_count > 0) {
      summary.rationale.push(
        "The repeated observations did not prove a public-scout/public-rpc route family.",
      );
    }

    return summary;
  });

  const report = {
    generated_at: new Date().toISOString(),
    input_paths: {
      tape: tapePath,
      pairs: pairsPath,
    },
    policy: {
      max_live_trade_lamports: args.maxLiveTradeLamports,
      max_notional_lamports: args.maxNotionalLamports,
      required_source: args.requiredSource,
      note: "Matches current R1-PUBLIC-A tiny public-scout envelope unless overridden.",
    },
    pair_reports: pairReports,
  };

  const text = `${JSON.stringify(report, null, 2)}\n`;
  if (outputPath) {
    fs.writeFileSync(outputPath, text, "utf8");
  } else {
    process.stdout.write(text);
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
