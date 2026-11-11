#!/bin/sh
# Install ZinMD from this folder. No package manager is involved.
#
#   ./install.sh                   for you alone, under ~/.local
#   sudo ./install.sh /usr/local   for everyone on this computer
#
# To remove it again, delete bin/zinmd and the files named app.zinmd.ZinMD
# under share/ in the same place.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
prefix="${1:-$HOME/.local}"

mkdir -p "$prefix/bin" "$prefix/share/applications"
cp "$here/bin/zinmd" "$prefix/bin/zinmd"
cp -R "$here/share/icons" "$here/share/licenses" "$prefix/share/"
# The entry in the apps menu names the program in full, so it starts even
# where this place is not among those the desktop searches for programs.
sed "s|^Exec=zinmd|Exec=$prefix/bin/zinmd|" "$here/share/applications/app.zinmd.ZinMD.desktop" \
    > "$prefix/share/applications/app.zinmd.ZinMD.desktop"

# Tell the desktop about the new app, where it has a way to be told.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$prefix/share/applications" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
fi

echo "ZinMD is installed in $prefix."
echo "Start it from your apps, or run: $prefix/bin/zinmd"
