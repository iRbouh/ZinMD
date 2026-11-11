#!/bin/sh
# Try the Linux downloads the way a person would. Run after
# scripts/package-linux.sh, with the same argument. Needs Docker.
#
#   scripts/try-linux.sh x64|arm64
#
#   .deb        installed on a bare Ubuntu and a bare Debian, with whatever it
#               says it needs, then asked its version and checked for Markdown
#   .rpm        the same on a bare Fedora
#   .AppImage   run as it is
#   .tar.gz     unpacked and installed into a folder
#
# Then the .deb is installed here and the app opened on a document, on a
# screen that exists only in memory. Last, the two copies that can update
# themselves (the AppImage, and the one installed from the .tar.gz) are told
# of a newer release and left to. What the app said is left in dist/seen/.
set -eu
cd "$(dirname "$0")/.."

arch="${1:-x64}"
case "$arch" in
    x64) platform="linux/amd64" ;;
    arm64) platform="linux/arm64" ;;
    *) echo "usage: scripts/try-linux.sh x64|arm64" >&2; exit 2 ;;
esac
version="${ZINMD_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
name="ZinMD-$version-linux-$arch"
seen="$PWD/dist/seen"
mkdir -p "$seen"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() {
    echo "FAILED: $1" >&2
    exit 1
}

# What is asked of an installed ZinMD, on any distribution.
cat > "$work/installed.sh" <<INSIDE
set -eu
said="\$(zinmd --version)"
[ "\$said" = "ZinMD $version" ] || { echo "it says \"\$said\", not \"ZinMD $version\"" >&2; exit 1; }
desktop-file-validate /usr/share/applications/app.zinmd.ZinMD.desktop
update-desktop-database /usr/share/applications
grep -q "^text/markdown=.*app.zinmd.ZinMD.desktop" /usr/share/applications/mimeinfo.cache ||
    { echo "it is not offered for Markdown files" >&2; exit 1; }
[ -f /usr/share/icons/hicolor/256x256/apps/app.zinmd.ZinMD.png ] || { echo "its icon is missing" >&2; exit 1; }
INSIDE

in_a_bare() {
    image="$1" install="$2"
    docker run --rm --platform "$platform" -v "$PWD/dist:/dist:ro" -v "$work/installed.sh:/installed.sh:ro" \
        "$image" sh -ec "$install && sh /installed.sh" >"$seen/$name-$(echo "$image" | tr ':/' '--').log" 2>&1 || {
        tail -40 "$seen/$name-$(echo "$image" | tr ':/' '--').log" >&2
        fail "ZinMD does not install and run on a bare $image"
    }
    echo "ok: installs and runs on a bare $image, and is offered for Markdown files"
}
apt="export DEBIAN_FRONTEND=noninteractive; apt-get update -qq && apt-get install -y -qq --no-install-recommends desktop-file-utils /dist/$name.deb"
in_a_bare ubuntu:22.04 "$apt"
in_a_bare debian:12 "$apt"
in_a_bare fedora:latest "dnf install -y -q desktop-file-utils /dist/$name.rpm"

# The AppImage, run as it is. (It unpacks itself; mounting itself, its usual
# way, needs a piece the pipeline's machines lack.)
cp "dist/$name.AppImage" "$work/ZinMD.AppImage"
chmod +x "$work/ZinMD.AppImage"
said="$("$work/ZinMD.AppImage" --appimage-extract-and-run --version)"
[ "$said" = "ZinMD $version" ] || fail "the AppImage says \"$said\", not \"ZinMD $version\""
echo "ok: the AppImage runs and says it is $said"

# The folder: unpack it and put it in place.
tar -C "$work" -xzf "dist/$name.tar.gz"
"$work/$name/install.sh" "$work/prefix" >/dev/null
said="$("$work/prefix/bin/zinmd" --version)"
[ "$said" = "ZinMD $version" ] || fail "the program from the .tar.gz says \"$said\""
grep -q "^Exec=$work/prefix/bin/zinmd" "$work/prefix/share/applications/app.zinmd.ZinMD.desktop" ||
    fail "the entry installed from the .tar.gz does not point at the program"
echo "ok: the .tar.gz unpacks and installs into a folder"

# Open a document for real: the .deb installed here, on a screen in memory,
# drawn without a graphics card.
if command -v xvfb-run >/dev/null 2>&1 && command -v sudo >/dev/null 2>&1; then
    sudo apt-get install -y -qq "./dist/$name.deb" >/dev/null
    scripts/see-linux.sh "$(command -v zinmd)" "$seen/$name-window.log"

    # Updates. Each copy is expected to replace itself and start again.
    on_a_screen() {
        xvfb-run -a -s "-screen 0 1280x800x24" "$@"
    }
    on_a_screen scripts/try-update.sh "$work/prefix/bin/zinmd" "$work/prefix/bin/zinmd" \
        "$seen/$name-update-tar.log"
    # (An AppImage unpacks itself here, as above; the new one must as well.)
    APPIMAGE_EXTRACT_AND_RUN=1 on_a_screen scripts/try-update.sh "$work/ZinMD.AppImage" \
        "$work/ZinMD.AppImage" "$seen/$name-update-appimage.log"
else
    echo "note: no screen in memory here; the app was not opened on a document"
fi

echo "the Linux downloads for $arch work"
