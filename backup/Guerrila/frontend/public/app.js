const state = {
  config: null,
  live: null,
  graph: null,
  capitals: [],
  timelineEvents: [],
  eventSource: null,
  fallbackTimer: null,
  liveSequence: -1,
  uiReview: null,
  uiReviewPollTimer: null
};

const $ = (selector) => document.querySelector(selector);
const SVG_NS = "http://www.w3.org/2000/svg";
const GRAPH_WIDTH = 1420;
const GRAPH_HEIGHT = 680;
const NODE_CARD_WIDTH = 220;
const NODE_CARD_HEIGHT = 116;

const LANES = [
  { label: "Source", x: 145, tone: "good" },
  { label: "Ingest", x: 380, tone: "good" },
  { label: "State", x: 550, tone: "warm" },
  { label: "Decision", x: 840, tone: "active" },
  { label: "Send", x: 1070, tone: "active" },
  { label: "Reconcile", x: 1300, tone: "idle" },
  { label: "Report", x: 1360, tone: "idle" }
];

const NODE_DEFS = [
  { id: "agave", title: "Local Agave", phase: "Ledger / RPC Truth", status: "checking", tone: "off", x: 180, y: 205, critical: true },
  { id: "afxdp", title: "AF_XDP", phase: "UDP / Shred Capture", status: "checking", tone: "off", x: 150, y: 395, critical: true },
  { id: "ingest", title: "shadow-ingest", phase: "Decode To Record", status: "checking", tone: "off", x: 380, y: 395, critical: true },
  { id: "ipc", title: "shadow-ipc", phase: "Feed Ring", status: "checking", tone: "off", x: 610, y: 395, critical: true },
  { id: "state", title: "shadow-state", phase: "State Publisher", status: "checking", tone: "off", x: 550, y: 205, critical: true },
  { id: "oracle", title: "shadow-oracle", phase: "Sidecars / LUTs", status: "idle", tone: "off", x: 840, y: 205, critical: false },
  { id: "tier0", title: "shadow-tier0", phase: "Decision Engine", status: "idle", tone: "off", x: 840, y: 395, critical: true },
  { id: "egress", title: "Egress proxy", phase: "Send / No-Send", status: "stopped", tone: "off", x: 1070, y: 395, critical: true },
  { id: "pm", title: "shadow-pm", phase: "Reconcile", status: "idle", tone: "off", x: 1300, y: 395, critical: true },
  { id: "control", title: "shadow-control", phase: "Proof / Report", status: "diagnostics", tone: "off", x: 1120, y: 555, critical: false }
];

const LINKS = [
  { id: "agave_state", from: "agave", to: "state", tone: "warm", payload: "blockhash/account truth" },
  { id: "agave_afxdp", from: "agave", to: "afxdp", tone: "good", payload: "local network source" },
  { id: "afxdp_ingest", from: "afxdp", to: "ingest", tone: "good", payload: "packet/shred capture" },
  { id: "ingest_ipc", from: "ingest", to: "ipc", tone: "good", payload: "IngressRecord feed" },
  { id: "ipc_tier0", from: "ipc", to: "tier0", tone: "warm", payload: "worker consume" },
  { id: "state_tier0", from: "state", to: "tier0", tone: "warm", payload: "pool/mint/ALT sidecars", via: [[695, 205], [695, 395]] },
  { id: "oracle_tier0", from: "oracle", to: "tier0", tone: "warm", payload: "advisory LUTs" },
  { id: "tier0_egress", from: "tier0", to: "egress", tone: "active", payload: "signed/no-send envelope" },
  { id: "egress_pm", from: "egress", to: "pm", tone: "active", payload: "result/NACK/status" },
  { id: "pm_control", from: "pm", to: "control", tone: "idle", payload: "safety proof", via: [[1300, 555]] }
];

const nodeById = new Map(NODE_DEFS.map((node) => [node.id, node]));

const NODE_DETAILS = {
  agave: {
    role: "Local validator substrate. It receives Solana network traffic, maintains ledger/accounts/state, and provides local RPC truth for blockhash, account reads, leaders, contacts, and status.",
    processing: "Agave unit, local RPC health, UDP 8000-8025 listeners, eno1 traffic"
  },
  afxdp: {
    role: "Owned packet capture boundary. AF_XDP should only light when Shadow actually owns live UDP/shred capture rather than merely seeing normal Agave wire traffic.",
    processing: "XDP attachment, XSK map, ingest process starts, fresh packet/shred artifacts"
  },
  ingest: {
    role: "Parses, reassembles, decodes, and normalizes packet/shred traffic into IngressRecord events for the feed ring.",
    processing: "shadow-ingest, shadow-ingest-replay, grpc-ingest, shred feed, IngressRecord emission"
  },
  ipc: {
    role: "Fixed-layout shared-memory language between Shadow daemons. It carries feed rings, state sidecars, NACK/status rings, health, control flags, and raw archives.",
    processing: "/dev/shm rings, feed-ring writes, shared binary contract movement"
  },
  state: {
    role: "Publishes worker-readable market truth from local Agave/RPC/account sources: pool, mint, ALT, health, blockhash, leader, and quote sidecars.",
    processing: "shadow-state ticks, pool/mint/ALT publication, state freshness"
  },
  oracle: {
    role: "Off-path advisory publisher. It prepares compact LUTs and sidecars for leader class, bidding/tip banks, ownership, competition, and crowd heat.",
    processing: "shadow-oracle ticks, LUT/sidecar advice, route advisory handoff"
  },
  tier0: {
    role: "The hot-path decision engine. It consumes event plus state, then does pool tag lookup, route, quote, policy, build/sign, PM/no-send classification, and egress enqueue.",
    processing: "shadow-tier0 ticks, route/quote/policy/build-sign stages, signer inventory and caps"
  },
  egress: {
    role: "Out-of-worker send/no-send proxy. It drains chosen envelopes for Jito, Harmonic, TPU, or explicit public paths and emits result/NACK/status.",
    processing: "shadow-egress-tpu ticks, egress envelope dequeue, would-send/live-send status"
  },
  pm: {
    role: "Position manager and reconciliation authority. It owns reservation, release, landed/fail/timeout truth, inventory truth, and fee truth.",
    processing: "shadow-pm ticks, NACK/status consumption, reserve/release reconciliation"
  },
  control: {
    role: "Control-plane proof surface. It reports gate input/report, blocker classification, run artifacts, diagnostics, and safety proof.",
    processing: "latest blocker, SSH health, gate reports, shadow-control diagnostics"
  }
};

function statusClass(value) {
  if (value === true || value === "ok") return "ok";
  if (value === false || value === "bad") return "bad";
  return "warn";
}

async function api(path, options = {}) {
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest();
    request.open(options.method || "GET", path, true);
    request.setRequestHeader("Content-Type", "application/json");
    request.onload = () => {
      if (request.status < 200 || request.status >= 300) {
        reject(new Error(`${request.status} ${request.statusText}`));
        return;
      }
      try {
        resolve(JSON.parse(request.responseText));
      } catch (error) {
        reject(error);
      }
    };
    request.onerror = () => reject(new Error("Network request failed"));
    request.send(options.body || null);
  });
}

function setText(selector, value) {
  const element = $(selector);
  if (element) element.textContent = value;
}

function renderCapitals(walletCapitals) {
  const list = $("#capital-list");
  const count = $("#capital-count");
  const primary = $("#capital-primary");
  if (!list || !count || !primary) return;

  const items = walletCapitals && Array.isArray(walletCapitals.items)
    ? walletCapitals.items.slice(0, 3)
    : [];
  state.capitals = items;

  list.innerHTML = "";
  if (!walletCapitals || walletCapitals.ok === false) {
    count.textContent = "Unavailable";
    primary.textContent = "Unreadable";
    const item = document.createElement("div");
    item.className = "capital-item loading";
    item.innerHTML = "<span>Wallet</span><strong>Unreadable</strong>";
    list.appendChild(item);
    return;
  }

  count.textContent = `${items.length} live`;
  primary.textContent = items[0] ? items[0].value : "No balance";
  items.forEach((capital) => {
    const item = document.createElement("div");
    item.className = "capital-item";

    const label = document.createElement("span");
    label.textContent = capital.label;

    const value = document.createElement("strong");
    value.textContent = capital.value;

    const note = document.createElement("em");
    note.textContent = capital.note || "";

    item.append(label, value, note);
    list.appendChild(item);
  });
}

function setupCapitalDropdown() {
  const hud = $("#capital-hud");
  const toggle = $("#capital-toggle");
  if (!hud || !toggle) return;

  const setOpen = (open) => {
    hud.classList.toggle("open", open);
    toggle.setAttribute("aria-expanded", String(open));
  };

  toggle.addEventListener("click", (event) => {
    event.stopPropagation();
    setOpen(!hud.classList.contains("open"));
  });

  document.addEventListener("click", (event) => {
    if (!hud.contains(event.target)) setOpen(false);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") setOpen(false);
  });
}

function renderConfig(config) {
  state.config = config;
  setText("#host-target", config.host || "ubuntu@109.94.99.15");
}

function uiReviewMetaLine(status) {
  if (!status || !status.lastRun) return "No run yet";
  const rounds = Array.isArray(status.lastRun.rounds) ? status.lastRun.rounds.length : 0;
  const stopReason = status.lastRun.stopReason || "completed";
  return `${rounds} round${rounds === 1 ? "" : "s"} · ${stopReason.replace(/_/g, " ")}`;
}

function renderUiReviewStatus(status) {
  state.uiReview = status;

  const pill = $("#ui-review-pill");
  const button = $("#ui-review-run");
  const title = $("#ui-review-status-title");
  const detail = $("#ui-review-status-detail");
  const model = $("#ui-review-meta-model");
  const last = $("#ui-review-meta-last");

  if (!pill || !button || !title || !detail || !model || !last) return;

  button.disabled = Boolean(status && status.running);
  if (!status || !status.keyConfigured) {
    pill.textContent = "UI review key missing";
    pill.className = "status-token bad";
    title.textContent = "API key not loaded";
    detail.textContent = "Restart this frontend server with OPENAI_API_KEY in the environment to enable one-click UI review.";
    model.textContent = "Model unavailable";
    last.textContent = uiReviewMetaLine(status);
    return;
  }

  if (status.running) {
    pill.textContent = "UI review running";
    pill.className = "status-token ok";
    title.textContent = "Loop in progress";
    detail.textContent = `Running against ${status.targetUrl || "the configured target"} and applying edits under frontend/public only.`;
  } else if (status.lastRun && status.lastRun.stopReason) {
    pill.textContent = "UI review ready";
    pill.className = "status-token warn";
    title.textContent = "Last loop finished";
    detail.textContent = `Stopped because ${String(status.lastRun.stopReason).replace(/_/g, " ")}.`;
  } else {
    pill.textContent = "UI review idle";
    pill.className = "status-token";
    title.textContent = "One-click loop ready";
    detail.textContent = "Captures beta, asks the reviewer, applies frontend-only edits, and stops on the configured threshold.";
  }

  model.textContent = status.model ? `Model ${status.model}` : "Model from config";
  last.textContent = uiReviewMetaLine(status);
}

async function loadUiReviewStatus() {
  renderUiReviewStatus(await api("/api/ui-review/status"));
}

async function triggerUiReviewRun() {
  const button = $("#ui-review-run");
  if (button) button.disabled = true;
  try {
    const result = await api("/api/ui-review/run", {
      method: "POST",
      body: JSON.stringify({ mode: "default" })
    });
    renderUiReviewStatus(result.status || null);
  } catch (error) {
    renderUiReviewStatus({
      running: false,
      keyConfigured: Boolean(state.config && state.config.uiReview && state.config.uiReview.keyConfigured),
      lastRun: state.uiReview && state.uiReview.lastRun ? state.uiReview.lastRun : null,
      error: error.message
    });
  } finally {
    window.setTimeout(() => loadUiReviewStatus().catch(() => {}), 600);
  }
}

function setupUiReviewControls() {
  const button = $("#ui-review-run");
  if (!button) return;
  button.addEventListener("click", () => {
    triggerUiReviewRun().catch(() => {});
  });

  loadUiReviewStatus().catch(() => {});
  if (state.uiReviewPollTimer) window.clearInterval(state.uiReviewPollTimer);
  state.uiReviewPollTimer = window.setInterval(() => {
    loadUiReviewStatus().catch(() => {});
  }, 4000);
}

function renderLiveSnapshot(data) {
  state.live = data;
  state.liveSequence = Number.isFinite(data.sequence) ? data.sequence : state.liveSequence + 1;

  if (data.host && data.host.target) setText("#host-target", data.host.target);
  if (data.eno1) setText("#eno1-owner", data.eno1.owner);
  setText("#latest-blocker", data.latestBlocker || "unknown");
  const generatedTime = new Date(data.generatedAt).toLocaleTimeString();
  const sshOk = Boolean(data.host && data.host.sshOk);
  setText("#generated-at", generatedTime);
  setText("#runtime-host", data.host && data.host.label ? `Host ${data.host.label}` : "Host .15");
  setText("#runtime-clock", generatedTime);
  setText("#runtime-proof", data.latestBlocker ? data.latestBlocker.replace(/_/g, " ") : "Proof");
  setText(
    "#topbar-runtime-summary",
    `Host .15 · No-Send · ${data.latestBlocker ? data.latestBlocker.replace(/_/g, " ") : "proof pending"} · ${generatedTime}`
  );
  const runtimeDot = $("#runtime-live-dot");
  if (runtimeDot) {
    runtimeDot.textContent = sshOk ? "LIVE" : "DEGRADED";
    runtimeDot.className = sshOk ? "ok" : "warn";
  }

  const hostStatus = $("#host-status");
  if (hostStatus) {
    hostStatus.textContent = sshOk ? ".15 live" : ".15 degraded";
    hostStatus.className = `status-token ${statusClass(sshOk)}`;
  }

  const cadence = $("#live-cadence");
  const latency = Number(data.durationMs || 0);
  if (cadence) {
    cadence.textContent = latency ? `${latency}ms` : `tick ${state.liveSequence}`;
    cadence.className = `status-token ${latency > 2800 ? "warn" : "ok"}`;
  }

  if (state.graph) state.graph.updateFromLive(data);
}

async function loadWalletCapitals() {
  renderCapitals(await api("/api/wallet-capitals"));
}

async function loadConfig() {
  renderConfig(await api("/api/config"));
}

async function pollLiveSnapshot() {
  const snapshot = await api("/api/live-snapshot");
  renderLiveSnapshot(snapshot);
}

function startLiveStream() {
  if (state.eventSource) state.eventSource.close();
  if (state.fallbackTimer) window.clearInterval(state.fallbackTimer);

  if (!window.EventSource) {
    pollLiveSnapshot().catch(() => {});
    state.fallbackTimer = window.setInterval(() => pollLiveSnapshot().catch(() => {}), 1500);
    return;
  }

  const source = new EventSource("/api/live");
  state.eventSource = source;
  source.addEventListener("open", () => {
    setText("#graph-summary", "Reading .15 live");
  });
  source.addEventListener("snapshot", (event) => {
    renderLiveSnapshot(JSON.parse(event.data));
  });
  source.addEventListener("error", () => {
    const cadence = $("#live-cadence");
    if (cadence) {
      cadence.textContent = "reconnecting";
      cadence.className = "status-token warn";
    }
  });
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function svgElement(name, attributes = {}) {
  const element = document.createElementNS(SVG_NS, name);
  Object.entries(attributes).forEach(([key, value]) => {
    element.setAttribute(key, String(value));
  });
  return element;
}

function graphPoint(node) {
  return [node.x, node.y];
}

function nodeEdgeAnchor(node, toward) {
  const [targetX, targetY] = toward;
  const dx = targetX - node.x;
  const dy = targetY - node.y;
  if (Math.abs(dx) >= Math.abs(dy)) {
    return [node.x + Math.sign(dx || 1) * NODE_CARD_WIDTH / 2, node.y];
  }
  return [node.x, node.y + Math.sign(dy || 1) * NODE_CARD_HEIGHT / 2];
}

function pathForLink(link) {
  const from = nodeById.get(link.from);
  const to = nodeById.get(link.to);
  const via = link.via || [];
  const fromAnchor = nodeEdgeAnchor(from, via[0] || graphPoint(to));
  const toAnchor = nodeEdgeAnchor(to, via[via.length - 1] || graphPoint(from));
  const points = [fromAnchor, ...via, toAnchor];
  if (points.length > 2) {
    return points
      .map(([x, y], index) => `${index ? "L" : "M"} ${x} ${y}`)
      .join(" ");
  }

  const [[x1, y1], [x2, y2]] = points;
  const midX = (x1 + x2) / 2;
  return `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`;
}

function routeLabel(link) {
  const from = nodeById.get(link.from);
  const to = nodeById.get(link.to);
  return `${from.title} to ${to.title}`;
}

function formatNumber(value, digits = 0) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "0";
  return number.toLocaleString(undefined, { maximumFractionDigits: digits });
}

function formatSlotLag(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "unknown slots";
  return `${formatNumber(number)} ${number === 1 ? "slot" : "slots"}`;
}

function formatAgeSeconds(value) {
  const seconds = Number(value);
  if (!Number.isFinite(seconds) || seconds < 0) return "unknown";
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  if (seconds < 90) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${Math.round(seconds / 3600)}h`;
}

function formatBytes(value) {
  const bytes = Number(value || 0);
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let amount = bytes;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount.toFixed(unit ? 1 : 0)} ${units[unit]}`;
}

function formatEpochAge(epochSeconds, sampleEpochMs) {
  const epoch = Number(epochSeconds || 0);
  if (!epoch) return "none";
  const sampleSeconds = Number(sampleEpochMs || Date.now()) / 1000;
  const age = Math.max(0, sampleSeconds - epoch);
  if (age < 10) return `${age.toFixed(1)}s ago`;
  if (age < 90) return `${Math.round(age)}s ago`;
  return `${Math.round(age / 60)}m ago`;
}

function processState(processes, name) {
  return processes && processes[name] ? "running" : "stopped";
}

function metricRow(label, value, tone = "off") {
  const row = document.createElement("div");
  row.className = `node-metric tone-${tone}`;

  const name = document.createElement("span");
  name.textContent = label;

  const metric = document.createElement("strong");
  metric.textContent = value;

  row.append(name, metric);
  return row;
}

function nodeMetrics(id, live, entry) {
  const data = live || {};
  const activity = data.activity || {};
  const processes = data.processes || {};
  const ticks = activity.processTickRates || {};
  const nodeTone = entry && entry.active ? "active" : entry && entry.tone || "off";
  const processTicks = (name) => `${formatNumber(ticks[name] || 0, 1)} t/s`;
  const yesNo = (value) => value ? "yes" : "no";

  if (id === "agave") {
    const truth = agaveTruth(data);
    return [
      ["getHealth", truth.rpcGreen ? "ok" : "lagging", truth.rpcGreen ? "good" : "warm"],
      ["slot lag", truth.slotLagText, truth.rpcGreen ? "good" : truth.slotLagKnown ? "warm" : "idle"],
      ["local slot", truth.localSlotKnown ? formatNumber(truth.localSlot) : "unknown", truth.localSlotKnown ? "idle" : "off"],
      ["unit", truth.unitActive ? "active" : data.agave && data.agave.activeUnit || "unknown", truth.unitActive ? "idle" : "off"],
      ["UDP listeners", formatNumber(truth.udpCount), truth.udpReady ? "idle" : "off"],
      ["eno1 owner", data.eno1 && data.eno1.owner || "unknown", data.eno1 && data.eno1.owner === "Shadow" ? "warm" : "idle"],
      ["wire RX", `${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s`, activity.wireMoving ? "active" : "off"],
      ["sample", `${formatNumber(data.durationMs || 0)}ms`, data.host && data.host.sshOk ? "idle" : "off"]
    ];
  }

  if (id === "afxdp") {
    const truth = ingestTruth(data);
    return [
      ["XDP", data.eno1 && data.eno1.xdpAttached ? "attached" : "detached", data.eno1 && data.eno1.xdpAttached ? "active" : "off"],
      ["XSK map", data.eno1 && data.eno1.xskMap ? "present" : "absent", data.eno1 && data.eno1.xskMap ? "active" : "off"],
      ["wire RX", `${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s`, truth.captureMoving ? "active" : "off"],
      ["capture", truth.captureMoving ? "capturing" : truth.captureReady ? "armed" : "detached", truth.captureMoving ? "active" : truth.captureReady ? "idle" : "off"],
      ["feed rings", truth.feedRingReady ? formatBytes(truth.feedRingBytes) : "none", truth.feedRingReady ? "good" : "off"],
      ["ingest evidence", truth.ingestMoving ? "live" : "idle", truth.ingestMoving ? "active" : "off"]
    ];
  }

  if (id === "wire") {
    return [
      ["rx packets", `${formatNumber(activity.rxPacketsPerSec || 0)} /s`, activity.wireMoving ? "good" : "off"],
      ["UDP listeners", formatNumber(data.agave && data.agave.udpListenerCount || 0), "idle"],
      ["eno1 owner", data.eno1 && data.eno1.owner || "unknown", nodeTone]
    ];
  }

  if (id === "ingest") {
    const truth = ingestTruth(data);
    return [
      ["process", truth.processUp ? "present" : "absent", truth.processUp ? "active" : "off"],
      ["ingest ticks", formatNumber(truth.tickRate, 1), activity.ingestBusy ? "active" : "off"],
      ["feed ring", truth.feedRingReady ? "present" : "missing", truth.feedRingReady ? "good" : "off"],
      ["feed age", formatEpochAge(data.ingest && data.ingest.feedRingLatestEpoch, data.sampleEpochMs), truth.feedRingRecent ? "active" : "idle"],
      ["ring delta", truth.feedRingMoving ? "moving" : "idle", truth.feedRingMoving ? "active" : "off"],
      ["last artifact", formatEpochAge(data.ingest && data.ingest.artifactLatestEpoch, data.sampleEpochMs), activity.ingestArtifactRecent ? "good" : "idle"]
    ];
  }

  if (id === "ipc" || id === "record") {
    return [
      ["shm rings", formatNumber(data.ipc && data.ipc.shmCount || 0), data.ipc && data.ipc.shmCount ? "good" : "off"],
      ["ring bytes", formatBytes(data.ipc && data.ipc.totalBytes), "idle"],
      ["delta", activity.shmMoving ? "moving" : "idle", activity.shmMoving ? "active" : "off"]
    ];
  }

  if (id === "state") {
    const truth = stateTruth(data);
    const agave = agaveTruth(data);
    return [
      ["process", processState(processes, "shadow-state"), truth.processUp ? "good" : "off"],
      ["publish", truth.sidecarMoving ? "fresh" : truth.stale ? "stale" : truth.publishReady ? "idle" : "missing", truth.sidecarMoving ? "active" : truth.stale ? "warm" : truth.publishReady ? "idle" : "off"],
      ["publish age", formatAgeSeconds(truth.publishAgeSec), truth.sidecarMoving ? "active" : truth.stale ? "warm" : "idle"],
      ["blockhash age", formatAgeSeconds(truth.blockhashAgeSec), truth.blockhashRecent ? "active" : truth.stale ? "warm" : "idle"],
      ["core age", formatAgeSeconds(truth.coreAgeSec), truth.sidecarMoving ? "active" : truth.stale ? "warm" : "idle"],
      ["sidecar bytes", formatBytes(truth.publishBytes), truth.publishReady ? "good" : "off"],
      ["state ticks", processTicks("shadow-state"), activity.stateProcessBusy ? "idle" : "off"],
      ["shm rings", formatNumber(data.ipc && data.ipc.shmCount || 0), data.ipc && data.ipc.shmCount ? "good" : "off"],
      ["Agave health", agave.rpcGreen ? "ok" : "lagging", agave.rpcGreen ? "good" : "warm"],
      ["Agave lag", agave.slotLagText, agave.rpcGreen ? "good" : agave.slotLagKnown ? "warm" : "idle"]
    ];
  }

  if (id === "tier0" || id === "route" || id === "tagmap" || id === "quote" || id === "policy") {
    return [
      ["tier0", processState(processes, "shadow-tier0"), processes["shadow-tier0"] ? "active" : "off"],
      ["tier0 ticks", processTicks("shadow-tier0"), activity.tier0Busy ? "active" : "off"],
      ["route stages", entry && entry.active ? "moving" : "idle", entry && entry.active ? "active" : "off"]
    ];
  }

  if (id === "oracle") {
    return [
      ["process", processState(processes, "shadow-oracle"), processes["shadow-oracle"] ? "warm" : "off"],
      ["oracle ticks", processTicks("shadow-oracle"), activity.oracleBusy ? "warm" : "off"],
      ["route advice", entry && entry.active ? "feeding" : "idle", nodeTone]
    ];
  }

  if (id === "buildsign" || id === "inventory") {
    return [
      ["inventory", data.inventory && data.inventory.filePresent ? "present" : "missing", data.inventory && data.inventory.filePresent ? "good" : "off"],
      ["inventory size", formatBytes(data.inventory && data.inventory.size), "idle"],
      ["capital rows", formatNumber(state.capitals.length), "good"]
    ];
  }

  if (id === "pm") {
    return [
      ["process", processState(processes, "shadow-pm"), processes["shadow-pm"] ? "active" : "off"],
      ["pm ticks", processTicks("shadow-pm"), activity.pmBusy ? "active" : "off"],
      ["class", entry && entry.active ? "reserve/no-send" : "idle", nodeTone]
    ];
  }

  if (id === "egress") {
    return [
      ["process", processState(processes, "shadow-egress-tpu"), processes["shadow-egress-tpu"] ? "active" : "off"],
      ["egress ticks", processTicks("shadow-egress-tpu"), activity.egressBusy ? "active" : "off"],
      ["TPU path", entry && entry.active ? "moving" : "stopped", nodeTone]
    ];
  }

  if (id === "reconcile") {
    return [
      ["pm busy", yesNo(activity.pmBusy), activity.pmBusy ? "active" : "off"],
      ["egress busy", yesNo(activity.egressBusy), activity.egressBusy ? "active" : "off"],
      ["proof", data.latestBlocker || "unknown", "idle"]
    ];
  }

  if (id === "control" || id === "private") {
    return [
      ["host", data.host && data.host.sshOk ? ".15 live" : "degraded", data.host && data.host.sshOk ? "good" : "off"],
      ["blocker", data.latestBlocker || "unknown", id === "private" ? "blocked" : "idle"],
      ["private send", id === "private" ? "blocked" : "disabled", id === "private" ? "blocked" : "off"]
    ];
  }

  return [
    ["status", entry && entry.text || "checking", nodeTone],
    ["phase", entry && entry.node.phase || "pipeline", "idle"],
    ["critical", entry && entry.node.critical ? "yes" : "no", entry && entry.node.critical ? "warm" : "off"]
  ];
}

function nodeMicroMetric(id, live, signal = {}, mode = "runtime") {
  const data = live || {};
  const activity = data.activity || {};
  const ticks = activity.processTickRates || {};
  const processes = data.processes || {};
  const rate = (name) => `${formatNumber(ticks[name] || 0, 1)} t/s`;
  const processUp = (name) => processes[name] ? "up" : "stopped";
  const sampleMs = `${formatNumber(data.durationMs || 0)}ms sample`;

  if (mode === "latency") {
    if (id === "agave") {
      const truth = agaveTruth(data);
      return truth.rpcGreen ? "getHealth ok · 0 slots" : `lagging ${truth.slotLagText} · ${sampleMs}`;
    }
    if (id === "state") {
      const truth = stateTruth(data);
      return truth.sidecarMoving ? `publish fresh · ${formatAgeSeconds(truth.publishAgeSec)}` : truth.stale ? `publish stale · ${formatAgeSeconds(truth.publishAgeSec)}` : truth.processUp ? "process up · no publish" : "process stopped";
    }
    if (id === "tier0") return signal.active ? "decision p95 live" : "decision idle";
    if (id === "egress") return signal.active ? "route latency live" : "route idle";
    return signal.active ? `live · ${sampleMs}` : `idle · ${sampleMs}`;
  }

  if (mode === "economics") {
    if (id === "agave") {
      const truth = agaveTruth(data);
      return truth.rpcGreen ? "RPC truth usable" : `RPC lagging · ${truth.slotLagText}`;
    }
    if (id === "state") {
      const truth = stateTruth(data);
      return truth.sidecarMoving ? "account sidecars fresh" : truth.stale ? "account sidecars stale" : "sidecars stopped";
    }
    if (id === "tier0") return signal.active ? "candidate path armed" : "no candidates";
    if (id === "egress") return "No-Send · route drain";
    if (id === "pm") return "reservation · inventory truth";
    return signal.active ? "economic path live" : "economic path idle";
  }

  if (mode === "health") {
    if (signal.active) return "healthy · moving";
    if (signal.tone && signal.tone !== "off") return "ready · idle";
    if (id === "agave") {
      const truth = agaveTruth(data);
      if (!truth.hostLive) return "host unreachable";
      if (truth.rpcGreen) return "getHealth ok · running";
      if (truth.unitActive) return `unit active · lagging ${truth.slotLagText}`;
      return truth.udpReady ? `UDP present · lagging ${truth.slotLagText}` : `lagging ${truth.slotLagText}`;
    }
    if (id === "state") {
      const truth = stateTruth(data);
      return truth.sidecarMoving ? "state publish fresh" : truth.stale ? `sidecars stale · ${formatAgeSeconds(truth.publishAgeSec)}` : truth.processUp ? "process up · no sidecars" : "process stopped";
    }
    return `${processUp(`shadow-${id}`)} · stopped gate`;
  }

  if (mode === "proof") {
    if (id === "control") return data.latestBlocker ? data.latestBlocker.replace(/_/g, " ") : "evidence pending";
    if (signal.active) return "gate clear · evidenced";
    const node = nodeById.get(id);
    return node && node.critical ? "gate stopped · evidence needed" : "advisory evidence";
  }

  if (id === "agave") {
    const truth = agaveTruth(data);
    if (truth.rpcGreen) return `getHealth ok · ${formatNumber(truth.udpCount)} UDP`;
    if (truth.unitActive) return `lagging ${truth.slotLagText} · unit active`;
    if (truth.udpReady) return `lagging ${truth.slotLagText} · ${formatNumber(truth.udpCount)} UDP`;
    return truth.hostLive ? `lagging ${truth.slotLagText} · no UDP` : "host unreachable";
  }
  if (id === "afxdp") {
    const truth = ingestTruth(data);
    const xdp = data.eno1 && data.eno1.xdpAttached ? "XDP on" : "XDP off";
    return truth.captureMoving ? `${xdp} · ${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s` : `${xdp} · ${truth.captureReady ? "armed" : "standby"}`;
  }
  if (id === "ingest") {
    const truth = ingestTruth(data);
    if (truth.tickRate >= 0.1) return `${formatNumber(truth.tickRate, 1)} t/s · decoding`;
    if (truth.feedRingMoving || truth.feedRingRecent) return "feed ring live";
    const proc = truth.processUp ? "process up" : "process absent";
    return `${proc} · ${signal.active ? "decoding" : "idle"}`;
  }
  if (id === "ipc") return `feed ring · ${signal.active ? "moving" : "idle"}`;
  if (id === "state") {
    const truth = stateTruth(data);
    if (truth.sidecarMoving) return `publish fresh · ${formatAgeSeconds(truth.publishAgeSec)}`;
    if (truth.stale) return `publish stale · ${formatAgeSeconds(truth.publishAgeSec)}`;
    if (truth.processUp) return "process up · no sidecars";
    return "process stopped";
  }
  if (id === "oracle") return `${processes["shadow-oracle"] ? "oracle up" : "oracle off"} · LUTs`;
  if (id === "tier0") return `${rate("shadow-tier0")} · ${processes["shadow-tier0"] ? "worker up" : "stopped"}`;
  if (id === "egress") return `${rate("shadow-egress-tpu")} · No-Send`;
  if (id === "pm") return `${rate("shadow-pm")} · inventory`;
  if (id === "control") return `${data.host && data.host.sshOk ? "SSH ok" : "SSH degraded"} · proof`;
  return signal.status || "waiting";
}

function setNodeClass(element, active, tone) {
  element.className = "graph-node";
  element.classList.add(`tone-${tone || "off"}`);
  if (tone === "blocked") {
    element.classList.add("blocked");
  } else if (active) {
    element.classList.add("active");
  } else if (tone && tone !== "off") {
    element.classList.add("ready");
  } else {
    element.classList.add("off");
  }
}

function nodeStateLabel(active, tone, text) {
  if (/stale/i.test(text || "")) return "STALE";
  if (/lagging/i.test(text || "")) return "LAGGING";
  if (tone === "blocked" || /blocked|fail|error/i.test(text || "")) return "BLOCKED";
  if (active) return "RUNNING";
  if (tone && tone !== "off") return "READY";
  return "STOPPED";
}

function listRow(primary, secondary, meta, tone = "off") {
  const row = document.createElement("div");
  row.className = `graph-list-row tone-${tone}`;

  const copy = document.createElement("span");
  copy.textContent = primary;

  const strong = document.createElement("strong");
  strong.textContent = secondary;

  const small = document.createElement("em");
  small.textContent = meta;

  row.append(copy, strong, small);
  return row;
}

function renderEmptyList(selector, text) {
  const list = $(selector);
  if (!list) return;
  const row = document.createElement("div");
  row.className = "graph-list-row empty";
  row.textContent = text;
  list.replaceChildren(row);
}

function liveNode(data, id) {
  return data && data.nodeSignals && data.nodeSignals[id] ? data.nodeSignals[id] : {};
}

function liveEdge(data, id) {
  return data && data.edgeSignals && data.edgeSignals[id] ? data.edgeSignals[id] : {};
}

function signal(active, tone, status) {
  return { active: Boolean(active), tone: tone || (active ? "active" : "off"), status: status || "idle" };
}

function processTickRate(data, name) {
  return Number(data && data.activity && data.activity.processTickRates && data.activity.processTickRates[name] || 0);
}

function agaveTruth(data = {}) {
  const agave = data.agave || {};
  const hostLive = Boolean(data.host && data.host.sshOk);
  const unitActive = agave.activeUnit === "active";
  const rpcGreen = agave.localHealth === "ok";
  const udpCount = Number(agave.udpListenerCount || 0);
  const udpReady = udpCount > 0;
  const slotLag = Number(agave.slotLag);
  const slotLagKnown = Number.isFinite(slotLag);
  const localSlot = Number(agave.localSlot);
  const localSlotKnown = Number.isFinite(localSlot);
  const slotLagText = rpcGreen ? "0 slots" : formatSlotLag(slotLag);
  return {
    hostLive,
    unitActive,
    rpcGreen,
    udpCount,
    udpReady,
    slotLag: slotLagKnown ? slotLag : null,
    slotLagKnown,
    slotLagText,
    localSlot: localSlotKnown ? localSlot : null,
    localSlotKnown,
    healthMessage: agave.healthMessage || (rpcGreen ? "getHealth ok" : "getHealth not ok")
  };
}

function stateTruth(data = {}) {
  const activity = data.activity || {};
  const processes = data.processes || {};
  const publish = data.state || {};
  const tickRate = processTickRate(data, "shadow-state");
  const processUp = Boolean(processes["shadow-state"] || publish.processActive);
  const publishEpoch = Number(publish.publishLatestEpoch || 0);
  const coreEpoch = Number(publish.coreLatestEpoch || 0);
  const blockhashEpoch = Number(publish.blockhashEpoch || 0);
  const publishReady = publishEpoch > 0 || coreEpoch > 0 || blockhashEpoch > 0;
  const publishAgeSec = Number(activity.statePublishAgeSec);
  const blockhashAgeSec = Number(activity.stateBlockhashAgeSec);
  const coreAgeSec = Number(activity.stateCoreAgeSec);
  const healthAgeSec = Number(activity.stateHealthAgeSec);
  const publishMoving = Boolean(activity.statePublishMoving);
  const publishRecent = Boolean(activity.statePublishRecent);
  const blockhashRecent = Boolean(activity.stateBlockhashRecent);
  const sidecarMoving = Boolean(processUp && (publishMoving || publishRecent || blockhashRecent));
  const shmMoving = Boolean(activity.shmMoving);
  const stale = Boolean(processUp && publishReady && !sidecarMoving);
  return {
    processUp,
    sidecarMoving,
    shmMoving,
    tickRate,
    publishReady,
    publishMoving,
    publishRecent,
    blockhashRecent,
    stale,
    publishAgeSec,
    blockhashAgeSec,
    coreAgeSec,
    healthAgeSec,
    publishBytes: Number(publish.publishTotalBytes || 0)
  };
}

function ingestTruth(data = {}) {
  const activity = data.activity || {};
  const ingest = data.ingest || {};
  const eno1 = data.eno1 || {};
  const tickRate =
    processTickRate(data, "shadow-ingest") +
    processTickRate(data, "shadow-ingest-replay") +
    processTickRate(data, "grpc-ingest") +
    processTickRate(data, "afxdp") +
    processTickRate(data, "shred");
  const captureReady = Boolean(eno1.xdpAttached || eno1.xskMap);
  const wireMoving = Boolean(activity.wireMoving);
  const captureMoving = Boolean(captureReady && wireMoving);
  const processUp = Boolean(ingest.processActive);
  const feedRingBytes = Number(ingest.feedRingTotalBytes || 0);
  const feedRingLatestEpoch = Number(ingest.feedRingLatestEpoch || 0);
  const feedRingReady = feedRingBytes > 0 || feedRingLatestEpoch > 0;
  const feedRingMoving = Boolean(activity.ingestShmMoving);
  const feedRingRecent = Boolean(activity.ingestShmRecent);
  const artifactMoving = Boolean(activity.ingestArtifactMoving);
  const artifactRecent = Boolean(activity.ingestArtifactRecent);
  const ingestMoving = Boolean(
    activity.ingestBusy ||
    feedRingMoving ||
    feedRingRecent ||
    artifactMoving ||
    artifactRecent ||
    activity.ingestProcessStarted ||
    (captureMoving && feedRingReady) ||
    (processUp && (wireMoving || feedRingReady))
  );
  return {
    captureReady,
    captureMoving,
    processUp,
    tickRate,
    feedRingBytes,
    feedRingLatestEpoch,
    feedRingReady,
    feedRingMoving,
    feedRingRecent,
    artifactMoving,
    artifactRecent,
    ingestMoving
  };
}

function projectCanonicalNodeSignals(data = {}) {
  const activity = data.activity || {};
  const processes = data.processes || {};
  const eno1 = data.eno1 || {};
  const ingest = data.ingest || {};
  const agave = agaveTruth(data);
  const shadowState = stateTruth(data);
  const ingestPath = ingestTruth(data);
  const agaveStatus = agave.rpcGreen ? "getHealth ok" : `lagging ${agave.slotLagText}`;
  const agaveTone = agave.rpcGreen
    ? "good"
    : agave.hostLive
      ? "warm"
      : "off";
  const stateStatus = shadowState.processUp
    ? shadowState.sidecarMoving
      ? `state fresh ${formatAgeSeconds(shadowState.publishAgeSec)}`
      : shadowState.stale
        ? `state stale ${formatAgeSeconds(shadowState.publishAgeSec)}`
        : "state process up"
    : "state stopped";
  const stateTone = shadowState.processUp
    ? shadowState.sidecarMoving ? "good" : shadowState.stale ? "warm" : "idle"
    : "off";
  const afxdpReady = Boolean(ingestPath.captureReady || processes.afxdp || processes.shred);
  const afxdpMoving = Boolean(
    ingestPath.captureMoving ||
    activity.ingestBusy ||
    activity.ingestShmMoving ||
    activity.ingestShmRecent ||
    activity.ingestArtifactMoving ||
    activity.ingestArtifactRecent ||
    activity.ingestProcessStarted
  );
  const ingestOld = liveNode(data, "ingest");
  const recordOld = liveNode(data, "record");
  const ingestMoving = Boolean(ingestPath.ingestMoving || ingestOld.active || liveNode(data, "decode").active || recordOld.active);
  const ingestStatus = ingestMoving
    ? ingestPath.tickRate >= 0.1
      ? `ingest ${formatNumber(ingestPath.tickRate, 1)} t/s`
      : ingestPath.feedRingMoving || ingestPath.feedRingRecent
        ? "feed ring live"
        : ingestPath.captureMoving && ingestPath.feedRingReady
          ? "capture to feed ring"
          : recordOld.active
            ? recordOld.status
            : ingestOld.active ? ingestOld.status : "IngressRecord moving"
    : ingestPath.feedRingReady || ingestPath.processUp || afxdpReady ? "decode idle" : "ingest absent";
  const tier0Moving = Boolean(
    activity.tier0Busy ||
    liveNode(data, "tagmap").active ||
    liveNode(data, "route").active ||
    liveNode(data, "quote").active ||
    liveNode(data, "policy").active ||
    liveNode(data, "buildsign").active
  );
  const pmMoving = Boolean(activity.pmBusy || liveNode(data, "pm").active || liveNode(data, "reconcile").active);
  const tier0Ticks = Math.round(processTickRate(data, "shadow-tier0"));

  return {
    agave: signal(agave.rpcGreen, agaveTone, agaveStatus),
    afxdp: signal(afxdpMoving, afxdpReady ? "active" : "off", afxdpMoving ? "capture moving" : afxdpReady ? "capture ready" : "XDP detached"),
    ingest: signal(ingestMoving, ingestMoving ? "active" : ingestPath.feedRingReady || ingestPath.processUp || afxdpReady ? "idle" : "off", ingestStatus),
    ipc: liveNode(data, "ipc"),
    state: signal(shadowState.processUp && shadowState.sidecarMoving, stateTone, stateStatus),
    oracle: liveNode(data, "oracle"),
    tier0: signal(tier0Moving, processes["shadow-tier0"] ? "active" : tier0Moving ? "warm" : "off", tier0Moving ? (tier0Ticks ? `tier0 ${tier0Ticks} t/s` : "route/quote/policy moving") : processes["shadow-tier0"] ? "tier0 idle" : "tier0 stopped"),
    egress: liveNode(data, "egress"),
    pm: signal(pmMoving, processes["shadow-pm"] ? "active" : pmMoving ? "warm" : "off", pmMoving ? liveNode(data, "pm").status || liveNode(data, "reconcile").status || "reconcile moving" : processes["shadow-pm"] ? "pm idle" : "pm stopped"),
    control: signal(Boolean(activity.controlBusy), data.host && data.host.sshOk ? "idle" : "off", data.latestBlocker || "diagnostics")
  };
}

function projectCanonicalEdgeSignals(data = {}, nodes = {}) {
  const activeIf = (id, active, payload, tone, intensity = 0.8) => ({
    id,
    active: Boolean(active),
    payload,
    intensity: active ? intensity : 0,
    status: active ? "moving" : "stopped",
    tone
  });
  const oldEdgeActive = (...ids) => ids.some((id) => Boolean(liveEdge(data, id).active));
  const activity = data.activity || {};
  const agaveActive = Boolean(nodes.agave && nodes.agave.active);
  const afxdpActive = Boolean(nodes.afxdp && nodes.afxdp.active);
  const ingestActive = Boolean(nodes.ingest && nodes.ingest.active);
  const ipcActive = Boolean(nodes.ipc && nodes.ipc.active);
  const stateActive = Boolean(nodes.state && nodes.state.active);
  const oracleActive = Boolean(nodes.oracle && nodes.oracle.active);
  const tier0Active = Boolean(nodes.tier0 && nodes.tier0.active);
  const egressActive = Boolean(nodes.egress && nodes.egress.active);
  const pmActive = Boolean(nodes.pm && nodes.pm.active);

  return {
    agave_state: activeIf("agave_state", agaveActive && stateActive, "blockhash/account truth", "warm", 0.65),
    agave_afxdp: activeIf("agave_afxdp", afxdpActive, "local network source", "good", 0.7),
    afxdp_ingest: activeIf("afxdp_ingest", afxdpActive && ingestActive, "packet/shred capture", "good", 1),
    ingest_ipc: activeIf("ingest_ipc", oldEdgeActive("decode_record", "record_ipc") || (ingestActive && (ipcActive || activity.ingestShmMoving || activity.ingestShmRecent)), "IngressRecord feed", "good", 0.9),
    ipc_tier0: activeIf("ipc_tier0", oldEdgeActive("ipc_tagmap", "tagmap_route") || (ipcActive && tier0Active), "worker consume", "warm", 0.9),
    state_tier0: activeIf("state_tier0", oldEdgeActive("state_ipc") || (stateActive && tier0Active), "pool/mint/ALT sidecars", "warm", 0.78),
    oracle_tier0: activeIf("oracle_tier0", oldEdgeActive("oracle_route") || (oracleActive && tier0Active), "advisory LUTs", "warm", 0.62),
    tier0_egress: activeIf("tier0_egress", oldEdgeActive("policy_buildsign", "buildsign_egress") || (tier0Active && egressActive), "signed/no-send envelope", "active", 1),
    egress_pm: activeIf("egress_pm", oldEdgeActive("egress_reconcile") || (egressActive && pmActive), "result/NACK/status", "active", 0.85),
    pm_control: activeIf("pm_control", oldEdgeActive("reconcile_control") || pmActive, "safety proof", "idle", 0.65)
  };
}

function initPipelineGraph() {
  const viewport = $("#pipeline-graph-viewport");
  const board = $("#pipeline-graph");
  const laneLayer = $("#lane-layer");
  const edgeLayer = $("#edge-layer");
  const nodeLayer = $("#node-layer");
  const zoomReadout = $("#graph-zoom-readout");
  const zoomIn = $("#graph-zoom-in");
  const zoomOut = $("#graph-zoom-out");
  const resetView = $("#graph-reset-view");
  const inspector = $("#node-inspector");
  const inspectorPhase = $("#node-inspector-phase");
  const inspectorTitle = $("#node-inspector-title");
  const inspectorStatus = $("#node-inspector-status");
  const inspectorRole = $("#node-inspector-role");
  const inspectorFlow = $("#node-inspector-flow");
  const inspectorMetrics = $("#node-inspector-metrics");
  const timeline = $("#event-timeline");
  const timelineSummary = $("#timeline-summary");
  const modeTabs = Array.from(document.querySelectorAll(".mode-tab"));
  if (!viewport || !board || !laneLayer || !edgeLayer || !nodeLayer) return null;

  laneLayer.innerHTML = "";
  edgeLayer.innerHTML = "";
  nodeLayer.innerHTML = "";

  const view = {
    scale: 1,
    x: 0,
    y: 0,
    mode: "fit",
    dragging: false,
    startX: 0,
    startY: 0,
    originX: 0,
    originY: 0,
    lockScrollY: 0,
    selectedNodeId: "agave",
    graphMode: "runtime",
    previousSignalKeys: new Map()
  };

  board.dataset.overlay = view.graphMode;

  function clampView() {
    const rect = viewport.getBoundingClientRect();
    const scaledWidth = GRAPH_WIDTH * view.scale;
    const scaledHeight = GRAPH_HEIGHT * view.scale;
    const margin = 96;

    if (scaledWidth <= rect.width) {
      view.x = (rect.width - scaledWidth) / 2;
    } else {
      view.x = clamp(view.x, rect.width - scaledWidth - margin, margin);
    }

    if (scaledHeight <= rect.height) {
      view.y = (rect.height - scaledHeight) / 2;
    } else {
      view.y = clamp(view.y, rect.height - scaledHeight - margin, margin);
    }
  }

  function applyView() {
    clampView();
    board.style.transform = `translate(${view.x}px, ${view.y}px) scale(${view.scale})`;
    board.dataset.zoom = view.scale.toFixed(3);
    board.dataset.panX = Math.round(view.x);
    board.dataset.panY = Math.round(view.y);
    viewport.dataset.viewMode = view.mode;
    if (zoomReadout) zoomReadout.textContent = `${Math.round(view.scale * 100)}%`;
    setText("#graph-window-state", view.mode === "fit" ? "Fit" : "Manual");
  }

  function fitGraph() {
    const rect = viewport.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const fitScale = Math.min((rect.width - 44) / GRAPH_WIDTH, (rect.height - 44) / GRAPH_HEIGHT);
    const mobileScaleFloor = rect.width < 720 ? 0.5 : 0.34;
    const mobileTopBias = rect.width < 720 ? 36 : 0;
    view.scale = clamp(fitScale, mobileScaleFloor, 1);
    view.x = (rect.width - GRAPH_WIDTH * view.scale) / 2;
    view.y = (rect.height - GRAPH_HEIGHT * view.scale) / 2 - mobileTopBias;
    view.mode = "fit";
    applyView();
  }

  function zoomAt(clientX, clientY, nextScale) {
    const rect = viewport.getBoundingClientRect();
    const pointerX = clientX - rect.left;
    const pointerY = clientY - rect.top;
    const graphX = (pointerX - view.x) / view.scale;
    const graphY = (pointerY - view.y) / view.scale;
    view.scale = clamp(nextScale, 0.34, 2.4);
    view.x = pointerX - graphX * view.scale;
    view.y = pointerY - graphY * view.scale;
    view.mode = "manual";
    applyView();
  }

  function zoomFromCenter(multiplier) {
    const rect = viewport.getBoundingClientRect();
    zoomAt(rect.left + rect.width / 2, rect.top + rect.height / 2, view.scale * multiplier);
  }

  function holdPageScroll() {
    document.documentElement.scrollTop = view.lockScrollY;
    document.body.scrollTop = view.lockScrollY;
  }

  LANES.forEach((lane) => {
    const separator = document.createElement("div");
    separator.className = `lane-separator tone-${lane.tone || "idle"}`;
    separator.style.left = `${(lane.x / GRAPH_WIDTH) * 100}%`;

    const label = document.createElement("div");
    label.className = `lane-label tone-${lane.tone || "idle"}`;
    label.style.left = `${(lane.x / GRAPH_WIDTH) * 100}%`;
    label.textContent = lane.label;
    laneLayer.append(separator, label);
  });

  const defs = svgElement("defs");
  defs.appendChild(svgElement("marker", {
    id: "edge-arrow",
    markerWidth: 8,
    markerHeight: 8,
    refX: 7,
    refY: 4,
    orient: "auto"
  }));
  defs.querySelector("marker").appendChild(svgElement("path", {
    d: "M 0 0 L 8 4 L 0 8 z",
    class: "edge-arrow"
  }));
  edgeLayer.appendChild(defs);

  const nodeLookup = new Map();
  function paintSelectedNode() {
    const relatedNodeIds = new Set([view.selectedNodeId]);
    const relatedEdgeIds = new Set();
    LINKS.forEach((link) => {
      if (link.from === view.selectedNodeId || link.to === view.selectedNodeId) {
        relatedEdgeIds.add(link.id);
        relatedNodeIds.add(link.from);
        relatedNodeIds.add(link.to);
      }
    });
    nodeLookup.forEach((entry) => {
      const selected = entry.node.id === view.selectedNodeId;
      const related = relatedNodeIds.has(entry.node.id);
      const dimmed = Boolean(view.selectedNodeId) && !related && !entry.active;
      entry.element.classList.toggle("selected", selected);
      entry.element.classList.toggle("related", related && !selected);
      entry.element.classList.toggle("dimmed", dimmed);
      entry.element.setAttribute("aria-pressed", String(entry.node.id === view.selectedNodeId));
      entry.element.style.opacity = entry.active
        ? "1"
        : selected
          ? "0.96"
          : related
            ? "0.7"
          : dimmed
            ? entry.tone === "off" ? "0.48" : "0.46"
            : entry.tone && entry.tone !== "off"
              ? "0.7"
              : "0.55";
    });
    if (typeof edgeLookup !== "undefined") {
      edgeLookup.forEach((edge, id) => {
        const related = relatedEdgeIds.has(id);
        edge.group.classList.toggle("selected", related);
        edge.group.classList.toggle("dimmed", Boolean(view.selectedNodeId) && !related && !edge.active);
      });
    }
  }

  function renderNodeInspector(nodeId = view.selectedNodeId, live = state.live) {
    const entry = nodeLookup.get(nodeId);
    if (!entry || !inspector) return;
    const detail = NODE_DETAILS[nodeId] || {
      role: "Runtime stage in the current Shadow pipeline.",
      processing: entry.node.phase
    };
    const tone = entry.tone || "off";
    inspector.className = `graph-stat node-inspector tone-${tone}${entry.active ? " active" : ""}`;
    inspector.dataset.nodeId = nodeId;
    if (inspectorPhase) inspectorPhase.textContent = entry.node.phase;
    if (inspectorTitle) inspectorTitle.textContent = entry.node.title;
    if (inspectorStatus) inspectorStatus.textContent = entry.text;
    if (inspectorRole) inspectorRole.textContent = detail.role;

    if (inspectorFlow) {
      const related = LINKS
        .filter((link) => link.from === nodeId || link.to === nodeId)
        .slice(0, 5)
        .map((link) => {
          const edge = edgeLookup.get(link.id);
          const chip = document.createElement("span");
          const direction = link.from === nodeId ? "Out" : "In";
          chip.className = `node-flow-chip tone-${edge && edge.active ? edge.tone : "off"}`;
          chip.textContent = `${direction} ${link.from === nodeId ? nodeById.get(link.to).title : nodeById.get(link.from).title}: ${edge && edge.active ? edge.currentPayload : "idle"}`;
          return chip;
        });

      const source = document.createElement("span");
      source.className = "node-flow-chip tone-idle";
      source.textContent = detail.processing;
      inspectorFlow.replaceChildren(source, ...related);
    }

    if (inspectorMetrics) {
      const rows = nodeMetrics(nodeId, live, entry).map(([label, value, rowTone]) =>
        metricRow(label, value, rowTone)
      );
      inspectorMetrics.replaceChildren(...rows);
    }
  }

  function selectNode(nodeId) {
    view.selectedNodeId = nodeId;
    paintSelectedNode();
    renderNodeInspector(nodeId);
    const entry = nodeLookup.get(nodeId);
    if (entry) setText("#graph-subsummary", `${entry.node.title}: ${entry.text}`);
  }

  function applyGraphMode(mode) {
    view.graphMode = mode || "runtime";
    board.dataset.overlay = view.graphMode;
    modeTabs.forEach((tab) => tab.classList.toggle("active", (tab.dataset.mode || "runtime") === view.graphMode));
    nodeLookup.forEach((entry) => {
      if (entry.micro) entry.micro.textContent = nodeMicroMetric(entry.node.id, state.live, entry, view.graphMode);
    });
    renderNodeInspector(view.selectedNodeId, state.live);
  }

  function centerOnNode(nodeId) {
    const entry = nodeLookup.get(nodeId);
    if (!entry) return;
    const rect = viewport.getBoundingClientRect();
    view.scale = clamp(Math.max(view.scale, 0.78), 0.34, 2.4);
    view.x = rect.width / 2 - entry.node.x * view.scale;
    view.y = rect.height / 2 - entry.node.y * view.scale;
    view.mode = "manual";
    applyView();
  }

  NODE_DEFS.forEach((node) => {
    const element = document.createElement("button");
    element.type = "button";
    element.style.left = `${(node.x / GRAPH_WIDTH) * 100}%`;
    element.style.top = `${(node.y / GRAPH_HEIGHT) * 100}%`;
    element.dataset.nodeId = node.id;
    element.dataset.critical = String(Boolean(node.critical));
    element.setAttribute("aria-label", `${node.title}: ${node.status}`);
    setNodeClass(element, false, node.tone);

    const dot = document.createElement("span");
    dot.className = "node-dot";

    const copy = document.createElement("span");
    copy.className = "node-copy";

    const statusLine = document.createElement("span");
    statusLine.className = "node-status-line";

    const badge = document.createElement("span");
    badge.className = "node-state-label";
    badge.textContent = "CHECKING";

    const phase = document.createElement("small");
    phase.textContent = node.phase;

    const title = document.createElement("strong");
    title.textContent = node.title;

    const status = document.createElement("em");
    status.textContent = node.status;

    const micro = document.createElement("span");
    micro.className = "node-micro";
    micro.textContent = "reading host";

    statusLine.append(dot, badge);
    copy.append(title, statusLine, micro);
    element.append(copy);
    nodeLayer.appendChild(element);

    const entry = {
      node,
      element,
      status,
      title,
      badge,
      micro,
      active: false,
      tone: node.tone,
      text: node.status
    };

    element.addEventListener("mouseenter", () => selectNode(node.id));
    element.addEventListener("focus", () => selectNode(node.id));
    element.addEventListener("click", () => selectNode(node.id));
    element.addEventListener("dblclick", () => centerOnNode(node.id));

    nodeLookup.set(node.id, entry);
  });

  const edgeLookup = new Map();
  LINKS.forEach((link) => {
    const group = svgElement("g", {
      class: `edge tone-${link.tone}`,
      "data-edge-id": link.id
    });
    const d = pathForLink(link);
    const line = svgElement("path", {
      id: `edge-path-${link.id}`,
      d,
      class: "edge-line",
      "marker-end": "url(#edge-arrow)"
    });
    const pulse = svgElement("path", {
      d,
      class: "edge-pulse"
    });
    const packet = svgElement("circle", {
      class: "edge-packet",
      r: 4
    });
    const packetMotion = svgElement("animateMotion", {
      dur: "1.8s",
      repeatCount: "indefinite",
      rotate: "auto"
    });
    packetMotion.appendChild(svgElement("mpath", {
      href: `#edge-path-${link.id}`
    }));
    packet.appendChild(packetMotion);
    group.append(line, pulse, packet);
    edgeLayer.appendChild(group);

    const edgeEntry = {
      ...link,
      group,
      line,
      pulse,
      packet,
      packetMotion,
      active: false,
      intensity: 0,
      currentPayload: link.payload,
      currentDetail: "dependency idle"
    };
    group.addEventListener("mouseenter", () => {
      group.classList.add("hovered");
      setText("#graph-subsummary", `${routeLabel(link)}: ${edgeEntry.currentDetail}`);
    });
    group.addEventListener("mouseleave", () => {
      group.classList.remove("hovered");
      const entry = nodeLookup.get(view.selectedNodeId);
      if (entry) setText("#graph-subsummary", `${entry.node.title}: ${entry.text}`);
    });
    edgeLookup.set(link.id, edgeEntry);
  });

  viewport.addEventListener("wheel", (event) => {
    event.preventDefault();
    const multiplier = event.deltaY > 0 ? 0.9 : 1.1;
    zoomAt(event.clientX, event.clientY, view.scale * multiplier);
  }, { passive: false });

  viewport.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || event.target.closest(".graph-node")) return;
    event.preventDefault();
    view.dragging = true;
    view.startX = event.clientX;
    view.startY = event.clientY;
    view.originX = view.x;
    view.originY = view.y;
    view.lockScrollY = document.documentElement.scrollTop || document.body.scrollTop || window.pageYOffset || 0;
    view.mode = "manual";
    viewport.classList.add("dragging");
    document.documentElement.classList.add("graph-dragging-page-lock");
    viewport.setPointerCapture(event.pointerId);
    viewport.focus();
  });

  viewport.addEventListener("pointermove", (event) => {
    if (!view.dragging) return;
    event.preventDefault();
    view.x = view.originX + event.clientX - view.startX;
    view.y = view.originY + event.clientY - view.startY;
    applyView();
    holdPageScroll();
  });

  const stopDrag = (event) => {
    if (!view.dragging) return;
    view.dragging = false;
    viewport.classList.remove("dragging");
    document.documentElement.classList.remove("graph-dragging-page-lock");
    holdPageScroll();
    if (event && viewport.hasPointerCapture(event.pointerId)) {
      viewport.releasePointerCapture(event.pointerId);
    }
  };
  viewport.addEventListener("pointerup", stopDrag);
  viewport.addEventListener("pointercancel", stopDrag);

  viewport.addEventListener("keydown", (event) => {
    const key = event.key.toLowerCase();
    if (key === "+" || key === "=") {
      event.preventDefault();
      zoomFromCenter(1.1);
    } else if (key === "-" || key === "_") {
      event.preventDefault();
      zoomFromCenter(0.9);
    } else if (key === "0") {
      event.preventDefault();
      fitGraph();
    } else if (key === "arrowleft" || key === "arrowright" || key === "arrowup" || key === "arrowdown") {
      event.preventDefault();
      const step = 34;
      view.mode = "manual";
      if (key === "arrowleft") view.x += step;
      if (key === "arrowright") view.x -= step;
      if (key === "arrowup") view.y += step;
      if (key === "arrowdown") view.y -= step;
      applyView();
    }
  });

  if (zoomIn) zoomIn.addEventListener("click", () => {
    zoomFromCenter(1.14);
    viewport.focus();
  });
  if (zoomOut) zoomOut.addEventListener("click", () => {
    zoomFromCenter(0.88);
    viewport.focus();
  });
  if (resetView) resetView.addEventListener("click", () => {
    fitGraph();
    viewport.focus();
  });
  modeTabs.forEach((button) => {
    button.addEventListener("click", () => {
      applyGraphMode(button.dataset.mode || "runtime");
    });
  });

  if (window.ResizeObserver) {
    new ResizeObserver(() => {
      if (view.mode === "fit") fitGraph();
      else applyView();
    }).observe(viewport);
  } else {
    window.addEventListener("resize", () => {
      if (view.mode === "fit") fitGraph();
      else applyView();
    });
  }

  function updateNode(id, signal = {}, live = state.live) {
    const entry = nodeLookup.get(id);
    if (!entry) return;
    const active = Boolean(signal.active);
    const tone = signal.tone || (active ? entry.node.tone : entry.node.tone === "blocked" ? "blocked" : "off");
    const text = signal.status || (active ? entry.node.status : entry.node.tone === "blocked" ? entry.node.status : "off");

    entry.active = active;
    entry.tone = tone;
    entry.text = text;
    entry.status.textContent = text;
    entry.element.dataset.status = text;
    entry.element.dataset.active = String(active);
    entry.element.dataset.state = nodeStateLabel(active, tone, text).toLowerCase();
    entry.element.setAttribute("aria-label", `${entry.node.title}: ${text}`);
    setNodeClass(entry.element, active, tone);
    entry.element.classList.toggle("selected", entry.node.id === view.selectedNodeId);
    if (entry.badge) entry.badge.textContent = nodeStateLabel(active, tone, text);
    if (entry.micro) entry.micro.textContent = nodeMicroMetric(id, live, signal, view.graphMode);
  }

  function updateEdges(edgeSignals = {}) {
    edgeLookup.forEach((edge, id) => {
      const signal = edgeSignals[id] || {};
      const active = Boolean(signal.active);
      const intensity = clamp(Number(signal.intensity || 0), 0, 1.6);
      const payload = signal.payload || edge.payload;
      const status = signal.status || (active ? "moving" : "stopped");
      const blocked = edge.tone === "blocked";

      edge.active = active;
      edge.intensity = intensity;
      edge.currentPayload = payload;
      const fromEntry = nodeLookup.get(edge.from);
      const toEntry = nodeLookup.get(edge.to);
      const fromState = fromEntry ? nodeStateLabel(fromEntry.active, fromEntry.tone, fromEntry.text).toLowerCase() : "unknown";
      const toState = toEntry ? nodeStateLabel(toEntry.active, toEntry.tone, toEntry.text).toLowerCase() : "unknown";
      const causal = !active && toEntry && !toEntry.active && (toEntry.node.critical || edge.id === "oracle_tier0");
      edge.currentDetail = active
        ? `${payload} · ${Math.round(intensity * 100)}% pulse · last update ${Math.max(40, Math.round(220 - intensity * 90))}ms ago`
        : causal
          ? `${nodeById.get(edge.from).title} ${fromState} -> ${nodeById.get(edge.to).title} ${toState}`
          : "dependency idle";
      edge.group.classList.toggle("active", active);
      edge.group.classList.toggle("blocked", blocked);
      edge.group.classList.toggle("causal", causal);
      edge.group.dataset.status = status;
      edge.group.dataset.detail = edge.currentDetail;
      edge.group.style.setProperty("--edge-speed", `${Math.max(0.85, 2.4 - intensity)}s`);
      edge.group.style.setProperty("--edge-weight", String(1 + intensity));
      if (edge.packetMotion) edge.packetMotion.setAttribute("dur", `${Math.max(0.9, 2.5 - intensity)}s`);
    });
  }

  function renderHandoffList() {
    const activeEdges = Array.from(edgeLookup.values())
      .filter((edge) => edge.active)
      .sort((a, b) => b.intensity - a.intensity);
    const list = $("#handoff-list");
    if (!list) return;
    if (!activeEdges.length) {
      renderEmptyList("#handoff-list", "No live handoffs in this sample");
      return;
    }

    const rows = activeEdges.slice(0, 6).map((edge) => {
      const percent = Math.round(edge.intensity * 100);
      return listRow(routeLabel(edge), edge.currentPayload, `${percent}% pulse`, edge.tone);
    });
    list.replaceChildren(...rows);
  }

  function renderGateList() {
    const stopped = Array.from(nodeLookup.values())
      .filter((entry) => entry.node.critical || entry.tone === "blocked")
      .filter((entry) => !entry.active || entry.tone === "blocked")
      .sort((a, b) => Number(b.tone === "blocked") - Number(a.tone === "blocked"));
    const list = $("#gate-list");
    if (!list) return;
    if (!stopped.length) {
      renderEmptyList("#gate-list", "All critical gates are moving");
      return;
    }
    const rows = stopped.slice(0, 7).map((entry) => {
      const meta = entry.tone === "blocked" ? "safety block" : entry.node.phase;
      return listRow(entry.node.title, entry.text, meta, entry.tone);
    });
    list.replaceChildren(...rows);
  }

  function pushTimelineEvent(time, title, detail, tone = "idle") {
    const key = `${title}:${detail}`;
    if (state.timelineEvents[0] && state.timelineEvents[0].key === key) return;
    state.timelineEvents.unshift({ key, time, title, detail, tone });
    state.timelineEvents = state.timelineEvents.slice(0, 9);
  }

  function renderTimeline() {
    if (!timeline) return;
    if (!state.timelineEvents.length) {
      timeline.replaceChildren(Object.assign(document.createElement("div"), {
        className: "timeline-item empty",
        textContent: "Live graph events will appear here"
      }));
      return;
    }
    const rows = state.timelineEvents.map((event, index) => {
      const item = document.createElement("div");
      item.className = `timeline-item tone-${event.tone || "idle"}${index === 0 ? " latest" : ""}`;

      const time = document.createElement("span");
      time.textContent = event.time;

      const title = document.createElement("strong");
      title.textContent = event.title;

      const detail = document.createElement("em");
      detail.textContent = event.detail;

      item.append(time, title, detail);
      return item;
    });
    timeline.replaceChildren(...rows);
    if (timelineSummary) {
      timelineSummary.textContent = `${state.timelineEvents.length} recent event${state.timelineEvents.length === 1 ? "" : "s"}`;
    }
  }

  function recordTimelineEvents(data, signals, edgeSignals) {
    const time = new Date(data.generatedAt || Date.now()).toLocaleTimeString();
    const important = ["agave", "afxdp", "ingest", "state", "tier0", "egress", "pm", "control"];
    important.forEach((id) => {
      const signal = signals[id] || {};
      const key = `${Boolean(signal.active)}:${signal.tone || "off"}:${signal.status || ""}`;
      if (view.previousSignalKeys.get(id) === key) return;
      view.previousSignalKeys.set(id, key);
      const node = nodeById.get(id);
      if (!node) return;
      pushTimelineEvent(time, node.title, signal.status || "status changed", signal.tone || "idle");
    });

    Object.values(edgeSignals || {}).forEach((edge) => {
      if (!edge.active) return;
      const key = `edge:${edge.id}:${edge.payload}`;
      if (view.previousSignalKeys.get(edge.id) === key) return;
      view.previousSignalKeys.set(edge.id, key);
      const link = LINKS.find((item) => item.id === edge.id);
      pushTimelineEvent(time, link ? routeLabel(link) : edge.id, edge.payload || "handoff moving", link && link.tone || "active");
    });

    renderTimeline();
  }

  function updateFromLive(data) {
    const signals = projectCanonicalNodeSignals(data);
    const edgeSignals = projectCanonicalEdgeSignals(data, signals);
    NODE_DEFS.forEach((node) => updateNode(node.id, signals[node.id], data));
    updateEdges(edgeSignals);
    paintSelectedNode();

    const activeNodes = Array.from(nodeLookup.values()).filter((entry) => entry.active).length;
    const activeEdges = Array.from(edgeLookup.values()).filter((edge) => edge.active).length;
    const stoppedGates = Array.from(nodeLookup.values())
      .filter((entry) => entry.node.critical && !entry.active).length;
    const hostLive = Boolean(data.host && data.host.sshOk);

    setText("#graph-summary", hostLive ? `${activeNodes} / ${NODE_DEFS.length} active` : "Host degraded");
    setText(
      "#graph-subsummary",
      `${activeEdges} live handoff${activeEdges === 1 ? "" : "s"} / ${stoppedGates} stopped gate${stoppedGates === 1 ? "" : "s"}`
    );
    renderHandoffList();
    renderGateList();
    renderNodeInspector(view.selectedNodeId, data);
    recordTimelineEvents(data, signals, edgeSignals);

    board.dataset.renderer = "svg-2d";
    board.dataset.activeNodes = String(activeNodes);
    board.dataset.activeEdges = String(activeEdges);
    board.dataset.stoppedGates = String(stoppedGates);
    board.dataset.liveSequence = String(state.liveSequence);
  }

  const getDebug = () => {
    const edges = Array.from(edgeLookup.values());
    const nodes = Array.from(nodeLookup.values());
    return {
      renderer: "svg-2d",
      nodeCount: nodes.length,
      activeNodes: nodes.filter((entry) => entry.active).length,
      linkCount: edges.length,
      activeEdges: edges.filter((edge) => edge.active).length,
      activePayloads: edges.filter((edge) => edge.active).map((edge) => edge.currentPayload),
      stoppedGates: nodes.filter((entry) => entry.node.critical && !entry.active).map((entry) => entry.node.id),
      selectedNode: view.selectedNodeId,
      capitalCount: state.capitals.length,
      liveSequence: state.liveSequence,
      zoom: Number(view.scale.toFixed(3)),
      panX: Math.round(view.x),
      panY: Math.round(view.y),
      viewMode: view.mode,
      overlayMode: view.graphMode
    };
  };

  board.dataset.renderer = "svg-2d";
  board.dataset.activeNodes = "0";
  board.dataset.activeEdges = "0";
  board.dataset.stoppedGates = String(NODE_DEFS.filter((node) => node.critical).length);
  paintSelectedNode();
  renderNodeInspector(view.selectedNodeId);
  fitGraph();
  window.__shadowGraphDebug = getDebug;
  window.shadowGraphDebug = getDebug;
  window.__shadowVolumeDebug = getDebug;
  window.shadowVolumeDebug = getDebug;

  return { updateFromLive };
}

state.graph = initPipelineGraph();

loadConfig().catch((error) => {
  setText("#host-target", error.message);
});
loadWalletCapitals().catch((error) => {
  renderCapitals({ ok: false, error: error.message, items: [] });
});
setupCapitalDropdown();
setupUiReviewControls();
startLiveStream();
