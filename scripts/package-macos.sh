#!/bin/sh
# Build ZinMD for one kind of Mac and wrap it the two ways it is given out:
# a disk image to install from, and a zip of the app on its own.
#
#   scripts/package-macos.sh arm64    for Apple Silicon
#   scripts/package-macos.sh x64      for Intel
#
# Both can be made on either kind of Mac. The files land in dist/.
set -eu
cd "$(dirname "$0")/.."

arch="${1:-arm64}"
case "$arch" in
    arm64) target="aarch64-apple-darwin" ;;
    x64) target="x86_64-apple-darwin" ;;
    *) echo "usage: scripts/package-macos.sh arm64|x64" >&2; exit 2 ;;
esac
# The version being released when a release is being made, otherwise the one
# in Cargo.toml. It names the files, and the app answers to it.
version="${ZINMD_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
export ZINMD_VERSION="$version"
name="ZinMD-$version-macos-$arch"

# The oldest macOS the app promises to run on, whatever the Mac building it.
export MACOSX_DEPLOYMENT_TARGET="12.0"
rustup target add "$target"
scripts/bundle.sh dist "$target"
mkdir -p dist
rm -f "dist/$name.dmg" "dist/$name.zip"

# The app alone, for those who would rather not install: unzip and run.
ditto -c -k --keepParent target/ZinMD.app "dist/$name.zip"

# The disk image: the app beside a shortcut to Applications, to drag it onto.
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp -R target/ZinMD.app "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "ZinMD" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "dist/$name.dmg"

echo "packaged dist/$name.dmg and dist/$name.zip"
