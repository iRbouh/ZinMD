#!/bin/sh
# Build ZinMD for Linux and wrap it the four ways it is given out:
#
#   .deb        Debian, Ubuntu, Mint, Pop!_OS    installs, and opens Markdown files
#   .rpm        Fedora, openSUSE, Red Hat        installs, and opens Markdown files
#   .AppImage   any distribution                 one file that runs as it is
#   .tar.gz     any distribution                 the program in a folder, to unpack
#
#   scripts/package-linux.sh x64      for Intel and AMD processors
#   scripts/package-linux.sh arm64    for ARM processors
#
# Needs cargo-deb and cargo-generate-rpm (`cargo install` gets either). The
# tool that makes the AppImage is fetched when it is not at hand. Run this on
# the kind of processor it builds for. The files land in dist/.
set -eu
cd "$(dirname "$0")/.."

arch="${1:-x64}"
case "$arch" in
    x64) target="x86_64-unknown-linux-gnu" machine="x86_64" ;;
    arm64) target="aarch64-unknown-linux-gnu" machine="aarch64" ;;
    *) echo "usage: scripts/package-linux.sh x64|arm64" >&2; exit 2 ;;
esac
# The version being released when a release is being made, otherwise the one
# in Cargo.toml. It names the files, and the app answers to it.
version="${ZINMD_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
export ZINMD_VERSION="$version"
name="ZinMD-$version-linux-$arch"

rustup target add "$target"
cargo build --locked --profile dist --target "$target"
program="target/$target/dist/zinmd"

mkdir -p dist
rm -f "dist/$name.deb" "dist/$name.rpm" "dist/$name.AppImage" "dist/$name.tar.gz"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The two packages. What goes in them is listed in Cargo.toml; each works out
# for itself which system libraries the program needs.
cargo deb --no-build --no-strip --profile dist --target "$target" --output "dist/$name.deb"
cargo generate-rpm --profile dist --target "$target" --output "dist/$name.rpm"

# The program and what the desktop needs to know of it, laid out as under /usr.
tree="$work/$name"
install -Dm755 "$program" "$tree/bin/zinmd"
install -Dm644 packaging/linux/app.zinmd.ZinMD.desktop "$tree/share/applications/app.zinmd.ZinMD.desktop"
for size in 48 64 128 256 512; do
    install -Dm644 "packaging/linux/icons/$size.png" \
        "$tree/share/icons/hicolor/${size}x${size}/apps/app.zinmd.ZinMD.png"
done
install -Dm644 LICENSE "$tree/share/licenses/zinmd/LICENSE"

# The AppImage: that same tree, with the three things an AppImage is found
# by at its top: what to run, the entry, and the icon.
appdir="$work/ZinMD.AppDir"
mkdir -p "$appdir"
cp -R "$tree" "$appdir/usr"
ln -s usr/bin/zinmd "$appdir/AppRun"
ln -s usr/share/applications/app.zinmd.ZinMD.desktop "$appdir/app.zinmd.ZinMD.desktop"
ln -s usr/share/icons/hicolor/256x256/apps/app.zinmd.ZinMD.png "$appdir/app.zinmd.ZinMD.png"
ln -s app.zinmd.ZinMD.png "$appdir/.DirIcon"

# The tool that packs it, and the small program put at the front of every
# AppImage to unpack it. Both are fixed versions, checked against what they
# are known to be before they are used.
tools="target/appimage-tools"
mkdir -p "$tools"
fetch() {
    [ -f "$tools/$1" ] || curl -fsSL --retry 3 -o "$tools/$1" "$2"
    echo "$3  $tools/$1" | sha256sum -c - >/dev/null || {
        rm -f "$tools/$1"
        echo "$1 is not the file it should be" >&2
        exit 1
    }
}
case "$machine" in
    x86_64)
        packer_sum="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
        runtime_sum="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"
        ;;
    aarch64)
        packer_sum="f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158"
        runtime_sum="00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444"
        ;;
esac
fetch "appimagetool-$machine" \
    "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$machine.AppImage" \
    "$packer_sum"
fetch "runtime-$machine" \
    "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-$machine" \
    "$runtime_sum"
chmod +x "$tools/appimagetool-$machine"
# The tool is an AppImage itself; unpacking it to run needs nothing installed.
ARCH="$machine" APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool-$machine" \
    --no-appstream --runtime-file "$tools/runtime-$machine" "$appdir" "dist/$name.AppImage"

# The folder to unpack: the tree, and a script that puts it in place.
install -m755 packaging/linux/install.sh "$tree/install.sh"
cp LICENSE "$tree/LICENSE"
tar -C "$work" -czf "dist/$name.tar.gz" "$name"

echo "packaged dist/$name.deb, .rpm, .AppImage and .tar.gz"
