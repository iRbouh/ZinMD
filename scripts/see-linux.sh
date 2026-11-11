#!/bin/sh
# Open a document with ZinMD on a screen that exists only in memory, drawn
# without a graphics card, and check that the app gets as far as a drawn
# window. This is how a machine with no screen can tell that the app starts
# on Linux.
#
#   scripts/see-linux.sh path/to/zinmd [where/to/write/what/it/said.log]
#
# Needs xvfb, a software Vulkan driver (mesa-vulkan-drivers) and xwininfo
# (x11-utils).
#
# It cannot show what the window looks like: a picture taken of that screen
# comes out empty. For that, ZinMD has to be looked at on a real desktop.
set -eu
cd "$(dirname "$0")/.."

program="$1"
said="${2:-dist/seen/linux-window.log}"
case "$program" in */*) program="$(cd "$(dirname "$program")" && pwd)/$(basename "$program")" ;; esac
mkdir -p "$(dirname "$said")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() {
    cat "$said" >&2 || true
    echo "FAILED: $1" >&2
    exit 1
}

cat > "$work/inside.sh" <<INSIDE
export ZINMD_TRACE=1 ZINMD_DATA="$work/data"
# First: start, draw one frame, say how long each step took, and leave.
echo "== one frame" >"$said"
ZINMD_BENCH=1 timeout 90 "$program" "$PWD/samples/roadmap.md" >>"$said" 2>&1 || exit 2
# Then: stay open, and be found among the windows on the screen.
echo "== left open" >>"$said"
"$program" "$PWD/samples/roadmap.md" >>"$said" 2>&1 &
app=\$!
sleep 10
kill -0 \$app 2>/dev/null || exit 3
echo "== windows on the screen" >>"$said"
xwininfo -root -tree >>"$said" 2>&1 || true
found=0
xwininfo -root -tree | grep -q '"app\.zinmd\.ZinMD"' || found=4
kill \$app
exit \$found
INSIDE

status=0
xvfb-run -a -s "-screen 0 1280x800x24" sh "$work/inside.sh" || status=$?
case "$status" in
    0) ;;
    2) fail "ZinMD did not draw a first frame" ;;
    3) fail "ZinMD closed by itself" ;;
    4) fail "ZinMD stayed open but put no window on the screen" ;;
    *) fail "the screen in memory could not be started" ;;
esac
grep -q "first frame on screen" "$said" || fail "ZinMD did not say it drew a first frame"
echo "ok: ZinMD opened a document, drew a first frame, and stayed open with its window up"
