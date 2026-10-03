const state = {
  config: null,
  live: null,
  lastLiveAt: 0,
  pollInFlight: false,
  graph: null,
  capitals: [],
  timelineEvents: [],
  eventSource: null,
  fallbackTimer: null,
  watchdogTimer: null,
  clockTimer: null,
  lastStreamRecoveryAt: 0,
  liveSequence: -1,
  uiReview: null,
  uiReviewPollTimer: null
};

const $ = (selector) => document.querySelector(selector);
const SVG_NS = "http://www.w3.org/2000/svg";
const GRAPH_WIDTH = 1420;
const GRAPH_HEIGHT = 680;
const NODE_CARD_WIDTH = 200;
const NODE_CARD_HEIGHT = 116;

const LANES = [
  { label: "Source", x: 135, tone: "good" },
  { label: "Ingest", x: 365, tone: "good" },
  { label: "State", x: 595, tone: "warm" },
  { label: "Decision", x: 825, tone: "active" },
  { label: "Send", x: 1055, tone: "active" },
  { label: "Reconcile", x: 1285, tone: "idle" },
  { label: "Report", x: 1360, tone: "idle" }
];

const NODE_DEFS = [
  { id: "agave", title: "Local Agave", symbol: "⌬", phase: "Ledger / RPC Truth", status: "checking", tone: "off", x: 595, y: 95, critical: true },
  { id: "state", title: "state", symbol: "◆", phase: "State Publisher", status: "checking", tone: "off", x: 595, y: 245, critical: true },
  { id: "oracle", title: "oracle", symbol: "ᛟ", phase: "Sidecars / LUTs", status: "idle", tone: "off", x: 825, y: 245, critical: false },
  { id: "laserstream", title: "LaserStream", symbol: "≋", phase: "External Source", status: "checking", tone: "off", x: 135, y: 245, critical: false },
  { id: "afxdp", title: "AF_XDP", symbol: "⌁", phase: "UDP / Shred Capture", status: "checking", tone: "off", x: 135, y: 430, critical: true },
  { id: "ingest", title: "ingest", symbol: "⟐", phase: "Decode To Record", status: "checking", tone: "off", x: 365, y: 430, critical: true },
  { id: "ipc", title: "ipc", symbol: "⧉", phase: "Feed Ring", status: "checking", tone: "off", x: 595, y: 430, critical: true },
  { id: "tier0", title: "tier0", symbol: "⦿", phase: "Decision Engine", status: "idle", tone: "off", x: 825, y: 430, critical: true },
  { id: "egress", title: "egress", symbol: "⟶", phase: "Send / No-Send", status: "stopped", tone: "off", x: 1055, y: 430, critical: true },
  { id: "pm", title: "pm", symbol: "⧖", phase: "Reconcile", status: "idle", tone: "off", x: 1285, y: 430, critical: true },
  { id: "control", title: "control", symbol: "✦", phase: "Proof / Report", status: "diagnostics", tone: "off", x: 1285, y: 600, critical: false }
];

const GRAPH_LAYOUTS = {
  runtime: {
    agave: [595, 95],
    state: [595, 245],
    oracle: [825, 245],
    laserstream: [135, 245],
    afxdp: [135, 430],
    ingest: [365, 430],
    ipc: [595, 430],
    tier0: [825, 430],
    egress: [1055, 430],
    pm: [1285, 430],
    control: [1285, 600]
  },
  topology: {
    tier0: [710, 340],
    state: [710, 120],
    agave: [470, 160],
    laserstream: [245, 240],
    oracle: [950, 160],
    ingest: [245, 340],
    ipc: [470, 340],
    egress: [1175, 340],
    afxdp: [470, 530],
    pm: [950, 530],
    control: [710, 590]
  }
};

function cloneGraphLayout(layout) {
  return Object.fromEntries(
    Object.entries(layout).map(([id, point]) => [id, [point[0], point[1]]])
  );
}

const LINKS = [
  { id: "agave_state", from: "agave", to: "state", tone: "warm", payload: "blockhash/account truth" },
  { id: "laserstream_ingest", from: "laserstream", to: "ingest", tone: "idle", payload: "breadth/continuity feed", toOffset: -28 },
  { id: "afxdp_ingest", from: "afxdp", to: "ingest", tone: "good", payload: "packet/shred capture", toOffset: 28 },
  { id: "ingest_ipc", from: "ingest", to: "ipc", tone: "good", payload: "IngressRecord feed" },
  { id: "ipc_tier0", from: "ipc", to: "tier0", tone: "warm", payload: "worker consume", toOffset: 28 },
  { id: "state_tier0", from: "state", to: "tier0", tone: "warm", payload: "pool/mint/ALT sidecars", toOffset: -28 },
  { id: "oracle_tier0", from: "oracle", to: "tier0", tone: "warm", payload: "advisory LUTs", toOffset: 28 },
  { id: "tier0_egress", from: "tier0", to: "egress", tone: "active", payload: "signed/no-send envelope" },
  { id: "egress_pm", from: "egress", to: "pm", tone: "active", payload: "result/NACK/status" },
  { id: "pm_control", from: "pm", to: "control", tone: "idle", payload: "safety proof" }
];

const nodeById = new Map(NODE_DEFS.map((node) => [node.id, node]));

const NODE_DETAILS = {
  agave: {
    role: "Local validator substrate. It receives Solana network traffic, maintains ledger/accounts/state, and provides local RPC truth for blockhash, account reads, leaders, contacts, and status.",
    processing: "Agave unit, local RPC health, UDP 8000-8025 listeners, eno1 traffic",
    observes: "getHealth, local slot, reference lag, active SH1 unit, RPC 8899, UDP listener count, eno1 owner.",
    emits: "Blockhash, account, leader/contact, and validator health truth consumed by state and control.",
    operator: "Running means getHealth=ok. Otherwise show lagging with slot drift or RPC unavailable; Agave should not be labeled blocked.",
    proof: "Agave-first preflight must be green before a bounded native AF_XDP lease."
  },
  afxdp: {
    role: "Owned packet capture boundary. AF_XDP should only light when Shadow actually owns live UDP/shred capture rather than merely seeing normal Agave wire traffic.",
    processing: "XDP attachment, XSK map, ingest process starts, fresh packet/shred artifacts",
    observes: "eno1 XDP program, XSK map, wire RX, shred-sized UDP evidence, AF_XDP/DPDK stage counters.",
    emits: "Packet/shred capture into ingest; this is the private-grade live-sensitive hot trigger.",
    operator: "Do not treat ordinary Agave wire RX as AF_XDP activity. This node lights only for Shadow-owned capture evidence.",
    proof: "Run77-family proof requires native XDP + AF_XDP zero-copy; xdpgeneric/copy is refused posture."
  },
  laserstream: {
    role: "External gRPC source used for breadth, continuity, and source-race research. Current docs treat it as credible supporting signal, while AF_XDP remains the private-grade live-sensitive trigger.",
    processing: "grpc-ingest process, LaserStream source grade, source-race artifacts, matched signature deltas",
    observes: "HELIUS_LASERSTREAM_URL/API key, grpc_source_grade=laserstream, RunLS window-local arbiter artifacts.",
    emits: "Breadth/continuity feed into ingest plus row-level source-race evidence for matched signatures.",
    operator: "RunLS6C had 15 matched rows and LaserStream was earlier by about 20.8 ms median on that sample, but trade-ready verdict stayed null.",
    proof: "Use as supporting/future strategy source until live architecture proves it can replace AF_XDP for hot-trigger truth."
  },
  ingest: {
    role: "Parses, reassembles, decodes, and normalizes packet/shred traffic into IngressRecord events for the feed ring.",
    processing: "shadow-ingest, shadow-ingest-replay, grpc-ingest, shred feed, IngressRecord emission",
    observes: "AF_XDP packets, LaserStream/grpc-ingest input, shred parser/reassembly/FEC recovery, artifact freshness.",
    emits: "Normalized IngressRecord events and raw archive records for IPC/feed consumers.",
    operator: "This node sees the market; it must not decide trades. If ingest is active but IPC is stale, the break is record publication.",
    proof: "Green only when its downstream feed ring moves in the same no-restart window."
  },
  ipc: {
    role: "Fixed-layout shared-memory language between Shadow daemons. It carries feed rings, state sidecars, NACK/status rings, health, control flags, and raw archives.",
    processing: "/dev/shm rings, feed-ring writes, shared binary contract movement",
    observes: "Versioned /dev/shm headers, feed/egress/NACK rings, health segments, control flags, raw archive surfaces.",
    emits: "Fixed binary contracts that Tier0, egress, PM, state, and control can read without dynamic schemas.",
    operator: "IPC is the nervous system. Moving producer data without consumer movement means the next stage is gated or stopped.",
    proof: "Hot path depends on fixed-layout, versioned, low-ambiguity shared memory."
  },
  state: {
    role: "Publishes worker-readable market truth from local Agave/RPC/account sources: pool, mint, ALT, health, blockhash, leader, and quote sidecars.",
    processing: "shadow-state ticks, pool/mint/ALT publication, state freshness",
    observes: "Local Agave/RPC, account bytes, pool/mint state, blockhash, leader schedule, quote sidecars, artifact ages.",
    emits: "Pool/mint/ALT/health/blockhash/leader snapshots for Tier0 and proof surfaces.",
    operator: "State can be fresh from artifacts even when the process is stopped; running should require live publish movement.",
    proof: "Freshness matters as much as existence. Stale state must not be confused with safe trading truth."
  },
  oracle: {
    role: "Off-path advisory publisher. It prepares compact LUTs and sidecars for leader class, bidding/tip banks, ownership, competition, and crowd heat.",
    processing: "shadow-oracle ticks, LUT/sidecar advice, route advisory handoff",
    observes: "Leader class, bid/tip bank inputs, competition/crowd heat, ownership/OPSO sidecar context.",
    emits: "Fixed-width advisory LUTs and strategy sidecars that Tier0 can read without asking Oracle to work.",
    operator: "Oracle is off-path advice. If it is off, Tier0 may still idle or block depending on the current route/proof gate.",
    proof: "No dynamic Oracle calls inside the money worker."
  },
  tier0: {
    role: "The hot-path decision engine. It consumes event plus state, then does pool tag lookup, route, quote, policy, build/sign, PM/no-send classification, and egress enqueue.",
    processing: "shadow-tier0 ticks, route/quote/policy/build-sign stages, signer inventory and caps",
    observes: "IngressRecord feed, state sidecars, oracle LUTs, inventory/fence state, route policy, egress health.",
    emits: "Drop, no-send event, PM reservation intent, or signed egress envelope.",
    operator: "This is where an observed event becomes a decision. No network I/O, RPC, blocking waits, or route search belongs here.",
    proof: "Conversion proof needs route/quote/policy/build-sign counters and downstream egress/PM movement."
  },
  egress: {
    role: "Out-of-worker send/no-send proxy. It drains chosen envelopes for TPU/QUIC, public scout, parked Sender/Jito/Harmonic lanes, then emits result/NACK/status.",
    processing: "shadow-egress-tpu ticks, egress envelope dequeue, would-send/live-send status, route-specific egress health",
    observes: "Signed envelopes from egress rings, TPU/QUIC worker state, public scout route, route-specific health.",
    emits: "Would-send/live-send result, NACK/status, health updates, PM reconcile inputs.",
    operator: "Tier0 builds and enqueues; egress drains and sends or records no-send. If Tier0 moves but egress is idle, dequeue/finalization is the boundary.",
    proof: "Accepted no-send proof needs candidate_seen, would_enqueue, ring_enqueue, sender_dequeue, and no_send_accept."
  },
  pm: {
    role: "Position manager and reconciliation authority. It owns reservation, release, landed/fail/timeout truth, inventory truth, and fee truth.",
    processing: "shadow-pm ticks, NACK/status consumption, reserve/release reconciliation",
    observes: "Pending table keyed by signature, egress result/NACK/status, bundle/signature status, inventory file and chain truth.",
    emits: "Reservation/release, landed/fail/timeout classification, inventory delta, fee truth, and risk flags.",
    operator: "PM is the accountant. If egress moves but PM is quiet, the system has not proven capital release or landed/fail truth.",
    proof: "Must prevent phantom inventory, leaked fees, and stuck reservations; stop-after-first-landed behavior is part of current route safety."
  },
  control: {
    role: "Control-plane proof surface. It reports gate input/report, blocker classification, run artifacts, diagnostics, and safety proof.",
    processing: "latest blocker, SSH health, gate reports, shadow-control diagnostics",
    observes: "g12 gate input/report, latest blocker, SSH health, run summaries, cleanup state, zero-candidate ladder evidence.",
    emits: "Blocker classification, run artifacts, diagnostics, safety proof, and supervisor-ready reports.",
    operator: "Control explains why a run is safe, blocked, stale, or incomplete. It is not a trading engine.",
    proof: "Show first red L0-L8 level, route policy, cleanup proof, and no hidden live-send path."
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
    setText("#capital-topline", "Capital unavailable");
    const item = document.createElement("div");
    item.className = "capital-item loading";
    item.innerHTML = "<span>Wallet</span><strong>Unreadable</strong>";
    list.appendChild(item);
    return;
  }

  count.textContent = `${items.length} live`;
  primary.textContent = items[0] ? items[0].value : "No balance";
  setText("#capital-topline", items[0] ? `${items[0].value} · ${items.length} capital` : "Capital empty");
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
  state.lastLiveAt = Date.now();
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
    `Host .15 · No-Send · ${data.latestBlocker ? data.latestBlocker.replace(/_/g, " ") : "proof pending"}`
  );
  const runtimeDot = $("#runtime-live-dot");
  if (runtimeDot) {
    runtimeDot.textContent = sshOk ? "LIVE" : "DEGRADED";
    runtimeDot.className = `operator-chip ${sshOk ? "ok" : "warn"}`;
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
    cadence.className = `operator-chip ${latency > 2800 ? "warn" : "ok"}`;
  }

  if (state.graph) state.graph.updateFromLive(data);
  renderHostPulse(data);

  if (
    data &&
    data.host &&
    data.host.sshOk === false &&
    /host_stream_closed|stream closed|host unreachable/i.test(`${data.latestBlocker || ""} ${data.rawError || ""}`)
  ) {
    const now = Date.now();
    if (now - state.lastStreamRecoveryAt > 2500) {
      state.lastStreamRecoveryAt = now;
      window.setTimeout(() => pollLiveSnapshot().catch(() => {}), 250);
    }
  }
}

function hostPulseTone(kind, value) {
  if (kind === "blocker") {
    return value === "ready_for_read_only_diagnostics" ? "good" : /not_green|unavailable|closed|unreachable|shadow_owned|xdp/i.test(value || "") ? "warm" : "idle";
  }
  if (kind === "agave") {
    if (value === "ok") return "good";
    if (value === "unavailable") return "blocked";
    return "warm";
  }
  if (kind === "owner") return value === "Agave" ? "good" : value === "Shadow" ? "warm" : "idle";
  if (kind === "process") return value > 0 ? "active" : "off";
  if (kind === "age") return value <= 20 ? "active" : value <= 300 ? "warm" : "off";
  return "idle";
}

function renderHostPulse(data = {}) {
  const rowsEl = $("#host-pulse-rows");
  const summaryEl = $("#host-pulse-summary");
  if (!rowsEl) return;

  const generated = data.generatedAt ? new Date(data.generatedAt) : new Date();
  const timeText = Number.isNaN(generated.getTime()) ? "--:--:--" : generated.toLocaleTimeString();
  const agave = data.agave || {};
  const activity = data.activity || {};
  const processes = data.processes || {};
  const eno1 = data.eno1 || {};
  const sampleEpochMs = data.sampleEpochMs || Date.now();
  const running = Object.entries(processes)
    .filter(([, value]) => Boolean(value))
    .map(([name]) => name);
  const processDetail = running.length
    ? running.slice(0, 4).join(", ") + (running.length > 4 ? ` +${running.length - 4}` : "")
    : "no shadow runtime processes alive";
  const blocker = data.latestBlocker || "unknown";
  const blockerText = blocker.replace(/_/g, " ");
  const rpcDetail = agave.localHealth === "ok"
    ? `slot ${formatNumber(agave.localSlot)} · lag ${formatSlotLag(agave.slotLag)}`
    : agave.localHealth === "unavailable"
      ? `RPC 8899 unavailable · unit ${agave.activeUnit || "unknown"}`
      : `${agave.healthMessage || "lagging"} · ${formatSlotLag(agave.slotLag)}`;
  const xdpText = eno1.xdpAttached ? "XDP on" : "XDP off";
  const xskText = eno1.xskMap ? "XSK present" : "XSK absent";
  const rxText = `${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s`;
  const feedAge = formatEpochAge(data.ingest && data.ingest.feedRingLatestEpoch, sampleEpochMs);
  const publishAge = formatEpochAge(data.state && data.state.publishLatestEpoch, sampleEpochMs);
  const feedAgeSec = Number(activity.ingestShmAgeSec);
  const publishAgeSec = Number(activity.statePublishAgeSec);
  const hostLive = Boolean(data.host && data.host.sshOk);

  const rows = [
    {
      tone: hostPulseTone("blocker", blocker),
      title: blocker === "ready_for_read_only_diagnostics" ? "Ready for diagnostics" : "Current blocker",
      detail: blockerText
    },
    {
      tone: hostPulseTone("agave", agave.localHealth),
      title: agave.localHealth === "ok" ? "Agave RPC green" : agave.localHealth === "unavailable" ? "Agave RPC unavailable" : "Agave lagging",
      detail: rpcDetail
    },
    {
      tone: hostPulseTone("owner", eno1.owner),
      title: `eno1 ${eno1.owner || "unknown"}-owned`,
      detail: `${xdpText} · ${xskText} · RX ${rxText}`
    },
    {
      tone: hostPulseTone("process", running.length),
      title: running.length ? `${running.length} Shadow process${running.length === 1 ? "" : "es"} alive` : "Shadow runtime idle",
      detail: processDetail
    }
  ];

  if (summaryEl) {
    summaryEl.textContent = hostLive
      ? `${agave.localHealth || "unknown"} · ${rxText}`
      : "host degraded";
  }

  rowsEl.replaceChildren(...rows.map((row) => {
    const item = document.createElement("div");
    item.className = `host-pulse-row tone-${row.tone || "idle"}`;

    const time = document.createElement("span");
    time.textContent = timeText;

    const title = document.createElement("strong");
    title.textContent = row.title;

    const detail = document.createElement("em");
    detail.textContent = row.detail;

    item.append(time, title, detail);
    return item;
  }));
}

async function loadWalletCapitals() {
  renderCapitals(await api("/api/wallet-capitals"));
}

async function loadConfig() {
  renderConfig(await api("/api/config"));
}

async function pollLiveSnapshot() {
  if (state.pollInFlight) return;
  state.pollInFlight = true;
  try {
    const snapshot = await api("/api/live-snapshot");
    renderLiveSnapshot(snapshot);
  } finally {
    state.pollInFlight = false;
  }
}

function updateClockAndWatchdog() {
  const now = Date.now();
  const clock = $("#runtime-clock");
  if (clock) clock.textContent = new Date(now).toLocaleTimeString();

  const ageMs = state.lastLiveAt ? now - state.lastLiveAt : Infinity;
  const cadence = $("#live-cadence");
  if (cadence && ageMs > 3500) {
    cadence.textContent = state.lastLiveAt ? `stale ${Math.round(ageMs / 1000)}s` : "connecting";
    cadence.className = "operator-chip warn";
  }

  if (ageMs > 4500) {
    pollLiveSnapshot().catch(() => {
      if (cadence) {
        cadence.textContent = "sample stalled";
        cadence.className = "operator-chip warn";
      }
    });
  }
}

function startLiveStream() {
  if (state.eventSource) state.eventSource.close();
  if (state.fallbackTimer) window.clearInterval(state.fallbackTimer);
  if (state.watchdogTimer) window.clearInterval(state.watchdogTimer);
  if (state.clockTimer) window.clearInterval(state.clockTimer);
  state.clockTimer = window.setInterval(updateClockAndWatchdog, 1000);
  state.watchdogTimer = window.setInterval(updateClockAndWatchdog, 1500);
  updateClockAndWatchdog();

  if (!window.EventSource) {
    pollLiveSnapshot().catch(() => {});
    state.fallbackTimer = window.setInterval(() => pollLiveSnapshot().catch(() => {}), 2000);
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
      cadence.className = "operator-chip warn";
    }
    pollLiveSnapshot().catch(() => {});
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

function portOffsetFor(link, endpoint, layout) {
  const layoutKey = `${layout}${endpoint === "from" ? "From" : "To"}Offset`;
  const genericKey = endpoint === "from" ? "fromOffset" : "toOffset";
  const offset = Number(link[layoutKey] ?? link[genericKey] ?? 0);
  return Number.isFinite(offset) ? offset : 0;
}

function nodeEdgePort(node, toward, offset = 0) {
  const [targetX, targetY] = toward;
  const dx = targetX - node.x;
  const dy = targetY - node.y;
  if (Math.abs(dx) >= Math.abs(dy)) {
    const nx = Math.sign(dx || 1);
    const yOffset = clamp(offset, -NODE_CARD_HEIGHT / 2 + 24, NODE_CARD_HEIGHT / 2 - 24);
    return {
      anchor: [node.x + nx * NODE_CARD_WIDTH / 2, node.y + yOffset],
      normal: [nx, 0]
    };
  }
  const ny = Math.sign(dy || 1);
  const xOffset = clamp(offset, -NODE_CARD_WIDTH / 2 + 28, NODE_CARD_WIDTH / 2 - 28);
  return {
    anchor: [node.x + xOffset, node.y + ny * NODE_CARD_HEIGHT / 2],
    normal: [0, ny]
  };
}

function offsetPoint(point, normal, distance) {
  return [point[0] + normal[0] * distance, point[1] + normal[1] * distance];
}

function approachDistance(fromPort, toPort) {
  const base = 30;
  const normalsFace = fromPort.normal[0] === -toPort.normal[0] && fromPort.normal[1] === -toPort.normal[1];
  if (!normalsFace) return base;
  const gap =
    (toPort.anchor[0] - fromPort.anchor[0]) * fromPort.normal[0] +
    (toPort.anchor[1] - fromPort.anchor[1]) * fromPort.normal[1];
  if (gap <= 0) return base;
  return clamp(gap / 3, 8, base);
}

function compactPathPoints(points) {
  const deduped = points.filter((point, index) => {
    if (!index) return true;
    const previous = points[index - 1];
    return Math.abs(point[0] - previous[0]) > 0.5 || Math.abs(point[1] - previous[1]) > 0.5;
  });
  return deduped.filter((point, index) => {
    if (!index || index === deduped.length - 1) return true;
    const previous = deduped[index - 1];
    const next = deduped[index + 1];
    const sameHorizontal = Math.abs(previous[1] - point[1]) < 0.5 && Math.abs(point[1] - next[1]) < 0.5;
    const sameVertical = Math.abs(previous[0] - point[0]) < 0.5 && Math.abs(point[0] - next[0]) < 0.5;
    if (!sameHorizontal && !sameVertical) return true;
    const dx1 = point[0] - previous[0];
    const dx2 = next[0] - point[0];
    const dy1 = point[1] - previous[1];
    const dy2 = next[1] - point[1];
    return dx1 * dx2 + dy1 * dy2 < 0;
  });
}

function orthogonalBendPoints(fromPoint, toPoint) {
  const [x1, y1] = fromPoint;
  const [x2, y2] = toPoint;
  if (Math.abs(x1 - x2) < 2 || Math.abs(y1 - y2) < 2) return [toPoint];
  if (Math.abs(x2 - x1) >= Math.abs(y2 - y1)) {
    const midX = (x1 + x2) / 2;
    return [[midX, y1], [midX, y2], toPoint];
  }
  const midY = (y1 + y2) / 2;
  return [[x1, midY], [x2, midY], toPoint];
}

function pathForLink(link, layout = "runtime") {
  const from = nodeById.get(link.from);
  const to = nodeById.get(link.to);
  const via = link[`${layout}Via`] || link.via || [];
  const directTarget = via[0] || graphPoint(to);
  const directSource = via[via.length - 1] || graphPoint(from);
  const fromPort = nodeEdgePort(from, directTarget, portOffsetFor(link, "from", layout));
  const toPort = nodeEdgePort(to, directSource, portOffsetFor(link, "to", layout));
  const approach = approachDistance(fromPort, toPort);
  const fromExit = offsetPoint(fromPort.anchor, fromPort.normal, approach);
  const toEntry = offsetPoint(toPort.anchor, toPort.normal, approach);
  const middle = via.length
    ? [...via, toEntry]
    : orthogonalBendPoints(fromExit, toEntry);
  const points = compactPathPoints([fromPort.anchor, fromExit, ...middle, toPort.anchor]);
  return points
    .map(([x, y], index) => `${index ? "L" : "M"} ${x} ${y}`)
    .join(" ");
}

function routeLabel(link) {
  const from = nodeById.get(link.from);
  const to = nodeById.get(link.to);
  return `${from.title} -> ${to.title}`;
}

function formatNumber(value, digits = 0) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "0";
  return number.toLocaleString(undefined, { maximumFractionDigits: digits });
}

function formatSlotLag(value) {
  if (value === null || value === undefined || value === "") return "unknown slots";
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
      ["getHealth", truth.rpcGreen ? "ok" : truth.rpcUnavailable ? "unavailable" : "lagging", truth.rpcGreen ? "good" : "warm"],
      ["slot lag", truth.slotLagKnown || truth.rpcGreen ? truth.slotLagText : "unavailable", truth.rpcGreen ? "good" : truth.slotLagKnown ? "warm" : "off"],
      ["local slot", truth.localSlotKnown ? formatNumber(truth.localSlot) : "unknown", truth.localSlotKnown ? "idle" : "off"],
      ["RPC 8899", truth.rpcPortListening ? "listening" : "not listening", truth.rpcPortListening ? "idle" : "off"],
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

  if (id === "laserstream") {
    const truth = laserstreamTruth(data);
    return [
      ["source grade", truth.grade, truth.configured ? "good" : "off"],
      ["process", truth.processUp ? "present" : "absent", truth.processUp ? "active" : "off"],
      ["grpc ticks", `${formatNumber(truth.tickRate, 1)} t/s`, truth.moving ? "active" : "off"],
      ["role", truth.configured ? "breadth / continuity" : "not configured", truth.configured ? "idle" : "off"],
      ["private-grade", "no", "off"],
      ["AF_XDP trigger", "separate", "good"]
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
    const publishValue = truth.sidecarMoving
      ? "fresh"
      : truth.artifactFresh
        ? truth.processUp ? "fresh" : "files fresh"
        : truth.stale
          ? "stale"
          : truth.publishReady ? "idle" : "missing";
    const publishTone = truth.sidecarMoving ? "active" : truth.artifactFresh ? "idle" : truth.stale ? "warm" : truth.publishReady ? "idle" : "off";
    return [
      ["process", processState(processes, "shadow-state"), truth.processUp ? "good" : "off"],
      ["publish", publishValue, publishTone],
      ["publish age", formatAgeSeconds(truth.publishAgeSec), truth.sidecarMoving ? "active" : truth.artifactFresh ? "idle" : truth.stale ? "warm" : "idle"],
      ["blockhash age", formatAgeSeconds(truth.blockhashAgeSec), truth.blockhashRecent ? "active" : truth.stale ? "warm" : "idle"],
      ["core age", formatAgeSeconds(truth.coreAgeSec), truth.sidecarMoving ? "active" : truth.artifactFresh ? "idle" : truth.stale ? "warm" : "idle"],
      ["sidecar bytes", formatBytes(truth.publishBytes), truth.publishReady ? "good" : "off"],
      ["state ticks", processTicks("shadow-state"), activity.stateProcessBusy ? "idle" : "off"],
      ["shm rings", formatNumber(data.ipc && data.ipc.shmCount || 0), data.ipc && data.ipc.shmCount ? "good" : "off"],
      ["Agave health", agave.rpcGreen ? "ok" : agave.rpcUnavailable ? "RPC unavailable" : "lagging", agave.rpcGreen ? "good" : "warm"],
      ["Agave lag", agave.slotLagKnown || agave.rpcGreen ? agave.slotLagText : "unavailable", agave.rpcGreen ? "good" : agave.slotLagKnown ? "warm" : "off"]
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
    const truth = egressTruth(data);
    return [
      ["process", truth.processUp ? "running" : "stopped", truth.processUp ? "active" : "off"],
      ["egress ticks", processTicks("shadow-egress-tpu"), truth.tickRate >= 0.1 ? "active" : "off"],
      ["rings", truth.ringReady ? `${formatNumber(truth.ringCount)} present` : "none", truth.ringReady ? "idle" : "off"],
      ["ring age", formatAgeSeconds(truth.ringAgeSec), truth.ringMoving || truth.ringRecent ? "active" : truth.stale ? "warm" : "off"],
      ["health age", formatAgeSeconds(truth.healthAgeSec), activity.egressHealthRecent ? "active" : truth.healthEpoch ? "warm" : "off"],
      ["TPU path", truth.moving ? "moving" : truth.processUp ? "ready" : truth.stale ? "stale" : "stopped", truth.moving ? "active" : truth.processUp ? "idle" : "off"]
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
      if (truth.rpcUnavailable) return `RPC 8899 unavailable · ${sampleMs}`;
      return truth.rpcGreen ? "getHealth ok · 0 slots" : `lagging ${truth.slotLagText} · ${sampleMs}`;
    }
    if (id === "state") {
      const truth = stateTruth(data);
      const age = stateArtifactAgeSec(truth);
      if (truth.sidecarMoving) return `publish fresh · ${formatAgeSeconds(age)}`;
      if (truth.artifactFresh) return `files fresh · ${formatAgeSeconds(age)}`;
      if (truth.stale) return `files stale · ${formatAgeSeconds(age)}`;
      return truth.processUp ? "process up · no publish" : "process stopped";
    }
    if (id === "laserstream") {
      const truth = laserstreamTruth(data);
      return truth.moving ? `${formatNumber(truth.tickRate, 1)} t/s · source feed` : truth.configured ? "configured · idle" : "not configured";
    }
    if (id === "tier0") return signal.active ? "decision p95 live" : "decision idle";
    if (id === "egress") return signal.active ? "route latency live" : "route idle";
    return signal.active ? `live · ${sampleMs}` : `idle · ${sampleMs}`;
  }

  if (mode === "economics") {
    if (id === "agave") {
      const truth = agaveTruth(data);
      return truth.rpcGreen ? "RPC truth usable" : truth.rpcUnavailable ? "RPC unavailable" : `RPC lagging · ${truth.slotLagText}`;
    }
    if (id === "state") {
      const truth = stateTruth(data);
      if (truth.sidecarMoving) return "account sidecars publishing";
      if (truth.artifactFresh) return "account sidecars fresh";
      return truth.stale ? "account sidecars stale" : "sidecars stopped";
    }
    if (id === "laserstream") return "breadth source · not private-grade";
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
      if (truth.rpcUnavailable) return truth.unitActive ? "unit active · RPC unavailable" : "RPC unavailable";
      if (truth.unitActive) return `unit active · lagging ${truth.slotLagText}`;
      return truth.udpReady ? `UDP present · lagging ${truth.slotLagText}` : `lagging ${truth.slotLagText}`;
    }
    if (id === "state") {
      const truth = stateTruth(data);
      const age = stateArtifactAgeSec(truth);
      if (truth.sidecarMoving) return "state publish fresh";
      if (truth.artifactFresh) return `state files fresh · ${formatAgeSeconds(age)}`;
      if (truth.stale) return `sidecars stale · ${formatAgeSeconds(age)}`;
      return truth.processUp ? "process up · no sidecars" : "process stopped";
    }
    if (id === "laserstream") {
      const truth = laserstreamTruth(data);
      return truth.processUp ? "grpc source up" : truth.configured ? "configured · process absent" : "source unavailable";
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
    if (truth.rpcUnavailable) return `RPC 8899 unavailable · ${truth.unitActive ? "unit active" : "unit unknown"}`;
    if (truth.unitActive) return `lagging ${truth.slotLagText} · unit active`;
    if (truth.udpReady) return `lagging ${truth.slotLagText} · ${formatNumber(truth.udpCount)} UDP`;
    return truth.hostLive ? `lagging ${truth.slotLagText} · no UDP` : "host unreachable";
  }
  if (id === "afxdp") {
    const truth = ingestTruth(data);
    const xdp = data.eno1 && data.eno1.xdpAttached ? "XDP on" : "XDP off";
    return truth.captureMoving ? `${xdp} · ${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s` : `${xdp} · ${truth.captureReady ? "armed" : "standby"}`;
  }
  if (id === "laserstream") {
    const truth = laserstreamTruth(data);
    if (truth.moving) return `${formatNumber(truth.tickRate, 1)} t/s · ${truth.grade}`;
    if (truth.processUp) return `connected · ${truth.grade}`;
    return truth.configured ? `${truth.grade} · idle` : "external source off";
  }
  if (id === "ingest") {
    const truth = ingestTruth(data);
    if (truth.tickRate >= 0.1) return `${formatNumber(truth.tickRate, 1)} t/s · decoding`;
    if (truth.feedRingMoving || truth.feedRingRecent) return "feed ring live";
    if (/stale/i.test(signal.status || "") || truth.feedRingReady) return "feed ring stale";
    const proc = truth.processUp ? "process up" : "process absent";
    return `${proc} · ${signal.active ? "decoding" : "idle"}`;
  }
  if (id === "ipc") return `feed ring · ${signal.active ? "moving" : "idle"}`;
  if (id === "state") {
    const truth = stateTruth(data);
    const age = stateArtifactAgeSec(truth);
    if (truth.sidecarMoving) return `publish fresh · ${formatAgeSeconds(age)}`;
    if (truth.artifactFresh) return `files fresh · ${formatAgeSeconds(age)}`;
    if (truth.stale) return `files stale · ${formatAgeSeconds(age)}`;
    if (truth.processUp) return "process up · no sidecars";
    return "process stopped";
  }
  if (id === "oracle") return `${processes["shadow-oracle"] ? "oracle up" : "oracle off"} · LUTs`;
  if (id === "tier0") return `${rate("shadow-tier0")} · ${processes["shadow-tier0"] ? "worker up" : "stopped"}`;
  if (id === "egress") {
    const truth = egressTruth(data);
    if (truth.moving) return truth.tickRate >= 0.1 ? `${formatNumber(truth.tickRate, 1)} t/s · No-Send` : "egress ring live";
    if (truth.processUp) return "TPU proxy ready · No-Send";
    if (truth.stale) return `rings stale · ${formatAgeSeconds(truth.ringAgeSec)}`;
    return "egress stopped";
  }
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
  if (tone === "blocked" || /blocked|fail|error/i.test(text || "")) return "BLOCKED";
  if (/lagging/i.test(text || "")) return "LAGGING";
  if (/stale/i.test(text || "") && tone && tone !== "off") return "LAGGING";
  if (/unknown|checking|connecting|insufficient/i.test(text || "")) return "UNKNOWN";
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
  const rpcUnavailable = agave.localHealth === "unavailable";
  const rpcPortListening = agave.rpcPortListening !== false;
  const udpCount = Number(agave.udpListenerCount || 0);
  const udpReady = udpCount > 0;
  const slotLag = agave.slotLag === null || agave.slotLag === undefined || agave.slotLag === "" ? NaN : Number(agave.slotLag);
  const slotLagKnown = Number.isFinite(slotLag);
  const localSlot = Number(agave.localSlot);
  const localSlotKnown = Number.isFinite(localSlot);
  const slotLagText = rpcGreen ? "0 slots" : rpcUnavailable ? "RPC unavailable" : formatSlotLag(slotLag);
  return {
    hostLive,
    unitActive,
    rpcGreen,
    rpcUnavailable,
    rpcPortListening,
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
  const artifactFresh = Boolean(publishMoving || publishRecent || blockhashRecent);
  const sidecarMoving = Boolean(processUp && artifactFresh);
  const shmMoving = Boolean(activity.shmMoving);
  const stale = Boolean(publishReady && !artifactFresh);
  return {
    processUp,
    artifactFresh,
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

function stateArtifactAgeSec(truth = {}) {
  const publishAge = Number(truth.publishAgeSec);
  if (Number.isFinite(publishAge) && publishAge >= 0) return publishAge;
  const blockhashAge = Number(truth.blockhashAgeSec);
  if (Number.isFinite(blockhashAge) && blockhashAge >= 0) return blockhashAge;
  const coreAge = Number(truth.coreAgeSec);
  if (Number.isFinite(coreAge) && coreAge >= 0) return coreAge;
  return NaN;
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
    (processUp && (wireMoving || feedRingMoving || feedRingRecent || artifactMoving || artifactRecent))
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

function laserstreamTruth(data = {}) {
  const activity = data.activity || {};
  const processes = data.processes || {};
  const source = data.source || {};
  const tickRate = processTickRate(data, "grpc-ingest");
  const processUp = Boolean(processes["grpc-ingest"] || source.grpcIngestActive || tickRate > 0);
  const configured = Boolean(source.laserstreamConfigured || /laserstream/i.test(source.grpcSourceGrade || ""));
  const moving = Boolean(processUp && (activity.laserstreamBusy || tickRate >= 0.1));
  return {
    configured,
    processUp,
    moving,
    tickRate,
    grade: source.grpcSourceGrade || (configured ? "laserstream" : "unknown")
  };
}

function egressTruth(data = {}) {
  const activity = data.activity || {};
  const processes = data.processes || {};
  const egress = data.egress || {};
  const tickRate = processTickRate(data, "shadow-egress-tpu");
  const processUp = Boolean(processes["shadow-egress-tpu"] || egress.processActive || tickRate > 0);
  const ringCount = Number(egress.ringCount || 0);
  const ringBytes = Number(egress.totalBytes || 0);
  const latestEpoch = Number(egress.latestEpoch || 0);
  const healthEpoch = Number(egress.healthEpoch || 0);
  const ringReady = ringCount > 0 || latestEpoch > 0 || ringBytes > 0 || healthEpoch > 0;
  const ringMoving = Boolean(activity.egressShmMoving);
  const ringRecent = Boolean(activity.egressShmRecent || activity.egressHealthRecent);
  const ringAgeSec = Number(activity.egressShmAgeSec);
  const healthAgeSec = Number(activity.egressHealthAgeSec);
  const moving = Boolean(activity.egressBusy || ringMoving || (processUp && ringRecent));
  const stale = Boolean(ringReady && !ringMoving && !ringRecent);
  return {
    processUp,
    ringReady,
    ringMoving,
    ringRecent,
    ringCount,
    ringBytes,
    latestEpoch,
    healthEpoch,
    ringAgeSec,
    healthAgeSec,
    tickRate,
    moving,
    stale
  };
}

function nodeInspectorSummary(id, live, entry, detail = {}) {
  const data = live || {};
  const activity = data.activity || {};
  const processes = data.processes || {};
  const sampleMs = `${formatNumber(data.durationMs || 0)}ms sample`;

  if (id === "agave") {
    const truth = agaveTruth(data);
    if (truth.rpcGreen) return `getHealth ok at slot ${formatNumber(truth.localSlot)} with ${formatNumber(truth.udpCount)} UDP listeners; ${data.eno1 && data.eno1.owner || "unknown"} owns eno1.`;
    if (truth.rpcUnavailable) return `RPC 8899 unavailable; unit is ${data.agave && data.agave.activeUnit || "unknown"} and slot lag cannot be trusted from this sample.`;
    return `Agave is lagging by ${truth.slotLagText}; local slot ${truth.localSlotKnown ? formatNumber(truth.localSlot) : "unknown"}; ${sampleMs}.`;
  }

  if (id === "laserstream") {
    const truth = laserstreamTruth(data);
    return `${truth.grade} source is ${truth.configured ? "configured" : "not configured"}; grpc-ingest is ${truth.processUp ? "present" : "stopped"} at ${formatNumber(truth.tickRate, 1)} t/s.`;
  }

  if (id === "afxdp") {
    const truth = ingestTruth(data);
    return truth.captureMoving
      ? "Shadow-owned packet capture is moving into ingest evidence."
      : truth.captureReady
        ? "Capture is armed, but this sample has no fresh feed movement."
        : "Host wire is visible, but Shadow capture ownership is currently off.";
  }

  if (id === "ingest") {
    const truth = ingestTruth(data);
    return `Ingest process ${truth.processUp ? "present" : "stopped"}; feed ring ${truth.feedRingReady ? "present" : "missing"}; last feed update ${formatEpochAge(data.ingest && data.ingest.feedRingLatestEpoch, data.sampleEpochMs)}.`;
  }

  if (id === "ipc") {
    return `${formatNumber(data.ipc && data.ipc.shmCount || 0)} shared-memory files, ${formatBytes(data.ipc && data.ipc.totalBytes)} total; latest movement ${formatEpochAge(data.ipc && data.ipc.latestEpoch, data.sampleEpochMs)}.`;
  }

  if (id === "state") {
    const truth = stateTruth(data);
    return `state process ${truth.processUp ? "running" : "stopped"} at ${formatNumber(activity.processTickRates && activity.processTickRates["shadow-state"] || 0, 1)} t/s; publish age ${formatAgeSeconds(truth.publishAgeSec)}; blockhash age ${formatAgeSeconds(truth.blockhashAgeSec)}.`;
  }

  if (id === "oracle") {
    return `oracle process ${processes["shadow-oracle"] ? "running" : "stopped"} at ${formatNumber(activity.processTickRates && activity.processTickRates["shadow-oracle"] || 0, 1)} t/s; route advice ${entry && entry.active ? "feeding" : "idle"}.`;
  }

  if (id === "tier0") {
    return `tier0 process ${processes["shadow-tier0"] ? "running" : "stopped"} at ${formatNumber(activity.processTickRates && activity.processTickRates["shadow-tier0"] || 0, 1)} t/s; blocker ${data.latestBlocker || "unknown"}.`;
  }

  if (id === "egress") {
    const truth = egressTruth(data);
    return `egress process ${truth.processUp ? "running" : "stopped"}; ${formatNumber(truth.ringCount)} rings; ring age ${formatAgeSeconds(truth.ringAgeSec)}; health age ${formatAgeSeconds(truth.healthAgeSec)}.`;
  }

  if (id === "pm") {
    return processes["shadow-pm"]
      ? "PM is available to reconcile reservations, releases, landed/fail, inventory, and fee truth."
      : "PM is stopped; no current reservation or reconciliation loop is proving capital truth.";
  }

  if (id === "control") {
    return data.host && data.host.sshOk
      ? `Control has a live .15 sample in ${sampleMs}; use the evidence rows for blocker and route posture.`
      : "Control cannot prove the host state from this sample.";
  }

  return entry && entry.text ? `${entry.text}; ${sampleMs}.` : detail.role || "No live evidence yet.";
}

function nodeEvidenceItems(id, live, entry) {
  const data = live || {};
  const activity = data.activity || {};
  const processes = data.processes || {};
  const ticks = activity.processTickRates || {};
  const items = [];
  const add = (label, value, tone = "idle") => {
    if (value === null || value === undefined || value === "") return;
    items.push({ label, value, tone });
  };
  const processTick = (name) => `${processes[name] ? "running" : "stopped"} · ${formatNumber(ticks[name] || 0, 1)} t/s`;
  const ageFromEpoch = (epoch) => formatEpochAge(epoch, data.sampleEpochMs);

  if (id === "agave") {
    const truth = agaveTruth(data);
    add("RPC", truth.rpcGreen ? `getHealth ok · slot ${formatNumber(truth.localSlot)}` : truth.rpcUnavailable ? "RPC 8899 unavailable" : `lagging ${truth.slotLagText}`, truth.rpcGreen ? "good" : "warm");
    add("Lag", truth.slotLagKnown || truth.rpcGreen ? truth.slotLagText : "unavailable", truth.rpcGreen ? "good" : truth.slotLagKnown ? "warm" : "off");
    add("UDP/RX", `${formatNumber(truth.udpCount)} listeners · ${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s`, truth.udpReady && activity.wireMoving ? "active" : truth.udpReady ? "idle" : "off");
    add("eno1", `${data.eno1 && data.eno1.owner || "unknown"} · XDP ${data.eno1 && data.eno1.xdpAttached ? "on" : "off"} · XSK ${data.eno1 && data.eno1.xskMap ? "present" : "absent"}`, data.eno1 && data.eno1.owner === "Agave" ? "good" : "warm");
  } else if (id === "laserstream") {
    const truth = laserstreamTruth(data);
    add("Source", `${truth.grade} · ${truth.configured ? "configured" : "not configured"}`, truth.configured ? "good" : "off");
    add("grpc-ingest", processTick("grpc-ingest"), truth.processUp ? "active" : "off");
    add("Current feed", truth.moving ? "moving" : "idle", truth.moving ? "active" : "idle");
    add("Latest proof", "RunLS6C: 15 matched rows; trade-ready delta null", "warm");
  } else if (id === "afxdp") {
    const truth = ingestTruth(data);
    add("XDP/XSK", `${data.eno1 && data.eno1.xdpAttached ? "XDP on" : "XDP off"} · ${data.eno1 && data.eno1.xskMap ? "XSK present" : "XSK absent"}`, truth.captureReady ? "active" : "off");
    add("Wire RX", `${formatNumber(activity.rxPacketsPerSec || 0)} pkt/s`, truth.captureMoving ? "active" : "idle");
    add("Capture", truth.captureMoving ? "moving" : truth.captureReady ? "armed" : "standby", truth.captureMoving ? "active" : truth.captureReady ? "idle" : "off");
    add("Feed evidence", `${formatBytes(truth.feedRingBytes)} · ${ageFromEpoch(data.ingest && data.ingest.feedRingLatestEpoch)}`, truth.feedRingRecent ? "active" : truth.feedRingReady ? "warm" : "off");
  } else if (id === "ingest") {
    const truth = ingestTruth(data);
    add("Process", processTick("shadow-ingest"), truth.processUp ? "active" : "off");
    add("Feed ring", `${formatBytes(truth.feedRingBytes)} · ${ageFromEpoch(data.ingest && data.ingest.feedRingLatestEpoch)}`, truth.feedRingMoving || truth.feedRingRecent ? "active" : truth.feedRingReady ? "warm" : "off");
    add("Artifacts", `${formatBytes(data.ingest && data.ingest.artifactTotalBytes)} · ${ageFromEpoch(data.ingest && data.ingest.artifactLatestEpoch)}`, activity.ingestArtifactRecent ? "active" : "idle");
    add("Delta", truth.ingestMoving ? "moving" : "idle", truth.ingestMoving ? "active" : "off");
  } else if (id === "ipc") {
    add("SHM files", `${formatNumber(data.ipc && data.ipc.shmCount || 0)} · ${formatBytes(data.ipc && data.ipc.totalBytes)}`, data.ipc && data.ipc.shmCount ? "good" : "off");
    add("Latest SHM", ageFromEpoch(data.ipc && data.ipc.latestEpoch), activity.shmMoving ? "active" : "idle");
    add("Feed ring", `${formatBytes(data.ingest && data.ingest.feedRingTotalBytes)} · ${ageFromEpoch(data.ingest && data.ingest.feedRingLatestEpoch)}`, activity.ingestShmRecent ? "active" : "warm");
  } else if (id === "state") {
    const truth = stateTruth(data);
    add("Process", processTick("shadow-state"), truth.processUp ? "good" : "off");
    add("Publish", `${formatBytes(truth.publishBytes)} · ${formatAgeSeconds(truth.publishAgeSec)}`, truth.sidecarMoving ? "active" : truth.artifactFresh ? "idle" : truth.stale ? "warm" : "off");
    add("Blockhash", formatAgeSeconds(truth.blockhashAgeSec), truth.blockhashRecent ? "active" : truth.stale ? "warm" : "idle");
    add("Health", formatAgeSeconds(truth.healthAgeSec), activity.stateHealthAgeSec <= 20 ? "active" : "warm");
  } else if (id === "oracle") {
    add("Process", processTick("shadow-oracle"), processes["shadow-oracle"] ? "warm" : "off");
    add("Route advice", entry && entry.active ? "feeding Tier0" : "idle", entry && entry.active ? "active" : "off");
    add("Tier0 edge", liveEdge(data, "oracle_route").active ? "advisory handoff moving" : "no advisory handoff", liveEdge(data, "oracle_route").active ? "active" : "off");
  } else if (id === "tier0") {
    add("Process", processTick("shadow-tier0"), processes["shadow-tier0"] ? "active" : "off");
    add("Route policy", data.routePolicy && data.routePolicy.blockPrivateBundleRoutesSeen ? "private bundles blocked" : "unknown", data.routePolicy && data.routePolicy.blockPrivateBundleRoutesSeen ? "good" : "warm");
    add("TPU workers", data.routePolicy && data.routePolicy.quicWorkersZeroSeen ? "quic_workers=0 seen" : "not zero in config sample", data.routePolicy && data.routePolicy.quicWorkersZeroSeen ? "warm" : "idle");
    add("Blocker", data.latestBlocker || "unknown", "warm");
  } else if (id === "egress") {
    const truth = egressTruth(data);
    add("Process", processTick("shadow-egress-tpu"), truth.processUp ? "active" : "off");
    add("Rings", `${formatNumber(truth.ringCount)} · ${formatBytes(truth.ringBytes)}`, truth.ringReady ? "idle" : "off");
    add("Ring age", formatAgeSeconds(truth.ringAgeSec), truth.ringMoving || truth.ringRecent ? "active" : truth.stale ? "warm" : "off");
    add("Health age", formatAgeSeconds(truth.healthAgeSec), activity.egressHealthRecent ? "active" : truth.healthEpoch ? "warm" : "off");
  } else if (id === "pm") {
    add("Process", processTick("shadow-pm"), processes["shadow-pm"] ? "active" : "off");
    add("Input status", activity.egressBusy || activity.egressShmRecent ? "egress status recent" : `egress age ${formatAgeSeconds(activity.egressShmAgeSec)}`, activity.egressBusy || activity.egressShmRecent ? "active" : "warm");
    add("Inventory", data.inventory && data.inventory.filePresent ? `${formatBytes(data.inventory.size)} · ${ageFromEpoch(data.inventory.mtime)}` : "missing", data.inventory && data.inventory.filePresent ? "good" : "off");
    add("Blocker", data.latestBlocker || "unknown", "idle");
  } else if (id === "control") {
    add("Host", data.host && data.host.sshOk ? `${data.host.label || ".15"} live · ${formatNumber(data.durationMs || 0)}ms` : "degraded", data.host && data.host.sshOk ? "good" : "off");
    add("Blocker", data.latestBlocker || "unknown", data.latestBlocker === "ready_for_read_only_diagnostics" ? "good" : "warm");
    add("Route policy", data.routePolicy && data.routePolicy.blockPrivateBundleRoutesSeen ? "private routes blocked" : "unknown", data.routePolicy && data.routePolicy.blockPrivateBundleRoutesSeen ? "good" : "warm");
    add("Private send", "disabled", "off");
  }

  return items;
}

function projectCanonicalNodeSignals(data = {}) {
  const activity = data.activity || {};
  const processes = data.processes || {};
  const eno1 = data.eno1 || {};
  const ingest = data.ingest || {};
  const agave = agaveTruth(data);
  const shadowState = stateTruth(data);
  const ingestPath = ingestTruth(data);
  const laserstreamPath = laserstreamTruth(data);
  const egressPath = egressTruth(data);
  const agaveStatus = agave.rpcGreen ? "getHealth ok" : agave.rpcUnavailable ? "lagging RPC unavailable" : `lagging ${agave.slotLagText}`;
  const agaveTone = agave.rpcGreen
    ? "good"
    : agave.hostLive
      ? "warm"
      : "off";
  const stateAgeSec = stateArtifactAgeSec(shadowState);
  const stateStatus = shadowState.processUp
    ? shadowState.sidecarMoving
      ? `state fresh ${formatAgeSeconds(stateAgeSec)}`
      : shadowState.stale
        ? `state stale ${formatAgeSeconds(stateAgeSec)}`
        : "state process up"
    : shadowState.artifactFresh
      ? `state files fresh ${formatAgeSeconds(stateAgeSec)}`
      : shadowState.stale
        ? `state files stale ${formatAgeSeconds(stateAgeSec)}`
        : "state stopped";
  const stateTone = shadowState.processUp
    ? shadowState.sidecarMoving ? "good" : shadowState.stale ? "warm" : "idle"
    : shadowState.artifactFresh ? "idle" : shadowState.stale ? "warm" : "off";
  const afxdpReady = Boolean(ingestPath.captureReady || processes.afxdp || processes.shred);
  const afxdpMoving = Boolean(
    afxdpReady &&
    (
      ingestPath.captureMoving ||
      activity.ingestBusy ||
      activity.ingestShmMoving ||
      activity.ingestShmRecent ||
      activity.ingestArtifactMoving ||
      activity.ingestArtifactRecent ||
      activity.ingestProcessStarted
    )
  );
  const ingestOld = liveNode(data, "ingest");
  const recordOld = liveNode(data, "record");
  const ingestRuntimeReady = Boolean(
    ingestPath.captureReady ||
    ingestPath.processUp ||
    ingestPath.feedRingMoving ||
    ingestPath.feedRingRecent ||
    ingestPath.artifactMoving ||
    ingestPath.artifactRecent
  );
  const ingestMoving = Boolean(ingestPath.ingestMoving || ingestOld.active || liveNode(data, "decode").active || recordOld.active);
  const laserstreamStatus = laserstreamPath.moving
    ? `LaserStream ${formatNumber(laserstreamPath.tickRate, 1)} t/s`
    : laserstreamPath.processUp
      ? "LaserStream connected"
      : laserstreamPath.configured ? "LaserStream configured" : "external source off";
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
    : ingestRuntimeReady ? "decode idle" : ingestPath.feedRingReady ? "feed ring stale" : "ingest absent";
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
  const egressStatus = egressPath.moving
    ? egressPath.tickRate >= 0.1 ? `egress ${formatNumber(egressPath.tickRate, 1)} t/s` : "egress ring live"
    : egressPath.processUp ? "TPU proxy ready" : egressPath.stale ? "egress rings stale" : "egress stopped";

  return {
    agave: signal(agave.rpcGreen, agaveTone, agaveStatus),
    laserstream: signal(laserstreamPath.moving, laserstreamPath.processUp ? "active" : laserstreamPath.configured ? "idle" : "off", laserstreamStatus),
    afxdp: signal(afxdpMoving, afxdpReady ? "active" : "off", afxdpMoving ? "capture moving" : afxdpReady ? "capture ready" : "XDP detached"),
    ingest: signal(ingestMoving, ingestMoving ? "active" : ingestRuntimeReady ? "idle" : "off", ingestStatus),
    ipc: liveNode(data, "ipc"),
    state: signal(shadowState.processUp && shadowState.sidecarMoving, stateTone, stateStatus),
    oracle: liveNode(data, "oracle"),
    tier0: signal(tier0Moving, processes["shadow-tier0"] ? "active" : tier0Moving ? "warm" : "off", tier0Moving ? (tier0Ticks ? `tier0 ${tier0Ticks} t/s` : "route/quote/policy moving") : processes["shadow-tier0"] ? "tier0 idle" : "tier0 stopped"),
    egress: signal(egressPath.moving, egressPath.moving || egressPath.processUp ? "active" : "off", egressStatus),
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
  const shadowState = stateTruth(data);
  const laserstreamPath = laserstreamTruth(data);
  const agaveActive = Boolean(nodes.agave && nodes.agave.active);
  const laserstreamActive = Boolean((nodes.laserstream && nodes.laserstream.active) || laserstreamPath.moving);
  const afxdpActive = Boolean(nodes.afxdp && nodes.afxdp.active);
  const ingestActive = Boolean(nodes.ingest && nodes.ingest.active);
  const ipcActive = Boolean(nodes.ipc && nodes.ipc.active);
  const stateActive = Boolean(nodes.state && nodes.state.active);
  const stateFlowFresh = Boolean(shadowState.sidecarMoving || shadowState.artifactFresh);
  const oracleActive = Boolean(nodes.oracle && nodes.oracle.active);
  const tier0Active = Boolean(nodes.tier0 && nodes.tier0.active);
  const egressActive = Boolean(nodes.egress && nodes.egress.active);
  const pmActive = Boolean(nodes.pm && nodes.pm.active);

  return {
    agave_state: activeIf("agave_state", agaveActive && stateFlowFresh, "blockhash/account truth", "warm", 0.65),
    laserstream_ingest: activeIf("laserstream_ingest", oldEdgeActive("laserstream_ingest") || (laserstreamActive && ingestActive), "breadth/continuity feed", "idle", 0.72),
    afxdp_ingest: activeIf("afxdp_ingest", afxdpActive && ingestActive, "packet/shred capture", "good", 1),
    ingest_ipc: activeIf("ingest_ipc", oldEdgeActive("decode_record", "record_ipc") || (ingestActive && (ipcActive || activity.ingestShmMoving || activity.ingestShmRecent)), "IngressRecord feed", "good", 0.9),
    ipc_tier0: activeIf("ipc_tier0", oldEdgeActive("ipc_tagmap", "tagmap_route") || (ipcActive && tier0Active), "worker consume", "warm", 0.9),
    state_tier0: activeIf("state_tier0", oldEdgeActive("state_ipc") || ((stateActive || stateFlowFresh) && tier0Active), "pool/mint/ALT sidecars", "warm", 0.78),
    oracle_tier0: activeIf("oracle_tier0", oldEdgeActive("oracle_route") || (oracleActive && tier0Active), "advisory LUTs", "warm", 0.62),
    tier0_egress: activeIf("tier0_egress", oldEdgeActive("buildsign_egress") || (tier0Active && egressActive), "signed/no-send envelope", "active", 1),
    egress_pm: activeIf("egress_pm", oldEdgeActive("egress_reconcile") || (egressActive && pmActive), "result/NACK/status", "active", 0.85),
    pm_control: activeIf("pm_control", oldEdgeActive("reconcile_control") || pmActive, "safety proof", "idle", 0.65)
  };
}

function initPipelineGraph() {
  const viewport = $("#pipeline-graph-viewport");
  const board = $("#pipeline-graph");
  const laneLayer = $("#lane-layer");
  const rippleLayer = $("#ripple-layer");
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
  if (rippleLayer) rippleLayer.innerHTML = "";
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
    previousSignalKeys: new Map(),
    livePrimed: false,
    nodeLayouts: {
      runtime: cloneGraphLayout(GRAPH_LAYOUTS.runtime),
      topology: cloneGraphLayout(GRAPH_LAYOUTS.topology)
    },
    nodeDrag: null
  };

  board.dataset.overlay = view.graphMode;
  board.dataset.layout = "runtime";

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
    const gridMinor = Math.max(28, 64 * view.scale);
    const gridMajor = gridMinor * 4;
    viewport.style.setProperty("--graph-grid-minor", `${gridMinor.toFixed(2)}px`);
    viewport.style.setProperty("--graph-grid-major", `${gridMajor.toFixed(2)}px`);
    viewport.style.setProperty("--graph-grid-x", `${(view.x % gridMinor).toFixed(2)}px`);
    viewport.style.setProperty("--graph-grid-y", `${(view.y % gridMinor).toFixed(2)}px`);
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
    const compact = rect.width < 280;
    const preferredScale = Math.max(fitScale, compact ? 0.58 : 0.78);
    const scaleFloor = compact ? 0.5 : 0.68;
    const topBias = compact ? 34 : 0;
    const contentLeft = Math.min(...NODE_DEFS.map((node) => node.x - NODE_CARD_WIDTH / 2));
    const contentRight = Math.max(...NODE_DEFS.map((node) => node.x + NODE_CARD_WIDTH / 2));
    const contentFits = (contentRight - contentLeft) * clamp(preferredScale, scaleFloor, 1) <= rect.width - 44;
    view.scale = clamp(preferredScale, scaleFloor, 1);
    const selectedNode = nodeById.get(view.selectedNodeId);
    view.x = contentFits
      ? (rect.width - GRAPH_WIDTH * view.scale) / 2 - (compact ? 0 : 16)
      : selectedNode
        ? (rect.width / 2) - selectedNode.x * view.scale
        : 24 - contentLeft * view.scale;
    view.y = (rect.height - GRAPH_HEIGHT * view.scale) / 2 - topBias;
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
    if (inspectorRole) inspectorRole.textContent = nodeInspectorSummary(nodeId, live, entry, detail);

    if (inspectorFlow) {
      const makeChip = (label, value, tone = "idle") => {
        const chip = document.createElement("span");
        chip.className = `node-flow-chip tone-${tone}`;
        const chipLabel = document.createElement("b");
        chipLabel.textContent = label;
        const chipValue = document.createElement("em");
        chipValue.textContent = value;
        chip.append(chipLabel, chipValue);
        return chip;
      };

      const evidence = nodeEvidenceItems(nodeId, live, entry)
        .slice(0, 6)
        .map((item) => makeChip(item.label, item.value, item.tone));

      const events = state.timelineEvents
        .filter((event) => Array.isArray(event.nodeIds) && event.nodeIds.includes(nodeId))
        .filter((event) => event.detail !== entry.text)
        .slice(0, 2)
        .map((event) => makeChip("Event", `${event.time} · ${event.title}: ${event.detail}`, event.tone || "idle"));

      const related = LINKS
        .filter((link) => link.from === nodeId || link.to === nodeId)
        .slice(0, 5)
        .map((link) => {
          const edge = edgeLookup.get(link.id);
          const direction = link.from === nodeId ? "Out" : "In";
          const peer = link.from === nodeId ? nodeById.get(link.to).title : nodeById.get(link.from).title;
          const value = `${peer}: ${edge && edge.active ? edge.currentPayload : "idle"}`;
          return makeChip(direction, value, edge && edge.active ? edge.tone : "off");
        });

      inspectorFlow.replaceChildren(...evidence, ...events, ...related);
    }

    if (inspectorMetrics) {
      inspectorMetrics.replaceChildren();
      inspectorMetrics.hidden = true;
    }
  }

  function selectNode(nodeId) {
    view.selectedNodeId = nodeId;
    paintSelectedNode();
    renderNodeInspector(nodeId);
    renderTimeline();
    const entry = nodeLookup.get(nodeId);
    if (entry) setText("#graph-subsummary", `${entry.node.title}: ${entry.text}`);
  }

  function applyGraphMode(mode) {
    const previousLayout = board.dataset.layout || "runtime";
    const nextLayout = mode === "topology" ? "topology" : "runtime";
    view.graphMode = mode || "runtime";
    board.dataset.overlay = view.graphMode;
    applyLayout(nextLayout);
    modeTabs.forEach((tab) => tab.classList.toggle("active", (tab.dataset.mode || "runtime") === view.graphMode));
    nodeLookup.forEach((entry) => {
      if (entry.micro) entry.micro.textContent = nodeMicroMetric(entry.node.id, state.live, entry, view.graphMode);
    });
    renderNodeInspector(view.selectedNodeId, state.live);
    if (previousLayout !== nextLayout) fitGraph();
  }

  function activeLayoutName() {
    return (board.dataset.layout || "runtime") === "topology" ? "topology" : "runtime";
  }

  function graphPointFromClient(clientX, clientY) {
    const rect = viewport.getBoundingClientRect();
    return {
      x: (clientX - rect.left - view.x) / view.scale,
      y: (clientY - rect.top - view.y) / view.scale
    };
  }

  function redrawEdges(layoutName = activeLayoutName()) {
    if (typeof edgeLookup === "undefined") return;
    edgeLookup.forEach((edge) => {
      const d = pathForLink(edge, layoutName);
      edge.line.setAttribute("d", d);
      edge.pulse.setAttribute("d", d);
    });
  }

  function setNodePosition(entry, x, y, layoutName = activeLayoutName()) {
    const node = entry.node;
    const nextX = clamp(x, NODE_CARD_WIDTH / 2 + 12, GRAPH_WIDTH - NODE_CARD_WIDTH / 2 - 12);
    const nextY = clamp(y, NODE_CARD_HEIGHT / 2 + 12, GRAPH_HEIGHT - NODE_CARD_HEIGHT / 2 - 12);
    node.x = nextX;
    node.y = nextY;
    if (!view.nodeLayouts[layoutName]) view.nodeLayouts[layoutName] = {};
    view.nodeLayouts[layoutName][node.id] = [nextX, nextY];
    entry.element.style.left = `${(nextX / GRAPH_WIDTH) * 100}%`;
    entry.element.style.top = `${(nextY / GRAPH_HEIGHT) * 100}%`;
    redrawEdges(layoutName);
  }

  function applyLayout(layoutName = "runtime") {
    const layout = view.nodeLayouts[layoutName] || GRAPH_LAYOUTS[layoutName] || GRAPH_LAYOUTS.runtime;
    board.dataset.layout = layoutName;
    NODE_DEFS.forEach((node) => {
      const point = layout[node.id] || GRAPH_LAYOUTS.runtime[node.id];
      if (!point) return;
      node.x = point[0];
      node.y = point[1];
      const entry = nodeLookup.get(node.id);
      if (!entry) return;
      entry.element.style.left = `${(node.x / GRAPH_WIDTH) * 100}%`;
      entry.element.style.top = `${(node.y / GRAPH_HEIGHT) * 100}%`;
    });
    if (typeof edgeLookup !== "undefined") {
      redrawEdges(layoutName);
    }
  }

  function startNodeDrag(entry, event) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    selectNode(entry.node.id);
    const point = graphPointFromClient(event.clientX, event.clientY);
    view.nodeDrag = {
      nodeId: entry.node.id,
      layoutName: activeLayoutName(),
      pointerId: event.pointerId,
      startGraphX: point.x,
      startGraphY: point.y,
      startNodeX: entry.node.x,
      startNodeY: entry.node.y,
      moved: false
    };
    view.mode = "manual";
    entry.element.classList.add("dragging");
    viewport.classList.add("node-dragging");
    board.dataset.draggingNode = entry.node.id;
    if (entry.element.setPointerCapture) entry.element.setPointerCapture(event.pointerId);
  }

  function moveNodeDrag(entry, event) {
    if (!view.nodeDrag || view.nodeDrag.nodeId !== entry.node.id) return;
    event.preventDefault();
    event.stopPropagation();
    const point = graphPointFromClient(event.clientX, event.clientY);
    const dx = point.x - view.nodeDrag.startGraphX;
    const dy = point.y - view.nodeDrag.startGraphY;
    if (Math.hypot(dx, dy) > 2) view.nodeDrag.moved = true;
    setNodePosition(
      entry,
      view.nodeDrag.startNodeX + dx,
      view.nodeDrag.startNodeY + dy,
      view.nodeDrag.layoutName
    );
  }

  function stopNodeDrag(entry, event) {
    if (!view.nodeDrag || view.nodeDrag.nodeId !== entry.node.id) return;
    event.preventDefault();
    event.stopPropagation();
    entry.element.classList.remove("dragging");
    viewport.classList.remove("node-dragging");
    delete board.dataset.draggingNode;
    if (entry.element.hasPointerCapture && entry.element.hasPointerCapture(event.pointerId)) {
      entry.element.releasePointerCapture(event.pointerId);
    }
    if (view.nodeDrag.moved) {
      setText("#graph-window-state", "Manual");
      setText("#graph-subsummary", `${entry.node.title}: repositioned`);
    }
    view.nodeDrag = null;
  }

  function emitActivationRipple(entry, tone = "active") {
    if (!rippleLayer || !entry) return;
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    const ripple = document.createElement("span");
    ripple.className = `graph-ripple tone-${tone || "active"}`;
    ripple.style.left = `${(entry.node.x / GRAPH_WIDTH) * 100}%`;
    ripple.style.top = `${(entry.node.y / GRAPH_HEIGHT) * 100}%`;
    rippleLayer.appendChild(ripple);
    while (rippleLayer.children.length > 8) {
      rippleLayer.firstElementChild.remove();
    }

    entry.element.classList.add("just-activated");
    window.setTimeout(() => entry.element.classList.remove("just-activated"), 820);
    window.setTimeout(() => ripple.remove(), 1450);
  }

  function emitDeactivationRipple(entry, tone = "off") {
    if (!rippleLayer || !entry) return;
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    const ripple = document.createElement("span");
    ripple.className = `graph-ripple shutdown tone-${tone || "off"}`;
    ripple.style.left = `${(entry.node.x / GRAPH_WIDTH) * 100}%`;
    ripple.style.top = `${(entry.node.y / GRAPH_HEIGHT) * 100}%`;
    rippleLayer.appendChild(ripple);
    while (rippleLayer.children.length > 8) {
      rippleLayer.firstElementChild.remove();
    }

    entry.element.classList.add("just-deactivated");
    window.setTimeout(() => entry.element.classList.remove("just-deactivated"), 740);
    window.setTimeout(() => ripple.remove(), 1250);
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

    const head = document.createElement("span");
    head.className = "node-head";

    const sigil = document.createElement("span");
    sigil.className = "node-sigil";
    sigil.setAttribute("aria-hidden", "true");
    sigil.textContent = node.symbol || "◇";

    const title = document.createElement("strong");
    title.textContent = node.title;

    const status = document.createElement("em");
    status.textContent = node.status;

    const micro = document.createElement("span");
    micro.className = "node-micro";
    micro.textContent = "reading host";

    head.append(sigil, title);
    statusLine.append(dot, badge);
    copy.append(head, statusLine, micro);
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

    element.addEventListener("click", () => selectNode(node.id));
    element.addEventListener("dblclick", () => centerOnNode(node.id));
    element.addEventListener("pointerdown", (event) => startNodeDrag(entry, event));
    element.addEventListener("pointermove", (event) => moveNodeDrag(entry, event));
    element.addEventListener("pointerup", (event) => stopNodeDrag(entry, event));
    element.addEventListener("pointercancel", (event) => stopNodeDrag(entry, event));

    nodeLookup.set(node.id, entry);
  });

  const edgeLookup = new Map();
  LINKS.forEach((link) => {
    const group = svgElement("g", {
      class: `edge tone-${link.tone}`,
      "data-edge-id": link.id
    });
    const d = pathForLink(link, view.graphMode === "topology" ? "topology" : "runtime");
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
    const wasActive = Boolean(entry.active);
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
    if (view.livePrimed && active && !wasActive) emitActivationRipple(entry, tone);
    if (view.livePrimed && !active && wasActive) emitDeactivationRipple(entry, tone);
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

  function pushTimelineEvent(time, title, detail, tone = "idle", nodeIds = []) {
    const key = `${title}:${detail}`;
    if (state.timelineEvents[0] && state.timelineEvents[0].key === key) return;
    state.timelineEvents.unshift({ key, time, title, detail, tone, nodeIds });
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
      const related = Array.isArray(event.nodeIds) && event.nodeIds.includes(view.selectedNodeId);
      item.className = `timeline-item tone-${event.tone || "idle"}${index === 0 ? " latest" : ""}${related ? " related" : ""}`;

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
    const important = ["agave", "laserstream", "afxdp", "ingest", "state", "tier0", "egress", "pm", "control"];
    important.forEach((id) => {
      const signal = signals[id] || {};
      const key = `${Boolean(signal.active)}:${signal.tone || "off"}:${signal.status || ""}`;
      if (view.previousSignalKeys.get(id) === key) return;
      view.previousSignalKeys.set(id, key);
      const node = nodeById.get(id);
      if (!node) return;
      pushTimelineEvent(time, node.title, signal.status || "status changed", signal.tone || "idle", [id]);
    });

    Object.values(edgeSignals || {}).forEach((edge) => {
      if (!edge.active) return;
      const key = `edge:${edge.id}:${edge.payload}`;
      if (view.previousSignalKeys.get(edge.id) === key) return;
      view.previousSignalKeys.set(edge.id, key);
      const link = LINKS.find((item) => item.id === edge.id);
      pushTimelineEvent(time, link ? routeLabel(link) : edge.id, edge.payload || "handoff moving", link && link.tone || "active", link ? [link.from, link.to] : []);
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
    view.livePrimed = true;
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
