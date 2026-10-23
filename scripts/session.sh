#!/usr/bin/env bash
# One measurement session, start to finish: a fresh cluster snapshot, the slot stream recorded alongside the run (an
# independent record of every window start and of the leader hand-overs), the run itself with its log, a final
# reconciliation with the chain and the analysis of all the data.
#
#   scripts/session.sh            # dry-run: nothing is sent
#   scripts/session.sh --live     # sends, under the spend cap (fund the payers first: `order-probe fund --live`)
#
# Arguments go to `order-probe run`. The campaign's targets and conditions are the defaults; override them with
# TARGETS, BURSTS, OFFSETS, SPACINGS and MINUTES. DATA_DIR, SECRETS_DIR, RPC_URL, WS_URL and SPEND_CAP_SOL are read
# by order-probe itself; REPORT_DIR (default report) receives the analysis.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=probe/target/release/order-probe
[ -x "$BIN" ] || cargo build --release --manifest-path probe/Cargo.toml
export DATA_DIR="${DATA_DIR:-data}"
REPORT_DIR="${REPORT_DIR:-report}"
TARGETS="${TARGETS:-7QQGNm3ptwinipDCyaCF7jY5katgmFUu1ieP2f7nwLpE,5Us18hLZPXJTS4QVuGSsUw137Dyd2tgBaem24Xsf5nBS,HEL1USMZKAL2odpNBj2oCjffnFGaYwmbGmyewGv1e2TU,E1r4Psq84tHfQ6aPTvvDka4U3u8zPVD7gEUrH25RdxHL,DRpbCBMxVnDK7maPM5tGv6MvB3v1sRMC86PZ8okm21hy,5Cchr1XGEg7dbBXByV5NY2ad8jfxAM7HA3x8D56rq9Ux,EvnRmnMrd69kFdbLMxWkTn1icZ7DCceRhvmb2SJXqDo4,9eGrDohdNTAo61DRHyfMuqKWXqYnA3i254Wiszxe8FoY,Fd7btgySsrjuo25CJCj7oE7VPMyezDhnx7pZkj2v69Nk,JUPiTERrZqgf1jUyR7dSkhMx4Kn2qJyekWsg3LT1h4b,ana2y2YvQ3ZPMwm6qhnN3nJoUSiT3qx5Pvetkq9xcfY,9jxgosAfHgHzwnxsHw4RAZYaLVokMbnYtmiZBreynGFP}"
BURSTS="${BURSTS:-24}"
OFFSETS="${OFFSETS:--300,100}"
SPACINGS="${SPACINGS:-0,10,50}"
MINUTES="${MINUTES:-200}"

# Keep a Mac awake while the session runs (a closed lid still sleeps)
keep_awake() { if command -v caffeinate >/dev/null; then caffeinate -i "$@"; else "$@"; fi; }

logs="$(mktemp -d)"
echo "== snapshot"
"$BIN" recon --horizon-minutes 240 --per-client 2 > "$logs/recon.log" 2>&1 || echo "recon failed (see $logs/recon.log); continuing"

echo "== slot stream recorded alongside the run"
"$BIN" calibrate --minutes "$MINUTES" > "$logs/calibrate.log" 2>&1 &
calibrate=$!
stop_calibrate() { kill -TERM "$calibrate" 2>/dev/null && wait "$calibrate" 2>/dev/null || true; }
trap stop_calibrate EXIT

echo "== run"
status=0
keep_awake "$BIN" run --targets "$TARGETS" --bursts-per-target "$BURSTS" --offsets-ms="$OFFSETS" --spacings-ms "$SPACINGS" \
    --max-minutes "$MINUTES" "$@" 2>&1 | tee "$logs/run.log" || status=$?
run_id="$(awk '/^run [0-9]+T[0-9]+Z / {print $2; exit}' "$logs/run.log")"

echo "== stopping the slot stream"
stop_calibrate
trap - EXIT
if [ -n "$run_id" ] && [ -d "$DATA_DIR/runs/$run_id" ]; then
    cp "$logs/run.log" "$DATA_DIR/runs/$run_id/run.log"
    cp "$logs/calibrate.log" "$DATA_DIR/runs/$run_id/calibrate.log"
    echo "logs saved in $DATA_DIR/runs/$run_id"
fi

echo "== reconciliation"
"$BIN" reconcile || status=$?

if command -v docker >/dev/null; then
    echo "== analysis"
    docker build -q -t solana-order-probe/analyze analyze >/dev/null
    mkdir -p "$REPORT_DIR"
    docker run --rm -v "$(cd "$DATA_DIR" && pwd):/work/data:ro" -v "$(cd "$REPORT_DIR" && pwd):/work/report" \
        solana-order-probe/analyze || status=$?
fi
exit "$status"
