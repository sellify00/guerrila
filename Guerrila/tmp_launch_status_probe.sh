ssh -o StrictHostKeyChecking=no ubuntu@.15 '
ROOT=/home/ubuntu/Guerrila/g1-host-pack/results/run_launch_r2_press_window_1h_20260531T015053Z
if [ -d "$ROOT" ]; then
  echo ROOT_OK
  ps -p 3994280 -o pid=,stat=,etime=,cmd= || true
  echo ---LOGTAIL---
  tail -n 120 "$ROOT/launch_r2_press_window.log" || true
  echo ---RUNSUM---
  find "$ROOT" -maxdepth 2 -type f \( -name summary.txt -o -name '*.json' \) | sort | tail -n 80
else
  echo ROOT_MISSING
fi
'