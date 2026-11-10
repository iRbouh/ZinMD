#!/bin/sh
# Build ZwinMD.app, the form macOS expects an application in. The menu bar and
# Dock take the name "ZwinMD" from the bundle; a bare binary shows its file name.
#
#   scripts/bundle.sh            quick build
#   scripts/bundle.sh dist       fully optimized build, for shipping
set -eu
cd "$(dirname "$0")/.."

profile="${1:-release}"
cargo build --profile "$profile"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

app="target/ZwinMD.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/zwinmd" "$app/Contents/MacOS/ZwinMD"
cp assets/ZwinMD.icns "$app/Contents/Resources/ZwinMD.icns"
# The icon Markdown files wear in Finder when ZwinMD is the app that opens them.
cp assets/ZwinMD-document.icns "$app/Contents/Resources/ZwinMD-document.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>ZwinMD</string>
    <key>CFBundleDisplayName</key><string>ZwinMD</string>
    <key>CFBundleIdentifier</key><string>app.zwinmd.ZwinMD</string>
    <key>CFBundleExecutable</key><string>ZwinMD</string>
    <key>CFBundleIconFile</key><string>ZwinMD</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Markdown document</string>
            <key>CFBundleTypeRole</key><string>Editor</string>
            <key>CFBundleTypeIconFile</key><string>ZwinMD-document</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array><string>net.daringfireball.markdown</string><string>public.plain-text</string></array>
            <key>CFBundleTypeExtensions</key>
            <array><string>md</string><string>markdown</string><string>mdx</string><string>mdown</string><string>mkd</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key><string>Folder</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>None</string>
            <key>LSItemContentTypes</key>
            <array><string>public.folder</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST

echo "built $app ($(du -sh "$app" | cut -f1))"
