#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const readline = require("readline");

function usage() {
  console.error(
    [
      "Usage: node run\\ps_p0_abcd_report.js --input <tape.jsonl> [--output <report.json>]",
      "",
      "Builds an expanded-surface PS-P0 A/B/C/D report from PM shock research JSONL.",
      "Classes are assigned per observation + motif using:",
      "  A = structural only / no edge",
      "  B = positive before fees, negative after fees",
      "  C = positive after fees, not landable under cap",
      "  D = positive after fees and landable under cap",
      "",
      "Notes:",
      "  - motif=same_pool comes from ps_p0_markout",
      "  - motif=cross_pool_route_lag comes from ps_p0_route_lag_markout",
      "  - landable_under_cap is inferred from rollout_shadow.would_send,",
      "    rollout_shadow.live_send_suppressed, or a landed final outcome",
      "  - prefers explicit pool_class from the tape; falls back to venue-family for older tapes",
    ].join("\n"),
  );
}

function parseArgs(argv) {
  const args = { input: null, output: null };
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
  return args;
}

function poolClassFromVenue(venue) {
  switch (venue) {
    case "raydium_v4":
    case "raydium_cpmm":
    case "raydium_clmm":
      return "raydium";
    case "meteora_damm_v2":
    case "meteora_dlmm":
      return "meteora";
    case "orca":
    case "orca_whirlpool":
      return "orca";
    case "pump_swap":
    case "pump_fun":
      return "pump";
    default:
      return "unknown";
  }
}

function createAccumulator() {
  return {
    observations: 0,
    positive_t0_retained_ev: 0,
    positive_forward_markout: 0,
    positive_net_after_fee: 0,
    landable_under_cap: 0,
    landed: 0,
    classes: { A: 0, B: 0, C: 0, D: 0 },
    sum_t0_retained_ev_lamports: 0,
    sum_best_forward_markout_lamports: 0,
    sum_best_net_after_fee_lamports: 0,
    max_t0_retained_ev_lamports: Number.NEGATIVE_INFINITY,
    max_best_forward_markout_lamports: Number.NEGATIVE_INFINITY,
    max_best_net_after_fee_lamports: Number.NEGATIVE_INFINITY,
  };
}

function updateAccumulator(acc, row) {
  acc.observations += 1;
  acc.positive_t0_retained_ev += row.t0RetainedEv > 0 ? 1 : 0;
  acc.positive_forward_markout += row.bestForwardMarkout > 0 ? 1 : 0;
  acc.positive_net_after_fee += row.bestNetAfterFee > 0 ? 1 : 0;
  acc.landable_under_cap += row.landableUnderCap ? 1 : 0;
  acc.landed += row.landed ? 1 : 0;
  acc.classes[row.classification] += 1;
  acc.sum_t0_retained_ev_lamports += row.t0RetainedEv;
  acc.sum_best_forward_markout_lamports += row.bestForwardMarkout;
  acc.sum_best_net_after_fee_lamports += row.bestNetAfterFee;
  acc.max_t0_retained_ev_lamports = Math.max(
    acc.max_t0_retained_ev_lamports,
    row.t0RetainedEv,
  );
  acc.max_best_forward_markout_lamports = Math.max(
    acc.max_best_forward_markout_lamports,
    row.bestForwardMarkout,
  );
  acc.max_best_net_after_fee_lamports = Math.max(
    acc.max_best_net_after_fee_lamports,
    row.bestNetAfterFee,
  );
}

function finalizeAccumulator(acc) {
  const count = acc.observations || 1;
  return {
    observations: acc.observations,
    positive_t0_retained_ev: acc.positive_t0_retained_ev,
    positive_forward_markout: acc.positive_forward_markout,
    positive_net_after_fee: acc.positive_net_after_fee,
    landable_under_cap: acc.landable_under_cap,
    landed: acc.landed,
    classes: acc.classes,
    avg_t0_retained_ev_lamports: Math.round(
      acc.sum_t0_retained_ev_lamports / count,
    ),
    avg_best_forward_markout_lamports: Math.round(
      acc.sum_best_forward_markout_lamports / count,
    ),
    avg_best_net_after_fee_lamports: Math.round(
      acc.sum_best_net_after_fee_lamports / count,
    ),
    max_t0_retained_ev_lamports:
      acc.max_t0_retained_ev_lamports === Number.NEGATIVE_INFINITY
        ? 0
        : acc.max_t0_retained_ev_lamports,
    max_best_forward_markout_lamports:
      acc.max_best_forward_markout_lamports === Number.NEGATIVE_INFINITY
        ? 0
        : acc.max_best_forward_markout_lamports,
    max_best_net_after_fee_lamports:
      acc.max_best_net_after_fee_lamports === Number.NEGATIVE_INFINITY
        ? 0
        : acc.max_best_net_after_fee_lamports,
  };
}

function getOrCreate(map, key) {
  if (!map.has(key)) {
    map.set(key, createAccumulator());
  }
  return map.get(key);
}

function classifyObservation(t0RetainedEv, bestForwardMarkout, bestNetAfterFee, landable) {
  const positiveBeforeFees = t0RetainedEv > 0 || bestForwardMarkout > 0;
  if (bestNetAfterFee > 0) {
    return landable ? "D" : "C";
  }
  if (positiveBeforeFees) {
    return "B";
  }
  return "A";
}

function finalizeMap(map, sortFn) {
  return Array.from(map.entries())
    .map(([key, acc]) => ({ key, ...finalizeAccumulator(acc) }))
    .sort(sortFn);
}

async function main() {
  const args = parseArgs(process.argv);
  const inputPath = path.resolve(args.input);
  const outputPath = args.output ? path.resolve(args.output) : null;

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

    if (!observations.has(observationId)) {
      observations.set(observationId, {
        rollout: null,
        finalOutcome: null,
        motifs: {
          same_pool: [],
          cross_pool_route_lag: [],
        },
      });
    }
    const observation = observations.get(observationId);

    switch (row.record_type) {
      case "rollout_shadow":
        observation.rollout = row;
        break;
      case "ps_p0_markout":
        observation.motifs.same_pool.push(row);
        break;
      case "ps_p0_route_lag_markout":
        observation.motifs.cross_pool_route_lag.push(row);
        break;
      case "ps_p0_final_outcome":
        observation.finalOutcome = row;
        break;
      default:
        break;
    }
  }

  const overall = createAccumulator();
  const byVenue = new Map();
  const byMotif = new Map();
  const byTacticFamily = new Map();
  const byRouteFamily = new Map();
  const byPoolClass = new Map();
  const byOwnerMode = new Map();
  const byImpulseTier = new Map();
  const bySurface = new Map();
  const byPool = new Map();

  const motifRows = [];
  let rolloutObservations = 0;

  for (const [observationId, observation] of observations.entries()) {
    if (observation.rollout) {
      rolloutObservations += 1;
    }

    const rollout = observation.rollout || {};
    const finalOutcome = observation.finalOutcome || {};
    const landed = finalOutcome.landed === true;
    const landableUnderCap = landed || rollout.would_send === true || rollout.live_send_suppressed === true;

    for (const [motif, markouts] of Object.entries(observation.motifs)) {
      if (markouts.length === 0) {
        continue;
      }

      let bestForward = Number.NEGATIVE_INFINITY;
      let bestNetAfterFee = Number.NEGATIVE_INFINITY;
      let bestForwardHorizon = null;
      let bestNetAfterFeeHorizon = null;

      for (const markout of markouts) {
        if (markout.markout_lamports > bestForward) {
          bestForward = markout.markout_lamports;
          bestForwardHorizon = markout.horizon;
        }
        if (markout.net_markout_after_fee_lamports > bestNetAfterFee) {
          bestNetAfterFee = markout.net_markout_after_fee_lamports;
          bestNetAfterFeeHorizon = markout.horizon;
        }
      }

      const firstMarkout = markouts[0];
      const venue = firstMarkout.venue || rollout.venue || "unknown";
      const tacticFamily =
        firstMarkout.tactic_family_label || rollout.tactic_family_label || "unknown";
      const routeFamily =
        firstMarkout.route_family_label || rollout.route_family_label || "unknown";
      const poolClass = rollout.pool_class || firstMarkout.pool_class || poolClassFromVenue(venue);
      const ownerMode =
        firstMarkout.selected_owner ||
        firstMarkout.owner_mode ||
        rollout.selected_owner ||
        rollout.owner_mode ||
        "unknown";
      const fallbackOwner =
        firstMarkout.fallback_owner ||
        firstMarkout.fallback_mode ||
        rollout.fallback_owner ||
        rollout.fallback_mode ||
        "unknown";
      const impulseTier =
        firstMarkout.impulse_tier || rollout.impulse_tier || "unknown";
      const impulseType =
        firstMarkout.impulse_type || rollout.impulse_type || "unknown";
      const impulseDirection =
        firstMarkout.impulse_direction || rollout.impulse_direction || "flat";
      const poolPubkey =
        firstMarkout.pool_pubkey_hex || rollout.pool_pubkey_hex || null;
      const mintIn =
        firstMarkout.mint_in_hex || rollout.mint_in_hex || null;
      const mintOut =
        firstMarkout.mint_out_hex || rollout.mint_out_hex || null;
      const t0RetainedEv =
        firstMarkout.t0_retained_ev_lamports_lcb ??
        rollout.t0_retained_ev_lamports_lcb ??
        0;
      const classification = classifyObservation(
        t0RetainedEv,
        bestForward === Number.NEGATIVE_INFINITY ? 0 : bestForward,
        bestNetAfterFee === Number.NEGATIVE_INFINITY ? 0 : bestNetAfterFee,
        landableUnderCap,
      );

      const motifRow = {
        observationId,
        poolIndex: firstMarkout.pool_index,
        venue,
        motif,
        tacticFamily,
        routeFamily,
        poolClass,
        ownerMode,
        fallbackOwner,
        impulseTier,
        impulseType,
        impulseDirection,
        poolPubkey,
        mintIn,
        mintOut,
        t0RetainedEv,
        bestForwardMarkout:
          bestForward === Number.NEGATIVE_INFINITY ? 0 : bestForward,
        bestNetAfterFee:
          bestNetAfterFee === Number.NEGATIVE_INFINITY ? 0 : bestNetAfterFee,
        bestForwardHorizon,
        bestNetAfterFeeHorizon,
        landableUnderCap,
        landed,
        wouldSend: rollout.would_send === true,
        liveSendSuppressed: rollout.live_send_suppressed === true,
        finalOutcome: finalOutcome.final_realized_outcome || "unknown",
        classification,
      };

      motifRows.push(motifRow);
      updateAccumulator(overall, motifRow);
      updateAccumulator(getOrCreate(byVenue, venue), motifRow);
      updateAccumulator(getOrCreate(byMotif, motif), motifRow);
      updateAccumulator(getOrCreate(byTacticFamily, tacticFamily), motifRow);
      updateAccumulator(getOrCreate(byRouteFamily, routeFamily), motifRow);
      updateAccumulator(getOrCreate(byPoolClass, poolClass), motifRow);
      updateAccumulator(getOrCreate(byOwnerMode, ownerMode), motifRow);
      updateAccumulator(getOrCreate(byImpulseTier, impulseTier), motifRow);

      const surfaceKey = [
        venue,
        motif,
        ownerMode,
        tacticFamily,
        routeFamily,
        poolClass,
      ].join("|");
      updateAccumulator(getOrCreate(bySurface, surfaceKey), motifRow);

      const poolKey = [
        venue,
        poolPubkey || firstMarkout.pool_index,
        motif,
        routeFamily,
      ].join("|");
      updateAccumulator(getOrCreate(byPool, poolKey), motifRow);
    }
  }

  const report = {
    input_path: inputPath,
    rollout_observations: rolloutObservations,
    motif_observations: motifRows.length,
    classification_notes: {
      A: "structural only / no edge",
      B: "positive before fees, negative after fees",
      C: "positive after fees, not landable under cap",
      D: "positive after fees and landable under cap",
      landable_under_cap:
        "rollout would_send or live_send_suppressed, or final outcome landed",
      pool_class_note:
        "prefers explicit pool_class from the tape and falls back to venue-family for older tapes",
    },
    overall: finalizeAccumulator(overall),
    by_venue: finalizeMap(
      byVenue,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_motif: finalizeMap(
      byMotif,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_tactic_family: finalizeMap(
      byTacticFamily,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_route_family: finalizeMap(
      byRouteFamily,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_pool_class: finalizeMap(
      byPoolClass,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_owner_mode: finalizeMap(
      byOwnerMode,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_impulse_tier: finalizeMap(
      byImpulseTier,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_surface: finalizeMap(
      bySurface,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ),
    by_pool: finalizeMap(
      byPool,
      (a, b) => b.classes.D - a.classes.D || b.classes.C - a.classes.C || b.observations - a.observations,
    ).slice(0, 100),
  };

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
