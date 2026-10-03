#!/usr/bin/env bash
# G1 Host-Run Pack — Build Script
#
# Builds shadow-tier0, shadow-egress-harmonic, shadow-oracle, and shadow-state with the
# release-guerrila
# profile (LTO, panic=abort, codegen-units=1) and copies binaries into
# the artifacts directory.
#
# Must run on Linux (or WSL). Not compatible with Windows.
#
# Usage:
#   ./g1-host-pack/build.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/.." && pwd)"
ARTIFACT_DIR="$SCRIPT_DIR/artifacts/release-guerrila"

echo "=== Shadow Engine G1 Build ==="
echo "Workspace:  $WORKSPACE"
echo "Artifacts:  $ARTIFACT_DIR"
echo ""

cd "$WORKSPACE"

echo "[1/3] Building shadow-tier0 + shadow-harmonic + shadow-oracle + shadow-state (release-guerrila profile)..."
cargo build --profile release-guerrila -p shadow-tier0 -p shadow-harmonic -p shadow-oracle -p shadow-state

echo "[2/3] Copying binaries..."
cp "target/release-guerrila/shadow-tier0" "$ARTIFACT_DIR/"
cp "target/release-guerrila/shadowguard-template-proof" "$ARTIFACT_DIR/"
cp "target/release-guerrila/shadow-egress-harmonic" "$ARTIFACT_DIR/"
cp "target/release-guerrila/shadow-oracle" "$ARTIFACT_DIR/"
cp "target/release-guerrila/shadow-state" "$ARTIFACT_DIR/"

echo "[3/3] Stripping debug symbols..."
strip "$ARTIFACT_DIR/shadow-tier0"
strip "$ARTIFACT_DIR/shadowguard-template-proof"
strip "$ARTIFACT_DIR/shadow-egress-harmonic"
strip "$ARTIFACT_DIR/shadow-oracle"
strip "$ARTIFACT_DIR/shadow-state"

echo ""
echo "Build complete."
ls -lh "$ARTIFACT_DIR"/shadow-*
