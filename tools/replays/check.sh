#!/usr/bin/env bash
# Record or check the per-frame replay hashes in roms/replays/. Run from the repository root.
#
#   tools/replays/check.sh --check [--quick | --continuous] [--bin PATH] [REPLAY_HASH FLAGS...]
#   tools/replays/check.sh --record [--bin PATH] [REPLAY_HASH FLAGS...]
#
# Builds replay_hash first, unless --bin names an existing binary, and runs a private copy so a
# rebuild during the run cannot change what is being checked. Extra flags such as --idle-skip go
# to every run. Exits 0 only when every run reports all frames matching (or, with --record,
# succeeds), and otherwise names each failing run and its log.
#
# A full check splits the long Mt. Moon replay into segments that resume from save states the
# recording wrote every 40,000 frames; each segment's last hash covers the state the next one
# starts from, so together they check every frame. --continuous replays it in one run instead,
# and --quick stops it at frame 45,700. See docs/testing.md.
set -u

mode= tier=full bin= extra=()
while [ $# -gt 0 ]; do
  case $1 in
    --check | --record) mode=$1 ;;
    --quick) tier=quick ;;
    --continuous) tier=continuous ;;
    --bin) bin=${2:?--bin needs a path}; shift ;;
    *) extra+=("$1") ;;
  esac
  shift
done
if [ -z "$mode" ]; then
  echo "usage: tools/replays/check.sh --check|--record [--quick|--continuous] [--bin PATH] [FLAGS...]" >&2
  exit 2
fi
if [ "$tier" != full ] && [ "$mode" = --record ]; then
  echo "--quick and --continuous only apply to --check" >&2
  exit 2
fi

if [ -z "$bin" ]; then
  direnv exec . cargo build --locked --release -p gba-diff --example replay_hash || exit 2
  bin=target/release/examples/replay_hash
fi
[ -x "$bin" ] || { echo "$bin: not an executable" >&2; exit 2; }
work=$(mktemp -d "${TMPDIR:-/tmp}/replay-check.XXXXXX")
trap 'rm -rf "$work"' EXIT
cp "$bin" "$work/replay_hash"

D=roms/replays
L=$D/logs
mkdir -p "$L"
FR=(--rom roms/pokemon-firered.gba --save-type flash128)
EM=(--rom roms/pokemon-emerald.gba --save-type flash128 --rtc 847932662)
SAVE=$D/firered.sav ESAVE=$D/emerald.sav
if [ "$mode" = --check ]; then SAVE=$D/firered-check.sav ESAVE=$D/emerald-check.sav; fi
MTMOON=(--frames 425024 --inputs tools/replays/firered-mtmoon.inputs --capture)
STATES=$D/states/firered-mtmoon
EVERY=40000

# run JOB HASHES ARGS...: one replay checked against (or recorded into) $D/HASHES.hashes, its log
# in $L/JOB.log, and its verdict in $work/JOB.ok.
run() {
  local job=$1 hashes=$2
  shift 2
  if "$work/replay_hash" "$@" "$mode" "$D/$hashes.hashes" ${extra[@]+"${extra[@]}"} > "$L/$job.log" 2>&1; then
    touch "$work/$job.ok"
  fi
  echo "$job: $(tail -1 "$L/$job.log")"
}

# skip_unless FIRST SECOND: succeeds (skipping SECOND) when FIRST failed, since SECOND needs its save.
skip_unless() {
  [ -e "$work/$1.ok" ] && return 1
  echo "skipped: $1 failed" > "$L/$2.log"
  echo "$2: skipped"
}

# Six replays cover all fourteen recorded scripts: each omitted script is a frame-for-frame prefix
# of one below with the same flags (firered-first-battle with --capture of firered-mtmoon, without
# it of firered-save; route1, viridian, parcel, forest, and brock of firered-mtmoon; emerald-tv-news
# with --capture of emerald-route101).
jobs=(firered-save firered-continue emerald-route101 emerald-continue emerald-bulk)
{
  run firered-save firered-save "${FR[@]}" --frames 22200 --inputs tools/replays/firered-save.inputs \
    --write-save "$SAVE"
  skip_unless firered-save firered-continue || run firered-continue firered-continue "${FR[@]}" \
    --frames 2400 --inputs tools/replays/firered-continue.inputs --load-save "$SAVE" --capture
} &
{
  run emerald-route101 emerald-route101 "${EM[@]}" --frames 45000 \
    --inputs tools/replays/emerald-route101.inputs --capture --write-save "$ESAVE"
  skip_unless emerald-route101 emerald-continue || run emerald-continue emerald-continue "${EM[@]}" \
    --frames 2400 --inputs tools/replays/emerald-continue.inputs --load-save "$ESAVE" --capture
} &
run emerald-bulk emerald-bulk "${EM[@]}" --frames 13000 --inputs tools/replays/emerald-tv-news.inputs &

if [ "$mode" = --record ]; then
  rm -rf "$STATES"
  jobs+=(firered-mtmoon)
  run firered-mtmoon firered-mtmoon "${FR[@]}" "${MTMOON[@]}" --states "$STATES" --every $EVERY &
elif [ $tier = quick ]; then
  jobs+=(firered-mtmoon)
  run firered-mtmoon firered-mtmoon "${FR[@]}" "${MTMOON[@]}" --frames 45700 &
elif [ $tier = continuous ] || ! ls "$STATES"/*.state > /dev/null 2>&1; then
  [ $tier = full ] && echo "no states in $STATES; checking Mt. Moon in one run (re-record to add them)"
  jobs+=(firered-mtmoon)
  run firered-mtmoon firered-mtmoon "${FR[@]}" "${MTMOON[@]}" &
else
  starts=(0 $(ls "$STATES" | sed -n 's/^\([0-9]*\)\.state$/\1/p' | sort -n))
  for i in "${!starts[@]}"; do
    start=${starts[$i]} end=${starts[$((i + 1))]:-}
    segment=(--frames "${end:-425024}")
    [ "$start" = 0 ] || segment+=(--states "$STATES" --start "$start")
    jobs+=("firered-mtmoon@$start")
    run "firered-mtmoon@$start" firered-mtmoon "${FR[@]}" "${MTMOON[@]}" "${segment[@]}" &
  done
fi
wait

status=0
for job in "${jobs[@]}"; do
  if [ ! -e "$work/$job.ok" ]; then
    echo "FAILED: $job (see $L/$job.log)" >&2
    status=1
  fi
done
label=$mode
[ $tier = full ] || label="$label --$tier"
[ $status = 0 ] && echo "all replays pass ($label)"
exit $status
