#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const readline = require("readline");

function usage() {
  console.error(
    [
      "Usage: node run\\ps_p0_strict_replay_report.js --tape <shock.jsonl> --pairs <run26_same_pool_d_by_pair.json> --isolation <run26_same_pool_d_isolation.json> [options]",
      "",
      "Builds a strict-replay report for isolated same-pool D candidates.",
      "",
      "Options:",
      "  --output <report.json>                 Optional output path",
      "  --max-live-trade-lamports <value>     Default: 50000000 (0.05 SOL)",
      "  --max-notional-lamports <value>       Default: 100000000 (0.1 SOL)",
      "  --required-source <label>             Default: afxdp",
      "",
      "Notes:",
      "  - retained_ev_lamports_lcb is already post-tip in the tape",
      "  - rollout_shadow.net_markout_after_fee_lamports is post-tip minus base public fee",
      "  - this report treats ShadowGuard/freshness as recorded proxies from rollout fields",
      "  - PM release truth remains unresolved when final outcomes stay pending_observe_only",
    ].join("\n"),
  );
}

function parseArgs(argv) {
  const args = {
    tape: null,
    pairs: null,
    isolation: null,
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
    if (current === "--isolation" && i + 1 < argv.length) {
      args.isolation = argv[++i];
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
  if (!args.tape || !args.pairs || !args.isolation) {
    usage();
    throw new Error("missing required arguments");
  }
  if (!Number.isFinite(args.maxLiveTradeLamports) || args.maxLiveTradeLamports < 0) {
    throw new Error("invalid --max-live-trade-lamports");
  }
  if (!Number.isFinite(args.maxNotionalLamports) || args.maxNotionalLamports < 0) {
    throw new Error("invalid --max-notional-lamports");
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

function unorderedMintPairKey(venue, poolPubkey, mintIn, mintOut) {
  const pair = [mintIn || "null", mintOut || "null"].sort();
  return [venue, poolPubkey || "null", pair[0], pair[1]].join("|");
}

function strictReplayLabel(summary) {
  const hasRecordedPositiveNow = summary.exact_d_rows.t0_positive_after_fee_count > 0;
  const hasCapCompatiblePositiveNow =
    summary.exact_d_rows.t0_positive_after_fee_and_caps_count > 0 &&
    summary.exact_d_rows.required_source_pass_count > 0 &&
    summary.exact_d_rows.route_family_public_scout_compatible_count > 0 &&
    summary.exact_d_rows.dominance_would_drop_exact_count === 0;
  const hasDelayedPositive =
    summary.landing_delay_sensitivity.exact_d_same_pool_horizons.some(
      (row) =>
        row.horizon.startsWith("same_pool_event_plus_") && row.positive_count > 0,
    );
  const repeated = summary.repeatability.run26_same_pool.classes.D > 1;
  const nearCap =
    summary.trade_size_stats.max <= Math.round(summary.policy.max_live_trade_lamports * 1.25);

  if (
    hasCapCompatiblePositiveNow &&
    repeated &&
    summary.guard_freshness_inventory_pm.pm_release_truth === "available"
  ) {
    return "HUNT";
  }
  if (
    hasRecordedPositiveNow &&
    nearCap &&
    summary.exact_d_rows.dominance_would_drop_exact_count === 0
  ) {
    return "WATCH";
  }
  if (hasDelayedPositive || hasRecordedPositiveNow) {
    return "TRAINING";
  }
  return "DENY";
}

async function loadTape(tapePath) {
  const rollouts = new Map();
  const markoutsByObservation = new Map();
  const finalOutcomes = new Map();

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
    switch (row.record_type) {
      case "rollout_shadow":
        rollouts.set(id, row);
        break;
      case "ps_p0_markout":
        if (!markoutsByObservation.has(id)) {
          markoutsByObservation.set(id, []);
        }
        markoutsByObservation.get(id).push(row);
        break;
      case "ps_p0_final_outcome":
        finalOutcomes.set(id, row);
        break;
      default:
        break;
    }
  }

  return { rollouts, markoutsByObservation, finalOutcomes };
}

async function main() {
  const args = parseArgs(process.argv);
  const tapePath = path.resolve(args.tape);
  const pairsPath = path.resolve(args.pairs);
  const isolationPath = path.resolve(args.isolation);
  const outputPath = args.output ? path.resolve(args.output) : null;

  const pairsInput = JSON.parse(fs.readFileSync(pairsPath, "utf8"));
  const isolationInput = JSON.parse(fs.readFileSync(isolationPath, "utf8"));
  const exactDByObservationId = new Map(
    isolationInput.exactRun26SamePoolDRows.map((row) => [String(row.observationId), row]),
  );
  const tape = await loadTape(tapePath);

  const pairReports = [];
  for (const pair of pairsInput.groupsByDurablePoolMintPair) {
    const exactObservationIds = pair.exactRun26ObservationIds.map((value) => String(value));
    const rolloutRows = exactObservationIds
      .map((id) => tape.rollouts.get(id))
      .filter(Boolean);
    const exactDRows = exactObservationIds
      .map((id) => exactDByObservationId.get(id))
      .filter(Boolean);
    const markoutRows = exactObservationIds.flatMap(
      (id) => tape.markoutsByObservation.get(id) || [],
    );
    const finalRows = exactObservationIds
      .map((id) => tape.finalOutcomes.get(id) || tape.rollouts.get(id))
      .filter(Boolean);

    const tradeSizeStats = createStats(
      rolloutRows.map((row) => Number(row.trade_size_lamports || 0)),
    );
    const reserveStats = createStats(
      rolloutRows.map((row) => Number(row.would_reserve_lamports || 0)),
    );
    const tipStats = createStats(rolloutRows.map((row) => Number(row.tip_lamports || 0)));
    const retainedStats = createStats(
      rolloutRows.map((row) => Number(row.retained_ev_lamports_lcb || 0)),
    );
    const t0NetAfterFeeStats = createStats(
      rolloutRows.map((row) => Number(row.net_markout_after_fee_lamports || 0)),
    );
    const strictBestNetAfterFeeStats = createStats(
      exactDRows.map((row) => Number(row.bestNetAfterFee || 0)),
    );
    const slippageHeadroomStats = createStats(
      rolloutRows.map((row) => {
        const expected = Number(row.quote_expected_out_lamports || 0);
        const minOut = Number(row.min_out_lamports || 0);
        return expected - minOut;
      }),
    );

    const t0PositiveAfterFeeCount = rolloutRows.filter(
      (row) => Number(row.net_markout_after_fee_lamports || 0) > 0,
    ).length;
    const t0PositiveAfterFeeAndCapsCount = rolloutRows.filter(
      (row) =>
        Number(row.net_markout_after_fee_lamports || 0) > 0 &&
        Number(row.trade_size_lamports || 0) <= args.maxLiveTradeLamports &&
        Number(row.would_reserve_lamports || 0) <= args.maxNotionalLamports,
    ).length;
    const exactCapCompatibleCount = rolloutRows.filter(
      (row) =>
        Number(row.trade_size_lamports || 0) <= args.maxLiveTradeLamports &&
        Number(row.would_reserve_lamports || 0) <= args.maxNotionalLamports,
    ).length;
    const requiredSourcePassCount = rolloutRows.filter(
      (row) => row.signal_source === args.requiredSource,
    ).length;
    const routeFamilyPublicScoutCompatibleCount = 0;
    const routeLegalityRecordedCount = rolloutRows.filter(
      (row) => row.would_build === true && row.would_sign === true,
    ).length;
    const shadowGuardFreshnessProxyPassCount = rolloutRows.filter(
      (row) =>
        row.would_build === true &&
        row.would_sign === true &&
        row.stale_state_reroute !== true &&
        row.egress_gate_drop !== true &&
        row.egress_gate_reroute !== true,
    ).length;
    const dominanceWouldDropExactCount = rolloutRows.filter(
      (row) => row.drop_reason === "would_drop_dominance_gate",
    ).length;
    const pmReleaseTruthAvailable = finalRows.some(
      (row) => row.final_realized_outcome && row.final_realized_outcome !== "pending_observe_only",
    );

    const horizons = new Map();
    for (const row of markoutRows) {
      const horizon = row.horizon || "unknown";
      if (!horizons.has(horizon)) {
        horizons.set(horizon, []);
      }
      horizons.get(horizon).push(Number(row.net_markout_after_fee_lamports || 0));
    }
    const horizonBreakdown = Array.from(horizons.entries())
      .map(([horizon, values]) => ({
        horizon,
        positive_count: values.filter((value) => value > 0).length,
        stats: createStats(values),
      }))
      .sort((a, b) => a.horizon.localeCompare(b.horizon));

    const summary = {
      key: pair.key,
      venue: pair.venue,
      pool_pubkey: pair.poolPubkey,
      mint_pair: pair.mintPair,
      exact_d_observation_ids: pair.exactRun26ObservationIds,
      policy: {
        max_live_trade_lamports: args.maxLiveTradeLamports,
        max_notional_lamports: args.maxNotionalLamports,
        required_source: args.requiredSource,
      },
      repeatability: pair.crossRun,
      exact_d_rows: {
        count: rolloutRows.length,
        t0_positive_after_fee_count: t0PositiveAfterFeeCount,
        t0_positive_after_fee_and_caps_count: t0PositiveAfterFeeAndCapsCount,
        cap_compatible_count: exactCapCompatibleCount,
        required_source_pass_count: requiredSourcePassCount,
        route_family_public_scout_compatible_count: routeFamilyPublicScoutCompatibleCount,
        route_legality_recorded_count: routeLegalityRecordedCount,
        shadow_guard_freshness_proxy_pass_count: shadowGuardFreshnessProxyPassCount,
        dominance_would_drop_exact_count: dominanceWouldDropExactCount,
        landed_count: rolloutRows.filter((row) => row.landed === true).length,
      },
      trade_size_stats: tradeSizeStats,
      reserve_stats: reserveStats,
      tip_stats: tipStats,
      retained_ev_post_tip_stats: retainedStats,
      strict_best_net_after_fee_stats: strictBestNetAfterFeeStats,
      t0_net_after_fee_stats: t0NetAfterFeeStats,
      slippage_headroom_lamports_stats: slippageHeadroomStats,
      recorded_fee_shape: {
        slippage_bps: groupCounts(rolloutRows, "quote_slippage_bps"),
        trade_fee_ratio: rolloutRows.length
          ? groupCounts(
              rolloutRows.map((row) => ({
                quote_trade_fee_ratio: `${row.quote_trade_fee_num}/${row.quote_trade_fee_denom}`,
              })),
              "quote_trade_fee_ratio",
            )
          : [],
        transfer_fee_bps: groupCounts(rolloutRows, "quote_mint_in_transfer_fee_bps"),
      },
      recorded_paths: {
        route_family_counts: groupCounts(rolloutRows, "route_family_label"),
        signal_source_counts: groupCounts(rolloutRows, "signal_source"),
        selected_owner_counts: groupCounts(rolloutRows, "selected_owner"),
        fallback_owner_counts: groupCounts(rolloutRows, "fallback_owner"),
        drop_reason_counts: groupCounts(rolloutRows, "drop_reason"),
        final_outcome_counts: groupCounts(finalRows, "final_realized_outcome"),
      },
      landing_delay_sensitivity: {
        exact_d_same_pool_horizons: horizonBreakdown,
      },
      route_legality: {
        recorded_path_proven_for_build_sign: routeLegalityRecordedCount === rolloutRows.length,
        recorded_route_family_counts: groupCounts(rolloutRows, "route_family_label"),
        public_scout_route_proven: false,
        note:
          "All isolated rows were recorded on jito_bundle. Public-scout route legality is not proven by this observe-only tape.",
      },
      guard_freshness_inventory_pm: {
        shadow_guard_proxy_pass_count: shadowGuardFreshnessProxyPassCount,
        stale_state_reroute_count: rolloutRows.filter((row) => row.stale_state_reroute === true).length,
        egress_gate_drop_count: rolloutRows.filter((row) => row.egress_gate_drop === true).length,
        egress_gate_reroute_count: rolloutRows.filter((row) => row.egress_gate_reroute === true).length,
        inventory_truth: "unresolved_in_observe_only_tape",
        pm_release_truth: pmReleaseTruthAvailable
          ? "available"
          : "pending_observe_only_unresolved",
      },
    };

    summary.final_label = strictReplayLabel(summary);
    summary.rationale = [];
    if (summary.exact_d_rows.t0_positive_after_fee_and_caps_count === 0) {
      summary.rationale.push(
        "No exact Run26 D rows stayed positive at t0 while also fitting the current tiny public-scout trade and notional caps.",
      );
    }
    if (summary.exact_d_rows.required_source_pass_count === 0) {
      summary.rationale.push(
        `All exact D rows came from '${args.requiredSource === "afxdp" ? "external_geyser, not afxdp" : "a non-required source"}', so the current source requirement does not pass.`,
      );
    }
    if (summary.route_legality.public_scout_route_proven === false) {
      summary.rationale.push(
        "The tape only proves jito_bundle build/sign legality; public-scout legality is still unproven.",
      );
    }
    if (summary.guard_freshness_inventory_pm.pm_release_truth !== "available") {
      summary.rationale.push(
        "PM release truth is unresolved because final outcomes stayed pending_observe_only in this tape.",
      );
    }
    if (summary.exact_d_rows.dominance_would_drop_exact_count > 0) {
      summary.rationale.push(
        `${summary.exact_d_rows.dominance_would_drop_exact_count} exact D rows still carried would_drop_dominance_gate.`,
      );
    }
    if (summary.final_label === "WATCH") {
      summary.rationale.push(
        "Recorded-path economics were positive, but the pair is still blocked by current deployment mismatches or low repeatability.",
      );
    }
    if (summary.final_label === "TRAINING") {
      summary.rationale.push(
        "The pair shows a real post-shock phenomenon, but strict replay says it is not yet a current tiny live candidate.",
      );
    }

    pairReports.push(summary);
  }

  const report = {
    generated_at: new Date().toISOString(),
    input_paths: {
      tape: tapePath,
      pairs: pairsPath,
      isolation: isolationPath,
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
