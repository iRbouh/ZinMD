#!/bin/sh
# Try an update the way it happens to a person, on macOS or Linux.
#
#   scripts/try-update.sh PROGRAM REPLACED [LOG]
#
# PROGRAM is a copy of ZinMD to start. REPLACED is what an update should
# replace: the app bundle on macOS, the program file or the AppImage on
# Linux.
#
# The copy is told of a release newer than itself: the downloads in dist/,
# under a later number. It is left to take that in with nobody there to
# click, and is then expected to have been replaced and to be running again.
set -eu
cd "$(dirname "$0")/.."

program="$1"
replaced="$2"
said="${3:-dist/seen/update.log}"
mkdir -p "$(dirname "$said")"
work="$(mktemp -d)"
tidy() {
    running | while read -r process; do kill "$process" 2>/dev/null || true; done
    rm -rf "$work"
}
trap tidy EXIT
fail() {
    cat "$said" >&2 || true
    echo "FAILED: $1" >&2
    exit 1
}
# The copies of PROGRAM that are running, by their numbers.
running() {
    ps -axo pid=,command= | while read -r process command; do
        case "$command" in "$program"*) echo "$process" ;; esac
    done
}
# What a file or folder is, apart from its name: it changes when another
# takes its place.
identity() {
    ls -di "$1" | awk '{ print $1 }'
}

python3 scripts/release-list.py 99.0.0 dist > "$work/latest.json"
before="$(identity "$replaced")"

ZINMD_UPDATE_FEED="$work/latest.json" ZINMD_UPDATE=now ZINMD_TRACE=1 ZINMD_DATA="$work/data" \
    "$program" "$PWD/samples/roadmap.md" >"$said" 2>&1 &
first=$!

# It leaves once the new version is in its place.
waited=0
while kill -0 "$first" 2>/dev/null; do
    sleep 1
    waited=$((waited + 1))
    if [ "$waited" -ge 180 ]; then
        kill "$first" 2>/dev/null || true
        fail "ZinMD did not restart into the new version"
    fi
done
grep -q "is ready" "$said" || fail "ZinMD left without a new version ready"
[ "$(identity "$replaced")" != "$before" ] || fail "ZinMD left, but was not replaced"

# And the new copy starts by itself.
waited=0
until [ -n "$(running)" ]; do
    sleep 1
    waited=$((waited + 1))
    [ "$waited" -lt 60 ] || fail "the new version was put in place, but did not start"
done
sleep 3
[ -n "$(running)" ] || fail "the new version started, and closed by itself"
echo "ok: ZinMD fetched a newer version, put it in its own place, and started it"
