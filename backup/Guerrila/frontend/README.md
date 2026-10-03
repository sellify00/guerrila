# Shadow Console Prototype

This is a non-invasive UI wrapper prototype for Shadow. It implements the first slice from `shadow_ui_wrapper_spec.md`: a localhost-only dashboard, read-only artifact/status probes, and allowlisted diagnostics.

## Run

```bash
cd frontend
npm start
```

Open:

```text
http://127.0.0.1:8787
```

## Automated UI Review Loop

There is now a frontend-local automation entrypoint:

```bash
cd frontend
npm run ui-review:loop
```

What it does:

- Opens the local Shadow UI target in a headless Chromium browser.
- Captures fresh desktop and mobile screenshots.
- Sends those screenshots to the OpenAI Responses API for UI critique.
- Optionally asks the model for targeted edits to `public/index.html`, `public/styles.css`, and `public/app.js`.
- Runs syntax checks and rolls back generated edits if they break JavaScript parsing.
- Repeats until the stop condition is met.

Configuration:

- Copy `ui-review.config.example.json` to `ui-review.config.json`.
- Set `OPENAI_API_KEY` in the environment used to launch the loop.
- Override the target URL, model, max rounds, score threshold, and viewport sizes in the config file.

Default stop condition:

- Stop after at most 4 rounds.
- Stop early when score is at least 8.5 and no major or medium issues remain.
- Stop when two rounds in a row contain only minor issues.
- Stop if the edit step produces no meaningful file changes.

Notes:

- The loop only writes inside `frontend`.
- Artifacts are stored under `frontend/data/ui-review/`.
- Use `npm run ui-review:loop -- --capture-only` to verify browser launch and screenshot capture without calling the API.
- Use `npm run ui-review:loop -- --review-only` to capture screenshots and critiques without applying edits.

## Safety

- Binds to `127.0.0.1` only.
- Uses a typed diagnostics registry in `server.js`.
- Does not expose arbitrary shell execution.
- Does not launch runs yet.
- Does not enable acquisition or live-send controls.
- Writes only a local UI audit log under `frontend/data/action-audit.log`.

## Defaults

```text
SHADOW_CONSOLE_HOST=ubuntu@109.94.99.15
SHADOW_CONSOLE_KEY=../g1-host-pack/ssh_file.txt
SHADOW_CONSOLE_PORT=8787
```

Override those environment variables if the host or key path changes.
