Continuation research artifacts live here.

Layout:
- `research/`: research notes and written findings.
- `live_probe/`: Helius live probe script and its captured report.
- `run80d/`: continuation habitat scorer outputs and the mock habitat seed used for the latest run.
- `studies/`: repeated persistence/churn studies built from multiple snapshots.
- `run_continuation_persistence_study.mjs`: repeatable study runner for habitat stability over time.
- `active_discovery/`: current-market active pool scans and their outputs.
- `discover_active_continuation_now.mjs`: discovers currently active WSOL pools and scores live continuation on them.
- `watchlist/`: live continuation watchlists, state diffs, and alert artifacts.
- `build_continuation_watchlist_alert.mjs`: builds a fresh watchlist from live discovery and emits material-change alerts.

Intentionally left outside this folder:
- `g1-host-pack/scripts/`: continuation and postshock engine scripts.
- `g1-host-pack/results/` and `g12-qc/`: older run archives that already belong to their result families.
