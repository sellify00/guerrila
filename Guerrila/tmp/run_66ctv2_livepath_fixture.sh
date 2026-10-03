#!/usr/bin/env bash
set -euo pipefail
cd '/mnt/c/Shadow Engine/Guerrila'
export SHADOW_RUN66CTV2_SOURCE_ROWS='/mnt/c/Shadow Engine/Guerrila/tmp/66ct_fixture_source.jsonl'
export SHADOW_RUN66CTV2_STEP='tmp_66ctv2_livepath_fixture'
./g1-host-pack/scripts/66ctv2_continuation_fresh_runtime_pressure_recompute.sh
