#!/usr/bin/env bash
set -euo pipefail
cd '/mnt/c/Shadow Engine/Guerrila'
export SHADOW_RUN66CTV_SOURCE_ROWS='/mnt/c/Shadow Engine/Guerrila/tmp/66ct_fixture_source.jsonl'
export SHADOW_RUN66CTV_RUNTIME_ROWS='/mnt/c/Shadow Engine/Guerrila/tmp/66ct_fixture_runtime.jsonl'
export SHADOW_RUN66CTV_STEP='tmp_66ctv_fixture_bash'
./g1-host-pack/scripts/66ctv_continuation_fresh_thesis_revalidation.sh
