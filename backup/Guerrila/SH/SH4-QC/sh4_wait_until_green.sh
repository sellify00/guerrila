#!/usr/bin/env bash
set -euo pipefail

source ~/.cargo/env 2>/dev/null || true

OUT="$HOME/shadow-host-audit/sh4-yellowstone-smoke"
mkdir -p "$OUT"

WAIT_LOG="$OUT/listener_wait.log"
CLIENT_LOG="$OUT/client_smoke.log"
WATCH_LOG="$OUT/post_listener_watch.log"
FINAL="$OUT/yellowstone_smoke_final.txt"

echo "=== SH4 wait start $(date -u) ===" | tee -a "$WAIT_LOG"

BOUND=0
for i in $(seq 1 2880); do
  {
    echo "=== $(date -u) ==="
    systemctl is-active sh4-agave-yellowstone-smoke.service || true
    free -h
    df -hT /mnt/solana-ledger /mnt/solana-accounts
    ss -ltnp | egrep ':(10000|8999|8899|8900)\b' || true
    grep -Ei 'snapshot|download|geyser|yellowstone|grpc|plugin|prometheus|error|failed|panic' \
      /var/log/solana/agave-devnet-yellowstone.log 2>/dev/null | tail -40 || true
  } >> "$WAIT_LOG" 2>&1

  if ! systemctl is-active --quiet sh4-agave-yellowstone-smoke.service; then
    echo "FAIL: sh4 service inactive" | tee -a "$WAIT_LOG"
    exit 1
  fi

  if free -m | awk '/Swap:/ { exit ($3 > 0 ? 1 : 0) }'; then
    :
  else
    echo "FAIL: swap in use" | tee -a "$WAIT_LOG"
    exit 1
  fi

  if ss -ltn | grep -q '127.0.0.1:10000' && ss -ltn | grep -q '127.0.0.1:8999'; then
    BOUND=1
    echo "=== Yellowstone localhost listeners bound $(date -u) ===" | tee -a "$WAIT_LOG"
    break
  fi

  sleep 10
done

if [[ "$BOUND" != "1" ]]; then
  echo "FAIL: timed out waiting for 127.0.0.1:10000 and 127.0.0.1:8999" | tee -a "$WAIT_LOG"
  exit 1
fi

cd "$HOME/SH/SH1-local-agave-yellowstone"

{
  echo "=== Yellowstone client smoke start $(date -u) ==="
  timeout 600 bash ./70_smoke_yellowstone_client.sh
  echo "=== Yellowstone client smoke done $(date -u) ==="
} | tee "$CLIENT_LOG"

set +e
timeout 3600 bash -c '
while true; do
  echo "=== $(date -u) ==="
  free -h
  df -hT /mnt/solana-ledger /mnt/solana-accounts
  systemctl is-active sh4-agave-yellowstone-smoke.service
  ps -C agave-validator -o pid,etime,%cpu,%mem,rss,vsz,args --no-headers || true
  ss -ltnp | egrep ":(10000|8999|8899|8900)\\b" || true
  ss -lunpt | egrep ":(8000|8001|8002|8003|8004|8005|8006|8007|8008|8009|8010|8011|8012|8013|8014|8015|8016|8017|8018|8019|8020|8021|8022|8023|8024|8025)\\b" || true
  curl -s http://127.0.0.1:8999/metrics 2>/dev/null | egrep -i "grpc|geyser|yellowstone|slot|transaction" | head -40 || true
  sleep 10
done
' | tee "$WATCH_LOG"
watch_status=${PIPESTATUS[0]}
set -e

if [[ "$watch_status" != "0" && "$watch_status" != "124" ]]; then
  echo "FAIL: post-listener watch exited with status $watch_status" | tee -a "$WATCH_LOG"
  exit "$watch_status"
fi

{
  echo "=== date ==="
  date -u

  echo "=== memory ==="
  free -h

  echo "=== disks ==="
  df -hT /mnt/solana-ledger /mnt/solana-accounts

  echo "=== service ==="
  sudo systemctl status sh4-agave-yellowstone-smoke.service --no-pager -l
  sudo systemctl cat sh4-agave-yellowstone-smoke.service

  echo "=== process ==="
  ps -C agave-validator -ww -o pid,etime,%cpu,%mem,rss,vsz,args || true

  echo "=== sockets tcp ==="
  sudo ss -ltnp

  echo "=== sockets solana dynamic ==="
  sudo ss -lunpt | egrep ':(8000|8001|8002|8003|8004|8005|8006|8007|8008|8009|8010|8011|8012|8013|8014|8015|8016|8017|8018|8019|8020|8021|8022|8023|8024|8025)\b' || true

  echo "=== yellowstone metrics sample ==="
  curl -s http://127.0.0.1:8999/metrics 2>/dev/null | head -200 || true

  echo "=== recent validator logs ==="
  sudo journalctl -u sh4-agave-yellowstone-smoke.service -n 300 --no-pager

  echo "=== client smoke tail ==="
  tail -120 "$CLIENT_LOG" 2>/dev/null || true

  echo "=== post-listener watch tail ==="
  tail -120 "$WATCH_LOG" 2>/dev/null || true

  echo "=== kernel oom ==="
  dmesg -T | egrep -i 'oom|killed process|out of memory' || true
} | tee "$FINAL"

echo "=== SH4 gate complete $(date -u) ==="
