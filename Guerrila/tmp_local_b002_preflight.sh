set -euo pipefail
SRC="/mnt/c/Shadow Engine/Guerrila"
DST="$HOME/Guerrila_runtime"
cp -a "$SRC"/g1-host-pack/scripts/local_b002_*.sh "$DST"/g1-host-pack/scripts/
perl -pi -e 's/\r$//' "$DST"/g1-host-pack/scripts/local_b002_*.sh
chmod +x "$DST"/g1-host-pack/scripts/local_b002_*.sh
cd "$DST"
ls -b g1-host-pack/scripts/local_b002_*.sh
bash -n g1-host-pack/scripts/local_b002_preflight.sh
bash g1-host-pack/scripts/local_b002_preflight.sh