#!/usr/bin/env bash
# Build replay_hash from a jj revision and print the binary's path. Run from the repository root.
#
#   tools/bench/build-rev.sh REV
#
# Binaries are cached by commit in target/bench/bin/. The revision is checked out in a detached git
# worktree at target/bench/src with its own target directory, so the working copy and its build
# are untouched and rebuilding a nearby revision is incremental. See docs/development.md.
set -eu

rev=${1:?usage: tools/bench/build-rev.sh REV}
root=$(pwd)
commit=$(jj log -r "$rev" --no-graph -T 'commit_id ++ "\n"')
[ "$(printf '%s\n' "$commit" | wc -l)" -eq 1 ] || { echo "$rev: not a single revision" >&2; exit 2; }
bin=$root/target/bench/bin/replay_hash-${commit:0:12}
if [ -x "$bin" ]; then
  echo "$bin"
  exit 0
fi

src=$root/target/bench/src
mkdir -p "$root/target/bench/bin"
if [ -e "$src/.git" ]; then
  git -C "$src" checkout --quiet --detach --force "$commit"
else
  git worktree add --quiet --detach "$src" "$commit"
fi
direnv exec "$root" cargo build --locked --release --quiet --manifest-path "$src/Cargo.toml" \
  --target-dir "$root/target/bench/target" -p gba-diff --example replay_hash >&2
cp "$root/target/bench/target/release/examples/replay_hash" "$bin"
echo "$bin"
