"use strict";

const fs = require("fs");
const path = require("path");
const { execFile } = require("child_process");
const { promisify } = require("util");

const execFileAsync = promisify(execFile);

const ROOT = __dirname;
const DATA_DIR = path.join(ROOT, "data", "ui-review");
const CONFIG_PATH = path.join(ROOT, "ui-review.config.json");
const DEFAULT_TARGET_URL = "http://127.0.0.1:8789/?verify=reactive-delta-2";
const DEFAULT_MODEL = "gpt-5.2";
const DEFAULT_BASE_URL = "https://api.openai.com/v1/responses";
const OPENAI_API_KEY = process.env.OPENAI_API_KEY || "";
const ALLOWED_FILES = [
  "public/index.html",
  "public/styles.css",
  "public/app.js"
];

function log(message) {
  process.stdout.write(`[ui-review] ${message}\n`);
}

function readJson(filePath, fallback) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (error) {
    if (fallback !== undefined) return fallback;
    throw error;
  }
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function loadConfig() {
  const fileConfig = fs.existsSync(CONFIG_PATH) ? readJson(CONFIG_PATH, {}) : {};
  return {
    targetUrl: String(fileConfig.targetUrl || process.env.SHADOW_UI_REVIEW_TARGET_URL || DEFAULT_TARGET_URL),
    model: String(fileConfig.model || process.env.SHADOW_UI_REVIEW_MODEL || DEFAULT_MODEL),
    maxRounds: numberConfig(fileConfig.maxRounds, process.env.SHADOW_UI_REVIEW_MAX_ROUNDS, 4),
    targetScore: numberConfig(fileConfig.targetScore, process.env.SHADOW_UI_REVIEW_TARGET_SCORE, 8.5),
    minorOnlyRoundsToStop: numberConfig(fileConfig.minorOnlyRoundsToStop, process.env.SHADOW_UI_REVIEW_MINOR_ONLY_ROUNDS, 2),
    screenshotDelayMs: numberConfig(fileConfig.screenshotDelayMs, process.env.SHADOW_UI_REVIEW_SCREENSHOT_DELAY_MS, 2500),
    desktop: {
      width: numberConfig(fileConfig.desktop && fileConfig.desktop.width, process.env.SHADOW_UI_REVIEW_DESKTOP_WIDTH, 1440),
      height: numberConfig(fileConfig.desktop && fileConfig.desktop.height, process.env.SHADOW_UI_REVIEW_DESKTOP_HEIGHT, 1024)
    },
    mobile: {
      width: numberConfig(fileConfig.mobile && fileConfig.mobile.width, process.env.SHADOW_UI_REVIEW_MOBILE_WIDTH, 390),
      height: numberConfig(fileConfig.mobile && fileConfig.mobile.height, process.env.SHADOW_UI_REVIEW_MOBILE_HEIGHT, 844),
      deviceScaleFactor: numberConfig(fileConfig.mobile && fileConfig.mobile.deviceScaleFactor, process.env.SHADOW_UI_REVIEW_MOBILE_DPR, 3),
      isMobile: fileConfig.mobile && typeof fileConfig.mobile.isMobile === "boolean"
        ? fileConfig.mobile.isMobile
        : true
    },
    openAiBaseUrl: String(fileConfig.openAiBaseUrl || process.env.OPENAI_API_BASE || DEFAULT_BASE_URL),
    reviewOnly: Boolean(fileConfig.reviewOnly || process.argv.includes("--review-only")),
    captureOnly: Boolean(fileConfig.captureOnly || process.argv.includes("--capture-only"))
  };
}

function numberConfig(fileValue, envValue, fallback) {
  const candidate = fileValue !== undefined ? fileValue : envValue;
  const parsed = Number(candidate);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function utcStamp() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function absolutePath(relativePath) {
  return path.join(ROOT, relativePath);
}

function readFrontendFiles() {
  return ALLOWED_FILES.map((relativePath) => ({
    path: relativePath,
    absolutePath: absolutePath(relativePath),
    content: fs.readFileSync(absolutePath(relativePath), "utf8")
  }));
}

function findChromiumExecutable() {
  const candidates = [
    process.env.PLAYWRIGHT_CHROMIUM_PATH,
    process.env.CHROME_PATH,
    process.env.MS_EDGE_PATH,
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    path.join(process.env.LOCALAPPDATA || "", "Google", "Chrome", "Application", "chrome.exe"),
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    path.join(process.env.LOCALAPPDATA || "", "Microsoft", "Edge", "Application", "msedge.exe")
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
  }

  throw new Error(
    "No Chromium-based browser executable found. Set PLAYWRIGHT_CHROMIUM_PATH or CHROME_PATH in the environment."
  );
}

async function takeScreenshot(page, filePath, viewport, delayMs) {
  await page.setViewportSize({ width: viewport.width, height: viewport.height });
  await page.goto(withCacheBuster(page.url() || DEFAULT_TARGET_URL), { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(delayMs);
  await page.screenshot({ path: filePath, fullPage: true });
}

function withCacheBuster(urlString) {
  const url = new URL(urlString || DEFAULT_TARGET_URL);
  url.searchParams.set("ui_review_round", String(Date.now()));
  return url.toString();
}

function imageDataUrl(filePath) {
  const base64 = fs.readFileSync(filePath).toString("base64");
  return `data:image/png;base64,${base64}`;
}

async function captureRoundScreenshots(config, roundDir) {
  const { chromium } = await import("playwright-core");
  const executablePath = findChromiumExecutable();
  const browser = await chromium.launch({
    executablePath,
    headless: true
  });

  try {
    const desktopContext = await browser.newContext({
      viewport: {
        width: config.desktop.width,
        height: config.desktop.height
      }
    });
    const mobileContext = await browser.newContext({
      viewport: {
        width: config.mobile.width,
        height: config.mobile.height
      },
      deviceScaleFactor: config.mobile.deviceScaleFactor,
      isMobile: config.mobile.isMobile,
      hasTouch: config.mobile.isMobile
    });

    const desktopPage = await desktopContext.newPage();
    const mobilePage = await mobileContext.newPage();
    const desktopPath = path.join(roundDir, "desktop.png");
    const mobilePath = path.join(roundDir, "mobile.png");

    await desktopPage.goto(withCacheBuster(config.targetUrl), { waitUntil: "domcontentloaded" });
    await desktopPage.waitForTimeout(config.screenshotDelayMs);
    await desktopPage.screenshot({ path: desktopPath, fullPage: true });

    await mobilePage.goto(withCacheBuster(config.targetUrl), { waitUntil: "domcontentloaded" });
    await mobilePage.waitForTimeout(config.screenshotDelayMs);
    await mobilePage.screenshot({ path: mobilePath, fullPage: true });

    await desktopContext.close();
    await mobileContext.close();

    return { desktopPath, mobilePath, executablePath };
  } finally {
    await browser.close();
  }
}

function buildReviewPrompt(roundNumber) {
  return [
    "You are reviewing a local frontend UI from desktop and mobile screenshots.",
    "Be strict, concrete, and implementation-aware.",
    "Focus on hierarchy, spacing, overflow, clipped text, readability, contrast, density, discoverability, composition, responsiveness, and polish.",
    "Assume the implementation may only change files under frontend/public.",
    "Return JSON only with this shape:",
    "{",
    '  "score": number,',
    '  "done": boolean,',
    '  "major_issues": string[],',
    '  "medium_issues": string[],',
    '  "minor_issues": string[],',
    '  "recommended_changes": [{"priority":"high|medium|low","path_hint":"public/index.html|public/styles.css|public/app.js|unknown","change":"string"}],',
    '  "stop_reason": "string"',
    "}",
    `This is round ${roundNumber}. Mark done=true only if the UI is production-worthy enough to stop iterating.`
  ].join("\n");
}

async function callResponsesApi(config, contentItems, timeoutMs = 60000) {
  if (!OPENAI_API_KEY) {
    throw new Error("OPENAI_API_KEY is not set in the environment.");
  }

  let lastError = null;
  for (let attempt = 1; attempt <= 2; attempt += 1) {
    try {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(new Error(`OpenAI API timeout after ${timeoutMs}ms`)), timeoutMs);
      const response = await fetch(config.openAiBaseUrl, {
        method: "POST",
        headers: {
          "Authorization": `Bearer ${OPENAI_API_KEY}`,
          "Content-Type": "application/json"
        },
        signal: controller.signal,
        body: JSON.stringify({
          model: config.model,
          input: [
            {
              role: "user",
              content: contentItems
            }
          ]
        })
      });
      clearTimeout(timeout);

      const payload = await response.json();
      if (!response.ok) {
        throw new Error(`OpenAI API error ${response.status}: ${JSON.stringify(payload)}`);
      }
      return payload;
    } catch (error) {
      lastError = error;
      if (attempt < 2) {
        await new Promise((resolve) => setTimeout(resolve, 1200));
      }
    }
  }

  throw lastError;
}

function extractOutputText(payload) {
  if (typeof payload.output_text === "string" && payload.output_text.trim()) {
    return payload.output_text.trim();
  }

  const texts = [];
  for (const item of payload.output || []) {
    if (!item || !Array.isArray(item.content)) continue;
    for (const content of item.content) {
      if (!content) continue;
      if (typeof content.text === "string") texts.push(content.text);
      if (content.type === "output_text" && typeof content.text === "string") texts.push(content.text);
    }
  }

  return texts.join("\n").trim();
}

function parseJsonFromModel(text) {
  const trimmed = text.trim();
  if (!trimmed) throw new Error("Model returned empty output.");

  const fenced = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/i);
  const candidate = fenced ? fenced[1].trim() : trimmed;

  try {
    return JSON.parse(candidate);
  } catch (error) {
    const objectText = firstBalancedJsonObject(candidate);
    if (objectText) {
      return JSON.parse(objectText);
    }
    throw error;
  }
}

function firstBalancedJsonObject(text) {
  const start = text.indexOf("{");
  if (start === -1) return "";

  let depth = 0;
  let inString = false;
  let escaped = false;

  for (let index = start; index < text.length; index += 1) {
    const char = text[index];

    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "\"") {
        inString = false;
      }
      continue;
    }

    if (char === "\"") {
      inString = true;
      continue;
    }

    if (char === "{") depth += 1;
    if (char === "}") {
      depth -= 1;
      if (depth === 0) {
        return text.slice(start, index + 1);
      }
    }
  }

  return "";
}

function normalizeReview(review) {
  return {
    score: Number(review.score || 0),
    done: Boolean(review.done),
    major_issues: Array.isArray(review.major_issues) ? review.major_issues : [],
    medium_issues: Array.isArray(review.medium_issues) ? review.medium_issues : [],
    minor_issues: Array.isArray(review.minor_issues) ? review.minor_issues : [],
    recommended_changes: Array.isArray(review.recommended_changes) ? review.recommended_changes : [],
    stop_reason: typeof review.stop_reason === "string" ? review.stop_reason : ""
  };
}

function normalizeEditPlan(plan) {
  const files = Array.isArray(plan.files) ? plan.files : [];
  return {
    summary: typeof plan.summary === "string" ? plan.summary : "",
    notes: Array.isArray(plan.notes) ? plan.notes : [],
    rules: Array.isArray(plan.rules) ? plan.rules : [],
    files: files
      .filter((file) => file && ALLOWED_FILES.includes(file.path) && typeof file.content === "string")
      .map((file) => ({
        path: file.path,
        content: file.content
      }))
  };
}

function selectFilesForEditPrompt(review, files) {
  const hinted = new Set(
    (review.recommended_changes || [])
      .map((change) => change && typeof change.path_hint === "string" ? change.path_hint : "")
      .filter((pathHint) => ALLOWED_FILES.includes(pathHint))
  );

  if (!hinted.size) return files;
  return files.filter((file) => hinted.has(file.path));
}

function saveJson(filePath, value) {
  fs.writeFileSync(filePath, JSON.stringify(value, null, 2) + "\n", "utf8");
}

async function requestReview(config, screenshots, roundNumber) {
  const payload = await callResponsesApi(config, [
    { type: "input_text", text: buildReviewPrompt(roundNumber) },
    { type: "input_image", image_url: imageDataUrl(screenshots.desktopPath), detail: "high" },
    { type: "input_image", image_url: imageDataUrl(screenshots.mobilePath), detail: "high" }
  ], 70000);

  return normalizeReview(parseJsonFromModel(extractOutputText(payload)));
}

function reviewCorpus(review) {
  return [
    ...(review.major_issues || []),
    ...(review.medium_issues || []),
    ...(review.minor_issues || []),
    ...(review.recommended_changes || []).map((item) => `${item.path_hint || ""} ${item.change || ""}`),
    review.stop_reason || ""
  ].join("\n").toLowerCase();
}

function upsertBlock(source, startMarker, endMarker, blockBody) {
  const block = `${startMarker}\n${blockBody}\n${endMarker}`;
  if (source.includes(startMarker) && source.includes(endMarker)) {
    const pattern = new RegExp(`${escapeRegExp(startMarker)}[\\s\\S]*?${escapeRegExp(endMarker)}`, "m");
    return source.replace(pattern, block);
  }
  return `${source.trimEnd()}\n\n${block}\n`;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function deriveRuleIds(review) {
  const corpus = reviewCorpus(review);
  const rules = new Set();

  if (/toolbar|tab labels|economi|scrollable tablist|allow wrapping|controls feel squeezed|mode-tab|mode tabs/.test(corpus)) {
    rules.add("responsive_toolbar");
  }
  if (/main graph area|default zoom|canvas default|increase initial node scale|reduce top empty space|auto-center|primary content hard to read|large empty grid/.test(corpus)) {
    rules.add("mobile_graph_scale");
  }
  if (/long unbroken strings|overflow\/clipping|horizontal scrolling|overflow-wrap|word-break|127\.0\.0\.1|readability suffers/.test(corpus)) {
    rules.add("long_string_wrap");
  }
  if (/contrast\/readability|small meta text|timestamps|status lines|hard to scan|low-contrast/.test(corpus)) {
    rules.add("meta_contrast");
  }
  if (/heavy shadows|visual noise|muddy|cards blend together|clearer 1px border/.test(corpus)) {
    rules.add("card_tone_down");
  }
  if (/density\/spacing|hierarchy|inter-card spacing|header spacing|what to look at first|more prominent/.test(corpus)) {
    rules.add("hierarchy_spacing");
  }
  if (/event strip|footer but competes|relationship to selected nodes|timeline/.test(corpus)) {
    rules.add("timeline_deemphasis");
  }
  if (/icon buttons|off-center|square hit area|hover\/focus states/.test(corpus)) {
    rules.add("icon_button_alignment");
  }

  return Array.from(rules);
}

function buildRuleCssBlock(ruleIds) {
  const blocks = [];
  const has = (ruleId) => ruleIds.includes(ruleId);

  if (has("responsive_toolbar")) {
    blocks.push([
      "/* rule: responsive_toolbar */",
      ".ui-review-button {",
      "  white-space: nowrap;",
      "}",
      ".graph-window-controls {",
      "  flex-wrap: wrap;",
      "  row-gap: 6px;",
      "  justify-content: flex-start;",
      "}",
      ".mode-tabs {",
      "  max-width: 100%;",
      "  display: flex;",
      "  flex-wrap: wrap;",
      "  gap: 6px;",
      "  overflow-x: auto;",
      "  scrollbar-width: none;",
      "}",
      ".mode-tabs::-webkit-scrollbar {",
      "  display: none;",
      "}",
      ".mode-tab {",
      "  flex: 0 0 auto;",
      "  white-space: nowrap;",
      "}",
      "@media (max-width: 720px) {",
      "  .pipeline-status {",
      "    width: 100%;",
      "    justify-content: flex-start;",
      "  }",
      "  .graph-window-controls {",
      "    width: 100%;",
      "  }",
      "  .mode-tabs {",
      "    width: 100%;",
      "    padding: 3px;",
      "  }",
      "  .mode-tab {",
      "    min-height: 30px;",
      "    padding: 6px 8px;",
      "    font-size: 10px;",
      "  }",
      "}"
    ].join("\n"));
  }

  if (has("long_string_wrap")) {
    blocks.push([
      "/* rule: long_string_wrap */",
      ".topbar-runtime-summary,",
      ".graph-stat span,",
      ".graph-stat p,",
      ".graph-list-row span,",
      ".graph-list-row em,",
      ".node-flow-chip,",
      ".node-metric span,",
      ".node-metric strong,",
      ".node-micro,",
      ".pipeline-details strong,",
      ".pipeline-details small,",
      "#host-target,",
      "#eno1-owner,",
      "#latest-blocker {",
      "  overflow-wrap: anywhere;",
      "  word-break: break-word;",
      "}"
    ].join("\n"));
  }

  if (has("meta_contrast")) {
    blocks.push([
      "/* rule: meta_contrast */",
      ".graph-stat span,",
      ".graph-stat p,",
      ".topbar-runtime-summary,",
      ".node-micro,",
      ".timeline-item span,",
      ".timeline-item em {",
      "  color: rgba(210, 220, 226, 0.92);",
      "}",
      ".graph-stat span,",
      ".graph-stat p {",
      "  font-size: 12px;",
      "  line-height: 1.45;",
      "}",
      "@media (max-width: 720px) {",
      "  .topbar-runtime-summary,",
      "  .graph-stat span,",
      "  .graph-stat p {",
      "    font-size: 13px;",
      "  }",
      "}"
    ].join("\n"));
  }

  if (has("card_tone_down")) {
    blocks.push([
      "/* rule: card_tone_down */",
      ".graph-inspector .graph-stat {",
      "  box-shadow: none;",
      "  border-color: rgba(255,255,255,0.09);",
      "  background:",
      "    linear-gradient(180deg, rgba(255,255,255,0.025), transparent 64%),",
      "    rgba(6, 10, 12, 0.9);",
      "}",
      ".graph-inspector {",
      "  gap: 12px;",
      "}"
    ].join("\n"));
  }

  if (has("hierarchy_spacing")) {
    blocks.push([
      "/* rule: hierarchy_spacing */",
      ".graph-inspector .graph-stat {",
      "  padding: 16px 18px;",
      "}",
      ".graph-stat strong {",
      "  margin-top: 6px;",
      "  font-size: 15px;",
      "}",
      ".graph-stat small {",
      "  margin-bottom: 2px;",
      "}",
      "@media (max-width: 720px) {",
      "  .graph-inspector .graph-stat {",
      "    padding: 14px 16px;",
      "  }",
      "}"
    ].join("\n"));
  }

  if (has("timeline_deemphasis")) {
    blocks.push([
      "/* rule: timeline_deemphasis */",
      ".timeline-strip {",
      "  min-height: 32px;",
      "  height: 32px;",
      "  padding: 4px 8px;",
      "  opacity: 0.82;",
      "}",
      ".timeline-item {",
      "  gap: 4px;",
      "}",
      ".timeline-item strong {",
      "  font-size: 10px;",
      "}",
      ".timeline-item em {",
      "  font-size: 9px;",
      "}"
    ].join("\n"));
  }

  if (has("icon_button_alignment")) {
    blocks.push([
      "/* rule: icon_button_alignment */",
      ".graph-icon-button {",
      "  width: 32px;",
      "  height: 32px;",
      "  min-height: 32px;",
      "  display: inline-flex;",
      "  align-items: center;",
      "  justify-content: center;",
      "  font-size: 14px;",
      "}",
      ".graph-icon-button:focus-visible,",
      ".ui-review-button:focus-visible {",
      "  outline: 2px solid rgba(73, 191, 212, 0.68);",
      "  outline-offset: 1px;",
      "}"
    ].join("\n"));
  }

  if (has("mobile_graph_scale")) {
    blocks.push([
      "/* rule: mobile_graph_scale */",
      "@media (max-width: 720px) {",
      "  .graph-viewport {",
      "    height: 46vh;",
      "    min-height: 300px;",
      "  }",
      "  .graph-node {",
      "    width: 196px;",
      "    height: 108px;",
      "    min-height: 108px;",
      "    padding: 12px 13px;",
      "  }",
      "}",
      "@media (max-width: 480px) {",
      "  .pipeline-topbar {",
      "    align-items: flex-start;",
      "    flex-direction: column;",
      "  }",
      "  .graph-window-title strong {",
      "    white-space: normal;",
      "  }",
      "}"
    ].join("\n"));
  }

  return blocks.join("\n\n");
}

function applyDeterministicCssRules(source, ruleIds) {
  const body = buildRuleCssBlock(ruleIds);
  if (!body) return source;
  return upsertBlock(
    source,
    "/* ui-review-rules:start */",
    "/* ui-review-rules:end */",
    body
  );
}

function applyDeterministicAppRules(source, ruleIds) {
  if (!ruleIds.includes("mobile_graph_scale")) {
    return source;
  }
  const pattern = /function fitGraph\(\) \{[\s\S]*?^\s*\}/m;
  const replacement = [
    "function fitGraph() {",
    "    const rect = viewport.getBoundingClientRect();",
    "    if (!rect.width || !rect.height) return;",
    "    const fitScale = Math.min((rect.width - 44) / GRAPH_WIDTH, (rect.height - 44) / GRAPH_HEIGHT);",
    "    const mobileScaleFloor = rect.width < 720 ? 0.5 : 0.34;",
    "    const mobileTopBias = rect.width < 720 ? 36 : 0;",
    "    view.scale = clamp(fitScale, mobileScaleFloor, 1);",
    "    view.x = (rect.width - GRAPH_WIDTH * view.scale) / 2;",
    "    view.y = (rect.height - GRAPH_HEIGHT * view.scale) / 2 - mobileTopBias;",
    "    view.mode = \"fit\";",
    "    applyView();",
    "  }"
  ].join("\n");

  return source.replace(pattern, replacement);
}

function buildDeterministicEditPlan(review, files, reason = "") {
  const ruleIds = deriveRuleIds(review);
  const planned = [];
  const stylesFile = files.find((file) => file.path === "public/styles.css");
  const appFile = files.find((file) => file.path === "public/app.js");

  if (stylesFile && ruleIds.length) {
    const nextContent = applyDeterministicCssRules(stylesFile.content, ruleIds);
    if (nextContent !== stylesFile.content) {
      planned.push({ path: stylesFile.path, content: nextContent });
    }
  }

  if (appFile && ruleIds.length) {
    const nextContent = applyDeterministicAppRules(appFile.content, ruleIds);
    if (nextContent !== appFile.content) {
      planned.push({ path: appFile.path, content: nextContent });
    }
  }

  return {
    summary: "Applied deterministic frontend patch rules from reviewer findings.",
    notes: [
      `Review score: ${review.score}`,
      reason ? `Planner note: ${reason}` : "Planner note: deterministic local rule engine",
      ruleIds.length
        ? `Applied rules: ${ruleIds.join(", ")}`
        : "Applied rules: none matched the current review"
    ],
    rules: ruleIds,
    files: planned
  };
}

function writeFiles(editPlan) {
  const backups = new Map();
  for (const file of editPlan.files) {
    const destination = absolutePath(file.path);
    backups.set(destination, fs.readFileSync(destination, "utf8"));
    fs.writeFileSync(destination, file.content, "utf8");
  }
  return backups;
}

function restoreFiles(backups) {
  for (const [filePath, content] of backups.entries()) {
    fs.writeFileSync(filePath, content, "utf8");
  }
}

async function runChecks() {
  await execFileAsync(process.execPath, ["--check", path.join(ROOT, "public", "app.js")], {
    cwd: ROOT,
    windowsHide: true
  });
  await execFileAsync(process.execPath, ["--check", path.join(ROOT, "server.js")], {
    cwd: ROOT,
    windowsHide: true
  });
}

function shouldStop(config, review, minorOnlyStreak, changedFiles, roundNumber) {
  if (review.done) {
    return `reviewer_done:${review.stop_reason || "done"}`;
  }
  if (review.score >= config.targetScore && review.major_issues.length === 0 && review.medium_issues.length === 0) {
    return `target_score_reached:${review.score}`;
  }
  if (changedFiles === 0) {
    return "no_meaningful_changes";
  }
  if (minorOnlyStreak >= config.minorOnlyRoundsToStop) {
    return `minor_only_streak:${minorOnlyStreak}`;
  }
  if (roundNumber >= config.maxRounds) {
    return `max_rounds:${config.maxRounds}`;
  }
  return "";
}

function reviewIsMinorOnly(review) {
  return review.major_issues.length === 0 && review.medium_issues.length === 0;
}

async function runRound(config, roundNumber, minorOnlyStreak) {
  const roundDir = path.join(DATA_DIR, `round-${String(roundNumber).padStart(2, "0")}-${utcStamp()}`);
  ensureDir(roundDir);
  log(`round ${roundNumber}: capturing screenshots from ${config.targetUrl}`);
  const screenshots = await captureRoundScreenshots(config, roundDir);
  saveJson(path.join(roundDir, "capture.json"), screenshots);

  if (config.captureOnly) {
    return {
      roundDir,
      review: normalizeReview({ score: 0, done: false }),
      changedFiles: 0,
      minorOnlyStreak,
      stopReason: "capture_only"
    };
  }

  log(`round ${roundNumber}: requesting UI review with ${config.model}`);
  const review = await requestReview(config, screenshots, roundNumber);
  saveJson(path.join(roundDir, "review.json"), review);

  const nextMinorOnlyStreak = reviewIsMinorOnly(review) ? minorOnlyStreak + 1 : 0;
  if (config.reviewOnly) {
    return {
      roundDir,
      review,
      changedFiles: 0,
      minorOnlyStreak: nextMinorOnlyStreak,
      stopReason: shouldStop(config, review, nextMinorOnlyStreak, 0, roundNumber) || "review_only"
    };
  }

  const files = readFrontendFiles();
  log(`round ${roundNumber}: applying deterministic patch rules`);
  const editPlan = buildDeterministicEditPlan(review, files);
  saveJson(path.join(roundDir, "edit-plan.json"), editPlan);

  let changedFiles = 0;
  if (editPlan.files.length) {
    const backups = writeFiles(editPlan);
    changedFiles = editPlan.files.length;
    try {
      await runChecks();
    } catch (error) {
      restoreFiles(backups);
      throw new Error(`Generated edits failed syntax checks and were rolled back: ${error.message}`);
    }
  }

  return {
    roundDir,
    review,
    changedFiles,
    minorOnlyStreak: nextMinorOnlyStreak,
    stopReason: shouldStop(config, review, nextMinorOnlyStreak, changedFiles, roundNumber)
  };
}

async function main() {
  ensureDir(DATA_DIR);
  const config = loadConfig();
  saveJson(path.join(DATA_DIR, "last-config.json"), config);

  const summary = {
    startedAt: new Date().toISOString(),
    targetUrl: config.targetUrl,
    model: config.model,
    rounds: [],
    stopped: false,
    stopReason: ""
  };

  let minorOnlyStreak = 0;
  for (let roundNumber = 1; roundNumber <= config.maxRounds; roundNumber += 1) {
    const result = await runRound(config, roundNumber, minorOnlyStreak);
    minorOnlyStreak = result.minorOnlyStreak;
    summary.rounds.push({
      roundNumber,
      roundDir: result.roundDir,
      score: result.review.score,
      majorIssues: result.review.major_issues.length,
      mediumIssues: result.review.medium_issues.length,
      minorIssues: result.review.minor_issues.length,
      changedFiles: result.changedFiles,
      stopReason: result.stopReason
    });
    saveJson(path.join(DATA_DIR, "last-run.json"), summary);

    if (result.stopReason) {
      summary.stopped = true;
      summary.stopReason = result.stopReason;
      break;
    }
  }

  summary.finishedAt = new Date().toISOString();
  saveJson(path.join(DATA_DIR, "last-run.json"), summary);
  log(`finished: ${summary.stopReason || "completed max rounds without early stop"}`);
}

main().catch((error) => {
  ensureDir(DATA_DIR);
  saveJson(path.join(DATA_DIR, `failed-${utcStamp()}.json`), {
    failedAt: new Date().toISOString(),
    message: error.message,
    stack: error.stack
  });
  process.stderr.write(`${error.stack}\n`);
  process.exitCode = 1;
});
