#!/bin/sh
# Try the macOS downloads the way a person would: open the disk image, drag
# the app out, and start it on a document. Then the same from the zip. Last,
# the app is told of a newer release and left to update itself.
# Run after scripts/package-macos.sh, with the same argument.
#
#   scripts/try-macos.sh arm64|x64
#
# In the release pipeline, a picture of the screen with the app open is left
# in dist/seen/. No picture is taken of anyone's own screen.
set -eu
cd "$(dirname "$0")/.."

arch="${1:-arm64}"
case "$arch" in
    arm64) built_for="arm64" ;;
    x64) built_for="x86_64" ;;
    *) echo "usage: scripts/try-macos.sh arm64|x64" >&2; exit 2 ;;
esac
version="${ZINMD_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
name="ZinMD-$version-macos-$arch"
seen="$PWD/dist/seen"
mkdir -p "$seen"
# Its real place, not the shortcut to it that macOS hands out: the app knows
# itself by the real one.
work="$(cd "$(mktemp -d)" && pwd -P)"
running=""
tidy() {
    [ -z "$running" ] || kill "$running" 2>/dev/null || true
    hdiutil detach -quiet "$work/image" 2>/dev/null || true
    rm -rf "$work"
}
trap tidy EXIT
fail() {
    echo "FAILED: $1" >&2
    exit 1
}

# An Apple Silicon Mac runs Intel programs through Rosetta. The pipeline's
# machines get it if they lack it; anyone else's Mac is left alone.
can_run=yes
if ! arch "-$built_for" /usr/bin/true 2>/dev/null; then
    if [ -n "${CI:-}" ] && [ "$built_for" = "x86_64" ]; then
        softwareupdate --install-rosetta --agree-to-license >/dev/null
    else
        can_run=no
        echo "note: this Mac cannot run $built_for programs; the app is checked but not started"
    fi
fi

try_app() {
    app="$1" from="$2"
    [ -d "$app" ] || fail "the $from holds no ZinMD.app"
    codesign --verify --deep --strict "$app" || fail "the app from the $from has a broken seal"
    lipo -archs "$app/Contents/MacOS/ZinMD" | grep -qx "$built_for" ||
        fail "the app from the $from is not built for $built_for"
    plutil -extract CFBundleDocumentTypes.0.CFBundleTypeExtensions json -o - "$app/Contents/Info.plist" |
        grep -q '"md"' || fail "the app from the $from does not offer itself for .md files"
    echo "ok: the app from the $from is whole, built for $built_for, and offers itself for Markdown"
    [ "$can_run" = yes ] || return 0

    said="$("$app/Contents/MacOS/ZinMD" --version)"
    [ "$said" = "ZinMD $version" ] || fail "the app from the $from says \"$said\", not \"ZinMD $version\""
    echo "ok: it says it is $said"

    # Open a document, and see that the app is still there a while later.
    ZINMD_DATA="$work/data-$from" "$app/Contents/MacOS/ZinMD" "$PWD/samples/roadmap.md" \
        >"$seen/$name-$from.log" 2>&1 &
    running=$!
    sleep 8
    if ! kill -0 "$running" 2>/dev/null; then
        cat "$seen/$name-$from.log" >&2
        fail "the app from the $from closed by itself"
    fi
    [ -z "${CI:-}" ] || screencapture -x "$seen/$name-$from.png" 2>/dev/null || true
    kill "$running" 2>/dev/null || true
    wait "$running" 2>/dev/null || true
    running=""
    echo "ok: it opened a document and stayed open"
}

# The disk image: the app beside a shortcut to Applications.
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$work/image" "dist/$name.dmg"
[ -L "$work/image/Applications" ] || fail "the disk image has no shortcut to Applications"
mkdir "$work/dmg"
cp -R "$work/image/ZinMD.app" "$work/dmg/"
hdiutil detach -quiet "$work/image"
try_app "$work/dmg/ZinMD.app" dmg

# The zip: the app on its own.
mkdir "$work/zip"
ditto -x -k "dist/$name.zip" "$work/zip"
try_app "$work/zip/ZinMD.app" zip

# An update: the copy taken from the disk image is told of a newer release,
# and is expected to replace itself with it and start again.
if [ "$can_run" = yes ]; then
    scripts/try-update.sh "$work/dmg/ZinMD.app/Contents/MacOS/ZinMD" "$work/dmg/ZinMD.app" \
        "$seen/$name-update.log"
fi

echo "the macOS downloads for $arch work"
