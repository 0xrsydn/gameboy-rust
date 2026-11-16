#!/usr/bin/env bash
# Record or check the per-frame replay hashes in roms/replays/. Run from the repository root.
#
#   tools/replays/check.sh --check [--quick] [--bin PATH] [REPLAY_HASH FLAGS...]
#   tools/replays/check.sh --record [--bin PATH] [REPLAY_HASH FLAGS...]
#
# Builds replay_hash first, unless --bin names an existing binary, and runs a private copy so a
# rebuild during the run cannot change what is being checked. --quick stops Mt. Moon at frame
# 45,700 (about five minutes instead of the full run). Extra flags such as --idle-skip go to every
# run. Exits 0 only when every run reports all frames matching (or, with --record, succeeds).
# See docs/testing.md.
set -u

mode= quick= bin= extra=()
while [ $# -gt 0 ]; do
  case $1 in
    --check | --record) mode=$1 ;;
    --quick) quick=1 ;;
    --bin) bin=${2:?--bin needs a path}; shift ;;
    *) extra+=("$1") ;;
  esac
  shift
done
if [ -z "$mode" ]; then
  echo "usage: tools/replays/check.sh --check|--record [--quick] [--bin PATH] [FLAGS...]" >&2
  exit 2
fi
if [ -n "$quick" ] && [ "$mode" = --record ]; then
  echo "--quick only applies to --check" >&2
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
mkdir -p "$D"
FR=(--rom roms/pokemon-firered.gba --save-type flash128)
EM=(--rom roms/pokemon-emerald.gba --save-type flash128 --rtc 847932662)
SAVE=$D/firered.sav ESAVE=$D/emerald.sav
if [ "$mode" = --check ]; then SAVE=$D/firered-check.sav ESAVE=$D/emerald-check.sav; fi
mtmoon=425024
[ -n "$quick" ] && mtmoon=45700

# run NAME ARGS...: one replay, its log in $D/NAME.log, its verdict in $work/NAME.ok.
run() {
  local name=$1
  shift
  if "$work/replay_hash" "$@" "$mode" "$D/$name.hashes" ${extra[@]+"${extra[@]}"} > "$D/$name.log" 2>&1; then
    touch "$work/$name.ok"
  fi
  echo "$name: $(tail -1 "$D/$name.log")"
}

# skip_unless FIRST SECOND: succeeds (skipping SECOND) when FIRST failed, since SECOND needs its save.
skip_unless() {
  [ -e "$work/$1.ok" ] && return 1
  echo "skipped: $1 failed" > "$D/$2.log"
  echo "$2: skipped"
}

# Six runs cover all fourteen recorded replays: each omitted script is a frame-for-frame prefix of
# one below with the same flags (firered-first-battle with --capture of firered-mtmoon, without it
# of firered-save; route1, viridian, parcel, forest, and brock of firered-mtmoon; emerald-tv-news
# with --capture of emerald-route101).
{
  run firered-save "${FR[@]}" --frames 22200 --inputs tools/replays/firered-save.inputs --write-save "$SAVE"
  skip_unless firered-save firered-continue || run firered-continue "${FR[@]}" --frames 2400 \
    --inputs tools/replays/firered-continue.inputs --load-save "$SAVE" --capture
} &
{
  run emerald-route101 "${EM[@]}" --frames 45000 --inputs tools/replays/emerald-route101.inputs --capture \
    --write-save "$ESAVE"
  skip_unless emerald-route101 emerald-continue || run emerald-continue "${EM[@]}" --frames 2400 \
    --inputs tools/replays/emerald-continue.inputs --load-save "$ESAVE" --capture
} &
run emerald-bulk "${EM[@]}" --frames 13000 --inputs tools/replays/emerald-tv-news.inputs &
run firered-mtmoon "${FR[@]}" --frames "$mtmoon" --inputs tools/replays/firered-mtmoon.inputs --capture &
wait

status=0
for name in firered-save firered-continue emerald-route101 emerald-continue emerald-bulk firered-mtmoon; do
  if [ ! -e "$work/$name.ok" ]; then
    echo "FAILED: $name (see $D/$name.log)" >&2
    status=1
  fi
done
[ $status = 0 ] && echo "all replays pass ($mode${quick:+ --quick})"
exit $status
