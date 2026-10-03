#!/usr/bin/env bash
set -euo pipefail

KEY=/tmp/shadow_launch10_hot_key
tr -d '\r' < '/mnt/c/Shadow Engine/Guerrila/g1-host-pack/ssh_file.txt' > "$KEY"
chmod 600 "$KEY"

HOST=ubuntu@109.94.99.15
TMP_REMOTE=/tmp/shadow_launch10_gate_$$

FILES=(
  g1-host-pack/scripts/22_launch10_hot_stream_executor.py
  g1-host-pack/scripts/22_launch10_hot_gate_autopsy.sh
  g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh
)

ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" "mkdir -p '$TMP_REMOTE' /home/ubuntu/Guerrila/g1-host-pack/scripts"
for file in "${FILES[@]}"; do
  scp -i "$KEY" -o StrictHostKeyChecking=no "/mnt/c/Shadow Engine/Guerrila/$file" "$HOST:$TMP_REMOTE/$(basename "$file")" >/dev/null
  echo "UPLOADED:$(basename "$file")"
done

ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" <<SSH
set -euo pipefail
cd /home/ubuntu/Guerrila
sudo install -m 0644 "$TMP_REMOTE/22_launch10_hot_stream_executor.py" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch10_hot_stream_executor.py
sudo install -m 0755 "$TMP_REMOTE/22_launch10_hot_gate_autopsy.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch10_hot_gate_autopsy.sh
sudo install -m 0755 "$TMP_REMOTE/22_launch10_hot_r2_press_window.sh" /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch10_hot_r2_press_window.sh
python3 -m py_compile /home/ubuntu/Guerrila/g1-host-pack/scripts/22_launch10_hot_stream_executor.py
rm -rf "$TMP_REMOTE"
printf '%s\n' 'INSTALLED:launch10_hot_gate_split'
SSH
