const http = require("http");
const fs = require("fs");
const path = require("path");
const { execFile, spawn } = require("child_process");

const ROOT = __dirname;
const PUBLIC_DIR = path.join(ROOT, "public");
const DATA_DIR = path.join(ROOT, "data");
const AUDIT_LOG = path.join(DATA_DIR, "action-audit.log");
const WALLET_DOC = path.resolve(ROOT, "..", "SH", "wallet.md");
const THREE_BUILD_DIR = path.join(ROOT, "node_modules", "three", "build");
const VENDOR_FILES = new Set([
  "three.module.js",
  "three.module.min.js",
  "three.core.js",
  "three.core.min.js"
]);

const PORT = Number(process.env.SHADOW_CONSOLE_PORT || 8787);
const SSH_TARGET = process.env.SHADOW_CONSOLE_HOST || "ubuntu@109.94.99.15";
const SSH_KEY =
  process.env.SHADOW_CONSOLE_KEY ||
  path.resolve(ROOT, "..", "g1-host-pack", "ssh_file.txt");
const UI_REVIEW_SCRIPT = path.join(ROOT, "ui-review-loop.js");
const UI_REVIEW_LAST_RUN = path.join(DATA_DIR, "ui-review", "last-run.json");
const UI_REVIEW_LAST_CONFIG = path.join(DATA_DIR, "ui-review", "last-config.json");
const UI_REVIEW_LOG = path.join(DATA_DIR, "ui-review", "loop-server.log");
const SSH_TIMEOUT_MS = Number(process.env.SHADOW_CONSOLE_SSH_TIMEOUT_MS || 9000);
const LIVE_SAMPLE_INTERVAL_MS = Number(process.env.SHADOW_CONSOLE_LIVE_INTERVAL_MS || 1500);
const LIVE_SSH_TIMEOUT_MS = Number(process.env.SHADOW_CONSOLE_LIVE_SSH_TIMEOUT_MS || 9000);
const MAX_ACTIVITY_GAP_SEC = Number(process.env.SHADOW_CONSOLE_MAX_ACTIVITY_GAP_SEC || 6);

const LIVE_STATUS_COMMAND =
  "printf 'sample_epoch_ms='; date +%s%3N; " +
  "printf 'agave_unit='; (systemctl is-active sh1-agave-no-geyser-2.service 2>/dev/null || systemctl is-active sh1-agave-no-geyser.service 2>/dev/null || true); " +
  "printf '\\nrpc_8899_listening='; ss -ltn 2>/dev/null | grep -q ':8899 ' && echo 1 || echo 0; " +
  "printf 'local_health='; { curl -sS -m 1.5 -X POST -H 'Content-Type: application/json' --data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getHealth\"}' http://127.0.0.1:8899 2>/dev/null || printf 'RPC_UNAVAILABLE'; } | tr -d '\\n'; " +
  "printf '\\nlocal_slot='; { curl -sS -m 1.5 -X POST -H 'Content-Type: application/json' --data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getSlot\"}' http://127.0.0.1:8899 2>/dev/null || printf 'RPC_UNAVAILABLE'; } | tr -d '\\n'; " +
  "printf '\\nudp_listener_count='; ss -lun 2>/dev/null | grep -E ':80(0[0-9]|1[0-9]|2[0-5])\\b' | wc -l; " +
  "printf 'eno1_line='; ip -details link show dev eno1 2>/dev/null | sed -n '1p' | tr -d '\\n'; " +
  "printf '\\neno1_rx_bytes='; cat /sys/class/net/eno1/statistics/rx_bytes 2>/dev/null || echo 0; " +
  "printf 'eno1_rx_packets='; cat /sys/class/net/eno1/statistics/rx_packets 2>/dev/null || echo 0; " +
  "printf 'eno1_tx_bytes='; cat /sys/class/net/eno1/statistics/tx_bytes 2>/dev/null || echo 0; " +
  "printf 'eno1_tx_packets='; cat /sys/class/net/eno1/statistics/tx_packets 2>/dev/null || echo 0; " +
  "printf '\\nxdp_program='; xdp_prog=$(ip -details link show dev eno1 2>/dev/null | grep -Eo 'prog/xdp[^ ]*|xdp[^ ]*' | head -1); printf '%s\\n' \"${xdp_prog:-none}\"; " +
  "test -e /sys/fs/bpf/shadow_xsks_map && echo 'xsk_map=present' || echo 'xsk_map=absent'; " +
  "printf 'shadow_processes='; pgrep -af 'shadow-(state|ingest|ingest-replay|tier0|oracle|egress-tpu|pm|control)|grpc-ingest|afxdp|shred' 2>/dev/null | grep -Ev 'pgrep -af|pgrep -f|shadow_proc_ticks|shadow_processes=|__SHADOW_SAMPLE|[[:space:]]find[[:space:]]|-iname' | tr '\\n' '|' || true; printf '\\n'; " +
  "printf 'shadow_proc_ticks='; for p in $(pgrep -f 'shadow-(state|ingest|ingest-replay|tier0|oracle|egress-tpu|pm|control)|grpc-ingest|afxdp|shred' 2>/dev/null); do if [ -r /proc/$p/cmdline ] && [ -r /proc/$p/stat ]; then line=$(tr '\\0' ' ' < /proc/$p/cmdline 2>/dev/null); printf '%s\\n' \"$line\" | grep -Eq 'pgrep -f|shadow_proc_ticks|shadow_processes=|__SHADOW_SAMPLE|[[:space:]]find[[:space:]]|-iname' && continue; name=$(printf '%s\\n' \"$line\" | grep -Eo 'shadow-ingest-replay|shadow-ingest|shadow-tier0|shadow-state|shadow-oracle|shadow-egress-tpu|shadow-pm|shadow-control|grpc-ingest|afxdp|shred' | head -1); if [ -n \"$name\" ]; then set -- $(cat /proc/$p/stat 2>/dev/null); ticks=$(( ${14:-0} + ${15:-0} )); printf '%s:%s,' \"$name\" \"$ticks\"; fi; fi; done; printf '\\n'; " +
  "printf 'shadow_shm_count='; ls /dev/shm 2>/dev/null | grep -Eci 'shadow|tpu|egress|ingress|ingest|feed|raw|umem|afxdp' || true; " +
  "printf 'shadow_shm_latest_epoch='; find /dev/shm -maxdepth 1 -type f \\( -iname '*shadow*' -o -iname '*tpu*' -o -iname '*egress*' -o -iname '*ingress*' -o -iname '*ingest*' -o -iname '*feed*' -o -iname '*raw*' -o -iname '*umem*' -o -iname '*afxdp*' \\) -printf '%T@\\n' 2>/dev/null | awk 'BEGIN{m=0}{if($1>m)m=$1} END{print m}' || echo 0; " +
  "printf 'shadow_shm_total_bytes='; find /dev/shm -maxdepth 1 -type f \\( -iname '*shadow*' -o -iname '*tpu*' -o -iname '*egress*' -o -iname '*ingress*' -o -iname '*ingest*' -o -iname '*feed*' -o -iname '*raw*' -o -iname '*umem*' -o -iname '*afxdp*' \\) -printf '%s\\n' 2>/dev/null | awk '{s+=$1} END{print s+0}' || echo 0; " +
  "printf 'egress_shm_count='; find /dev/shm -maxdepth 1 -type f \\( -iname '*egress*' -o -iname '*tpu*' -o -iname '*jito*' -o -iname '*harmonic*' -o -iname '*nack*' -o -iname '*status*' \\) -printf '.\\n' 2>/dev/null | wc -l; " +
  "printf 'egress_shm_latest_epoch='; find /dev/shm -maxdepth 1 -type f \\( -iname '*egress*' -o -iname '*tpu*' -o -iname '*jito*' -o -iname '*harmonic*' -o -iname '*nack*' -o -iname '*status*' \\) -printf '%T@\\n' 2>/dev/null | awk 'BEGIN{m=0}{if($1>m)m=$1} END{print m}' || echo 0; " +
  "printf 'egress_shm_total_bytes='; find /dev/shm -maxdepth 1 -type f \\( -iname '*egress*' -o -iname '*tpu*' -o -iname '*jito*' -o -iname '*harmonic*' -o -iname '*nack*' -o -iname '*status*' \\) -printf '%s\\n' 2>/dev/null | awk '{s+=$1} END{print s+0}' || echo 0; " +
  "stat -c 'egress_health_epoch=%Y' /dev/shm/shadow_egress_health 2>/dev/null || echo 'egress_health_epoch=0'; " +
  "printf 'state_publish_latest_epoch='; find /dev/shm -maxdepth 1 -type f \\( -name 'pool_state' -o -name 'pool_index_map' -o -name 'pool_meta' -o -name 'pool_meta_index_map' -o -name 'mint_state' -o -name 'mint_index_map' -o -name 'alt_state' -o -name 'alt_index_map' -o -name 'shadow_health' -o -name 'shadow_blockhash' -o -name 'shadow_leader_schedule' -o -name 'clmm_quote_core' -o -name 'clmm_exact_quote' \\) -printf '%T@\\n' 2>/dev/null | awk 'BEGIN{m=0}{if($1>m)m=$1} END{print m}' || echo 0; " +
  "printf 'state_publish_total_bytes='; find /dev/shm -maxdepth 1 -type f \\( -name 'pool_state' -o -name 'pool_index_map' -o -name 'pool_meta' -o -name 'pool_meta_index_map' -o -name 'mint_state' -o -name 'mint_index_map' -o -name 'alt_state' -o -name 'alt_index_map' -o -name 'shadow_health' -o -name 'shadow_blockhash' -o -name 'shadow_leader_schedule' -o -name 'clmm_quote_core' -o -name 'clmm_exact_quote' \\) -printf '%s\\n' 2>/dev/null | awk '{s+=$1} END{print s+0}' || echo 0; " +
  "printf 'state_core_latest_epoch='; find /dev/shm -maxdepth 1 -type f \\( -name 'pool_state' -o -name 'pool_index_map' -o -name 'pool_meta' -o -name 'pool_meta_index_map' -o -name 'mint_state' -o -name 'mint_index_map' -o -name 'alt_state' -o -name 'alt_index_map' -o -name 'clmm_quote_core' -o -name 'clmm_exact_quote' \\) -printf '%T@\\n' 2>/dev/null | awk 'BEGIN{m=0}{if($1>m)m=$1} END{print m}' || echo 0; " +
  "stat -c 'state_blockhash_epoch=%Y' /dev/shm/shadow_blockhash 2>/dev/null || echo 'state_blockhash_epoch=0'; " +
  "stat -c 'state_health_epoch=%Y' /dev/shm/shadow_health 2>/dev/null || echo 'state_health_epoch=0'; " +
  "printf 'ingest_shm_latest_epoch='; find /dev/shm -maxdepth 1 -type f \\( -iname '*feed*' -o -iname '*ingress*' -o -iname '*ingest*' -o -iname '*raw*' -o -iname '*umem*' -o -iname '*afxdp*' \\) -printf '%T@\\n' 2>/dev/null | awk 'BEGIN{m=0}{if($1>m)m=$1} END{print m}' || echo 0; " +
  "printf 'ingest_shm_total_bytes='; find /dev/shm -maxdepth 1 -type f \\( -iname '*feed*' -o -iname '*ingress*' -o -iname '*ingest*' -o -iname '*raw*' -o -iname '*umem*' -o -iname '*afxdp*' \\) -printf '%s\\n' 2>/dev/null | awk '{s+=$1} END{print s+0}' || echo 0; " +
  "printf 'ingest_artifact_latest_epoch='; find /home/ubuntu/shadow-host-audit/dpdk-runs -maxdepth 5 -type f \\( -iname '*ingest*' -o -iname '*afxdp*' -o -iname '*grpc*' -o -iname '*shred*' -o -iname '*feed*' -o -iname '*source*' \\) -printf '%T@\\n' 2>/dev/null | awk 'BEGIN{m=0}{if($1>m)m=$1} END{print m}' || echo 0; " +
  "printf 'ingest_artifact_total_bytes='; find /home/ubuntu/shadow-host-audit/dpdk-runs -maxdepth 5 -type f \\( -iname '*ingest*' -o -iname '*afxdp*' -o -iname '*grpc*' -o -iname '*shred*' -o -iname '*feed*' -o -iname '*source*' \\) -printf '%s\\n' 2>/dev/null | awk '{s+=$1} END{print s+0}' || echo 0; " +
  "stat -c 'inventory_size=%s inventory_mtime=%Y' /etc/shadow-engine/signer-token-inventory.json 2>/dev/null || echo 'inventory_size=0 inventory_mtime=0'; " +
  "grpc_grade=$(awk -F: '/grpc_source_grade:/ {gsub(/^[ \\t\"]+|[ \\t\"]+$/, \"\", $2); print $2; exit}' /etc/shadow-engine/config.guerrila.yaml 2>/dev/null); printf 'grpc_source_grade=%s\\n' \"${grpc_grade:-unknown}\"; " +
  "grep -q 'block_private_bundle_routes: true' /etc/shadow-engine/config.guerrila.yaml 2>/dev/null && echo 'route_private_block=1' || echo 'route_private_block=0'; " +
  "grep -q 'quic_workers: 0' /etc/shadow-engine/config.guerrila.yaml 2>/dev/null && echo 'quic_workers_zero=1' || echo 'quic_workers_zero=0'";
const LIVE_LOOP_COMMAND =
  "while true; do echo __SHADOW_SAMPLE_BEGIN__; " +
  LIVE_STATUS_COMMAND +
  "; echo __SHADOW_SAMPLE_END__; sleep 1; done";
const LIVE_BEGIN = "__SHADOW_SAMPLE_BEGIN__";
const LIVE_END = "__SHADOW_SAMPLE_END__";

fs.mkdirSync(DATA_DIR, { recursive: true });

const liveClients = new Set();
let liveChild = null;
let liveBuffer = "";
let liveSequence = 0;
let latestLiveSample = null;
let uiReviewChild = null;
let uiReviewRunId = 0;
let uiReviewStatus = {
  running: false,
  startedAt: null,
  finishedAt: null,
  exitCode: null,
  error: "",
  pid: null
};

const diagnostics = {
  agave_preflight: {
    label: "Local Agave Preflight",
    risk: "read_only",
    description: "Checks expected Agave units, local RPC health, and UDP listener count.",
    command:
      "printf 'active_unit='; (systemctl is-active sh1-agave-no-geyser-2.service 2>/dev/null || systemctl is-active sh1-agave-no-geyser.service 2>/dev/null || true); printf '\\nlocal_health='; curl -sS -m 2 -X POST -H 'Content-Type: application/json' --data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getHealth\"}' http://127.0.0.1:8899 2>/dev/null | tr -d '\\n' || true; printf '\\nlocal_slot='; curl -sS -m 2 -X POST -H 'Content-Type: application/json' --data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getSlot\"}' http://127.0.0.1:8899 2>/dev/null | tr -d '\\n' || true; printf '\\nudp_listener_count='; ss -lun 2>/dev/null | grep -E ':80(0[0-9]|1[0-9]|2[0-5])\\b' | wc -l"
  },
  eno1_xdp: {
    label: "eno1 XDP State",
    risk: "read_only",
    description: "Shows link state, XDP attachment details, and XSK map presence.",
    command:
      "ip -details link show dev eno1 2>/dev/null | sed -n '1,14p'; test -e /sys/fs/bpf/shadow_xsks_map && echo 'xsk_map=present' || echo 'xsk_map=absent'"
  },
  shadow_processes: {
    label: "Shadow Processes",
    risk: "read_only",
    description: "Lists Shadow ingest, state, oracle, tier0, PM, and TPU egress processes.",
    command:
      "pgrep -af 'shadow-(state|ingest|ingest-replay|tier0|oracle|egress-tpu|pm|control)|grpc-ingest|afxdp|shred' 2>/dev/null | grep -Ev 'pgrep -af' || true"
  },
  route_policy: {
    label: "Route Policy Check",
    risk: "read_only",
    description: "Greps non-secret route safety fields from active host config.",
    command:
      "grep -nE 'block_private_bundle_routes|default_jito_searcher|enable_harmonic_lane|pre_sign_egress_admission_gate|quic_workers|tpu_quic_public' /etc/shadow-engine/config.guerrila.yaml 2>/dev/null || true"
  },
  tpu_ring: {
    label: "TPU Ring Readiness",
    risk: "read_only",
    description: "Checks for TPU/shared-memory segments and TPU egress process.",
    command:
      "ls -lh /dev/shm 2>/dev/null | grep -Ei 'tpu|egress|shadow' | head -40; pgrep -af 'shadow-egress-tpu' 2>/dev/null || true"
  },
  signer_inventory: {
    label: "Signer Inventory File",
    risk: "read_only",
    description: "Reports inventory file size and timestamp without modifying it.",
    command:
      "stat -c 'inventory=%n size=%s mtime=%y' /etc/shadow-engine/signer-token-inventory.json 2>/dev/null || echo 'inventory=missing'"
  },
  latest_artifacts: {
    label: "Latest Run Artifacts",
    risk: "read_only",
    description: "Indexes recent run summaries and JSON reports.",
    command:
      "find /home/ubuntu/shadow-host-audit/dpdk-runs -maxdepth 2 -type f \\( -name 'summary.txt' -o -name '*summary*.txt' -o -name '*report*.json' -o -name '*live_readiness*.json' \\) -printf '%T@|%p\\n' 2>/dev/null | sort -nr | head -12"
  },
  zero_candidate_ladder: {
    label: "Zero-Candidate Ladder",
    risk: "read_only",
    description: "Samples L0-L5 prerequisites before interpreting a zero-candidate run.",
    command:
      "printf 'L0_host='; hostname; printf 'L1_agave_unit='; (systemctl is-active sh1-agave-no-geyser-2.service 2>/dev/null || systemctl is-active sh1-agave-no-geyser.service 2>/dev/null || true); printf '\\nL2_large_udp_sample='; timeout 5 tcpdump -ni eno1 'udp portrange 8000-8025 and greater 1000' -c 1 2>&1 | tail -1 || true; printf '\\nL3_xdp='; ip -details link show dev eno1 2>/dev/null | grep -Eo 'prog/xdp[^ ]*|xdp[^ ]*' | head -1 || echo none; printf 'L4_shm='; ls /dev/shm 2>/dev/null | grep -Ec 'shadow|tpu|egress' || true; printf 'L5_worker='; pgrep -af shadow-tier0 2>/dev/null | head -1 || true"
  }
};

const runCards = [
  {
    id: "run56",
    title: "Run56 Canonical AF_XDP Sequence",
    risk: "medium",
    liveSend: false,
    status: "planned",
    purpose: "Canonical current-candidate AF_XDP sequence after Agave and eno1 preflight.",
    expectedArtifacts: ["run56_summary.txt", "surface_scorecard.json", "shadow-tier0.log"],
    enabled: false
  },
  {
    id: "run61",
    title: "Run61 Continuous No-Send Simulation",
    risk: "medium",
    liveSend: false,
    status: "planned",
    purpose: "Continuous would-fire simulation with owner-mode and route split.",
    expectedArtifacts: ["continuous would-fire metrics", "owner mode split", "deployable_by_us"],
    enabled: false
  },
  {
    id: "run62",
    title: "Run62 Conversion Autopsy",
    risk: "low",
    liveSend: false,
    status: "planned",
    purpose: "Explain observed opportunity to would-fire conversion blockers.",
    expectedArtifacts: ["conversion autopsy", "blocker ranking", "route/inventory/template split"],
    enabled: false
  },
  {
    id: "run64d-plan",
    title: "Run64D Acquisition Plan/Simulate",
    risk: "medium",
    liveSend: false,
    status: "plan_only",
    purpose: "Metadata held-input acquisition planner with exit-liquidity gate visible.",
    expectedArtifacts: ["plan report", "simulate report", "exit quote status"],
    enabled: false
  },
  {
    id: "run65",
    title: "Run65 Shard Simulation",
    risk: "low",
    liveSend: false,
    status: "planned",
    purpose: "Compare one shared Shadow brain with 1/2/4/8/16 execution shards.",
    expectedArtifacts: ["shard matrix", "captured EV", "missed EV", "conflict rates"],
    enabled: false
  }
];

function audit(event) {
  const line = JSON.stringify({ ts: new Date().toISOString(), ...event }) + "\n";
  fs.appendFile(AUDIT_LOG, line, () => {});
}

function readJsonFile(filePath, fallback = null) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (error) {
    return fallback;
  }
}

function readRequestBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      if (!chunks.length) {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      } catch (error) {
        reject(error);
      }
    });
    req.on("error", reject);
  });
}

function uiReviewSnapshot() {
  const lastRun = readJsonFile(UI_REVIEW_LAST_RUN, null);
  const lastConfig = readJsonFile(UI_REVIEW_LAST_CONFIG, null);
  return {
    running: uiReviewStatus.running,
    startedAt: uiReviewStatus.startedAt,
    finishedAt: uiReviewStatus.finishedAt,
    exitCode: uiReviewStatus.exitCode,
    error: uiReviewStatus.error,
    pid: uiReviewStatus.pid,
    keyConfigured: Boolean(process.env.OPENAI_API_KEY),
    targetUrl: lastConfig && lastConfig.targetUrl ? lastConfig.targetUrl : null,
    model: lastConfig && lastConfig.model ? lastConfig.model : null,
    lastRun
  };
}

function startUiReviewLoop(mode = "default") {
  if (uiReviewChild) {
    return { ok: false, reason: "already_running", status: uiReviewSnapshot() };
  }

  if (!process.env.OPENAI_API_KEY) {
    return { ok: false, reason: "missing_api_key", status: uiReviewSnapshot() };
  }

  fs.mkdirSync(path.dirname(UI_REVIEW_LOG), { recursive: true });
  const args = [UI_REVIEW_SCRIPT];
  if (mode === "review_only") args.push("--review-only");
  if (mode === "capture_only") args.push("--capture-only");

  const runId = ++uiReviewRunId;
  const child = spawn(process.execPath, args, {
    cwd: ROOT,
    env: process.env,
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"]
  });

  uiReviewChild = child;
  uiReviewStatus = {
    running: true,
    startedAt: new Date().toISOString(),
    finishedAt: null,
    exitCode: null,
    error: "",
    pid: child.pid
  };

  const header = `\n[${uiReviewStatus.startedAt}] started run ${runId} mode=${mode}\n`;
  fs.appendFileSync(UI_REVIEW_LOG, header);

  child.stdout.on("data", (chunk) => {
    fs.appendFile(UI_REVIEW_LOG, chunk, () => {});
  });
  child.stderr.on("data", (chunk) => {
    fs.appendFile(UI_REVIEW_LOG, chunk, () => {});
  });
  child.on("error", (error) => {
    uiReviewStatus.error = error.message;
  });
  child.on("close", (code) => {
    if (uiReviewRunId !== runId) return;
    uiReviewChild = null;
    uiReviewStatus.running = false;
    uiReviewStatus.finishedAt = new Date().toISOString();
    uiReviewStatus.exitCode = typeof code === "number" ? code : null;
  });

  audit({ type: "ui_review_started", mode, pid: child.pid });
  return { ok: true, started: true, status: uiReviewSnapshot() };
}

function runSsh(command, timeoutMs = SSH_TIMEOUT_MS) {
  return new Promise((resolve) => {
    const started = Date.now();
    const args = [
      "-i",
      SSH_KEY,
      "-o",
      "BatchMode=yes",
      "-o",
      "ConnectTimeout=6",
      "-o",
      "StrictHostKeyChecking=no",
      SSH_TARGET,
      command
    ];

    execFile("ssh", args, { timeout: timeoutMs, windowsHide: true }, (error, stdout, stderr) => {
      resolve({
        ok: !error,
        exitCode: error && typeof error.code === "number" ? error.code : 0,
        durationMs: Date.now() - started,
        stdout: (stdout || "").trim(),
        stderr: (stderr || "").trim(),
        error: error ? error.message : ""
      });
    });
  });
}

function parseAgave(output) {
  const udpMatch = output.match(/udp_listener_count=(\d+)/);
  const unitMatch = output.match(/(?:active_unit|agave_unit)=([^\n]+)/);
  const values = parseKeyValueLines(output);
  const health = parseHealthDetails(values.local_health || output);
  return {
    activeUnit: unitMatch ? unitMatch[1].trim() : "unknown",
    localHealth: health.localHealth,
    slotLag: health.slotLag,
    healthMessage: health.healthMessage,
    localSlot: parseRpcResultNumber(values.local_slot),
    rpcPortListening: values.rpc_8899_listening === "1",
    udpListenerCount: udpMatch ? Number(udpMatch[1]) : null
  };
}

function parseEno1(output) {
  const lowerUp = /LOWER_UP|state\s+UP|<[^>]*\bUP\b/.test(output);
  const xdpAttached = /prog\/xdp| xdp /.test(output);
  const xskMap = /xsk_map=present/.test(output);
  let owner = "Unknown";
  if (lowerUp && !xdpAttached && !xskMap) owner = "Agave";
  if (xdpAttached || xskMap) owner = "Shadow";
  return { lowerUp, xdpAttached, xskMap, owner };
}

function parseProcesses(output) {
  const names = ["shadow-state", "shadow-oracle", "shadow-tier0", "shadow-egress-tpu"];
  return names.map((name) => ({
    name,
    running: new RegExp(name).test(output)
  }));
}

function parseArtifacts(output) {
  if (!output) return [];
  return output
    .split(/\r?\n/)
    .map((line) => {
      const [mtime, filePath] = line.split("|");
      if (!filePath) return null;
      const parts = filePath.split("/");
      const runDir = parts.length > 1 ? parts[parts.length - 2] : "unknown";
      return {
        mtime: Number(mtime || 0),
        path: filePath,
        run: runDir,
        file: parts[parts.length - 1]
      };
    })
    .filter(Boolean)
    .slice(0, 10);
}

function firstMatch(text, patterns) {
  for (const pattern of patterns) {
    const match = text.match(pattern);
    if (match) return match[1].trim();
  }
  return null;
}

function parseKeyValueLines(output) {
  const values = {};
  (output || "").split(/\r?\n/).forEach((line) => {
    const index = line.indexOf("=");
    if (index <= 0) return;
    values[line.slice(0, index).trim()] = line.slice(index + 1).trim();
  });
  return values;
}

function parseRpcResultNumber(text) {
  const raw = String(text || "");
  if (!raw.trim() || /RPC_UNAVAILABLE|unavailable|failed to connect|couldn'?t connect|connection refused/i.test(raw)) {
    return null;
  }
  const match = raw.match(/"result"\s*:\s*(-?\d+(?:\.\d+)?)/);
  if (match) return Number(match[1]);
  const direct = Number(raw);
  return Number.isFinite(direct) ? direct : null;
}

function parseHealthDetails(text) {
  const raw = String(text || "");
  if (!raw.trim() || /RPC_UNAVAILABLE|failed to connect|couldn'?t connect|connection refused|no response/i.test(raw)) {
    return {
      localHealth: "unavailable",
      slotLag: null,
      healthMessage: "RPC 8899 unavailable"
    };
  }
  const healthOk = /"result"\s*:\s*"ok"/i.test(raw) || raw.trim().toLowerCase() === "ok";
  const lagMatch =
    raw.match(/"numSlotsBehind"\s*:\s*(\d+)/i) ||
    raw.match(/\bnumSlotsBehind=(\d+)/i);
  const messageMatch = raw.match(/"message"\s*:\s*"([^"]+)"/i);
  return {
    localHealth: healthOk ? "ok" : "lagging",
    slotLag: healthOk ? 0 : lagMatch ? Number(lagMatch[1]) : null,
    healthMessage: healthOk ? "getHealth ok" : messageMatch ? messageMatch[1] : "getHealth not ok"
  };
}

function readWalletCapitals() {
  let text = "";
  try {
    text = fs.readFileSync(WALLET_DOC, "utf8");
  } catch (error) {
    return {
      source: WALLET_DOC,
      ok: false,
      error: error.message,
      items: []
    };
  }

  const items = [];
  const add = (label, value, note) => {
    if (!value || items.some((item) => item.label === label)) return;
    items.push({ label, value, note });
  };

  add(
    "Native signer",
    firstMatch(text, [
      /Latest checked native worker-signer balance:\s*`([^`]+)`/i,
      /Latest checked worker-signer balance[\s\S]*?```text\s*([^`]+?)\s*```/i
    ]),
    "fee payer"
  );
  add(
    "WSOL funded",
    firstMatch(text, [
      /WSOL ATA parsed token balance after sync:\s*`([^`]+)`/i,
      /Current funded\/synced amount:\s*```text\s*([^`]+?)\s*```/i
    ]),
    "CLMM input"
  );
  add(
    "USDC funded",
    firstMatch(text, [
      /USDC ATA parsed token balance after `[^`]+` funding:\s*`([^`]+)`/i
    ]),
    "broad-surface input"
  );

  return {
    source: WALLET_DOC,
    ok: true,
    items
  };
}

function numericMetric(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function rateDelta(current, previous, field, elapsedSec) {
  if (!previous || !elapsedSec) return 0;
  const currentValue = numericMetric(current[field]);
  const previousValue = numericMetric(previous[field]);
  return Math.max(0, (currentValue - previousValue) / elapsedSec);
}

const PROCESS_NAMES = [
  "shadow-state",
  "shadow-ingest",
  "shadow-ingest-replay",
  "grpc-ingest",
  "afxdp",
  "shred",
  "shadow-oracle",
  "shadow-tier0",
  "shadow-egress-tpu",
  "shadow-pm",
  "shadow-control"
];

const INGEST_PROCESS_NAMES = [
  "shadow-ingest",
  "shadow-ingest-replay",
  "grpc-ingest",
  "afxdp",
  "shred"
];

const PROCESS_PATTERNS = [
  ["shadow-ingest-replay", /shadow-ingest-replay/],
  ["shadow-ingest", /shadow-ingest(?!-replay)/],
  ["shadow-tier0", /shadow-tier0/],
  ["shadow-state", /shadow-state/],
  ["shadow-oracle", /shadow-oracle/],
  ["shadow-egress-tpu", /shadow-egress-tpu/],
  ["shadow-pm", /shadow-pm/],
  ["shadow-control", /shadow-control/],
  ["grpc-ingest", /grpc-ingest/],
  ["afxdp", /\bafxdp\b/i],
  ["shred", /\bshred\b/i]
];

function classifyProcessNames(processText) {
  const names = new Set();
  (processText || "").split("|").forEach((line) => {
    const text = line.trim();
    if (!text) return;
    if (
      /pgrep\s+-af|pgrep\s+-f|shadow_proc_ticks|shadow_processes=|__SHADOW_SAMPLE|LIVE_STATUS_COMMAND/.test(text) ||
      /(^|\s)find\s|\s-iname\s/.test(text)
    ) {
      return;
    }
    PROCESS_PATTERNS.forEach(([name, pattern]) => {
      if (pattern.test(text)) names.add(name);
    });
  });
  return names;
}

function parseProcessCpu(text) {
  const totals = {};
  (text || "").split("|").forEach((line) => {
    const nameMatch = line.match(/shadow-(state|tier0|oracle|egress-tpu|pm|control)/);
    const cpuMatch = line.match(/([0-9]+(?:\.[0-9]+)?)\s*$/);
    if (!nameMatch || !cpuMatch) return;
    const name = `shadow-${nameMatch[1]}`;
    totals[name] = (totals[name] || 0) + Number(cpuMatch[1]);
  });
  return totals;
}

function parseProcessTicks(text) {
  const totals = {};
  (text || "").split(",").forEach((item) => {
    const [name, ticks] = item.split(":");
    if (!name || !ticks) return;
    totals[name.trim()] = (totals[name.trim()] || 0) + numericMetric(ticks);
  });
  return totals;
}

function buildActivity(sample, previousSample) {
  const elapsedSec = previousSample
    ? Math.max(0.25, (sample.sampleEpochMs - previousSample.sampleEpochMs) / 1000)
    : 0;
  const freshDelta = Boolean(previousSample && elapsedSec > 0 && elapsedSec <= MAX_ACTIVITY_GAP_SEC);
  const network = sample.network || {};
  const previousNetwork = previousSample && previousSample.network ? previousSample.network : null;
  const rxPacketsPerSec = freshDelta ? rateDelta(network, previousNetwork, "rxPackets", elapsedSec) : 0;
  const rxBytesPerSec = freshDelta ? rateDelta(network, previousNetwork, "rxBytes", elapsedSec) : 0;
  const txBytesPerSec = freshDelta ? rateDelta(network, previousNetwork, "txBytes", elapsedSec) : 0;
  const previousIpc = previousSample && previousSample.ipc ? previousSample.ipc : null;
  const previousState = previousSample && previousSample.state ? previousSample.state : null;
  const previousIngest = previousSample && previousSample.ingest ? previousSample.ingest : null;
  const previousEgress = previousSample && previousSample.egress ? previousSample.egress : null;
  const statePublishAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.state && sample.state.publishLatestEpoch);
  const stateCoreAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.state && sample.state.coreLatestEpoch);
  const stateBlockhashAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.state && sample.state.blockhashEpoch);
  const stateHealthAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.state && sample.state.healthEpoch);
  const statePublishRecent = statePublishAgeSec >= 0 && statePublishAgeSec <= 20;
  const stateBlockhashRecent = stateBlockhashAgeSec >= 0 && stateBlockhashAgeSec <= 20;
  const ingestArtifactAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.ingest && sample.ingest.artifactLatestEpoch);
  const ingestArtifactRecent = ingestArtifactAgeSec >= 0 && ingestArtifactAgeSec <= 6;
  const ingestShmAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.ingest && sample.ingest.feedRingLatestEpoch);
  const ingestShmRecent = ingestShmAgeSec >= 0 && ingestShmAgeSec <= 20;
  const egressShmAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.egress && sample.egress.latestEpoch);
  const egressHealthAgeSec =
    numericMetric(sample.sampleEpochMs) / 1000 -
    numericMetric(sample.egress && sample.egress.healthEpoch);
  const egressShmRecent = egressShmAgeSec >= 0 && egressShmAgeSec <= 20;
  const egressHealthRecent = egressHealthAgeSec >= 0 && egressHealthAgeSec <= 20;
  const shmLatestMoved = Boolean(
    freshDelta &&
    previousIpc &&
    numericMetric(sample.ipc.latestEpoch) > numericMetric(previousIpc.latestEpoch)
  );
  const shmSizeMoved = Boolean(
    freshDelta &&
    previousIpc &&
    numericMetric(sample.ipc.totalBytes) !== numericMetric(previousIpc.totalBytes)
  );
  const statePublishMoved = Boolean(
    freshDelta &&
    previousState &&
    (
      numericMetric(sample.state.publishLatestEpoch) > numericMetric(previousState.publishLatestEpoch) ||
      numericMetric(sample.state.publishTotalBytes) !== numericMetric(previousState.publishTotalBytes) ||
      numericMetric(sample.state.blockhashEpoch) > numericMetric(previousState.blockhashEpoch) ||
      numericMetric(sample.state.healthEpoch) > numericMetric(previousState.healthEpoch)
    )
  );
  const ingestArtifactMoved = Boolean(
    freshDelta &&
    previousIngest &&
    (
      numericMetric(sample.ingest.artifactLatestEpoch) > numericMetric(previousIngest.artifactLatestEpoch) ||
      numericMetric(sample.ingest.artifactTotalBytes) !== numericMetric(previousIngest.artifactTotalBytes)
    )
  );
  const ingestShmMoved = Boolean(
    freshDelta &&
    previousIngest &&
    (
      numericMetric(sample.ingest.feedRingLatestEpoch) > numericMetric(previousIngest.feedRingLatestEpoch) ||
      numericMetric(sample.ingest.feedRingTotalBytes) !== numericMetric(previousIngest.feedRingTotalBytes)
    )
  );
  const egressShmMoved = Boolean(
    freshDelta &&
    previousEgress &&
    (
      numericMetric(sample.egress.latestEpoch) > numericMetric(previousEgress.latestEpoch) ||
      numericMetric(sample.egress.totalBytes) !== numericMetric(previousEgress.totalBytes) ||
      numericMetric(sample.egress.healthEpoch) > numericMetric(previousEgress.healthEpoch)
    )
  );
  const ingestProcessStarted = Boolean(
    freshDelta &&
    previousIngest &&
    !previousIngest.processActive &&
    sample.ingest &&
    sample.ingest.processActive
  );
  const processCpu = parseProcessCpu(sample.rawProcessCpu);
  const currentTicks = sample.processTicks || {};
  const previousTicks = previousSample && previousSample.processTicks ? previousSample.processTicks : {};
  const processTickRates = Object.fromEntries(
    Object.keys(currentTicks).map((name) => [
      name,
      freshDelta
        ? Math.max(0, (numericMetric(currentTicks[name]) - numericMetric(previousTicks[name])) / elapsedSec)
        : 0
    ])
  );
  const busy = (name, threshold = 0.5) =>
    numericMetric(processTickRates[name]) >= threshold ||
    numericMetric(processCpu[name]) >= 0.1;

  return {
    primed: Boolean(previousSample),
    elapsedSec,
    rxPacketsPerSec,
    rxBytesPerSec,
    txBytesPerSec,
    wireMoving: rxPacketsPerSec >= 50 || rxBytesPerSec >= 120000 || txBytesPerSec >= 80000,
    shmMoving: shmLatestMoved || shmSizeMoved,
    ingestArtifactMoving: ingestArtifactMoved,
    ingestArtifactRecent,
    ingestShmMoving: ingestShmMoved,
    ingestShmRecent,
    ingestShmAgeSec,
    ingestProcessStarted,
    processCpu,
    processTickRates,
    freshDelta,
    staleDeltaSec: previousSample && !freshDelta ? elapsedSec : 0,
    stateProcessBusy: busy("shadow-state"),
    stateBusy: statePublishMoved || statePublishRecent || stateBlockhashRecent,
    statePublishMoving: statePublishMoved,
    statePublishRecent,
    statePublishAgeSec,
    stateCoreAgeSec,
    stateBlockhashAgeSec,
    stateBlockhashRecent,
    stateHealthAgeSec,
    laserstreamBusy: busy("grpc-ingest", 0.1),
    ingestBusy: INGEST_PROCESS_NAMES.some((name) => busy(name, 0.1)),
    tier0Busy: busy("shadow-tier0"),
    oracleBusy: busy("shadow-oracle"),
    egressBusy: busy("shadow-egress-tpu") || egressShmMoved || egressHealthRecent,
    egressShmMoving: egressShmMoved,
    egressShmRecent,
    egressShmAgeSec,
    egressHealthAgeSec,
    egressHealthRecent,
    pmBusy: busy("shadow-pm"),
    controlBusy: busy("shadow-control")
  };
}

function buildNodeSignals(sample) {
  const agaveActive = sample.agave.localHealth === "ok";
  const udpActive = Number(sample.agave.udpListenerCount || 0) > 0;
  const activity = sample.activity || {};
  const wireMoving = Boolean(activity.wireMoving);
  const shmMoving = Boolean(activity.shmMoving);
  const ingestProcessActive = Boolean(sample.ingest && sample.ingest.processActive);
  const captureReady = Boolean(sample.eno1.xdpAttached || sample.eno1.xskMap);
  const captureMoving = Boolean(captureReady && wireMoving);
  const ingestFeedReady = Boolean(
    numericMetric(sample.ingest && sample.ingest.feedRingTotalBytes) > 0 ||
    numericMetric(sample.ingest && sample.ingest.feedRingLatestEpoch) > 0
  );
  const ingestReady =
    captureReady ||
    ingestProcessActive ||
    activity.ingestArtifactMoving ||
    activity.ingestArtifactRecent ||
    activity.ingestShmMoving ||
    activity.ingestShmRecent;
  const ingestMoving = Boolean(
    activity.ingestBusy ||
    activity.ingestArtifactMoving ||
    activity.ingestArtifactRecent ||
    activity.ingestShmMoving ||
    activity.ingestShmRecent ||
    activity.ingestProcessStarted ||
    (captureMoving && ingestFeedReady) ||
    (ingestProcessActive && (
      wireMoving ||
      activity.ingestShmMoving ||
      activity.ingestShmRecent ||
      activity.ingestArtifactMoving ||
      activity.ingestArtifactRecent
    ))
  );
  const shadowStateActive = sample.processes["shadow-state"];
  const oracleActive = sample.processes["shadow-oracle"];
  const tier0Active = sample.processes["shadow-tier0"];
  const pmActive = sample.processes["shadow-pm"];
  const controlReady = sample.processes["shadow-control"] || sample.host.sshOk;
  const tpuActive = sample.processes["shadow-egress-tpu"];
  const inventoryActive = sample.inventory.filePresent;
  const routePolicyActive = sample.routePolicy.blockPrivateBundleRoutesSeen;
  const laserstreamConfigured = Boolean(sample.source && sample.source.laserstreamConfigured);
  const laserstreamActive = Boolean(sample.processes["grpc-ingest"] || sample.source && sample.source.grpcIngestActive);
  const laserstreamMoving = Boolean(laserstreamActive && activity.laserstreamBusy);
  const ipcReady = Number(sample.ipc && sample.ipc.shmCount || 0) > 0 || shadowStateActive || ingestReady || tpuActive;
  const stateMoving = shadowStateActive && activity.stateBusy;
  const stateFresh = Boolean(activity.statePublishMoving || activity.statePublishRecent || activity.stateBlockhashRecent);
  const tier0Moving = tier0Active && activity.tier0Busy;
  const oracleMoving = oracleActive && activity.oracleBusy;
  const pmMoving = pmActive && activity.pmBusy;
  const egressRingReady = Boolean(sample.egress && (sample.egress.ringCount > 0 || sample.egress.latestEpoch > 0));
  const egressMoving = Boolean(
    tpuActive && (activity.egressBusy || activity.egressShmMoving || activity.egressShmRecent || activity.egressHealthRecent)
  );
  const rxRate = Math.round(activity.rxPacketsPerSec || 0);
  const tickRates = activity.processTickRates || {};
  const ingestTicks = Math.round(
    (tickRates["shadow-ingest"] || 0) +
    (tickRates["shadow-ingest-replay"] || 0) +
    (tickRates["grpc-ingest"] || 0) +
    (tickRates.afxdp || 0) +
    (tickRates.shred || 0)
  );
  const stateTicks = Math.round(tickRates["shadow-state"] || 0);
  const tier0Ticks = Math.round(tickRates["shadow-tier0"] || 0);
  const oracleTicks = Math.round(tickRates["shadow-oracle"] || 0);
  const pmTicks = Math.round(tickRates["shadow-pm"] || 0);
  const egressTicks = Math.round(tickRates["shadow-egress-tpu"] || 0);

  return {
    wire: {
      active: captureMoving,
      tone: captureReady ? "good" : agaveActive && udpActive ? "good" : "off",
      status: wireMoving
        ? captureMoving ? `${rxRate} pkt/s AF_XDP` : `${rxRate} pkt/s Agave`
        : udpActive ? `${sample.agave.udpListenerCount} UDP idle` : "no wire"
    },
    ingest: {
      active: ingestMoving,
      tone: ingestReady ? "active" : "off",
      status: ingestReady
        ? ingestMoving
          ? ingestTicks
            ? `ingest ${ingestTicks} t/s`
            : activity.ingestShmMoving || activity.ingestShmRecent
              ? "feed ring live"
              : captureMoving && ingestFeedReady
                ? "capture to feed ring"
                : activity.ingestProcessStarted ? "ingest started" : "ingest artifact delta"
          : "ingest ready"
        : ingestFeedReady ? "feed ring stale" : "ingest absent"
    },
    laserstream: {
      active: laserstreamMoving,
      tone: laserstreamActive ? "active" : laserstreamConfigured ? "idle" : "off",
      status: laserstreamActive
        ? laserstreamMoving ? "LaserStream moving" : "LaserStream connected"
        : laserstreamConfigured ? "LaserStream configured" : "external source off"
    },
    decode: {
      active: ingestMoving,
      tone: ingestReady ? "active" : "off",
      status: ingestReady ? ingestMoving ? "decode active" : "decode idle" : ingestFeedReady ? "feed ring stale" : "waiting ingest"
    },
    record: {
      active: ingestMoving && (shmMoving || activity.ingestArtifactMoving || activity.ingestArtifactRecent),
      tone: ingestReady ? "active" : "off",
      status: ingestMoving && (shmMoving || activity.ingestArtifactMoving || activity.ingestArtifactRecent)
        ? "IngressRecord moving"
        : "no record delta"
    },
    ipc: {
      active: shmMoving,
      tone: ipcReady ? "good" : "off",
      status: ipcReady ? shmMoving ? "shm changing" : `${sample.ipc.shmCount} shm idle` : "no shm"
    },
    state: {
      active: stateMoving,
      tone: stateMoving ? "good" : shadowStateActive ? "warm" : stateFresh ? "idle" : sample.state && sample.state.publishLatestEpoch ? "warm" : "off",
      status: shadowStateActive
        ? stateMoving
          ? stateFresh ? "state publish fresh" : `state ${stateTicks} t/s`
          : sample.state && sample.state.publishLatestEpoch ? "state stale" : "state process up"
        : stateFresh ? "state files fresh" : sample.state && sample.state.publishLatestEpoch ? "state files stale" : "stopped"
    },
    tagmap: {
      active: stateMoving && tier0Moving,
      tone: shadowStateActive && ipcReady ? "warm" : "off",
      status: shadowStateActive && ipcReady ? tier0Moving ? "tag read" : "pool tags ready" : "no tags"
    },
    route: {
      active: tier0Moving,
      tone: tier0Active ? "active" : routePolicyActive ? "idle" : "off",
      status: tier0Active ? tier0Moving ? `tier0 ${tier0Ticks} t/s` : "tier0 idle" : routePolicyActive ? "policy staged" : "route idle"
    },
    oracle: {
      active: oracleMoving && tier0Moving,
      tone: oracleActive ? "warm" : "off",
      status: oracleActive ? oracleMoving ? `oracle ${oracleTicks} t/s` : "oracle ready" : "oracle off"
    },
    quote: {
      active: tier0Moving,
      tone: tier0Active ? "active" : "off",
      status: tier0Active ? tier0Moving ? "quote path" : "quote idle" : "idle"
    },
    policy: {
      active: tier0Moving && routePolicyActive,
      tone: tier0Active ? "active" : routePolicyActive ? "idle" : "off",
      status: routePolicyActive ? "caps loaded" : "policy unknown"
    },
    buildsign: {
      active: tier0Moving && inventoryActive,
      tone: inventoryActive ? "good" : "off",
      status: inventoryActive ? tier0Moving ? "template/sign" : "signer ready" : "inventory missing"
    },
    pm: {
      active: pmMoving,
      tone: pmActive || tier0Active || tpuActive ? "active" : "off",
      status: pmActive || tier0Active || tpuActive ? pmMoving ? `pm ${pmTicks} t/s` : "pm idle" : "idle"
    },
    egress: {
      active: egressMoving,
      tone: tpuActive ? "active" : "off",
      status: tpuActive
        ? egressMoving
          ? egressTicks ? `egress ${egressTicks} t/s` : "egress ring live"
          : "TPU proxy ready"
        : egressRingReady ? "egress rings stale" : "egress stopped"
    },
    reconcile: {
      active: pmMoving || egressMoving,
      tone: pmActive || tpuActive ? "active" : "off",
      status: pmActive || tpuActive ? pmMoving || egressMoving ? "truth loop" : "reconcile idle" : "idle"
    },
    control: {
      active: Boolean(activity.controlBusy),
      tone: controlReady ? "idle" : "off",
      status: sample.latestBlocker || "diagnostics"
    },
    inventory: {
      active: tier0Moving && inventoryActive,
      tone: inventoryActive ? "good" : "off",
      status: inventoryActive ? "signer capital" : "missing"
    },
    private: {
      active: false,
      tone: "blocked",
      status: "not executable"
    }
  };
}

function buildEdgeSignals(sample) {
  const signals = sample.nodeSignals;
  const activity = sample.activity || {};
  const wireMoving = Boolean(activity.wireMoving);
  const shmMoving = Boolean(activity.shmMoving);
  const captureMoving = Boolean((sample.eno1.xdpAttached || sample.eno1.xskMap) && wireMoving);
  const laserstreamMoving = Boolean(sample.processes["grpc-ingest"] && activity.laserstreamBusy);
  const ingestMoving = Boolean(
    activity.ingestBusy ||
    activity.ingestArtifactMoving ||
    activity.ingestArtifactRecent ||
    activity.ingestShmMoving ||
    activity.ingestShmRecent ||
    activity.ingestProcessStarted ||
    (sample.ingest && sample.ingest.processActive)
  );
  const shadowStateMoving = sample.processes["shadow-state"] && activity.stateBusy;
  const tier0Moving = sample.processes["shadow-tier0"] && activity.tier0Busy;
  const oracleMoving = sample.processes["shadow-oracle"] && activity.oracleBusy;
  const tpuMoving = Boolean(
    sample.processes["shadow-egress-tpu"] &&
    (activity.egressBusy || activity.egressShmMoving || activity.egressShmRecent || activity.egressHealthRecent)
  );
  const pmMoving = sample.processes["shadow-pm"] && activity.pmBusy;
  const inventoryActive = sample.inventory.filePresent;
  const routePolicyActive = sample.routePolicy.blockPrivateBundleRoutesSeen;
  const intensityFromUdp = clampNumber((activity.rxPacketsPerSec || 0) / 250, 0.35, 1.4);
  const activeIf = (id, active, payload, intensity = 0.7) => ({
    id,
    active: Boolean(active),
    payload,
    intensity: active ? intensity : 0,
    status: active ? "moving" : "stopped"
  });

  return {
    wire_ingest: activeIf("wire_ingest", captureMoving, "packet/shred capture", intensityFromUdp),
    laserstream_ingest: activeIf("laserstream_ingest", laserstreamMoving && ingestMoving, "LaserStream source feed", 0.72),
    ingest_decode: activeIf("ingest_decode", ingestMoving, "tx decode", 0.95),
    decode_record: activeIf("decode_record", ingestMoving && (shmMoving || activity.ingestArtifactMoving || activity.ingestArtifactRecent), "IngressRecord", 0.9),
    record_ipc: activeIf("record_ipc", signals.record.active && shmMoving, "feed ring write", 0.85),
    state_ipc: activeIf("state_ipc", shadowStateMoving && shmMoving, "pool/mint/ALT state", 0.75),
    ipc_tagmap: activeIf("ipc_tagmap", shmMoving && tier0Moving, "worker tag read", 0.9),
    tagmap_route: activeIf("tagmap_route", signals.tagmap.active && tier0Moving, "pool tag match", 0.9),
    oracle_route: activeIf("oracle_route", oracleMoving && tier0Moving, "advisory LUT", 0.65),
    route_quote: activeIf("route_quote", tier0Moving, "route candidate", 0.95),
    quote_policy: activeIf("quote_policy", tier0Moving && routePolicyActive, "quote + caps", 0.95),
    policy_buildsign: activeIf("policy_buildsign", tier0Moving && routePolicyActive && inventoryActive, "approved template", 1),
    inventory_buildsign: activeIf("inventory_buildsign", tier0Moving && inventoryActive, "signer inventory", 0.72),
    buildsign_pm: activeIf("buildsign_pm", tier0Moving && inventoryActive, "reserve/no-send class", 0.9),
    buildsign_egress: activeIf("buildsign_egress", tier0Moving && tpuMoving, "egress envelope", 1),
    egress_reconcile: activeIf("egress_reconcile", tpuMoving, "status/NACK", 0.9),
    reconcile_control: activeIf("reconcile_control", pmMoving || tpuMoving, "terminal proof", 0.65),
    control_wire: activeIf("control_wire", false, sample.latestBlocker || "diagnostic proof", 0),
    private_block: activeIf("private_block", false, "private lane blocked", 0)
  };
}

function clampNumber(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function parseLiveSample(result, previousSample = null) {
  const values = parseKeyValueLines(result.stdout);
  const udpListenerCount = Number(values.udp_listener_count || 0);
  const xdpProgram = values.xdp_program || "none";
  const lowerUp = /LOWER_UP|state\s+UP|<[^>]*\bUP\b/.test(values.eno1_line || "");
  const xdpAttached = xdpProgram !== "none" && /xdp/i.test(xdpProgram);
  const xskMap = values.xsk_map === "present";
  const processText = values.shadow_processes || "";
  const processNameSet = classifyProcessNames(processText);
  const processTicks = parseProcessTicks(values.shadow_proc_ticks || "");
  const processes = Object.fromEntries(
    PROCESS_NAMES.map((name) => [name, processNameSet.has(name)])
  );
  const health = parseHealthDetails(values.local_health || result.stdout);
  const activeUnit = values.agave_unit || "unknown";
  const agave = {
    activeUnit,
    localHealth: health.localHealth,
    slotLag: health.slotLag,
    healthMessage: health.healthMessage,
    localSlot: parseRpcResultNumber(values.local_slot),
    rpcPortListening: values.rpc_8899_listening === "1",
    udpListenerCount: Number.isFinite(udpListenerCount) ? udpListenerCount : 0
  };
  const network = {
    rxBytes: numericMetric(values.eno1_rx_bytes),
    rxPackets: numericMetric(values.eno1_rx_packets),
    txBytes: numericMetric(values.eno1_tx_bytes),
    txPackets: numericMetric(values.eno1_tx_packets)
  };
  const eno1 = {
    lowerUp,
    xdpAttached,
    xskMap,
    owner: xdpAttached || xskMap ? "Shadow" : lowerUp ? "Agave" : "Unknown"
  };
  const routePolicy = {
    blockPrivateBundleRoutesSeen: values.route_private_block === "1",
    quicWorkersZeroSeen: values.quic_workers_zero === "1"
  };
  const ipc = {
    shmCount: Number(values.shadow_shm_count || 0),
    latestEpoch: numericMetric(values.shadow_shm_latest_epoch),
    totalBytes: numericMetric(values.shadow_shm_total_bytes)
  };
  const statePublish = {
    processActive: Boolean(processes["shadow-state"] || numericMetric(processTicks["shadow-state"]) > 0),
    publishLatestEpoch: numericMetric(values.state_publish_latest_epoch),
    publishTotalBytes: numericMetric(values.state_publish_total_bytes),
    coreLatestEpoch: numericMetric(values.state_core_latest_epoch),
    blockhashEpoch: numericMetric(values.state_blockhash_epoch),
    healthEpoch: numericMetric(values.state_health_epoch)
  };
  const ingest = {
    processActive: INGEST_PROCESS_NAMES.some((name) => processes[name] || numericMetric(processTicks[name]) > 0),
    artifactLatestEpoch: numericMetric(values.ingest_artifact_latest_epoch),
    artifactTotalBytes: numericMetric(values.ingest_artifact_total_bytes),
    feedRingLatestEpoch: numericMetric(values.ingest_shm_latest_epoch),
    feedRingTotalBytes: numericMetric(values.ingest_shm_total_bytes)
  };
  const egress = {
    processActive: Boolean(processes["shadow-egress-tpu"] || numericMetric(processTicks["shadow-egress-tpu"]) > 0),
    ringCount: Number(values.egress_shm_count || 0),
    latestEpoch: numericMetric(values.egress_shm_latest_epoch),
    totalBytes: numericMetric(values.egress_shm_total_bytes),
    healthEpoch: numericMetric(values.egress_health_epoch)
  };
  const source = {
    grpcSourceGrade: values.grpc_source_grade || "unknown",
    laserstreamConfigured: /laserstream/i.test(values.grpc_source_grade || ""),
    grpcIngestActive: Boolean(processes["grpc-ingest"] || numericMetric(processTicks["grpc-ingest"]) > 0)
  };
  const inventoryMatch = result.stdout.match(/inventory_size=(\d+)\s+inventory_mtime=(\d+)/);
  const inventorySize = inventoryMatch ? Number(inventoryMatch[1]) : Number(values.inventory_size || 0);
  const inventoryMtime = inventoryMatch ? Number(inventoryMatch[2]) : Number(values.inventory_mtime || 0);
  const inventory = {
    filePresent: inventorySize > 0,
    size: inventorySize,
    mtime: inventoryMtime
  };
  const sshOk = result.ok;
  const blocker =
    !sshOk ? "host_unreachable" :
    agave.localHealth !== "ok" ? "agave_preflight_not_green" :
    eno1.owner === "Shadow" ? "eno1_shadow_owned_or_xdp_attached" :
    "ready_for_read_only_diagnostics";
  const sampleEpochMs = Number(values.sample_epoch_ms || Date.now());
  const durationMs = Number.isFinite(result.durationMs)
    ? result.durationMs
    : Math.max(0, Date.now() - sampleEpochMs);
  const sample = {
    generatedAt: new Date().toISOString(),
    sampleEpochMs,
    durationMs,
    host: {
      label: ".15",
      target: SSH_TARGET,
      sshOk
    },
    agave,
    network,
    eno1,
    processes,
    processRows: PROCESS_NAMES.map((name) => ({ name, running: processes[name] })),
    routePolicy,
    ipc,
    source,
    state: statePublish,
    ingest,
    egress,
    inventory,
    latestBlocker: blocker,
    rawProcessCpu: values.shadow_proc_cpu || "",
    processTicks,
    rawError: result.stderr || result.error || ""
  };

  sample.activity = buildActivity(sample, previousSample);
  sample.nodeSignals = buildNodeSignals(sample);
  sample.edgeSignals = buildEdgeSignals(sample);
  return sample;
}

async function buildLiveSample() {
  const result = await runSsh(LIVE_STATUS_COMMAND, LIVE_SSH_TIMEOUT_MS);
  const sample = parseLiveSample(result, latestLiveSample);
  sample.sequence = liveSequence;
  liveSequence += 1;
  latestLiveSample = sample;
  return sample;
}

function writeSse(res, event, payload) {
  res.write(`event: ${event}\n`);
  res.write(`data: ${JSON.stringify(payload)}\n\n`);
}

function broadcastLive(event, payload) {
  for (const res of liveClients) {
    writeSse(res, event, payload);
  }
}

function stopLiveHostStream() {
  if (!liveChild) return;
  const child = liveChild;
  liveChild = null;
  liveBuffer = "";
  child.kill();
}

function startLiveHostStream() {
  if (liveChild) return;
  const args = [
    "-i",
    SSH_KEY,
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=6",
    "-o",
    "StrictHostKeyChecking=no",
    SSH_TARGET,
    LIVE_LOOP_COMMAND
  ];
  liveBuffer = "";
  liveChild = spawn("ssh", args, { windowsHide: true });

  liveChild.stdout.on("data", (chunk) => {
    liveBuffer += chunk.toString("utf8");
    while (true) {
      const begin = liveBuffer.indexOf(LIVE_BEGIN);
      const end = liveBuffer.indexOf(LIVE_END, begin + LIVE_BEGIN.length);
      if (begin === -1 || end === -1) break;
      const block = liveBuffer.slice(begin + LIVE_BEGIN.length, end).trim();
      liveBuffer = liveBuffer.slice(end + LIVE_END.length);
      const sample = parseLiveSample({
        ok: true,
        stdout: block,
        stderr: "",
        error: ""
      }, latestLiveSample);
      sample.sequence = liveSequence;
      liveSequence += 1;
      latestLiveSample = sample;
      broadcastLive("snapshot", sample);
    }
  });

  liveChild.stderr.on("data", (chunk) => {
    const message = chunk.toString("utf8").trim();
    if (!message) return;
    broadcastLive("notice", {
      generatedAt: new Date().toISOString(),
      message
    });
  });

  liveChild.on("close", (code) => {
    liveChild = null;
    liveBuffer = "";
    const sample = {
      generatedAt: new Date().toISOString(),
      sequence: liveSequence,
      host: { label: ".15", target: SSH_TARGET, sshOk: false },
      latestBlocker: "host_stream_closed",
      rawError: `ssh stream closed (${code})`,
      nodeSignals: {}
    };
    liveSequence += 1;
    latestLiveSample = sample;
    broadcastLive("snapshot", sample);
    if (liveClients.size) {
      setTimeout(startLiveHostStream, 1800);
    }
  });
}

function streamLive(req, res) {
  res.writeHead(200, {
    "Content-Type": "text/event-stream; charset=utf-8",
    "Cache-Control": "no-store",
    "Connection": "keep-alive",
    "X-Accel-Buffering": "no"
  });
  res.write(": connected\n\n");
  liveClients.add(res);
  if (latestLiveSample) writeSse(res, "snapshot", latestLiveSample);
  startLiveHostStream();

  const keepAlive = setInterval(() => {
    res.write(": heartbeat\n\n");
  }, 15000);
  req.on("close", () => {
    clearInterval(keepAlive);
    liveClients.delete(res);
    if (!liveClients.size) stopLiveHostStream();
  });
}

async function runDiagnostic(id) {
  const diagnostic = diagnostics[id];
  if (!diagnostic) {
    return { ok: false, error: `Unknown diagnostic: ${id}` };
  }
  audit({ type: "diagnostic", id, label: diagnostic.label, risk: diagnostic.risk });
  const result = await runSsh(diagnostic.command);
  return {
    id,
    label: diagnostic.label,
    risk: diagnostic.risk,
    description: diagnostic.description,
    ...result
  };
}

async function buildOverview() {
  const [agave, eno1, processes, route, inventory, artifacts] = await Promise.all([
    runDiagnostic("agave_preflight"),
    runDiagnostic("eno1_xdp"),
    runDiagnostic("shadow_processes"),
    runDiagnostic("route_policy"),
    runDiagnostic("signer_inventory"),
    runDiagnostic("latest_artifacts")
  ]);

  const agaveState = parseAgave(agave.stdout);
  const eno1State = parseEno1(eno1.stdout);
  const processState = parseProcesses(processes.stdout);
  const artifactRows = parseArtifacts(artifacts.stdout);
  const sshOk = [agave, eno1, processes].some((item) => item.ok);
  const blocker =
    !sshOk ? "host_unreachable" :
    agaveState.localHealth !== "ok" ? "agave_preflight_not_green" :
    eno1State.owner === "Shadow" ? "eno1_shadow_owned_or_xdp_attached" :
    "ready_for_read_only_diagnostics";

  return {
    generatedAt: new Date().toISOString(),
    host: {
      label: ".15",
      target: SSH_TARGET,
      sshOk,
      keyPath: SSH_KEY
    },
    agave: agaveState,
    eno1: eno1State,
    processes: processState,
    routePolicy: {
      blockPrivateBundleRoutesSeen: /block_private_bundle_routes.*true/.test(route.stdout),
      quicWorkersZeroSeen: /quic_workers.*0/.test(route.stdout),
      raw: route.stdout
    },
    inventory: {
      filePresent: /inventory=/.test(inventory.stdout) && !/missing/.test(inventory.stdout),
      raw: inventory.stdout
    },
    latestArtifacts: artifactRows,
    walletCapitals: readWalletCapitals(),
    latestBlocker: blocker,
    commandCenter: {
      mode: "observe_control_no_send",
      liveSendControls: "disabled",
      arbitraryShell: "disabled",
      checklistPhase: "phase_1_phase_2_prototype"
    }
  };
}

function json(res, status, body) {
  const payload = JSON.stringify(body, null, 2);
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store"
  });
  res.end(payload);
}

function serveFile(res, pathname) {
  if (pathname.startsWith("/vendor/")) {
    const vendorFile = path.basename(pathname);
    if (!VENDOR_FILES.has(vendorFile)) {
      res.writeHead(403);
      res.end("Forbidden");
      return;
    }
    fs.readFile(path.join(THREE_BUILD_DIR, vendorFile), (err, data) => {
      if (err) {
        res.writeHead(404);
        res.end("Vendor module not found");
        return;
      }
      res.writeHead(200, {
        "Content-Type": "application/javascript; charset=utf-8",
        "Cache-Control": "public, max-age=31536000, immutable"
      });
      res.end(data);
    });
    return;
  }

  const safePath = pathname === "/" ? "/index.html" : pathname;
  const filePath = path.normalize(path.join(PUBLIC_DIR, safePath));
  if (!filePath.startsWith(PUBLIC_DIR)) {
    res.writeHead(403);
    res.end("Forbidden");
    return;
  }
  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(404);
      res.end("Not found");
      return;
    }
    const ext = path.extname(filePath).toLowerCase();
    const type = {
      ".html": "text/html; charset=utf-8",
      ".css": "text/css; charset=utf-8",
      ".js": "application/javascript; charset=utf-8",
      ".json": "application/json; charset=utf-8",
      ".svg": "image/svg+xml"
    }[ext] || "application/octet-stream";
    res.writeHead(200, { "Content-Type": type, "Cache-Control": "no-store" });
    res.end(data);
  });
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host || "localhost"}`);
  try {
    if (req.method === "GET" && url.pathname === "/api/config") {
      return json(res, 200, {
        host: SSH_TARGET,
        port: PORT,
        backend: "node-stdlib",
        mode: "localhost_only",
        arbitraryShell: false,
        liveSend: false,
        uiReview: {
          enabled: true,
          keyConfigured: Boolean(process.env.OPENAI_API_KEY)
        }
      });
    }

    if (req.method === "GET" && url.pathname === "/api/actions") {
      return json(res, 200, {
        diagnostics: Object.entries(diagnostics).map(([id, item]) => ({
          id,
          label: item.label,
          risk: item.risk,
          description: item.description
        })),
        runs: runCards
      });
    }

    if (req.method === "GET" && url.pathname === "/api/overview") {
      return json(res, 200, await buildOverview());
    }

    if (req.method === "GET" && url.pathname === "/api/wallet-capitals") {
      return json(res, 200, readWalletCapitals());
    }

    if (req.method === "GET" && url.pathname === "/api/live-snapshot") {
      return json(res, 200, await buildLiveSample());
    }

    if (req.method === "GET" && url.pathname === "/api/live") {
      return streamLive(req, res);
    }

    if (req.method === "GET" && url.pathname === "/api/runs") {
      const result = await runDiagnostic("latest_artifacts");
      return json(res, 200, {
        ok: result.ok,
        artifacts: parseArtifacts(result.stdout),
        raw: result.stdout,
        stderr: result.stderr
      });
    }

    if (req.method === "GET" && url.pathname === "/api/ui-review/status") {
      return json(res, 200, uiReviewSnapshot());
    }

    if (req.method === "POST" && url.pathname === "/api/ui-review/run") {
      const body = await readRequestBody(req).catch(() => ({}));
      const mode = body && typeof body.mode === "string" ? body.mode : "default";
      const result = startUiReviewLoop(mode);
      return json(res, result.ok ? 200 : 409, result);
    }

    const diagnosticMatch = url.pathname.match(/^\/api\/diagnostics\/([a-z0-9_-]+)$/);
    if (req.method === "POST" && diagnosticMatch) {
      const id = diagnosticMatch[1];
      return json(res, 200, await runDiagnostic(id));
    }

    if (req.method === "POST" && url.pathname.startsWith("/api/run/")) {
      audit({ type: "blocked_run_attempt", path: url.pathname });
      return json(res, 403, {
        ok: false,
        reason: "Run launching is intentionally disabled in this prototype. Phase 1/2 are read-only diagnostics only."
      });
    }

    return serveFile(res, url.pathname);
  } catch (error) {
    return json(res, 500, { ok: false, error: error.message });
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`Shadow Console prototype listening on http://127.0.0.1:${PORT}`);
  console.log(`Read-only target: ${SSH_TARGET}`);
});
