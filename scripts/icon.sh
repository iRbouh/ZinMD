#!/bin/sh
# Rebuild assets/ZwinMD.icns from design/icon.svg. Only needed when the drawing
# changes; the result is kept in the repository. Uses tools that ship with macOS.
set -eu
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
qlmanage -t -s 1024 -o "$work" design/icon.svg >/dev/null 2>&1

set="$work/ZwinMD.iconset"
mkdir "$set"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$work/icon.svg.png" --out "$set/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$work/icon.svg.png" --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o assets/ZwinMD.icns
echo "built assets/ZwinMD.icns"
