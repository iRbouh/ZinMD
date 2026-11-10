#!/bin/sh
# Rebuild the icons from the drawings in design/logo. Only needed when a drawing
# changes; the results are kept in the repository. Uses tools that ship with
# macOS (swift needs the Xcode command line tools).
#
#   assets/ZwinMD.icns            the app's icon
#   assets/ZwinMD-document.icns   the icon Markdown files wear in Finder
#   design/logo/png/              PNG copies, for the README and the web
set -eu
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Draw an SVG at a given width, keeping its transparency. (Quick Look, the
# other built-in way to draw an SVG, paints a white square behind it.)
render() {
    swift scripts/svg2png.swift "design/logo/$1.svg" "$2" "$3"
}

# An .icns holds the same picture at every size macOS asks for.
icns() {
    render "$1" "$work/$1.png" 1024
    set="$work/$2.iconset"
    mkdir "$set"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$work/$1.png" --out "$set/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        sips -z "$double" "$double" "$work/$1.png" --out "$set/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$set" -o "assets/$2.icns"
    echo "built assets/$2.icns"
}

icns app-icon ZwinMD
icns file-icon ZwinMD-document

mkdir -p design/logo/png
for name in app-icon file-icon; do
    cp "$work/$name.png" "design/logo/png/$name-1024.png"
    sips -z 256 256 "$work/$name.png" --out "design/logo/png/$name-256.png" >/dev/null
done
for name in mark mark-dark; do
    render "$name" "design/logo/png/$name-512.png" 512
done
echo "built design/logo/png"
