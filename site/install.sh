#!/bin/sh
# Installs ZinMD on a Mac, from Terminal:
#
#   curl -fsSL https://irbouh.github.io/ZinMD/install.sh | sh
#
# It gets the latest release for this Mac's processor from GitHub, checks the
# file against the sums published with the release, and puts ZinMD.app in
# Applications. Nothing else is touched, and it asks for no password.
#
# Why bother, when there is a disk image to download? ZinMD is not signed with
# Apple yet. macOS marks what a browser downloads, and asks before it opens an
# unsigned app that carries the mark. A file fetched by this script carries no
# mark, so the app opens like any other.
#
# To choose for yourself:
#   ZINMD_VERSION=0.1.0   that version, not the latest
#   ZINMD_DIR=~/Apps      that folder, not Applications
#   ZINMD_OPEN=no         do not open the app when it is in place
set -eu

repo="iRbouh/ZinMD"
page="https://irbouh.github.io/ZinMD/#download"

fail() {
    printf 'ZinMD was not installed: %s\n' "$1" >&2
    exit 1
}

[ "$(uname -s)" = "Darwin" ] || fail "this command is for macOS. For other systems, see $page"

major="$(sw_vers -productVersion | cut -d. -f1)"
[ "$major" -ge 12 ] || fail "ZinMD needs macOS 12 or later, and this is $(sw_vers -productVersion)"

# The processor the Mac has, not the one a shell under Rosetta says it has.
if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
    arch="arm64"
else
    arch="x64"
fi

# The latest release is where GitHub sends anyone who asks for "latest".
version="${ZINMD_VERSION:-}"
if [ -z "$version" ]; then
    latest="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")" ||
        fail "GitHub could not be reached"
    version="${latest##*/}"
fi
version="${version#v}"
case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) fail "the latest version could not be found" ;;
esac

file="ZinMD-$version-macos-$arch.zip"
from="https://github.com/$repo/releases/download/v$version"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

printf 'Getting ZinMD %s for %s...\n' "$version" "$([ "$arch" = arm64 ] && echo "Apple Silicon" || echo "Intel")"
curl -fsSL --retry 3 -o "$work/$file" "$from/$file" || fail "$file could not be fetched from $from"
curl -fsSL --retry 3 -o "$work/sums" "$from/SHA256SUMS.txt" || fail "the release's sums could not be fetched"

# The file has to be the one the release says it is.
want="$(awk -v f="$file" '$2 == f { print $1 }' "$work/sums")"
have="$(shasum -a 256 "$work/$file" | cut -d' ' -f1)"
[ -n "$want" ] || fail "the release lists no sum for $file"
[ "$want" = "$have" ] || fail "$file is not the file the release lists. Nothing was changed"

ditto -x -k "$work/$file" "$work/out"
app="$work/out/ZinMD.app"
[ -d "$app" ] || fail "the download holds no ZinMD.app"
codesign --verify --deep --strict "$app" 2>/dev/null || fail "the app's seal is broken. Nothing was changed"

# Applications, or the one in the home folder where the first is locked.
dir="${ZINMD_DIR:-/Applications}"
if [ -z "${ZINMD_DIR:-}" ] && [ ! -w "$dir" ]; then
    dir="$HOME/Applications"
fi
mkdir -p "$dir" || fail "$dir could not be made"
[ -w "$dir" ] || fail "$dir cannot be written to"

if [ -d "$dir/ZinMD.app" ] && pgrep -f "$dir/ZinMD.app/Contents/MacOS/ZinMD" >/dev/null 2>&1; then
    fail "ZinMD is open. Quit it, then run this again"
fi

# The new copy is laid down whole beside the old one before it takes its place.
rm -rf "$dir/.ZinMD.app.new"
ditto "$app" "$dir/.ZinMD.app.new" || fail "the app could not be copied to $dir"
rm -rf "$dir/ZinMD.app"
mv "$dir/.ZinMD.app.new" "$dir/ZinMD.app"

printf 'ZinMD %s is in %s.\n' "$version" "$dir"
if [ "${ZINMD_OPEN:-yes}" != "no" ]; then
    open "$dir/ZinMD.app"
else
    printf 'Open it from Launchpad, or with: open "%s/ZinMD.app"\n' "$dir"
fi
