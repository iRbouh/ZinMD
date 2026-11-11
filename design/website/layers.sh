#!/bin/sh
# layers.sh [folder] : draw the scene again and cut it into the layers the
# website moves, one picture each, into site/img/scene (or the folder given).
# Says where each one goes on the stage, as it is written in site/index.html.
# Needs Chrome, ImageMagick and cwebp.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$here/../../site/img/scene}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"

python3 "$here/scene.py" "$tmp/art.html" >/dev/null
mkdir -p "$out"

# The canvas is 900 by 940; it is drawn at twice that, for sharp screens.
for name in $(grep -o '<svg id="[a-z0-9-]*"' "$tmp/art.html" | cut -d'"' -f2); do
  "$chrome" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$tmp/chrome" \
    --default-background-color=00000000 --force-device-scale-factor=2 --window-size=900,940 \
    --screenshot="$tmp/$name.png" "file://$tmp/art.html?only=$name&still=1" >/dev/null 2>&1
  # What is drawn, with a little room around it, kept inside the canvas.
  box="$(magick "$tmp/$name.png" -format '%@' info: | awk -F'[x+]' '{
    x = $3 - 6; if (x < 0) x = 0; y = $4 - 6; if (y < 0) y = 0
    r = $3 + $1 + 6; if (r > 1800) r = 1800; b = $4 + $2 + 6; if (b > 1880) b = 1880
    printf "%dx%d+%d+%d", r - x, b - y, x, y }')"
  magick "$tmp/$name.png" -crop "$box" +repage "$tmp/$name-cut.png"
  cwebp -quiet -q 88 -alpha_q 100 -m 6 "$tmp/$name-cut.png" -o "$out/$name.webp"
  echo "$box" | awk -v n="$name" -F'[x+]' '{
    printf "%-14s left:%.3f%%;top:%.3f%%;width:%.3f%%  (%s by %s)\n", n, $3/18, $4/18.8, $1/18, $1/2, $2/2 }'
done
